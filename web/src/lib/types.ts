/** 领域类型（与 Rust core 的序列化结构对齐；mock/真实 IPC 共用） */
import type { MotionPreference } from './motion';

export type ContentKind = 'video' | 'bangumi' | 'course' | 'audio';

export type TaskPhase = 'resolve' | 'video' | 'audio' | 'merge' | 'postprocess';

export type TaskStatus =
  | 'queued'
  | 'resolving'
  | 'downloading'
  | 'merging'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'cancelled';

export type CodecId = 'avc' | 'hevc' | 'av1';
export type AudioFormatId = 'original' | 'mp3';

export interface QualityOption {
  /** qn 值（接口真实返回，不虚构档位） */
  id: number;
  label: string;
  resolution: string;
  fps: number;
  /** 需大会员 */
  vip: boolean;
  /** 需付费/区域限制等不可用原因 */
  unavailableReason?: string;
  available: boolean;
}

export interface CodecOption {
  id: CodecId;
  label: string;
  available: boolean;
}

export interface AudioFormatOption {
  id: AudioFormatId;
  label: string;
  available: boolean;
  hint?: string;
}

export type ErrorKind =
  | 'risk'
  | 'auth'
  | 'vip'
  | 'paid'
  | 'region'
  | 'network'
  | 'cdn'
  | 'ffmpeg'
  | 'disk'
  | 'parse'
  | 'unknown';

export type ErrorAction =
  | 'retry'
  | 'relogin'
  | 'open-settings'
  | 'copy-diagnostics'
  | 'open-log'
  | 'choose-dir'
  | 'install-ffmpeg';

export interface TaskError {
  kind: ErrorKind;
  message: string;
  hint?: string;
  detail?: string;
  actions: ErrorAction[];
}

export interface TaskStage {
  phase: TaskPhase;
  label: string;
  status: 'pending' | 'active' | 'done' | 'failed' | 'skipped';
  doneBytes: number;
  totalBytes: number;
}

export interface TaskParams {
  /** 实际选择的条目（分 P / 剧集 / 曲目 / 单篇） */
  entries: string[];
  entryCount: number;
  quality: number;
  qualityLabel: string;
  codec: CodecId;
  audio: AudioFormatId;
  outputDir: string;
  note?: string;
}

export interface TaskProgress {
  doneBytes: number;
  totalBytes: number;
  percent: number;
  speedBps: number;
  etaSec: number | null;
}

/** 下载产物（只读展示；与 core TaskExtras / shell TaskExtrasDTO 对齐） */
export interface TaskExtras {
  /** 已下载字幕文件（绝对路径/文件名） */
  subtitles: string[];
  /** 封面文件路径 */
  cover: string | null;
  /** 字幕后处理失败原因（非致命） */
  subtitleError?: string | null;
  /** 封面后处理失败原因（非致命） */
  coverError?: string | null;
}

export type ArtifactKind = 'media' | 'video-stream' | 'audio-stream';
export type ArtifactCoverage = 'complete' | 'legacy-single';

export interface TaskArtifact {
  path: string;
  size: number;
  entryIndex: number | null;
  entryTitle: string | null;
  kind: ArtifactKind;
  exists: boolean;
  sizeMatches: boolean;
  valid: boolean;
}

export interface QueueTask {
  id: string;
  status: TaskStatus;
  phase: TaskPhase | null;
  kind: ContentKind;
  sourceId: string;
  title: string;
  uploader: string;
  coverUrl: string | null;
  pageUrl: string;
  durationSec: number | null;
  params: TaskParams;
  progress: TaskProgress;
  stages: TaskStage[];
  outputPath: string | null;
  artifacts?: TaskArtifact[];
  artifactCoverage?: ArtifactCoverage;
  /** 下载产物（可选；真实模式待 contract/adapt 接线） */
  extras?: TaskExtras;
  error: TaskError | null;
  /** 从上次退出恢复 */
  restored: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface QueueStats {
  total: number;
  /** 解析中/下载中/合并中 */
  downloading: number;
  queued: number;
  paused: number;
  failed: number;
  completed: number;
  totalSpeedBps: number;
}

export interface AccountState {
  signedIn: boolean;
  /** 恢复的 cookie 已失效 */
  expired: boolean;
  mid: number | null;
  name: string | null;
  avatarUrl: string | null;
  vip: 'none' | 'monthly' | 'annual';
  /** 能力说明：未登录 480P，登录 1080P，大会员 4K */
  capability: { anonymousMax: number; signedInMax: number; vipMax: number };
}

export interface QueueSnapshot {
  tasks: QueueTask[];
  stats: QueueStats;
  account: AccountState;
}

export type HistoryStatus = 'completed' | 'failed' | 'cancelled';

export interface HistoryEntry {
  id: string;
  taskId: string | null;
  title: string;
  uploader: string;
  coverUrl: string | null;
  kind: ContentKind;
  qualityLabel: string;
  outputPath: string;
  fileSize: number;
  artifacts?: TaskArtifact[];
  artifactCoverage?: ArtifactCoverage;
  status: HistoryStatus;
  error: string | null;
  finishedAt: number;
  pageUrl: string;
  /** 文件是否仍存在（外部移动/删除后为 false） */
  fileExists: boolean;
}

export interface HistoryQuery {
  keyword: string;
  status: HistoryStatus | 'all';
}

export type HistoryClearPolicy = 'never' | 'count';

export interface GeneralSettings {
  launchAtLogin: boolean;
  notifyOnComplete: boolean;
  soundOnComplete: boolean;
  revealInFinderOnComplete: boolean;
  keepQueueOnExit: boolean;
  historyLimit: number;
  historyClearPolicy: HistoryClearPolicy;
}

export type Appearance = 'system' | 'light' | 'dark';

export interface InterfaceSettings {
  appearance: Appearance;
  dockBadge: boolean;
  reduceMotion: boolean;
  showSpeedInStatus: boolean;
  /** 磨砂玻璃背景（原生 vibrancy 透出；持久化在设置） */
  glassBackground: boolean;
  /** 强调色（#RRGGBB；缺省 #3B82F6） */
  accentColor?: string;
  /** 动态效果偏好（system 默认 / reduce / normal） */
  motion?: MotionPreference;
}

export interface DownloadSettings {
  outputDir: string;
  /** 占位：SettingsDTO 字段待 Lead 接线（downloadSubtitle/downloadCover/checkUpdatesOnStartup） */
  downloadSubtitle?: boolean;
  downloadCover?: boolean;
  checkUpdatesOnStartup?: boolean;
  maxConcurrent: number;
  parseConcurrency: number;
  clipboardWatch: boolean;
}

export type QualityStrategy = 'best' | 'fixed';
export type CodecPreference = 'auto' | CodecId;
export type AudioFormatPreference = 'original' | 'mp3';

export interface QualitySettings {
  strategy: QualityStrategy;
  fixedQuality: number;
  codecPreference: CodecPreference;
  audioFormat: AudioFormatPreference;
  /** 指定档位不可用时是否自动回退 */
  fallback: boolean;
  writeMetadata: boolean;
  ffmpegPath: string;
  ffmpegDetected: boolean;
  ffmpegVersion: string | null;
}

export interface NetworkSettings {
  proxyMode: 'direct' | 'manual';
  proxyHost: string;
  proxyPort: number;
  proxyUser: string;
  proxyPass: string;
}

export interface Settings {
  general: GeneralSettings;
  interface: InterfaceSettings;
  download: DownloadSettings;
  quality: QualitySettings;
  network: NetworkSettings;
}

export type SettingsPatch = {
  [K in keyof Settings]?: Partial<Settings[K]>;
};

/** 添加面板确认后的入队选择（UI → add_tasks 参数） */
export interface EnqueueSelection {
  itemIds: string[];
  entries: Record<string, string[]>;
  quality: Record<string, number>;
  codec: Record<string, 'avc' | 'hevc' | 'av1'>;
  audio: Record<string, 'original' | 'mp3'>;
  /** 可选音质 qn（30250/30251...；不选 = 自动最高） */
  audioQn?: Record<string, number>;
  outputDir: string;
  note?: string;
}

export interface SettingsValidationIssue {
  path: string;
  message: string;
}

export interface ParseEntry {
  id: string;
  title: string;
  durationSec: number | null;
  sizeBytes: number | null;
  selectedByDefault: boolean;
  unavailableReason?: string;
}

export interface AudioQualityOption {
  id: number;
  label: string;
}

export interface ParseItem {
  id: string;
  /** 原始输入（add_tasks / 重新设置用） */
  rawInput?: string;
  kind: ContentKind;
  title: string;
  uploader: string;
  coverUrl: string | null;
  pageUrl: string;
  durationSec: number | null;
  publishedAt: number | null;
  /** 默认选择语义：带 ?p=N 只选该分 P，整稿/整季默认全选 */
  entries: ParseEntry[];
  qualities: QualityOption[];
  codecs: CodecOption[];
  audioFormats: AudioFormatOption[];
  /** 可选音质（DASH 音轨；空 = 不展示音质选择） */
  audioQualities: AudioQualityOption[];
}

export interface ParseFailure {
  raw: string;
  reason: string;
}

export interface ParseResult {
  items: ParseItem[];
  failures: ParseFailure[];
  /** 需要确认面板（多条目 / 多链接 / 设置要求确认） */
  requiresConfirmation: boolean;
}

export type LoginState = 'idle' | 'waiting' | 'scanned' | 'success' | 'expired' | 'failed';

export interface LoginChallenge {
  sessionId: string;
  /** 二维码模块矩阵（true=深色模块） */
  modules: boolean[][];
  expiresInSec: number;
  state: LoginState;
}

export interface LoginPollResult {
  state: LoginState;
  message: string;
  account: AccountState | null;
}

export type NoticeKind = 'info' | 'success' | 'warning' | 'error';

export interface AppNotice {
  id: string;
  kind: NoticeKind;
  message: string;
  actionLabel?: string;
  actionCommand?: string;
}

/** Rust/TS 共用事件名（design D8） */
export const EVENTS = {
  queueSnapshot: 'queue://snapshot',
  queueEvents: 'queue://events',
  accountChanged: 'account://changed',
  settingsChanged: 'settings://changed',
  historyChanged: 'history://changed',
  appNotice: 'app://notice',
} as const;

export type QueueEvent =
  | { type: 'task-updated'; task: QueueTask }
  | { type: 'task-removed'; taskId: string }
  | { type: 'toast'; notice: AppNotice };
