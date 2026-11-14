/**
 * 应用更新 / 关于链接（task-12）
 *
 * 契约（shell-engineer 实现）：
 * · 命令：get_app_update_state / check_app_update / start_app_update_download(version) /
 *   cancel_app_update_download / open_downloaded_update / open_update_file_location /
 *   open_about_link(link)
 * · 事件：app-update://progress（payload 同 state）
 *
 * 说明：当前 `lib/ipc.ts` 与 `lib/contract.ts` 由 Lead 收尾冻结，本模块暂以独立
 * invoke/listen 封装；mock 模式在本地模拟状态机（无网络），待契约合并后可迁回 Backend。
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type AppUpdateStatus =
  | 'idle'
  | 'checking'
  | 'up-to-date'
  | 'available'
  | 'downloading'
  | 'downloaded'
  | 'error';

/** shell 原始状态（含 verifying/completed/cancelled）；UI 侧归一化后使用 AppUpdateState */
export interface RawAppUpdateState {
  status: 'idle' | 'checking' | 'available' | 'downloading' | 'verifying' | 'completed' | 'error' | 'cancelled';
  version?: string;
  filename?: string;
  path?: string;
  downloadedBytes: number;
  totalBytes?: number;
  /** shell 输出 0–100 */
  progress?: number;
  bytesPerSecond?: number;
  etaSeconds?: number;
  error?: string;
}

/** 与 shell `AppUpdateState` 对齐 */
export interface AppUpdateState {
  status: AppUpdateStatus;
  version?: string;
  filename?: string;
  path?: string;
  downloadedBytes: number;
  totalBytes?: number;
  /** UI 进度使用 0–1，供 formatPercent 直接展示 */
  progress?: number;
  bytesPerSecond?: number;
  etaSeconds?: number;
  error?: string;
}

/** 与 shell `AppUpdateCheck` 对齐 */
export interface AppUpdateRelease {
  version: string;
  tagName: string;
  name: string;
  notes: string;
  publishedAt: string;
  htmlUrl: string;
  assetName?: string;
  assetSize?: number;
}

export interface AppUpdateCheck {
  currentVersion: string;
  updateAvailable: boolean;
  checkedAtMs: number;
  release: AppUpdateRelease | null;
  releasesUrl: string;
}

export type AboutLink =
  | 'repository'
  | 'releases'
  | 'issues'
  | 'license'
  | 'author'
  | 'acknowledgements';

export const ABOUT_LINK_LABEL: Record<AboutLink, string> = {
  repository: 'GitHub 仓库',
  releases: 'Releases',
  issues: 'Issues',
  license: 'MIT 许可证',
  author: '作者',
  acknowledgements: '致谢',
};

const EVENTS = { progress: 'app-update://progress' } as const;

/** shell 0–100 + verifying/completed/cancelled → UI 0–1 + downloading/downloaded */
export function normalizeState(raw: RawAppUpdateState): AppUpdateState {
  const status: AppUpdateStatus =
    raw.status === 'completed'
      ? 'downloaded'
      : raw.status === 'verifying'
        ? 'downloading'
        : raw.status === 'cancelled'
          ? 'idle'
          : raw.status;
  const progress = raw.progress == null ? undefined : raw.progress / 100;
  return { ...raw, status, progress } as AppUpdateState;
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

/* ------------------------------------------------------------------ */
/* mock 状态机（无网络；通过 URL `updates=` 切换，供截图/自测）       */
/* ------------------------------------------------------------------ */

type MockMode = 'latest' | 'available' | 'downloading' | 'downloaded' | 'error' | 'idle';

function mockMode(): MockMode {
  const value = new URLSearchParams(window.location.search).get('updates');
  if (value === 'available' || value === 'downloading' || value === 'downloaded' || value === 'error' || value === 'idle') {
    return value;
  }
  return 'latest';
}

const MOCK_CURRENT = '2.0.0';
const MOCK_RELEASE: AppUpdateRelease = {
  version: '2.1.0',
  tagName: 'v2.1.0',
  name: 'bilidown 2.1.0',
  notes: '· 新增字幕与封面下载\n· 修复合流偶发失败\n· 更新关于页与更新检查',
  publishedAt: '2026-10-20T12:00:00Z',
  htmlUrl: 'https://github.com/Goldloli/bilidown/releases/tag/v2.1.0',
  assetName: 'bilidown_2.1.0_aarch64.dmg',
  assetSize: 18_432_000,
};

const listeners = new Set<(state: AppUpdateState) => void>();
let mockState: AppUpdateState | null = null;
let mockTimer: ReturnType<typeof setInterval> | null = null;
let mockCheck: AppUpdateCheck | null = null;

function emitMock(state: AppUpdateState): void {
  mockState = state;
  listeners.forEach((listener) => listener(state));
}

function stopMockTimer(): void {
  if (mockTimer) {
    clearInterval(mockTimer);
    mockTimer = null;
  }
}

function initialMockState(): AppUpdateState {
  const mode = mockMode();
  switch (mode) {
    case 'available':
      return { status: 'available', version: MOCK_RELEASE.version, downloadedBytes: 0, totalBytes: MOCK_RELEASE.assetSize };
    case 'downloading':
      return {
        status: 'downloading',
        version: MOCK_RELEASE.version,
        filename: MOCK_RELEASE.assetName,
        downloadedBytes: 11_427_840,
        totalBytes: MOCK_RELEASE.assetSize,
        progress: 0.62,
        bytesPerSecond: 2_400_000,
        etaSeconds: 3,
      };
    case 'downloaded':
      return {
        status: 'downloaded',
        version: MOCK_RELEASE.version,
        filename: MOCK_RELEASE.assetName,
        path: '~/Downloads/bilidown_2.1.0_aarch64.dmg',
        downloadedBytes: MOCK_RELEASE.assetSize ?? 0,
        totalBytes: MOCK_RELEASE.assetSize,
        progress: 1,
      };
    case 'error':
      return {
        status: 'error',
        version: MOCK_RELEASE.version,
        downloadedBytes: 0,
        error: '网络不可用：无法连接 GitHub Releases（mock）',
      };
    case 'idle':
      return { status: 'idle', downloadedBytes: 0 };
    default:
      return {
        status: 'up-to-date',
        version: MOCK_CURRENT,
        downloadedBytes: 0,
        progress: 1,
      };
  }
}

function mockCheckResult(): AppUpdateCheck {
  const mode = mockMode();
  const available = mode !== 'latest' && mode !== 'idle';
  mockCheck = {
    currentVersion: MOCK_CURRENT,
    updateAvailable: available,
    checkedAtMs: Date.now(),
    release: available ? MOCK_RELEASE : null,
    releasesUrl: 'https://github.com/bilidown/bilidown/releases',
  };
  return mockCheck;
}

/* ------------------------------------------------------------------ */
/* 对外 API                                                           */
/* ------------------------------------------------------------------ */

export async function getAppUpdateState(): Promise<AppUpdateState> {
  if (!isTauri()) {
    mockState ??= initialMockState();
    return mockState;
  }
  const raw = await invoke<RawAppUpdateState>('get_app_update_state');
  return normalizeState(raw);
}

export async function checkAppUpdate(): Promise<AppUpdateCheck> {
  if (!isTauri()) {
    const check = mockCheckResult();
    emitMock(
      check.updateAvailable
        ? {
            status: 'available',
            version: check.release?.version,
            downloadedBytes: 0,
            totalBytes: check.release?.assetSize,
          }
        : { status: 'up-to-date', version: check.currentVersion, downloadedBytes: 0, progress: 1 },
    );
    return check;
  }
  return invoke<AppUpdateCheck>('check_app_update');
}

export async function startAppUpdateDownload(version: string): Promise<AppUpdateState> {
  if (!isTauri()) {
    stopMockTimer();
    const total = MOCK_RELEASE.assetSize ?? 18_000_000;
    let done = Math.round(total * 0.12);
    emitMock({
      status: 'downloading',
      version,
      filename: MOCK_RELEASE.assetName,
      downloadedBytes: done,
      totalBytes: total,
      progress: done / total,
      bytesPerSecond: 2_200_000,
      etaSeconds: 8,
    });
    mockTimer = setInterval(() => {
      done = Math.min(total, done + Math.round(total * 0.12));
      const progress = done / total;
      emitMock({
        status: progress >= 1 ? 'downloaded' : 'downloading',
        version,
        filename: MOCK_RELEASE.assetName,
        path: progress >= 1 ? '~/Downloads/bilidown_2.1.0_aarch64.dmg' : undefined,
        downloadedBytes: done,
        totalBytes: total,
        progress,
        bytesPerSecond: progress >= 1 ? undefined : 2_200_000,
        etaSeconds: progress >= 1 ? undefined : Math.max(1, Math.round((total - done) / 2_200_000)),
      });
      if (progress >= 1) stopMockTimer();
    }, 450);
    return mockState as AppUpdateState;
  }
  const raw = await invoke<RawAppUpdateState>('start_app_update_download', { version });
  return normalizeState(raw);
}

export async function cancelAppUpdateDownload(): Promise<void> {
  if (!isTauri()) {
    stopMockTimer();
    emitMock({ status: 'available', version: MOCK_RELEASE.version, downloadedBytes: 0 });
    return;
  }
  await invoke('cancel_app_update_download');
}

export async function openDownloadedUpdate(): Promise<void> {
  if (!isTauri()) return;
  await invoke('open_downloaded_update');
}

export async function openUpdateFileLocation(): Promise<void> {
  if (!isTauri()) return;
  await invoke('open_update_file_location');
}

/**
 * Windows：先保存任务并退出应用，再以静默更新模式启动 NSIS 安装器，安装完成后自动重启。
 * 调用成功后应用会退出（前端无需再做后续处理）。
 */
export async function installAppUpdate(): Promise<void> {
  if (!isTauri()) return;
  await invoke('install_app_update');
}

export async function openAboutLink(link: AboutLink): Promise<void> {
  if (!isTauri()) return;
  await invoke('open_about_link', { link });
}

export function subscribeAppUpdate(listener: (state: AppUpdateState) => void): () => void {
  listeners.add(listener);
  if (isTauri()) {
    let unlisten: UnlistenFn | null = null;
    void listen<RawAppUpdateState>(EVENTS.progress, (event) => listener(normalizeState(event.payload))).then((fn) => {
      unlisten = fn;
    });
    return () => {
      listeners.delete(listener);
      unlisten?.();
    };
  }
  return () => listeners.delete(listener);
}
