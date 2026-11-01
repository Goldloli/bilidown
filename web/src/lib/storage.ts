/**
 * 存储与维护（OpenSpec app-maintenance D3）
 * shell 命令：get_storage_stats / clear_app_cache / open_data_dir
 * mock：离线确定性数据，可通过 URL ?storage=low|error 与 ?storageClear=error 切换状态。
 */
import { invoke } from '@tauri-apps/api/core';

export interface StorageStats {
  dataDir: string;
  downloadDir: string;
  dataBytes: number;
  coverBytes: number;
  tempBytes: number;
  logBytes: number;
  downloadFreeBytes?: number | null;
  downloadTotalBytes?: number | null;
  ffmpegPath?: string | null;
}

export type ClearTarget = 'covers' | 'temp' | 'logs' | 'all';

export interface ClearResult {
  freedBytes: number;
  removedFiles: number;
}

export const LOW_SPACE_THRESHOLD = 2 * 1024 ** 3;

export function isLowSpace(stats: StorageStats | null): boolean {
  return Boolean(
    stats &&
      stats.downloadFreeBytes != null &&
      stats.downloadFreeBytes >= 0 &&
      stats.downloadFreeBytes < LOW_SPACE_THRESHOLD,
  );
}

function isTauri(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

function storageScenario(): string | null {
  try {
    return new URLSearchParams(window.location.search).get('storage');
  } catch {
    return null;
  }
}

function clearScenario(): string | null {
  try {
    return new URLSearchParams(window.location.search).get('storageClear');
  } catch {
    return null;
  }
}

const MP = 1024 ** 2;
const GB = 1024 ** 3;

const MOCK_STATS: StorageStats = {
  dataDir: '~/Library/Application Support/bilidown',
  downloadDir: '~/Downloads/bilidown',
  dataBytes: 2.34 * GB,
  coverBytes: 26.4 * MP,
  tempBytes: 184 * MP,
  logBytes: 12.6 * MP,
  downloadFreeBytes: 42.8 * GB,
  downloadTotalBytes: 494.4 * GB,
  ffmpegPath: '/opt/homebrew/bin/ffmpeg',
};

export async function getStorageStats(): Promise<StorageStats> {
  if (isTauri()) {
    return invoke<StorageStats>('get_storage_stats');
  }
  await new Promise((resolve) => setTimeout(resolve, 220));
  if (storageScenario() === 'error') {
    throw new Error('无法读取磁盘空间（mock）');
  }
  if (storageScenario() === 'low') {
    return { ...MOCK_STATS, downloadFreeBytes: 1.2 * GB };
  }
  return { ...MOCK_STATS };
}

export async function clearAppCache(target: ClearTarget): Promise<ClearResult> {
  if (isTauri()) {
    return invoke<ClearResult>('clear_app_cache', { target });
  }
  await new Promise((resolve) => setTimeout(resolve, 480));
  if (clearScenario() === 'error') {
    throw new Error('清理失败：目录不可写（mock）');
  }
  const freed =
    target === 'logs'
      ? MOCK_STATS.logBytes
      : target === 'covers'
        ? MOCK_STATS.coverBytes
        : target === 'temp'
          ? MOCK_STATS.tempBytes
          : MOCK_STATS.coverBytes + MOCK_STATS.tempBytes + MOCK_STATS.logBytes;
  return { freedBytes: freed, removedFiles: target === 'all' ? 321 : 96 };
}

export async function openDataDir(): Promise<void> {
  if (isTauri()) {
    await invoke('open_data_dir');
  }
}
