/**
 * src-tauri IPC 契约（与 src-tauri/COMMANDS.md 对齐）
 *
 * · 命令参数与返回字段一律 camelCase；枚举值 kebab-case
 * · 标注 EXTENSION 的字段是契约表未列、但前端渲染必需的扩展，已同步给 shell-engineer/Lead
 */

export type SourceKind = 'video' | 'bangumi' | 'course' | 'audio';

export type ContractTaskStatus =
  | 'queued'
  | 'resolving'
  | 'downloading'
  | 'merging'
  | 'paused'
  | 'done'
  | 'failed'
  | 'cancelled';

export type ContractPhase = 'resolve' | 'video' | 'merge' | 'postprocess';

export type ContractTaskEventKind =
  | 'enqueued'
  | 'started'
  | 'phase-changed'
  | 'paused'
  | 'resumed'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'removed'
  | 'restored';

export type ContractLoginStatus = 'waiting' | 'scanned' | 'expired' | 'success' | 'error';

export type ContractHistoryStatus = 'completed' | 'failed' | 'cancelled';

/** invoke Err(AppError) 的 payload */
export interface AppErrorDTO {
  kind: string;
  message: string;
  actions: string[];
}

export interface TaskProgressDTO {
  bytesDone: number;
  bytesTotal: number;
  stageDone: number;
  stageTotal: number;
  percent: number;
  speedBps: number;
  etaSecs: number | null;
}

export type ArtifactCoverageDTO = 'complete' | 'legacy-single';
export type ArtifactKindDTO = 'media' | 'video-stream' | 'audio-stream';

export interface TaskArtifactDTO {
  path: string;
  size: number;
  entryIndex: number | null;
  entryTitle: string | null;
  kind: ArtifactKindDTO;
  exists: boolean;
  sizeMatches: boolean;
  valid: boolean;
}

export interface TaskDTO {
  id: string;
  status: ContractTaskStatus;
  phase: ContractPhase;
  sourceKind: SourceKind;
  sourceId: string;
  title: string;
  coverUrl: string | null;
  pageUrl: string;
  quality: string;
  outputPath: string | null;
  artifacts?: TaskArtifactDTO[];
  artifactCoverage?: ArtifactCoverageDTO;
  error: AppErrorDTO | null;
  restored: boolean;
  sortIndex: number;
  createdAt: number;
  updatedAt: number;
  progress: TaskProgressDTO;
  /** EXTENSION：契约表遗漏的展示字段 */
  owner?: string;
  entries?: string[];
  entryCount?: number;
  codec?: string;
  audioFormat?: string;
  outputDir?: string;
  durationSecs?: number | null;
  extras?: {
    subtitles: string[];
    cover: string | null;
    subtitleError: string | null;
    coverError: string | null;
  };
  /**
   * 记录保存失败的可读原因（design D8/3.8）；null/省略 = 已可靠保存。
   * 非空时界面需提供「重试保存」（retry_persistence），且不得宣称该任务已可靠记录。
   */
  persistError?: string | null;
}

/** 批量清理结果：真实移除数量 + 删除失败、仍保留在队列中的任务 id（design D2/3.5） */
export interface ClearOutcomeDTO {
  removed: number;
  failed: string[];
}

export interface QueueStatsDTO {
  total: number;
  queued: number;
  active: number;
  paused: number;
  failed: number;
  finished: number;
  cancelled: number;
  bytesDone: number;
  bytesTotal: number;
}

export interface QueueCapabilityDTO {
  anonymousMax: string;
  signedInMax: string;
  vipMax: string;
}

export interface AccountInfoDTO {
  mid: number;
  uname: string;
  avatarUrl: string | null;
  vip: string;
  level: number;
}

export interface QueueSnapshotDTO {
  revision: number;
  tasks: TaskDTO[];
  stats: QueueStatsDTO;
  account: AccountInfoDTO | null;
  capability: QueueCapabilityDTO;
  updatedAt: number;
}

export interface TaskEventDTO {
  taskId: string;
  kind: ContractTaskEventKind;
  message?: string;
  at: number;
}

export interface QualityOptionDTO {
  id: number;
  label: string;
  resolution: string;
  fps: number;
  vip: boolean;
  available: boolean;
  unavailableReason?: string;
}

export interface CodecOptionDTO {
  id: string;
  label: string;
  available: boolean;
}

export interface AudioFormatOptionDTO {
  id: string;
  label: string;
  available: boolean;
  hint?: string;
}

export interface AudioQualityOptionDTO {
  id: number;
  label: string;
}

export interface ParseEntryDTO {
  index: number;
  title: string;
  durationSecs: number | null;
  selectedDefault: boolean;
  sizeBytes?: number;
  unavailableReason?: string;
}

export interface ParseResultDTO {
  kind: SourceKind;
  sourceId: string;
  title: string;
  coverUrl: string | null;
  pageUrl: string;
  owner: string;
  entries: ParseEntryDTO[];
  rawInput: string;
  /** EXTENSION：展示与档位选择所需 */
  durationSecs?: number | null;
  publishedAt?: number | null;
  qualities?: QualityOptionDTO[];
  codecs?: CodecOptionDTO[];
  audioFormats?: AudioFormatOptionDTO[];
  audioQualities?: AudioQualityOptionDTO[];
}

export interface AddOptionsDTO {
  quality: string;
  /** 清晰度 qn（唯一真相；0/省略 = 自动） */
  qualityQn?: number;
  codec: string;
  audioFormat: string;
  /** 音质 qn（可选；透传到 TaskSpec.audio_quality） */
  audioQn?: number;
  outputDir: string;
  entryIndexes?: number[];
  note?: string;
}

export interface AddTasksResultDTO {
  taskIds: string[];
  rejected: Array<{ line: string; reason: string }>;
}

export interface HistoryFilterDTO {
  status?: ContractHistoryStatus;
  keyword?: string;
}

export interface HistoryEntryDTO {
  id: string;
  taskId: string | null;
  title: string;
  coverUrl: string | null;
  sourceKind: SourceKind;
  sourceId: string;
  pageUrl: string;
  quality: string;
  outputPath: string;
  fileSize: number;
  status: ContractHistoryStatus;
  error: string | null;
  finishedAt: number;
  /** EXTENSION */
  owner?: string;
  fileExists?: boolean;
  artifacts?: TaskArtifactDTO[];
  artifactCoverage?: ArtifactCoverageDTO;
}

export interface LoginStartDTO {
  key: string;
  qrMatrix: boolean[][];
  qrUrl: string;
  expiresInSecs: number;
  /** EXTENSION：mock/已扫码场景直接返回当前状态，避免首帧闪烁 */
  status?: ContractLoginStatus;
}

export interface LoginPollDTO {
  status: ContractLoginStatus;
  message: string;
  account?: AccountInfoDTO | null;
}

export interface ProxyDTO {
  mode: 'direct' | 'manual';
  host?: string;
  port?: number;
  user?: string;
  pass?: string;
}

export interface SettingsDTO {
  downloadDir: string;
  maxConcurrent: number;
  quality: string;
  codec: string;
  audioFormat: string;
  ffmpegPath: string;
  proxy: ProxyDTO;
  theme: 'system' | 'light' | 'dark';
  notifications: boolean;
  watchClipboard: boolean;
  /** EXTENSION：设置窗口其余字段（契约表未列，已同步） */
  parseConcurrency?: number;
  historyLimit?: number;
  historyClearPolicy?: 'never' | 'count';
  dockBadge?: boolean;
  reduceMotion?: boolean;
  showSpeedInStatus?: boolean;
  writeMetadata?: boolean;
  soundOnComplete?: boolean;
  revealInFinderOnComplete?: boolean;
  keepQueueOnExit?: boolean;
  downloadSubtitle?: boolean;
  downloadCover?: boolean;
  checkUpdatesOnStartup?: boolean;
  accentColor?: string;
  motion?: 'system' | 'reduce' | 'normal';
  qualityFallback?: boolean;
  glassBackground?: boolean;
  ffmpegDetected?: boolean;
  ffmpegVersion?: string | null;
  /** 进程内单调递增的设置修订号（get/patch/set 响应与 settings://changed 事件都携带） */
  revision?: number;
}

/**
 * 设置增量提交（design D3）：只包含可编辑业务字段。
 * 省略字段 = 保持不变；`null` = 清空可空字段（downloadDir/ffmpegPath/accentColor）。
 */
export interface SettingsPatchDTO {
  downloadDir?: string | null;
  maxConcurrent?: number;
  quality?: string;
  codec?: string;
  audioFormat?: string;
  ffmpegPath?: string | null;
  proxy?: ProxyDTO;
  theme?: 'system' | 'light' | 'dark';
  notifications?: boolean;
  watchClipboard?: boolean;
  historyLimit?: number;
  writeMetadata?: boolean;
  downloadSubtitle?: boolean;
  downloadCover?: boolean;
  checkUpdatesOnStartup?: boolean;
  accentColor?: string | null;
  motion?: 'system' | 'reduce' | 'normal';
  qualityFallback?: boolean;
  glassBackground?: boolean;
}

export interface AppInfoDTO {
  productName: string;
  version: string;
  tauriVersion: string;
  os: string;
  arch: string;
  identifier: string;
  dataDir: string;
  logDir: string;
  ffmpegFound: boolean;
  ffmpegPath: string | null;
  /** 当前窗口模式是否支持原生材质，与用户开关偏好无关 */
  glassSupported: boolean;
  glassUnavailableReason?: string | null;
  /** 系统通知是否可靠可用（Windows 开发/免安装运行下为 false） */
  notificationsSupported: boolean;
  /** 本次启动是否由更新安装后的自动重启触发（--updated） */
  justUpdated: boolean;
}

export interface MenuStateDTO {
  enabled: Record<string, boolean>;
}

export interface MenuActionDTO {
  id: string;
  label?: string;
}

export interface DragPayloadDTO {
  active: boolean;
}

/** core 启动阶段（design D5）：本地先就绪，网络阶段独立并在 5 秒预算内进入 ready/degraded。 */
export type CoreStatusStage = 'local-ready' | 'network-ready' | 'network-degraded';

export interface CoreStatusDTO {
  stage: CoreStatusStage;
  /** 网络准备代次：旧代次结果不得覆盖新状态（tasks 5.4） */
  generation: number;
  degraded: boolean;
  error?: string | null;
}

export interface DropPayloadDTO {
  paths: string[];
  text: string;
}

export interface NoticeDTO {
  level: 'info' | 'success' | 'warning' | 'error';
  code?: string;
  message: string;
  taskId?: string;
}

/** 事件名（COMMANDS.md §事件） */
export const CONTRACT_EVENTS = {
  queueSnapshot: 'queue://snapshot',
  queueEvents: 'queue://events',
  accountChanged: 'account://changed',
  settingsChanged: 'settings://changed',
  historyChanged: 'history://changed',
  notice: 'app://notice',
  shellMenu: 'shell://menu',
  shellDrag: 'shell://drag',
  shellDrop: 'shell://drop',
  shellQuitRequested: 'shell://quit-requested',
  /** 启动分阶段（design D5） */
  coreStatus: 'shell://core-status',
  /** 封面缓存清理完成：前端失效内存图片缓存（design D11/7.7） */
  coversCleared: 'covers://cleared',
  /** 文件状态展示缓存失效：前端重新拉取队列/历史（design D9/7.2） */
  filesInvalidated: 'files://invalidated',
} as const;

export type ContractEventName = (typeof CONTRACT_EVENTS)[keyof typeof CONTRACT_EVENTS];
