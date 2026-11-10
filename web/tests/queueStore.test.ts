import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useQueueStore } from '../src/stores/useQueueStore';
import { useUiStore } from '../src/stores/useUiStore';
import { api } from '../src/lib/ipc';
import type { QueueSnapshot, QueueTask } from '../src/lib/types';
import { ANONYMOUS_ACCOUNT } from '../src/lib/adapt';

function makeTask(id: string, overrides: Partial<QueueTask> = {}): QueueTask {
  return {
    id,
    status: 'queued',
    phase: null,
    kind: 'video',
    sourceId: 'BV1',
    title: `任务 ${id}`,
    uploader: 'UP 主',
    coverUrl: null,
    pageUrl: 'https://www.bilibili.com/video/BV1mock0001',
    durationSec: 600,
    params: {
      entries: ['P1'],
      entryCount: 1,
      quality: 80,
      qualityLabel: '1080P',
      codec: 'avc',
      audio: 'original',
      outputDir: '~/Downloads/bilidown',
    },
    progress: { doneBytes: 0, totalBytes: 1_000, percent: 0, speedBps: 0, etaSec: null },
    stages: [],
    outputPath: null,
    error: null,
    restored: false,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

function makeSnapshot(tasks: QueueTask[]): QueueSnapshot {
  return {
    tasks,
    stats: {
      total: tasks.length,
      downloading: 0,
      queued: tasks.length,
      paused: 0,
      failed: 0,
      completed: 0,
      totalSpeedBps: 0,
    },
    account: ANONYMOUS_ACCOUNT,
  };
}

describe('useQueueStore：快照与事件合并', () => {
  beforeEach(() => {
    useQueueStore.setState({
      snapshot: null,
      loading: true,
      error: null,
      keyword: '',
      selectedTaskId: null,
    });
  });

  it('setSnapshot 建立队列并把 loading 置 false', () => {
    useQueueStore.getState().setSnapshot(makeSnapshot([makeTask('a'), makeTask('b')]));
    const state = useQueueStore.getState();
    expect(state.snapshot?.tasks).toHaveLength(2);
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it('task-updated 事件按 id 原地合并（不新增行）', () => {
    useQueueStore.getState().setSnapshot(makeSnapshot([makeTask('a'), makeTask('b')]));
    useQueueStore.getState().handleQueueEvent({
      type: 'task-updated',
      task: makeTask('a', { status: 'downloading', progress: { doneBytes: 500, totalBytes: 1_000, percent: 0.5, speedBps: 1024, etaSec: 10 } }),
    });
    const tasks = useQueueStore.getState().snapshot?.tasks ?? [];
    expect(tasks).toHaveLength(2);
    expect(tasks[0].status).toBe('downloading');
    expect(tasks[0].progress.percent).toBe(0.5);
  });

  it('task-updated 对未知 id 追加到队首（事件先于快照的容错）', () => {
    useQueueStore.getState().setSnapshot(makeSnapshot([makeTask('a')]));
    useQueueStore.getState().handleQueueEvent({ type: 'task-updated', task: makeTask('new') });
    const tasks = useQueueStore.getState().snapshot?.tasks ?? [];
    expect(tasks).toHaveLength(2);
    expect(tasks[0].id).toBe('new');
  });

  it('修订号防旧值覆盖：迟到的初始读取不覆盖新快照（tasks 5.4）', () => {
    useQueueStore.getState().setSnapshot({ ...makeSnapshot([makeTask('a')]), revision: 5 });
    useQueueStore.getState().setSnapshot({ ...makeSnapshot([makeTask('old')]), revision: 3 });
    expect((useQueueStore.getState().snapshot?.tasks ?? []).map((task) => task.id)).toEqual(['a']);

    useQueueStore.getState().setSnapshot({ ...makeSnapshot([makeTask('b')]), revision: 6 });
    expect((useQueueStore.getState().snapshot?.tasks ?? []).map((task) => task.id)).toEqual(['b']);
  });

  it('文件状态失效标记为未核实，成功刷新后恢复（tasks 7.2）', async () => {
    useQueueStore.getState().setSnapshot(makeSnapshot([makeTask('a')]));
    expect(useQueueStore.getState().filesVerified).toBe(true);
    useQueueStore.getState().markFilesPending();
    expect(useQueueStore.getState().filesVerified).toBe(false);

    vi.spyOn(api, 'getQueueSnapshot').mockResolvedValue(makeSnapshot([makeTask('a')]));
    await expect(useQueueStore.getState().refreshFiles()).resolves.toBe(true);
    expect(useQueueStore.getState().filesVerified).toBe(true);
  });

  it('task-removed 事件移除对应行', () => {
    useQueueStore.getState().setSnapshot(makeSnapshot([makeTask('a'), makeTask('b')]));
    useQueueStore.getState().handleQueueEvent({ type: 'task-removed', taskId: 'a' });
    const ids = (useQueueStore.getState().snapshot?.tasks ?? []).map((task) => task.id);
    expect(ids).toEqual(['b']);
  });
});

describe('useQueueStore：清空与持久化重试反馈（design D2/D8）', () => {
  function lastToast() {
    return useUiStore.getState().toasts.at(-1);
  }

  beforeEach(() => {
    useUiStore.setState({ toasts: [] });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    useUiStore.setState({ toasts: [] });
  });

  it('清空已完成使用返回的 removed 数量提示', async () => {
    vi.spyOn(api, 'clearCompleted').mockResolvedValue({ removed: 3, failed: [] });
    const pushToast = vi.spyOn(useUiStore.getState(), 'pushToast');

    await useQueueStore.getState().clearCompleted();

    expect(pushToast).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'info', message: expect.stringContaining('3') }),
    );
  });

  it('部分失败时不宣称全部清理成功，明确保留行数', async () => {
    vi.spyOn(api, 'clearCompleted').mockResolvedValue({ removed: 2, failed: ['a', 'b'] });

    await useQueueStore.getState().clearCompleted();

    const toast = lastToast();
    expect(toast?.kind).toBe('warning');
    expect(toast?.message).toContain('部分失败');
    expect(toast?.message).toContain('保留 2 行');
  });

  it('清空未激活统一走 clear_inactive（不再逐项取消）', async () => {
    const clearInactive = vi
      .spyOn(api, 'clearInactive')
      .mockResolvedValue({ removed: 4, failed: [] });
    const cancelTask = vi.spyOn(api, 'cancelTask');

    await useQueueStore.getState().clearInactive();

    expect(clearInactive).toHaveBeenCalledTimes(1);
    expect(cancelTask).not.toHaveBeenCalled();
    expect(lastToast()?.message).toContain('已清空 4 个未激活任务');
  });

  it('重试保存成功给出成功提示并只调用 retry_persistence', async () => {
    const retryPersistence = vi.spyOn(api, 'retryPersistence').mockResolvedValue(undefined);

    await useQueueStore.getState().retryPersistence();

    expect(retryPersistence).toHaveBeenCalledTimes(1);
    expect(lastToast()).toEqual(
      expect.objectContaining({ kind: 'success', message: expect.stringContaining('重试保存') }),
    );
  });

  it('重试保存失败保留可读错误', async () => {
    vi.spyOn(api, 'retryPersistence').mockRejectedValue({
      kind: 'disk',
      message: 'database is locked',
      actions: [],
    });

    await useQueueStore.getState().retryPersistence();

    const toast = lastToast();
    expect(toast?.kind).toBe('error');
    expect(toast?.message).toContain('database is locked');
  });
});
