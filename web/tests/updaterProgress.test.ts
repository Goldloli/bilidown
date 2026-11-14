import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { formatPercent } from '../src/lib/format';

const mocks = vi.hoisted(() => {
  const handler: {
    current: ((event: { payload: Record<string, unknown> }) => void) | null;
  } = { current: null };
  return {
    invoke: vi.fn(),
    listen: vi.fn(
      async (
        _event: string,
        next: (event: { payload: Record<string, unknown> }) => void,
      ) => {
        handler.current = next;
        return vi.fn();
      },
    ),
    handler,
  };
});

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));

import {
  getAppUpdateState,
  normalizeState,
  startAppUpdateDownload,
  subscribeAppUpdate,
  type RawAppUpdateState,
} from '../src/lib/updater';

function rawProgress(progress?: number): RawAppUpdateState {
  return {
    status: 'downloading',
    downloadedBytes: 10,
    totalBytes: 100,
    progress,
  };
}

function installTauriWindow(): void {
  vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
}

beforeEach(() => {
  installTauriWindow();
  mocks.handler.current = null;
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('应用更新进度单位', () => {
  it('把 shell 的 0–100 百分比统一转换为 UI 的 0–1', () => {
    for (const [raw, displayed] of [
      [0, '0%'],
      [0.5, '0.5%'],
      [1, '1%'],
      [1.5, '1.5%'],
      [50, '50%'],
      [100, '100%'],
    ] as const) {
      const normalized = normalizeState(rawProgress(raw));
      expect(formatPercent(normalized.progress, Number.isInteger(raw) ? 0 : 1)).toBe(displayed);
    }
  });

  it('缺失进度保持未知，不显示为 0%', () => {
    expect(normalizeState(rawProgress()).progress).toBeUndefined();
    expect(formatPercent(undefined, 1)).toBe('—');
  });

  it('状态查询与启动返回值只换算一次', async () => {
    mocks.invoke.mockResolvedValueOnce(rawProgress(0.5));
    await expect(getAppUpdateState()).resolves.toMatchObject({ progress: 0.005 });

    mocks.invoke.mockResolvedValueOnce(rawProgress(1));
    await expect(startAppUpdateDownload('2.5.0')).resolves.toMatchObject({ progress: 0.01 });
  });

  it('实时事件与状态查询使用同一换算', async () => {
    const observed: number[] = [];
    const unsubscribe = subscribeAppUpdate((state) => {
      if (state.progress != null) observed.push(state.progress);
    });
    await vi.waitFor(() => expect(mocks.handler.current).not.toBeNull());

    mocks.handler.current?.({ payload: rawProgress(1.5) });
    expect(observed).toEqual([0.015]);
    expect(formatPercent(observed[0], 1)).toBe('1.5%');
    unsubscribe();
  });
});
