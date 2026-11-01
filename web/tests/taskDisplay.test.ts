import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { QueueTask } from '../src/lib/types';
import { taskSubline } from '../src/lib/taskDisplay';

function multiPartTask(entryCount: number): QueueTask {
  return {
    id: 'task-multip',
    status: 'completed',
    phase: null,
    kind: 'video',
    sourceId: 'BV1dw411s78P',
    title: '30P 测试视频',
    uploader: '树上的阿绿',
    coverUrl: null,
    pageUrl: 'https://www.bilibili.com/video/BV1dw411s78P',
    durationSec: null,
    params: {
      entries: Array.from({ length: 30 }, (_, index) => `P${index + 1}`),
      entryCount,
      quality: 80,
      qualityLabel: '1080P',
      codec: 'avc',
      audio: 'original',
      outputDir: '/tmp',
    },
    progress: {
      doneBytes: 0,
      totalBytes: 0,
      percent: 1,
      speedBps: 0,
      etaSec: null,
    },
    stages: [],
    outputPath: null,
    error: null,
    restored: false,
    createdAt: 0,
    updatedAt: 0,
  };
}

describe('taskSubline entry count', () => {
  beforeEach(() => {
    vi.stubGlobal('document', {
      documentElement: { dataset: { os: 'macos' } },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows the selected count instead of the full multi-part catalogue', () => {
    expect(taskSubline(multiPartTask(2))).toContain('· 2 项');
    expect(taskSubline(multiPartTask(2))).not.toContain('30 项');
  });

  it('hides the count for a single entry and shows it for a full selection', () => {
    expect(taskSubline(multiPartTask(1))).not.toContain('项');
    expect(taskSubline(multiPartTask(30))).toContain('· 30 项');
  });
});
