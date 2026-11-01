import { afterEach, describe, expect, it, vi } from 'vitest';
import { buildDefaultSelectionPlan } from '../src/lib/addSelection';
import { defaultInternalSettings } from '../src/lib/adapt';
import { api, createApiClient, type Backend } from '../src/lib/ipc';
import { handleParseText } from '../src/lib/actions';
import type { AddOptionsDTO, AddTasksResultDTO } from '../src/lib/contract';
import type { EnqueueSelection, ParseItem, ParseResult, Settings } from '../src/lib/types';
import { useSettingsStore } from '../src/stores/useSettingsStore';
import { useUiStore } from '../src/stores/useUiStore';

function makeItem(id: string, titles: string[], selected: number[] = [0]): ParseItem {
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
    entries: titles.map((title, index) => ({
      id: `${id}-${index}`,
      title,
      durationSec: 60,
      sizeBytes: 1,
      selectedByDefault: selected.includes(index),
    })),
    qualities: [
      {
        id: 120,
        label: '4K',
        resolution: '4K',
        fps: 60,
        vip: true,
        available: true,
      },
      {
        id: 64,
        label: '720P',
        resolution: '720P',
        fps: 30,
        vip: false,
        available: true,
      },
    ],
    codecs: [
      { id: 'avc', label: 'AVC', available: true },
      { id: 'hevc', label: 'HEVC', available: true },
    ],
    audioFormats: [{ id: 'original', label: '原格式', available: true }],
    audioQualities: [{ id: 30251, label: 'FLAC' }],
  };
}

function parse(items: ParseItem[]): ParseResult {
  return { items, failures: [], requiresConfirmation: true };
}

function settingsWith(
  patch: Partial<Settings['quality']> = {},
): Settings {
  const settings = defaultInternalSettings();
  settings.quality = { ...settings.quality, ...patch };
  return settings;
}

function captureBackend(): {
  backend: Backend;
  calls: Array<{ input: string; options: AddOptionsDTO }>;
} {
  const calls: Array<{ input: string; options: AddOptionsDTO }> = [];
  const backend = {
    addTasks: async (input: string, options: AddOptionsDTO): Promise<AddTasksResultDTO> => {
      calls.push({ input, options });
      return { taskIds: [`task-${calls.length}`], rejected: [] };
    },
  } as unknown as Backend;
  return { backend, calls };
}

afterEach(() => {
  vi.restoreAllMocks();
  useSettingsStore.setState({ settings: null });
});

describe('默认添加入口', () => {
  it('固定 720P / HEVC 同时用于选择值、编码和最高音质', () => {
    const plan = buildDefaultSelectionPlan(
      parse([makeItem('BV1fixed', ['P1'])]),
      settingsWith({ strategy: 'fixed', fixedQuality: 64, codecPreference: 'hevc' }),
    );

    expect(plan.selection.quality.BV1fixed).toBe(64);
    expect(plan.selection.codec.BV1fixed).toBe('hevc');
    expect(plan.selection.audioQn?.BV1fixed).toBe(30251);
    expect(plan.issuesByItem.BV1fixed).toBeUndefined();
  });

  it('固定档位不可用且关闭回退时保留固定档并报告阻断问题', () => {
    const item = makeItem('BV1missing', ['P1']);
    item.qualities = item.qualities.filter((quality) => quality.id !== 64);
    const plan = buildDefaultSelectionPlan(
      parse([item]),
      settingsWith({ strategy: 'fixed', fixedQuality: 64, fallback: false }),
    );

    expect(plan.selection.quality.BV1missing).toBe(64);
    expect(plan.issuesByItem.BV1missing?.code).toBe('fixed-quality-unavailable');
  });

  it('固定档位不可用但开启回退时选择最高可用档', () => {
    const item = makeItem('BV1fallback', ['P1']);
    item.qualities = item.qualities.filter((quality) => quality.id !== 64);
    const plan = buildDefaultSelectionPlan(
      parse([item]),
      settingsWith({ strategy: 'fixed', fixedQuality: 64, fallback: true }),
    );

    expect(plan.selection.quality.BV1fallback).toBe(120);
    expect(plan.issuesByItem.BV1fallback).toBeUndefined();
  });

  it('无视频清晰度选项的音频不因固定视频档位被阻断', () => {
    const audio = makeItem('audio-default', ['A1']);
    audio.kind = 'audio';
    audio.qualities = [];
    const plan = buildDefaultSelectionPlan(
      parse([audio]),
      settingsWith({ strategy: 'fixed', fixedQuality: 125, fallback: false }),
    );

    expect(plan.issuesByItem[audio.id]).toBeUndefined();
  });
});

describe('入队适配器的空选择语义', () => {
  it('A 选择 1 项、B 明确清空时只为 A 调用 add_tasks', async () => {
    const a = makeItem('BV1A', ['P1']);
    const b = makeItem('BV1B', ['P1']);
    const selection: EnqueueSelection = {
      itemIds: [a.id, b.id],
      entries: {
        [a.id]: [a.entries[0].id],
        [b.id]: [],
      },
      quality: { [a.id]: 120, [b.id]: 120 },
      codec: { [a.id]: 'avc', [b.id]: 'avc' },
      audio: { [a.id]: 'original', [b.id]: 'original' },
      outputDir: '/tmp',
    };
    const { backend, calls } = captureBackend();
    const client = createApiClient();
    client.setBackend(backend);

    await client.enqueue(parse([a, b]), selection);

    expect(calls).toHaveLength(1);
    expect(calls[0].input).toBe('BV1A');
    expect(calls[0].options.entryIndexes).toEqual([0]);
  });

  it('没有 entries 键时仍保留未指定语义', async () => {
    const item = makeItem('BV1default', ['P1', 'P2']);
    const { backend, calls } = captureBackend();
    const client = createApiClient();
    client.setBackend(backend);

    await client.enqueue(parse([item]), {
      itemIds: [item.id],
      entries: {},
      quality: { [item.id]: 80 },
      codec: { [item.id]: 'avc' },
      audio: { [item.id]: 'original' },
      outputDir: '/tmp',
    });

    expect(calls).toHaveLength(1);
    expect(calls[0].options.entryIndexes).toBeUndefined();
  });
});

describe('快速添加的阻断问题', () => {
  it('固定档位不可用且关闭回退时不创建任务', async () => {
    const item = makeItem('audio-like-id', ['P1']);
    item.qualities = item.qualities.filter((quality) => quality.id !== 64);
    vi.spyOn(api, 'parseLinks').mockResolvedValue({
      ...parse([item]),
      requiresConfirmation: false,
    });
    const enqueue = vi.spyOn(api, 'enqueue');
    const toast = vi.spyOn(useUiStore.getState(), 'pushToast');
    useSettingsStore.setState({
      settings: settingsWith({ strategy: 'fixed', fixedQuality: 64, fallback: false }),
    });

    const result = await handleParseText('audio-like-id');

    expect(result).toBeNull();
    expect(enqueue).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'warning' }),
    );
  });
});
