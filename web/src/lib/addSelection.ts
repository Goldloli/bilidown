import type { EnqueueSelection, ParseItem, ParseResult, Settings } from './types';

/**
 * 是否需要进入「添加确认」面板（design D6 / tasks 6.1）。
 *
 * 只依据「解析出的内容数」与「每项实际条目总数」判断：
 * · 单内容单条目 → 快速添加，不弹确认；
 * · 多内容（多链接 / 合集拆出的多条）→ 必须确认；
 * · 任一项实际包含多个分 P / 剧集 / 章节 / 曲目 → 必须确认。
 *
 * 特别注意：不能用默认勾选数量代替条目总数。番剧/课程默认只选当前集、
 * 多 P 链接带 `p=N` 时默认也只选一个，但解析结果实际包含多个条目，
 * 仍然必须让用户确认，且不擅自把其他条目全选。
 */
export function requiresAddConfirmation(
  items: ReadonlyArray<{ entries: ReadonlyArray<unknown> }>,
): boolean {
  return items.length > 1 || (items[0]?.entries.length ?? 0) > 1;
}

export interface DefaultSelectionIssue {
  code: 'fixed-quality-unavailable';
  message: string;
}

export interface DefaultSelectionPlan {
  selection: EnqueueSelection;
  issuesByItem: Record<string, DefaultSelectionIssue>;
}

function availableQualities(item: ParseItem) {
  return item.qualities.filter((quality) => quality.available).sort((a, b) => b.id - a.id);
}

export function resolveDefaultQuality(
  item: ParseItem,
  settings: Settings,
): { quality: number; issue?: DefaultSelectionIssue } {
  if (item.qualities.length === 0) {
    return { quality: 80 };
  }
  const available = availableQualities(item);
  if (settings.quality.strategy === 'best') {
    return { quality: available[0]?.id ?? 80 };
  }

  const fixed = item.qualities.find((quality) => quality.id === settings.quality.fixedQuality);
  if (fixed?.available) {
    return { quality: fixed.id };
  }
  if (settings.quality.fallback) {
    return { quality: available[0]?.id ?? settings.quality.fixedQuality };
  }
  return {
    quality: settings.quality.fixedQuality,
    issue: {
      code: 'fixed-quality-unavailable',
      message: `设置的清晰度当前不可用；请改选可用档位或开启自动回退`,
    },
  };
}

export function buildDefaultSelectionPlan(
  parse: ParseResult,
  settings: Settings,
): DefaultSelectionPlan {
  const itemIds = parse.items.map((item) => item.id);
  const entries: Record<string, string[]> = {};
  const quality: Record<string, number> = {};
  const codec: Record<string, 'avc' | 'hevc' | 'av1'> = {};
  const audio: Record<string, 'original' | 'mp3'> = {};
  const audioQn: Record<string, number> = {};
  const issuesByItem: Record<string, DefaultSelectionIssue> = {};

  for (const item of parse.items) {
    entries[item.id] = item.entries.filter((entry) => entry.selectedByDefault).map((entry) => entry.id);
    const resolved = resolveDefaultQuality(item, settings);
    quality[item.id] = resolved.quality;
    if (resolved.issue) issuesByItem[item.id] = resolved.issue;
    codec[item.id] =
      settings.quality.codecPreference === 'auto' ? 'avc' : settings.quality.codecPreference;
    audio[item.id] = settings.quality.audioFormat;
    audioQn[item.id] = item.audioQualities[0]?.id ?? 0;
  }

  return {
    selection: {
      itemIds,
      entries,
      quality,
      codec,
      audio,
      audioQn,
      outputDir: settings.download.outputDir,
    },
    issuesByItem,
  };
}

export function buildDefaultSelection(parse: ParseResult, settings: Settings): EnqueueSelection {
  return buildDefaultSelectionPlan(parse, settings).selection;
}
