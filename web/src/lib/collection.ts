/**
 * UGC 合集（仅保留「整季下载」）：视频解析成功后 resolve_ugc_season，
 * 多集时由 actions.ts 弹 ConfirmDialog，整个合集 → ParseItem[] 进 AddPanel。
 * shell command 名与 DTO 字段保持不变。
 */
import { invoke } from '@tauri-apps/api/core';
import { AUDIO_FORMATS, AUDIO_QUALITY_TIERS, QUALITY_TIERS, VIDEO_CODECS } from './constants';
import type { ParseItem, ParseResult, QualityOption } from './types';
import { devParams } from '../dev/scenario';

export interface UgcEpisode {
  bvid: string;
  aid?: number;
  cid: number;
  title: string;
  cover?: string | null;
  duration?: number | null;
  sectionTitle?: string;
  index: number;
  isCurrent: boolean;
}

export interface UgcSeason {
  seasonId: number;
  title: string;
  coverUrl?: string | null;
  intro?: string;
  upper: string;
  currentIndex: number;
  episodes: UgcEpisode[];
}

const COVERS = Array.from({ length: 6 }, (_, i) => `/covers/cover-0${i + 1}.svg`);

function isTauri(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

/** mock 合集（QA 截图与本地自测；真实模式由 shell 返回） */
export function mockUgcSeason(): UgcSeason {
  return {
    seasonId: 1001,
    title: '冰岛环岛 4K 旅拍全记录',
    coverUrl: COVERS[0],
    intro: '合集：极地影像的冰岛旅行系列',
    upper: '极地影像',
    currentIndex: 1,
    episodes: Array.from({ length: 6 }, (_, index) => ({
      bvid: `BV1mockSeason${index + 1}`,
      cid: 9000 + index,
      title: `第 ${String(index + 1).padStart(2, '0')} 集 · ${['出发', '冰川', '黑沙滩', '极光', '高地', '归途'][index]}`,
      cover: COVERS[index % COVERS.length],
      duration: 900 + index * 120,
      sectionTitle: '正片',
      index: index + 1,
      isCurrent: index === 0,
    })),
  };
}

export async function resolveUgcSeason(bvid: string): Promise<UgcSeason | null> {
  if (isTauri()) {
    try {
      return await invoke<UgcSeason | null>('resolve_ugc_season', { bvid });
    } catch {
      // shell 未就绪/非合集时保持普通单视频流程
      return null;
    }
  }
  if (devParams().scenario !== 'season-confirm' && !/SEASON/i.test(bvid)) return null;
  return mockUgcSeason();
}

const QUALITY_OPTIONS: QualityOption[] = QUALITY_TIERS.map((tier) => ({
  ...tier,
  // 合成 parse 结果不虚构会员能力
  available: !tier.vip,
}));

/** UGC 合集 episodes → ParseItem[]（每个视频一条，沿用 AddPanel 勾选/清晰度） */
export function seasonToParseResult(season: UgcSeason): ParseResult {
  const items: ParseItem[] = season.episodes.map((episode) => ({
    id: episode.bvid,
    rawInput: `https://www.bilibili.com/video/${episode.bvid}`,
    kind: 'video',
    title: episode.title,
    uploader: season.upper,
    coverUrl: episode.cover ?? season.coverUrl ?? null,
    pageUrl: `https://www.bilibili.com/video/${episode.bvid}`,
    durationSec: episode.duration ?? null,
    publishedAt: null,
    entries: [
      {
        id: episode.bvid,
        title: episode.title,
        durationSec: episode.duration ?? null,
        sizeBytes: null,
        selectedByDefault: true,
      },
    ],
    qualities: QUALITY_OPTIONS,
    codecs: VIDEO_CODECS,
    audioFormats: AUDIO_FORMATS,
    audioQualities: AUDIO_QUALITY_TIERS,
  }));
  return { items, failures: [], requiresConfirmation: true };
}
