import { afterEach, describe, expect, it, vi } from 'vitest';
import { requiresAddConfirmation } from '../src/lib/addSelection';
import { parseResultFromDTO } from '../src/lib/adapt';
import { api, createApiClient, type Backend } from '../src/lib/ipc';
import type { AddOptionsDTO, ParseResultDTO } from '../src/lib/contract';
import type { ParseItem, ParseResult } from '../src/lib/types';
import { useUiStore } from '../src/stores/useUiStore';

/** 只关心条目总数：默认勾选数量不参与确认判断（design D6） */
function content(entries: number, selectedByDefault = 1): { entries: unknown[] } {
  return { entries: Array.from({ length: entries }, (_, index) => index < selectedByDefault) };
}

function parseDTO(entryCount: number, selectedDefault = 1): ParseResultDTO {
  return {
    kind: 'bangumi',
    sourceId: 'ss41410',
    title: '《孤独摇滚！》',
    coverUrl: null,
    pageUrl: 'https://www.bilibili.com/bangumi/play/ss41410',
    owner: '制作委员会',
    rawInput: 'https://www.bilibili.com/bangumi/play/ss41410',
    entries: Array.from({ length: entryCount }, (_, index) => ({
      index,
      title: `第 ${index + 1} 话`,
      durationSecs: 1420,
      selectedDefault: index < selectedDefault,
    })),
  };
}

function makeItem(id: string, entryCount: number, selected: number[] = [0]): ParseItem {
  return {
    id,
    rawInput: id,
    kind: 'video',
    title: id,
    uploader: 'UP',
    coverUrl: null,
    pageUrl: `https://www.bilibili.com/video/${id}`,
    durationSec: 60,
    publishedAt: null,
    entries: Array.from({ length: entryCount }, (_, index) => ({
      id: `${id}-${index}`,
      title: `P${index + 1}`,
      durationSec: 60,
      sizeBytes: 1,
      selectedByDefault: selected.includes(index),
    })),
    qualities: [],
    codecs: [],
    audioFormats: [],
    audioQualities: [],
  };
}

function parse(items: ParseItem[]): ParseResult {
  return { items, failures: [], requiresConfirmation: requiresAddConfirmation(items) };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('添加确认判断（design D6 / tasks 6.1）', () => {
  it('普通单条目仍然快速添加，不弹确认面板', () => {
    expect(requiresAddConfirmation([content(1)])).toBe(false);
  });

  it('多 P 内容即使默认只勾选一项也必须确认', () => {
    // 默认只选 P1，但实际有 3 个分 P
    expect(requiresAddConfirmation([content(3, 1)])).toBe(true);
  });

  it('带 p=N 的多 P 链接仍按真实条目总数进入确认', () => {
    expect(requiresAddConfirmation([content(12, 1)])).toBe(true);
  });

  it('番剧/课程当前集默认只选一集也必须确认', () => {
    expect(requiresAddConfirmation([content(24, 1)])).toBe(true);
    // 空选择不会改变「条目总数」判断
    expect(requiresAddConfirmation([content(24, 0)])).toBe(true);
  });

  it('歌单多曲目即使默认只选一首也必须确认', () => {
    expect(requiresAddConfirmation([content(5, 1)])).toBe(true);
  });

  it('一次解析出多个内容（多链接）必须确认', () => {
    expect(requiresAddConfirmation([content(1), content(1)])).toBe(true);
    expect(requiresAddConfirmation([content(1), content(3, 1)])).toBe(true);
  });

  it('空选择内容不会回落默认选择', () => {
    expect(requiresAddConfirmation([content(1, 0)])).toBe(false);
  });

  it('真实 ParseResultDTO 形状：一条内容 12 条目只默认勾选 1 个仍需确认', () => {
    expect(parseResultFromDTO(parseDTO(12, 1), null).requiresConfirmation).toBe(true);
    expect(parseResultFromDTO(parseDTO(1, 1), null).requiresConfirmation).toBe(false);
    expect(parseResultFromDTO(parseDTO(3, 0), null).requiresConfirmation).toBe(true);
  });
});

describe('真实 IPC 形状的 parseLinks 确认判断', () => {
  function parseBackend(dtoFor: (input: string) => ParseResultDTO): Backend {
    return {
      getAccount: async () => null,
      parseInput: async (input: string) => dtoFor(input),
    } as unknown as Backend;
  }

  it('单条内容 12 条目、默认只勾选 1 个 → 需要确认', async () => {
    const client = createApiClient();
    client.setBackend(parseBackend(() => parseDTO(12, 1)));

    const result = await client.parseLinks('https://www.bilibili.com/bangumi/play/ss41410');

    expect(result.items).toHaveLength(1);
    expect(result.items[0].entries).toHaveLength(12);
    expect(result.requiresConfirmation).toBe(true);
  });

  it('单内容单条目 → 保持快速添加', async () => {
    const client = createApiClient();
    client.setBackend(parseBackend(() => parseDTO(1, 1)));

    const result = await client.parseLinks('https://www.bilibili.com/bangumi/play/ss41410');

    expect(result.requiresConfirmation).toBe(false);
  });

  it('多行输入多个内容 → 需要确认', async () => {
    const client = createApiClient();
    client.setBackend(
      parseBackend((input) => ({ ...parseDTO(1, 1), sourceId: input, pageUrl: input })),
    );

    const result = await client.parseLinks('https://b23.tv/a\nhttps://b23.tv/b');

    expect(result.items).toHaveLength(2);
    expect(result.requiresConfirmation).toBe(true);
  });
});

describe('多内容分别入队（design D8 / tasks 6.4）', () => {
  function captureBackend(handler: (input: string) => Promise<{ taskIds: string[]; rejected: [] }>): {
    backend: Backend;
    calls: Array<{ input: string; options: AddOptionsDTO }>;
  } {
    const calls: Array<{ input: string; options: AddOptionsDTO }> = [];
    const backend = {
      addTasks: async (input: string, options: AddOptionsDTO) => {
        calls.push({ input, options });
        return handler(input);
      },
    } as unknown as Backend;
    return { backend, calls };
  }

  const selection = {
    itemIds: ['A', 'B'],
    entries: { A: ['A-0'], B: ['B-0'] },
    quality: { A: 80, B: 80 },
    codec: { A: 'avc' as const, B: 'avc' as const },
    audio: { A: 'original' as const, B: 'original' as const },
    outputDir: '/tmp',
  };

  it('按内容拆成独立请求并保留逐内容成功/失败结果', async () => {
    const { backend, calls } = captureBackend(async (input) => {
      if (input === 'B') throw { kind: 'disk', message: '记录写入失败', actions: [] };
      return { taskIds: ['task-A'], rejected: [] };
    });
    const client = createApiClient();
    client.setBackend(backend);

    const outcome = await client.enqueue(parse([makeItem('A', 1), makeItem('B', 1)]), selection);

    expect(calls.map((call) => call.input)).toEqual(['A', 'B']);
    expect(outcome.taskIds).toEqual(['task-A']);
    expect(outcome.succeededItemIds).toEqual(['A']);
    expect(outcome.failures).toEqual([
      expect.objectContaining({ itemId: 'B', message: '记录写入失败' }),
    ]);
  });

  it('部分失败后只重试失败内容，不重复提交已成功内容', async () => {
    let failB = true;
    const { backend, calls } = captureBackend(async (input) => {
      if (input === 'B' && failB) {
        failB = false;
        throw { kind: 'disk', message: '记录写入失败', actions: [] };
      }
      return { taskIds: [`task-${input}`], rejected: [] };
    });
    const client = createApiClient();
    client.setBackend(backend);

    const first = await client.enqueue(parse([makeItem('A', 1), makeItem('B', 1)]), selection);
    expect(first.failures).toHaveLength(1);

    // 只对失败内容 B 重试
    const retry = await client.enqueue(parse([makeItem('B', 1)]), {
      ...selection,
      itemIds: ['B'],
      entries: { B: ['B-0'] },
    });

    expect(retry.failures).toHaveLength(0);
    expect(retry.taskIds).toEqual(['task-B']);
    expect(calls.map((call) => call.input)).toEqual(['A', 'B', 'B']);
  });

  it('明确清空的内容不创建任务，也不回落默认选择', async () => {
    const { backend, calls } = captureBackend(async (input) => ({
      taskIds: [`task-${input}`],
      rejected: [],
    }));
    const client = createApiClient();
    client.setBackend(backend);

    const outcome = await client.enqueue(parse([makeItem('A', 1), makeItem('B', 1)]), {
      ...selection,
      entries: { A: ['A-0'], B: [] },
    });

    expect(calls.map((call) => call.input)).toEqual(['A']);
    expect(outcome.succeededItemIds).toEqual(['A']);
  });
});

describe('快速添加入口的部分失败反馈', () => {
  async function runQuickAdd(enqueueResult: Awaited<ReturnType<typeof api.enqueue>>) {
    vi.spyOn(api, 'parseLinks').mockResolvedValue(parse([makeItem('A', 1)]));
    vi.spyOn(api, 'enqueue').mockResolvedValue(enqueueResult);
    const pushToast = vi.spyOn(useUiStore.getState(), 'pushToast');
    const { handleParseText } = await import('../src/lib/actions');
    const { useSettingsStore } = await import('../src/stores/useSettingsStore');
    const { defaultInternalSettings } = await import('../src/lib/adapt');
    useSettingsStore.setState({ settings: defaultInternalSettings() });

    const result = await handleParseText('plain-input');
    useSettingsStore.setState({ settings: null });
    return { result, pushToast };
  }

  it('只对失败内容提示可重试，不宣称全部成功', async () => {
    const { result, pushToast } = await runQuickAdd({
      taskIds: [],
      succeededItemIds: [],
      failures: [{ itemId: 'A', title: 'A', message: '记录写入失败' }],
    });

    expect(result?.items).toHaveLength(1);
    expect(pushToast).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'error', message: '记录写入失败' }),
    );
  });

  it('部分成功时提示数量准确，保留已成功结果', async () => {
    const { pushToast } = await runQuickAdd({
      taskIds: ['task-1'],
      succeededItemIds: ['A'],
      failures: [{ itemId: 'B', title: 'B', message: '第二条写入失败' }],
    });

    expect(pushToast).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: 'warning',
        message: '已添加 1 个任务，1 个内容入队失败',
      }),
    );
  });

  it('全部成功时才提示任务数成功结果', async () => {
    const { pushToast } = await runQuickAdd({
      taskIds: ['task-1'],
      succeededItemIds: ['A'],
      failures: [],
    });

    expect(pushToast).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'success', message: '已添加 1 个任务' }),
    );
  });
});
