import type { UgcSeason } from '../lib/collection';

const COVERS = Array.from({ length: 6 }, (_, i) => `/covers/cover-0${i + 1}.svg`);

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
