/** Mock 数据（离线、确定性；封面/SVG 均为项目自绘） */
import { AUDIO_FORMATS, AUDIO_QUALITY_TIERS, QUALITY_TIERS, VIDEO_CODECS } from '../lib/constants';
import type {
  AccountState,
  HistoryEntry,
  LoginChallenge,
  ParseItem,
  ParseResult,
  QualityOption,
  QueueTask,
  Settings,
  TaskError,
  TaskPhase,
  TaskStage,
  TaskStatus,
} from '../lib/types';

export const COVERS = [
  '/covers/cover-01.svg',
  '/covers/cover-02.svg',
  '/covers/cover-03.svg',
  '/covers/cover-04.svg',
  '/covers/cover-05.svg',
  '/covers/cover-06.svg',
];

export const AVATAR = '/avatars/avatar-01.svg';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const MIN = 60_000;
export const NOW = Date.now();

export { QUALITY_TIERS } from '../lib/constants';

export function qualityOptions(opts: { loggedIn?: boolean; vipAllowed?: boolean } = {}): QualityOption[] {
  const { loggedIn = true, vipAllowed = false } = opts;
  return QUALITY_TIERS.map((tier) => {
    if (!loggedIn && tier.id > 32) {
      return {
        ...tier,
        available: false,
        unavailableReason: '登录后可解锁',
      };
    }
    if (tier.vip && !vipAllowed) {
      return { ...tier, available: false, unavailableReason: '需大会员' };
    }
    return { ...tier, available: true };
  });
}

export { VIDEO_CODECS, AUDIO_FORMATS } from '../lib/constants';

export const ANONYMOUS_ACCOUNT: AccountState = {
  signedIn: false,
  expired: false,
  mid: null,
  name: null,
  avatarUrl: null,
  vip: 'none',
  capability: { anonymousMax: 480, signedInMax: 1080, vipMax: 2160 },
};

export const SIGNED_IN_ACCOUNT: AccountState = {
  signedIn: true,
  expired: false,
  mid: 38472911,
  name: '示例用户',
  avatarUrl: AVATAR,
  vip: 'none',
  capability: { anonymousMax: 480, signedInMax: 1080, vipMax: 2160 },
};

export const VIP_ACCOUNT: AccountState = {
  ...SIGNED_IN_ACCOUNT,
  vip: 'annual',
};

export const EXPIRED_ACCOUNT: AccountState = {
  signedIn: false,
  expired: true,
  mid: 38472911,
  name: '示例用户',
  avatarUrl: AVATAR,
  vip: 'none',
  capability: { anonymousMax: 480, signedInMax: 1080, vipMax: 2160 },
};

export function defaultSettings(theme: 'light' | 'dark' | null): Settings {
  return {
    general: {
      launchAtLogin: false,
      notifyOnComplete: true,
      soundOnComplete: true,
      revealInFinderOnComplete: true,
      keepQueueOnExit: true,
      historyLimit: 500,
      historyClearPolicy: 'count',
    },
    interface: {
      appearance: theme ?? 'system',
      dockBadge: true,
      reduceMotion: false,
      showSpeedInStatus: true,
      glassBackground: true,
    },
    download: {
      outputDir: '~/Downloads/bilidown',
      maxConcurrent: 3,
      parseConcurrency: 2,
      clipboardWatch: false,
    },
    quality: {
      strategy: 'best',
      fixedQuality: 80,
      codecPreference: 'avc',
      audioFormat: 'original',
      fallback: true,
      writeMetadata: true,
      ffmpegPath: '/opt/homebrew/bin/ffmpeg',
      ffmpegDetected: true,
      ffmpegVersion: '7.1.1',
    },
    network: {
      proxyMode: 'direct',
      proxyHost: '',
      proxyPort: 7890,
      proxyUser: '',
      proxyPass: '',
      updateProxyMode: 'system',
      updateProxyHost: '',
      updateProxyPort: 7890,
      updateProxyUser: '',
      updateProxyPass: '',
    },
  };
}

const PHASE_ORDER: TaskPhase[] = ['resolve', 'video', 'audio', 'merge', 'postprocess'];

function makeStages(
  status: TaskStatus,
  percent: number,
  failedPhase: TaskPhase = 'video',
): TaskStage[] {
  const total = 1_000;
  const done = Math.round(total * percent);
  return PHASE_ORDER.map((phase, index): TaskStage => {
    let stageStatus: TaskStage['status'] = 'pending';
    if (status === 'completed') {
      stageStatus = phase === 'postprocess' ? 'skipped' : 'done';
    } else if (status === 'cancelled') {
      stageStatus = index < 2 ? 'done' : 'skipped';
    } else if (status === 'failed') {
      const failIndex = PHASE_ORDER.indexOf(failedPhase);
      stageStatus = index < failIndex ? 'done' : index === failIndex ? 'failed' : 'pending';
    } else if (status === 'queued') {
      stageStatus = 'pending';
    } else if (status === 'resolving') {
      stageStatus = phase === 'resolve' ? 'active' : 'pending';
    } else if (status === 'merging') {
      stageStatus = index < 3 ? 'done' : phase === 'merge' ? 'active' : 'pending';
    } else if (status === 'paused' || status === 'downloading') {
      stageStatus = index < 1 ? 'done' : index < 3 ? 'active' : 'pending';
    }
    const stageDone = stageStatus === 'done' ? total : stageStatus === 'active' ? done : 0;
    return {
      phase,
      label: phase,
      status: stageStatus,
      doneBytes: Math.round((stageDone / total) * 1.2 * GB),
      totalBytes: 1.2 * GB,
    };
  });
}

export interface TaskSeed {
  id: string;
  title: string;
  uploader: string;
  kind: QueueTask['kind'];
  coverIndex: number;
  status: TaskStatus;
  percent: number;
  quality?: string;
  totalBytes?: number;
  speedBps?: number;
  etaSec?: number | null;
  restored?: boolean;
  entryCount?: number;
  entryLabel?: string;
  outputPath?: string | null;
  error?: TaskError | null;
  ageMinutes?: number;
}

export function makeTask(seed: TaskSeed): QueueTask {
  const {
    id,
    title,
    uploader,
    kind,
    coverIndex,
    status,
    percent,
    quality = '1080P',
    totalBytes = 1.2 * GB,
    speedBps = status === 'downloading' ? 6.2 * MB : status === 'merging' ? 0 : 0,
    etaSec = status === 'downloading' ? 212 : null,
    restored = false,
    entryCount = 1,
    entryLabel = 'P1',
    outputPath = null,
    error = null,
    ageMinutes = 0,
  } = seed;
  const doneBytes = Math.round(totalBytes * percent);
  const created = NOW - ageMinutes * MIN;
  return {
    id,
    status,
    phase:
      status === 'downloading'
        ? 'video'
        : status === 'merging'
          ? 'merge'
          : status === 'resolving'
            ? 'resolve'
            : status === 'completed'
              ? 'postprocess'
              : status === 'failed'
                ? 'video'
                : null,
    kind,
    sourceId: id.toUpperCase(),
    title,
    uploader,
    coverUrl: COVERS[coverIndex % COVERS.length],
    pageUrl: 'https://www.bilibili.com/video/BV1mock0001',
    durationSec: 1122,
    params: {
      entries: entryLabel ? [entryLabel] : [],
      entryCount,
      quality: quality === '1080P' ? 80 : 64,
      qualityLabel: quality,
      codec: 'avc',
      audio: 'original',
      outputDir: '~/Downloads/bilidown',
    },
    progress: {
      doneBytes,
      totalBytes,
      percent: status === 'completed' ? 1 : percent,
      speedBps,
      etaSec,
    },
    stages: makeStages(status, percent, 'video'),
    outputPath,
    error,
    restored,
    createdAt: created,
    updatedAt: created + 5 * MIN,
  };
}

export const ERR_AUTH: TaskError = {
  kind: 'auth',
  message: '登录已失效，请重新扫码',
  hint: '重新登录后可继续下载 1080P 及以上清晰度',
  detail:
    'playurl 返回 -101：账号未登录\nrequest: GET /x/player/playurl?bvid=BV1mock0001&qn=80\nat video_stream.rs:214',
  actions: ['relogin', 'retry'],
};

export const ERR_FFMPEG: TaskError = {
  kind: 'ffmpeg',
  message: '找不到 ffmpeg，无法合并音视频流',
  hint: '安装 ffmpeg 或在偏好设置中指定可执行文件路径',
  detail: 'spawn ffmpeg: No such file or directory (os error 2)\nat merge.rs:88',
  actions: ['install-ffmpeg', 'open-settings', 'retry'],
};

export const ERR_RISK: TaskError = {
  kind: 'risk',
  message: '触发 B 站风控，请稍后重试或先登录',
  hint: '请等待几分钟后再试，避免连续请求',
  detail: 'playurl 返回 -352\nrequest: GET /x/player/wbi/playurl',
  actions: ['retry', 'relogin'],
};

/** 主窗口混合队列（7 行，覆盖常见状态） */
export function queueTasks(): QueueTask[] {
  return [
    makeTask({
      id: 'task-dl',
      title: '「星海」冰岛环岛 4K 旅拍全记录',
      uploader: '极地影像',
      kind: 'video',
      coverIndex: 0,
      status: 'downloading',
      percent: 0.452,
      totalBytes: 1.24 * GB,
      speedBps: 6.2 * MB,
      etaSec: 212,
      quality: '1080P',
      ageMinutes: 3,
    }),
    makeTask({
      id: 'task-merge',
      title: '《送葬者》第 28 话 苍月之下',
      uploader: 'Muse 木棉说',
      kind: 'bangumi',
      coverIndex: 1,
      status: 'merging',
      percent: 0.92,
      totalBytes: 486 * MB,
      quality: '1080P',
      ageMinutes: 8,
    }),
    makeTask({
      id: 'task-pause',
      title: '【Hi-Res】钢琴改编 · 夜航西飞',
      uploader: '白键计划',
      kind: 'audio',
      coverIndex: 2,
      status: 'paused',
      percent: 0.62,
      totalBytes: 268 * MB,
      quality: '无损',
      ageMinutes: 14,
    }),
    makeTask({
      id: 'task-queue',
      title: '现代前端工程化 · 第 03 讲 构建流水线',
      uploader: '比特学院',
      kind: 'course',
      coverIndex: 3,
      status: 'queued',
      percent: 0,
      totalBytes: 640 * MB,
      quality: '720P',
      ageMinutes: 16,
    }),
    makeTask({
      id: 'task-fail',
      title: '「纳塔」交响音乐会 现场实录',
      uploader: '米游社音乐',
      kind: 'video',
      coverIndex: 4,
      status: 'failed',
      percent: 0.18,
      totalBytes: 980 * MB,
      quality: '1080P',
      error: ERR_AUTH,
      ageMinutes: 21,
    }),
    makeTask({
      id: 'task-done',
      title: '【1080P】城市夜景延时摄影合集',
      uploader: '光影笔记',
      kind: 'video',
      coverIndex: 5,
      status: 'completed',
      percent: 1,
      totalBytes: 738 * MB,
      quality: '1080P',
      outputPath: '~/Downloads/bilidown/城市夜景延时摄影合集[1080P].mp4',
      ageMinutes: 34,
    }),
    makeTask({
      id: 'task-cancel',
      title: '纪录片：深海之下 第 1 集',
      uploader: '蓝海纪实',
      kind: 'video',
      coverIndex: 0,
      status: 'cancelled',
      percent: 0.37,
      totalBytes: 1.1 * GB,
      quality: '720P',
      ageMinutes: 42,
    }),
  ];
}

/** 九态矩阵（行列固定顺序，供 /gallery 与截图使用） */
export function statesTasks(): QueueTask[] {
  return [
    makeTask({
      id: 'state-downloading',
      title: '下载中：城市夜景延时摄影 4K',
      uploader: '光影笔记',
      kind: 'video',
      coverIndex: 5,
      status: 'downloading',
      percent: 0.452,
      totalBytes: 1.24 * GB,
      speedBps: 6.2 * MB,
      etaSec: 212,
    }),
    makeTask({
      id: 'state-resolving',
      title: '解析中：番剧《送葬者》第 28 话',
      uploader: 'Muse 木棉说',
      kind: 'bangumi',
      coverIndex: 1,
      status: 'resolving',
      percent: 0.04,
      totalBytes: 486 * MB,
    }),
    makeTask({
      id: 'state-merging',
      title: '合并中：现代前端工程化 第 03 讲',
      uploader: '比特学院',
      kind: 'course',
      coverIndex: 3,
      status: 'merging',
      percent: 0.92,
      totalBytes: 640 * MB,
      quality: '720P',
    }),
    makeTask({
      id: 'state-paused',
      title: '已暂停：【Hi-Res】夜航西飞',
      uploader: '白键计划',
      kind: 'audio',
      coverIndex: 2,
      status: 'paused',
      percent: 0.62,
      totalBytes: 268 * MB,
      quality: '无损',
    }),
    makeTask({
      id: 'state-queued',
      title: '排队中：「星海」冰岛环岛旅拍',
      uploader: '极地影像',
      kind: 'video',
      coverIndex: 0,
      status: 'queued',
      percent: 0,
      totalBytes: 1.24 * GB,
    }),
    makeTask({
      id: 'state-completed',
      title: '已完成：城市夜景延时摄影合集',
      uploader: '光影笔记',
      kind: 'video',
      coverIndex: 5,
      status: 'completed',
      percent: 1,
      totalBytes: 738 * MB,
      outputPath: '~/Downloads/bilidown/城市夜景延时摄影合集[1080P].mp4',
    }),
    makeTask({
      id: 'state-failed',
      title: '失败：「纳塔」交响音乐会 现场实录',
      uploader: '米游社音乐',
      kind: 'video',
      coverIndex: 4,
      status: 'failed',
      percent: 0.18,
      totalBytes: 980 * MB,
      error: ERR_AUTH,
    }),
    makeTask({
      id: 'state-cancelled',
      title: '已取消：纪录片 深海之下 第 1 集',
      uploader: '蓝海纪实',
      kind: 'video',
      coverIndex: 0,
      status: 'cancelled',
      percent: 0.37,
      totalBytes: 1.1 * GB,
      quality: '720P',
    }),
    makeTask({
      id: 'state-restored',
      title: '恢复：番剧《送葬者》第 27 话',
      uploader: 'Muse 木棉说',
      kind: 'bangumi',
      coverIndex: 1,
      status: 'paused',
      percent: 0.73,
      totalBytes: 452 * MB,
      restored: true,
    }),
  ];
}

const BULK_TITLES = [
  '「星海」冰岛环岛 4K 旅拍全记录',
  '《送葬者》第 28 话 苍月之下',
  '【Hi-Res】钢琴改编 · 夜航西飞',
  '现代前端工程化 · 构建流水线',
  '「纳塔」交响音乐会 现场实录',
  '【1080P】城市夜景延时摄影合集',
  '深海之下 第 1 集',
];

export function bulkTasks(count = 500): QueueTask[] {
  const out: QueueTask[] = [];
  let seed = 20261029;
  const rand = () => {
    seed = (seed * 48271) % 2147483647;
    return seed / 2147483647;
  };
  const statuses: TaskStatus[] = [
    'downloading',
    'queued',
    'paused',
    'completed',
    'failed',
    'merging',
    'resolving',
    'cancelled',
  ];
  for (let i = 0; i < count; i++) {
    const status = statuses[i % statuses.length];
    out.push(
      makeTask({
        id: `bulk-${i}`,
        title: `批量任务 ${i + 1}：${BULK_TITLES[i % BULK_TITLES.length]}`,
        uploader: ['极地影像', 'Muse 木棉说', '白键计划', '比特学院'][i % 4],
        kind: (['video', 'bangumi', 'audio', 'course'] as const)[i % 4],
        coverIndex: i % COVERS.length,
        status,
        percent: status === 'completed' ? 1 : rand(),
        totalBytes: (300 + rand() * 900) * MB,
        speedBps: status === 'downloading' ? 4 * MB : 0,
        quality: ['1080P', '720P', '4K'][i % 3],
        ageMinutes: i * 3,
      }),
    );
  }
  return out;
}

export function historyEntries(): HistoryEntry[] {
  const day = 86_400_000;
  const mk = (
    id: string,
    title: string,
    uploader: string,
    kind: HistoryEntry['kind'],
    coverIndex: number,
    status: HistoryEntry['status'],
    finishedAt: number,
    sizeMB: number,
    quality = '1080P',
    fileExists = true,
    error: string | null = null,
  ): HistoryEntry => ({
    id,
    taskId: null,
    title,
    uploader,
    coverUrl: COVERS[coverIndex % COVERS.length],
    kind,
    qualityLabel: quality,
    outputPath: `~/Downloads/bilidown/${title}[${quality}].${kind === 'audio' ? 'flac' : 'mp4'}`,
    fileSize: Math.round(sizeMB * MB),
    status,
    error,
    finishedAt,
    pageUrl: 'https://www.bilibili.com/video/BV1mock0001',
    fileExists,
  });
  const today = new Date(NOW);
  today.setHours(14, 5, 0, 0);
  const t0 = today.getTime();
  return [
    mk('h-1', '城市夜景延时摄影合集', '光影笔记', 'video', 5, 'completed', t0, 738),
    mk('h-2', '夜航西飞 · 钢琴改编', '白键计划', 'audio', 2, 'completed', t0 - 42 * MIN, 268, '无损'),
    mk('h-3', '「纳塔」交响音乐会 现场实录', '米游社音乐', 'video', 4, 'failed', t0 - 90 * MIN, 512, '1080P', true, '登录已失效'),
    mk('h-4', '现代前端工程化 第 03 讲', '比特学院', 'course', 3, 'completed', t0 - 5 * 60 * MIN, 640, '720P'),
    mk('h-5', '深海之下 第 1 集', '蓝海纪实', 'video', 0, 'cancelled', t0 - 6 * 60 * MIN, 212, '720P'),
    mk('h-6', '送葬者 第 28 话', 'Muse 木棉说', 'bangumi', 1, 'completed', t0 - 7 * 60 * MIN, 486),
    mk('h-7', '冰岛环岛 4K 旅拍', '极地影像', 'video', 0, 'completed', t0 - day - 60 * MIN, 1240, '4K'),
    mk('h-9', '城市夜景延时摄影合集', '光影笔记', 'video', 5, 'failed', t0 - day - 5 * 60 * MIN, 300, '1080P', true, 'CDN 拒绝访问'),
    mk('h-10', '夜航西飞 · 钢琴改编', '白键计划', 'audio', 2, 'completed', t0 - 3 * day - 2 * 60 * MIN, 268, '无损', false),
    mk('h-11', '前端工程化 第 02 讲', '比特学院', 'course', 3, 'completed', t0 - 3 * day - 4 * 60 * MIN, 588, '720P'),
    mk('h-12', '深海之下 第 2 集', '蓝海纪实', 'video', 0, 'cancelled', t0 - 4 * day, 1024),
  ];
}

export function addPanelParseResult(): ParseResult {
  const seasonEntries = Array.from({ length: 12 }, (_, i) => ({
    id: `ep-${i + 1}`,
    title: `第 ${String(i + 1).padStart(2, '0')} 话 ${[
      '孤独的转校生',
      '第一次合宿',
      '海边与吉他',
      '文化祭前夜',
      '雨天练习',
      '初次 Live',
      '误会与和解',
      '深夜的编曲',
      '夏日祭',
      '修学旅行',
      '决赛前',
      '尾声',
    ][i]}`,
    durationSec: 1420 + i * 30,
    sizeBytes: (420 + i * 8) * MB,
    selectedByDefault: true,
  }));
  const bangumi: ParseItem = {
    id: 'parse-bangumi',
    kind: 'bangumi',
    title: '《孤独摇滚！》全 12 话',
    uploader: '孤独摇滚制作委员会',
    coverUrl: COVERS[1],
    pageUrl: 'https://www.bilibili.com/bangumi/play/ss41410',
    durationSec: seasonEntries.reduce((sum, e) => sum + (e.durationSec ?? 0), 0),
    publishedAt: NOW - 400 * 86_400_000,
    entries: seasonEntries,
    qualities: qualityOptions({ loggedIn: true, vipAllowed: false }),
    codecs: VIDEO_CODECS,
    audioFormats: AUDIO_FORMATS,
    audioQualities: AUDIO_QUALITY_TIERS,
  };
  const audio: ParseItem = {
    id: 'parse-audio',
    kind: 'audio',
    title: '夜航西飞 · 钢琴改编合集（Hi-Res）',
    uploader: '白键计划',
    coverUrl: COVERS[2],
    pageUrl: 'https://www.bilibili.com/audio/au123456',
    durationSec: 268,
    publishedAt: NOW - 20 * 86_400_000,
    entries: [
      { id: 'au-1', title: '夜航西飞', durationSec: 268, sizeBytes: 58 * MB, selectedByDefault: true },
      { id: 'au-2', title: '候鸟', durationSec: 221, sizeBytes: 49 * MB, selectedByDefault: true },
      { id: 'au-3', title: '归途', durationSec: 302, sizeBytes: 64 * MB, selectedByDefault: true },
    ],
    qualities: [],
    codecs: [],
    audioFormats: AUDIO_FORMATS,
    audioQualities: AUDIO_QUALITY_TIERS,
  };
  return {
    items: [bangumi, audio],
    failures: [
      {
        raw: 'https://www.bilibili.com/video/BV1xx411c7mD',
        reason: '视频不存在或已被删除',
      },
      {
        raw: '复制这段文字 http://example.com/not-bilibili',
        reason: '未识别到有效的 B 站链接',
      },
    ],
    requiresConfirmation: true,
  };
}

/** 二维码模块矩阵：仅用于 mock 视觉（真实扫码由 core 输出） */
export function fakeQrMatrix(size = 33, seed = 7): boolean[][] {
  const grid: boolean[][] = Array.from({ length: size }, () => Array<boolean>(size).fill(false));
  const finder = (row: number, col: number) => {
    for (let r = 0; r < 7; r++) {
      for (let c = 0; c < 7; c++) {
        const on =
          r === 0 || r === 6 || c === 0 || c === 6 || (r >= 2 && r <= 4 && c >= 2 && c <= 4);
        grid[row + r][col + c] = on;
      }
    }
  };
  finder(0, 0);
  finder(0, size - 7);
  finder(size - 7, 0);
  for (let i = 8; i < size - 8; i++) {
    grid[6][i] = i % 2 === 0;
    grid[i][6] = i % 2 === 0;
  }
  let s = seed;
  const rand = () => {
    s = (s * 1103515245 + 12345) % 2147483648;
    return s / 2147483648;
  };
  for (let r = 0; r < size; r++) {
    for (let c = 0; c < size; c++) {
      const inFinder =
        (r < 8 && c < 8) || (r < 8 && c >= size - 8) || (r >= size - 8 && c < 8);
      if (inFinder || r === 6 || c === 6) continue;
      grid[r][c] = rand() > 0.52;
    }
  }
  return grid;
}

export function loginChallenge(state: LoginChallenge['state']): LoginChallenge {
  return {
    sessionId: 'mock-login-session',
    modules: fakeQrMatrix(),
    expiresInSec: state === 'expired' ? 0 : 180,
    state,
  };
}

export function errorTasks(): QueueTask[] {
  return [
    makeTask({
      id: 'err-auth',
      title: '「纳塔」交响音乐会 现场实录',
      uploader: '米游社音乐',
      kind: 'video',
      coverIndex: 4,
      status: 'failed',
      percent: 0.18,
      totalBytes: 980 * MB,
      error: ERR_AUTH,
      ageMinutes: 2,
    }),
    makeTask({
      id: 'err-ffmpeg',
      title: '城市夜景延时摄影合集',
      uploader: '光影笔记',
      kind: 'video',
      coverIndex: 5,
      status: 'failed',
      percent: 1,
      totalBytes: 738 * MB,
      error: ERR_FFMPEG,
      ageMinutes: 6,
    }),
    makeTask({
      id: 'err-risk',
      title: '番剧《送葬者》第 28 话',
      uploader: 'Muse 木棉说',
      kind: 'bangumi',
      coverIndex: 1,
      status: 'failed',
      percent: 0,
      totalBytes: 486 * MB,
      error: ERR_RISK,
      ageMinutes: 9,
    }),
    makeTask({
      id: 'err-running',
      title: '现代前端工程化 · 第 03 讲',
      uploader: '比特学院',
      kind: 'course',
      coverIndex: 3,
      status: 'downloading',
      percent: 0.31,
      totalBytes: 640 * MB,
      speedBps: 3.4 * MB,
      etaSec: 128,
      quality: '720P',
      ageMinutes: 1,
    }),
  ];
}
