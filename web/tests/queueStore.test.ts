import { beforeEach, describe, expect, it } from 'vitest';
import { useQueueStore } from '../src/stores/useQueueStore';
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

  it('task-removed 事件移除对应行', () => {
    useQueueStore.getState().setSnapshot(makeSnapshot([makeTask('a'), makeTask('b')]));
    useQueueStore.getState().handleQueueEvent({ type: 'task-removed', taskId: 'a' });
    const ids = (useQueueStore.getState().snapshot?.tasks ?? []).map((task) => task.id);
    expect(ids).toEqual(['b']);
  });
});
