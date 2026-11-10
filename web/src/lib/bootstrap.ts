import { api, CONTRACT_EVENTS } from './ipc';
import { devParams } from '../dev/scenario';
import type {
  AccountInfoDTO,
  DropPayloadDTO,
  HistoryEntryDTO,
  MenuActionDTO,
  NoticeDTO,
  QueueSnapshotDTO,
  QueueStatsDTO,
  SettingsDTO,
  TaskEventDTO,
} from './contract';
import { accountFromDTO, historyFromDTO, settingsFromDTO, snapshotFromDTO } from './adapt';
import { applyTheme, readThemeOverride, resolveTheme, systemTheme, watchSystemTheme } from './theme';
import { useQueueStore } from '../stores/useQueueStore';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useAccountStore } from '../stores/useAccountStore';
import { useHistoryStore } from '../stores/useHistoryStore';
import { useUiStore } from '../stores/useUiStore';
import { handleMenuAction } from './menu';
import { addFromClipboard, handleParseText } from './actions';
import { attemptQuit } from './quit';
import { applyPlatform, applyCapabilities } from './platform';
import { applyGlass } from './glass';

let booted: Promise<void> | null = null;

export function bootstrapApp(): Promise<void> {
  // 失败后允许重试：不缓存被拒绝的 promise（tasks 5.1/5.4）。
  booted ??= doBootstrap().catch((error: unknown) => {
    booted = null;
    throw error;
  });
  return booted;
}

async function doBootstrap(): Promise<void> {
  const params = devParams();

  // 1) 尽早应用主题，避免白闪；settings 到达后再以设置为准
  applyTheme(readThemeOverride() ?? systemTheme());

  // 2) 选择后端：mock（浏览器/截图）或 Tauri
  if (params.mock) {
    const { MockBackend } = await import('../dev/MockBackend');
    api.setBackend(new MockBackend());
  } else {
    const { createTauriBackend } = await import('./tauri-backend');
    api.setBackend(createTauriBackend());
  }

  // 3) 按 COMMANDS.md 契约订阅事件
  api.on<QueueSnapshotDTO>(CONTRACT_EVENTS.queueSnapshot, (dto) => {
    const snapshot = snapshotFromDTO(dto);
    useQueueStore.getState().setSnapshot(snapshot);
    useAccountStore.getState().setAccount(snapshot.account);
  });
  api.on<TaskEventDTO>(CONTRACT_EVENTS.queueEvents, (event) => {
    if (event.kind === 'removed') {
      useQueueStore.getState().handleQueueEvent({ type: 'task-removed', taskId: event.taskId });
    } else if (event.kind === 'failed') {
      useUiStore.getState().pushToast({
        kind: 'error',
        message: event.message ?? '任务失败',
      });
    }
  });
  api.on<AccountInfoDTO | null>(CONTRACT_EVENTS.accountChanged, (dto) => {
    useAccountStore.getState().setAccount(accountFromDTO(dto));
  });
  api.on<SettingsDTO>(CONTRACT_EVENTS.settingsChanged, (dto) => {
    const store = useSettingsStore.getState();
    store.setSettings(settingsFromDTO(dto, store.settings));
  });
  api.on<HistoryEntryDTO[]>(CONTRACT_EVENTS.historyChanged, (entries) => {
    useHistoryStore.getState().setEntries(entries.map(historyFromDTO));
  });
  api.on<NoticeDTO>(CONTRACT_EVENTS.notice, (notice) => {
    if (notice.code === 'clipboard-link') {
      useUiStore.getState().pushToast({
        kind: notice.level,
        message: notice.message,
        actionLabel: '一键添加',
        durationMs: 8000,
        action: () => void addFromClipboard(),
      });
      return;
    }
    useUiStore.getState().pushToast({ kind: notice.level, message: notice.message });
  });
  api.on<MenuActionDTO>(CONTRACT_EVENTS.shellMenu, (action) => {
    void handleMenuAction(action.id);
  });
  api.on<DropPayloadDTO>(CONTRACT_EVENTS.shellDrop, (payload) => {
    const text = payload.text || payload.paths.join('\n');
    if (text.trim()) void handleParseText(text);
  });
  api.on<QueueStatsDTO>(CONTRACT_EVENTS.shellQuitRequested, (stats) => {
    void (async () => {
      const pending = stats.queued + stats.active + stats.paused;
      const confirmed = await useUiStore.getState().confirm({
        title: '退出 bilidown？',
        body: `有 ${pending} 个未完成任务。退出后将暂停传输并保存进度，下次启动时可恢复。`,
        confirmLabel: '退出',
      });
      // 保存失败时由 attemptQuit 显示「重试保存 / 仍然退出」；正常退出不额外确认。
      if (confirmed) await attemptQuit();
    })();
  });

  // 4) 初始数据
  const [settings, snapshot, history, account, appInfo] = await Promise.all([
    api.getSettings(),
    api.getQueueSnapshot(),
    api.getHistory({ keyword: '', status: 'all' }),
    api.getAccount(),
    api.getAppInfo(),
  ]);
  // 平台信息供 CSS 与少量组件分支（不依赖 UA 嗅探）
  applyPlatform(appInfo.os);
  applyCapabilities({
    glassSupported: appInfo.glassSupported,
    glassUnavailableReason: appInfo.glassUnavailableReason,
    notificationsSupported: appInfo.notificationsSupported,
  });
  // 能力确定后重新应用玻璃偏好：不支持原生材质时强制不透明
  const glassEnabled = appInfo.glassSupported && settings.interface.glassBackground;
  applyGlass(glassEnabled);
  // 更新安装完成后的自动重启：提示一次新版本并跳过启动检查（Rust 侧已跳过）
  if (appInfo.justUpdated) {
    useUiStore.getState().pushToast({
      kind: 'success',
      message: `已更新到 ${appInfo.version}`,
      durationMs: 8000,
    });
  }
  useSettingsStore.getState().setSettings(settings);
  useQueueStore.getState().setSnapshot(snapshot);
  useAccountStore.getState().setAccount(account ?? snapshot.account);
  useHistoryStore.getState().setEntries(history);

  // 5) 跟随系统外观（仅跟随系统且无截图覆盖时）
  watchSystemTheme((theme) => {
    const appearance = useSettingsStore.getState().settings?.interface.appearance;
    if (appearance === 'system' && !readThemeOverride()) {
      applyTheme(theme);
    }
  });
  applyTheme(resolveTheme(settings.interface.appearance));

  // 6) mock 场景预置（面板/视图/toast）
  const scenario = await api.scenarioBootstrap();
  if (scenario) {
    const ui = useUiStore.getState();
    if (scenario.view) ui.setView(scenario.view);
    switch (scenario.panel) {
      case 'add-link':
        ui.openPanel({ type: 'add-link' });
        break;
      case 'add':
        if (scenario.parseResult) ui.openPanel({ type: 'add', parse: scenario.parseResult });
        break;
      case 'detail':
        if (scenario.taskId) ui.openPanel({ type: 'detail', taskId: scenario.taskId });
        break;
      case 'login':
        ui.openPanel({ type: 'login' });
        break;
      default:
        break;
    }
    for (const notice of scenario.notices ?? []) {
      ui.pushNotice(notice);
    }
    if (scenario.quitFailure) ui.showQuitFailure(scenario.quitFailure);
  }
}
