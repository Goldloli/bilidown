import type { EnqueueSelection, ParseItem, ParseResult, Settings } from './types';

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
