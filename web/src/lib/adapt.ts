/** 契约 DTO ↔ UI 内部模型 的适配层（唯一转换点，便于真实 IPC 对齐） */
import { AUDIO_FORMATS, QUALITY_TIERS, VIDEO_CODECS } from './constants';
import type {
  AccountInfoDTO,
  AppErrorDTO,
  AudioFormatOptionDTO,
  CodecOptionDTO,
  HistoryEntryDTO,
  LoginPollDTO,
  LoginStartDTO,
  ParseEntryDTO,
  ParseResultDTO,
  QualityOptionDTO,
  QueueCapabilityDTO,
  QueueSnapshotDTO,
  SettingsDTO,
  TaskArtifactDTO,
  TaskDTO,
  TaskProgressDTO,
} from './contract';
import type {
  AccountState,
  AudioFormatId,
  CodecId,
  ErrorAction,
  ErrorKind,
  HistoryEntry,
  HistoryStatus,
  LoginChallenge,
  LoginPollResult,
  LoginState,
  ParseItem,
  ParseResult,
  QualityOption,
  QueueSnapshot,
  QueueStats,
  QueueTask,
  Settings,
  SettingsPatch,
  SettingsValidationIssue,
  TaskError,
  TaskArtifact,
  TaskPhase,
  TaskStage,
  TaskStatus,
} from './types';

function artifactFromDTO(dto: TaskArtifactDTO): TaskArtifact {
  return {
    path: dto.path,
    size: dto.size,
    entryIndex: dto.entryIndex,
    entryTitle: dto.entryTitle,
    kind: dto.kind,
    exists: dto.exists,
    sizeMatches: dto.sizeMatches,
    valid: dto.valid,
  };
}

function artifactToDTO(artifact: TaskArtifact): TaskArtifactDTO {
  return {
    path: artifact.path,
    size: artifact.size,
    entryIndex: artifact.entryIndex,
    entryTitle: artifact.entryTitle,
    kind: artifact.kind,
    exists: artifact.exists,
    sizeMatches: artifact.sizeMatches,
    valid: artifact.valid,
  };
}

const PHASES: TaskPhase[] = ['resolve', 'video', 'audio', 'merge', 'postprocess'];
const ERROR_ACTIONS: ErrorAction[] = [
  'retry',
  'relogin',
  'open-settings',
  'copy-diagnostics',
  'open-log',
  'choose-dir',
  'install-ffmpeg',
];
const ERROR_KINDS: ErrorKind[] = [
  'risk',
  'auth',
  'vip',
  'paid',
  'region',
  'network',
  'cdn',
  'ffmpeg',
  'disk',
  'parse',
  'unknown',
];

export const DEFAULT_CAPABILITY = { anonymousMax: 480, signedInMax: 1080, vipMax: 2160 };

export const ANONYMOUS_ACCOUNT: AccountState = {
  signedIn: false,
  expired: false,
  mid: null,
  name: null,
  avatarUrl: null,
  vip: 'none',
  capability: DEFAULT_CAPABILITY,
};

export function capabilityFromDTO(dto?: QueueCapabilityDTO | null): AccountState['capability'] {
  if (!dto) return DEFAULT_CAPABILITY;
  const parse = (label: string, fallback: number) => {
    const match = /^(\d+)P/i.exec(label ?? '');
    return match ? Number(match[1]) : fallback;
  };
  return {
    anonymousMax: parse(dto.anonymousMax, DEFAULT_CAPABILITY.anonymousMax),
    signedInMax: parse(dto.signedInMax, DEFAULT_CAPABILITY.signedInMax),
    vipMax: parse(dto.vipMax, DEFAULT_CAPABILITY.vipMax),
  };
}

export function accountFromDTO(
  dto: AccountInfoDTO | null | undefined,
  capability?: QueueCapabilityDTO | null,
): AccountState {
  const cap = capabilityFromDTO(capability);
  if (!dto) return { ...ANONYMOUS_ACCOUNT, capability: cap };
  const vipText = String(dto.vip ?? '').toLowerCase();
  const vip: AccountState['vip'] = vipText.includes('annual')
    ? 'annual'
    : vipText.includes('month') || vipText.includes('vip')
      ? 'monthly'
      : 'none';
  return {
    signedIn: true,
    expired: false,
    mid: dto.mid,
    name: dto.uname,
    avatarUrl: dto.avatarUrl,
    vip,
    capability: cap,
  };
}

export function accountToDTO(account: AccountState): AccountInfoDTO | null {
  if (!account.signedIn || account.mid == null || !account.name) return null;
  return {
    mid: account.mid,
    uname: account.name,
    avatarUrl: account.avatarUrl,
    vip: account.vip,
    level: 6,
  };
}

export function qualityLabelToQn(label: string | undefined | null): number {
  if (!label) return 80;
  const tier = QUALITY_TIERS.find((item) => item.label === label);
  if (tier) return tier.id;
  const number = Number.parseInt(label, 10);
  return Number.isFinite(number) ? number : 80;
}

export function qualityOptionsForAccount(account: AccountState | null): QualityOption[] {
  return QUALITY_TIERS.map((tier) => {
    const loggedIn = Boolean(account?.signedIn);
    if (!loggedIn) {
      return tier.id <= 32
        ? { ...tier, available: true }
        : { ...tier, available: false, unavailableReason: '登录后可解锁' };
    }
    if (tier.vip && account?.vip === 'none') {
      return { ...tier, available: false, unavailableReason: '需大会员' };
    }
    return { ...tier, available: true };
  });
}

function contractStatusToInternal(status: TaskDTO['status']): TaskStatus {
  if (status === 'done') return 'completed';
  return status;
}

export function internalStatusToContract(status: TaskStatus): TaskDTO['status'] {
  if (status === 'completed') return 'done';
  return status;
}

function errorFromDTO(dto: AppErrorDTO | null | undefined): TaskError | null {
  if (!dto) return null;
  const raw = String(dto.kind ?? '').toLowerCase();
  const mapped: ErrorKind =
    raw === 'login-expired' || raw === 'unauthorized'
      ? 'auth'
      : raw === 'invalid-argument' || raw === 'parse-error'
        ? 'parse'
        : (ERROR_KINDS.find((kind) => kind === raw) ?? 'unknown');
  const actions = (dto.actions ?? []).filter((action): action is ErrorAction =>
    ERROR_ACTIONS.includes(action as ErrorAction),
  );
  return {
    kind: mapped,
    message: dto.message,
    actions: actions.length > 0 ? actions : ['retry'],
  };
}

export function errorToDTO(error: TaskError | null): AppErrorDTO | null {
  if (!error) return null;
  return { kind: error.kind, message: error.message, actions: error.actions };
}

function stagesFromDTO(dto: TaskDTO, status: TaskStatus): TaskStage[] {
  const phaseIndex = Math.max(0, PHASES.indexOf(dto.phase));
  const percent = dto.progress.percent;
  const total = dto.progress.bytesTotal || 0;
  // 视频流/音频流按 DASH 真实占比拆分（视频约 88%，音频约 12%），避免阶段数据雷同
  const stageWeights: Record<TaskPhase, number> = {
    resolve: 0,
    video: 0.88,
    audio: 0.12,
    merge: 0,
    postprocess: 0,
  };
  const stageProgress: Record<TaskPhase, number> = {
    resolve: 1,
    video: Math.min(1, percent * 1.08),
    audio: Math.min(1, percent * 0.86),
    merge: percent,
    postprocess: percent,
  };
  return PHASES.map((phase, index): TaskStage => {
    let stageStatus: TaskStage['status'] = 'pending';
    if (status === 'completed') {
      stageStatus = phase === 'postprocess' ? 'skipped' : 'done';
    } else if (status === 'cancelled') {
      stageStatus = index < Math.max(1, phaseIndex) ? 'done' : 'skipped';
    } else if (status === 'failed') {
      stageStatus = index < phaseIndex ? 'done' : index === phaseIndex ? 'failed' : 'pending';
    } else if (status === 'queued') {
      stageStatus = 'pending';
    } else if (status === 'resolving') {
      stageStatus = phase === 'resolve' ? 'active' : 'pending';
    } else if (status === 'merging') {
      stageStatus = index < 3 ? 'done' : phase === 'merge' ? 'active' : 'pending';
    } else {
      stageStatus = index < 1 ? 'done' : index <= 2 ? 'active' : 'pending';
    }
    const stageTotal = Math.round(total * stageWeights[phase]);
    return {
      phase,
      label: phase,
      status: stageStatus,
      doneBytes:
        stageTotal === 0
          ? 0
          : stageStatus === 'done'
            ? stageTotal
            : stageStatus === 'active'
              ? Math.round(stageTotal * stageProgress[phase])
              : 0,
      totalBytes: stageTotal,
    };
  });
}

export function taskFromDTO(dto: TaskDTO): QueueTask {
  const status = contractStatusToInternal(dto.status);
  const progress = dto.progress;
  // core 在任务完成后会重置 progress 字节数；完成态按 100% 呈现
  const percent = status === 'completed' ? 1 : progress.percent;
  return {
    id: dto.id,
    status,
    phase: dto.phase,
    kind: dto.sourceKind,
    sourceId: dto.sourceId,
    title: dto.title,
    uploader: dto.owner ?? 'B 站',
    coverUrl: dto.coverUrl,
    pageUrl: dto.pageUrl,
    durationSec: dto.durationSecs ?? null,
    params: {
      entries: dto.entries ?? [],
      entryCount: dto.entryCount ?? Math.max(1, dto.entries?.length ?? 1),
      quality: qualityLabelToQn(dto.quality),
      qualityLabel: dto.quality || '—',
      codec: (['avc', 'hevc', 'av1'].includes(dto.codec ?? '') ? dto.codec : 'avc') as CodecId,
      audio: dto.audioFormat === 'mp3' ? 'mp3' : 'original',
      outputDir: dto.outputDir ?? '',
    },
    progress: {
      doneBytes: progress.bytesDone,
      totalBytes: progress.bytesTotal,
      percent,
      speedBps: progress.speedBps,
      etaSec: progress.etaSecs,
    },
    stages: stagesFromDTO(dto, status),
    outputPath: dto.outputPath,
    artifacts: dto.artifacts?.map(artifactFromDTO),
    artifactCoverage: dto.artifactCoverage ?? (dto.artifacts?.length ? 'complete' : undefined),
    extras: dto.extras
      ? {
          subtitles: dto.extras.subtitles ?? [],
          cover: dto.extras.cover ?? null,
          subtitleError: dto.extras.subtitleError ?? null,
          coverError: dto.extras.coverError ?? null,
        }
      : undefined,
    error: errorFromDTO(dto.error),
    restored: dto.restored,
    createdAt: dto.createdAt,
    updatedAt: dto.updatedAt,
  };
}

export function progressToDTO(task: QueueTask): TaskProgressDTO {
  return {
    bytesDone: task.progress.doneBytes,
    bytesTotal: task.progress.totalBytes,
    stageDone: 0,
    stageTotal: 0,
    percent: task.progress.percent,
    speedBps: task.progress.speedBps,
    etaSecs: task.progress.etaSec,
  };
}

function internalPhaseToContract(phase: TaskPhase | null): TaskDTO['phase'] {
  // 契约无独立 audio 阶段（core 同时下载音视频，进度按 video 阶段呈现）
  if (!phase) return 'resolve';
  if (phase === 'audio') return 'video';
  return phase;
}

export function taskToDTO(task: QueueTask, sortIndex = 0): TaskDTO {
  return {
    id: task.id,
    status: internalStatusToContract(task.status),
    phase: internalPhaseToContract(task.phase),
    sourceKind: task.kind,
    sourceId: task.sourceId,
    title: task.title,
    coverUrl: task.coverUrl,
    pageUrl: task.pageUrl,
    quality: task.params.qualityLabel,
    outputPath: task.outputPath,
    artifacts: task.artifacts?.map(artifactToDTO),
    artifactCoverage: task.artifactCoverage,
    error: errorToDTO(task.error),
    restored: task.restored,
    sortIndex,
    createdAt: task.createdAt,
    updatedAt: task.updatedAt,
    progress: progressToDTO(task),
    owner: task.uploader,
    entries: task.params.entries,
    entryCount: task.params.entryCount,
    codec: task.params.codec,
    audioFormat: task.params.audio,
    outputDir: task.params.outputDir,
    durationSecs: task.durationSec,
  };
}

export function statsFromDTO(dto: QueueSnapshotDTO): QueueStats {
  const speed = dto.tasks.reduce((sum, task) => sum + task.progress.speedBps, 0);
  return {
    total: dto.stats.total,
    downloading: dto.stats.active,
    queued: dto.stats.queued,
    paused: dto.stats.paused,
    failed: dto.stats.failed,
    completed: dto.stats.finished,
    totalSpeedBps: speed,
  };
}

export function statsToDTO(tasks: QueueTask[]): QueueSnapshotDTO['stats'] {
  const count = (statuses: TaskStatus[]) => tasks.filter((task) => statuses.includes(task.status)).length;
  return {
    total: tasks.length,
    queued: count(['queued']),
    active: count(['resolving', 'downloading', 'merging']),
    paused: count(['paused']),
    failed: count(['failed']),
    finished: count(['completed']),
    cancelled: count(['cancelled']),
    bytesDone: tasks.reduce((sum, task) => sum + task.progress.doneBytes, 0),
    bytesTotal: tasks.reduce((sum, task) => sum + task.progress.totalBytes, 0),
  };
}

export function snapshotFromDTO(dto: QueueSnapshotDTO): QueueSnapshot {
  return {
    tasks: [...dto.tasks].sort((a, b) => a.sortIndex - b.sortIndex).map(taskFromDTO),
    stats: statsFromDTO(dto),
    account: accountFromDTO(dto.account, dto.capability),
  };
}

export function snapshotToDTO(tasks: QueueTask[], account: AccountState, revision: number): QueueSnapshotDTO {
  return {
    revision,
    tasks: tasks.map((task, index) => taskToDTO(task, index)),
    stats: statsToDTO(tasks),
    account: accountToDTO(account),
    capability: {
      anonymousMax: `${account.capability.anonymousMax}P`,
      signedInMax: `${account.capability.signedInMax}P`,
      vipMax: `${account.capability.vipMax}P`,
    },
    updatedAt: Date.now(),
  };
}

export function historyFromDTO(dto: HistoryEntryDTO): HistoryEntry {
  return {
    id: dto.id,
    taskId: dto.taskId,
    title: dto.title,
    uploader: dto.owner ?? 'B 站',
    coverUrl: dto.coverUrl,
    kind: dto.sourceKind,
    qualityLabel: dto.quality,
    outputPath: dto.outputPath,
    fileSize: dto.fileSize,
    artifacts: dto.artifacts?.map(artifactFromDTO),
    artifactCoverage: dto.artifactCoverage,
    status: dto.status as HistoryStatus,
    error: dto.error,
    finishedAt: dto.finishedAt,
    pageUrl: dto.pageUrl,
    fileExists: dto.fileExists ?? Boolean(dto.outputPath),
  };
}

export function historyToDTO(entry: HistoryEntry): HistoryEntryDTO {
  return {
    id: entry.id,
    taskId: entry.taskId,
    title: entry.title,
    coverUrl: entry.coverUrl,
    sourceKind: entry.kind,
    sourceId: entry.taskId ?? entry.id,
    pageUrl: entry.pageUrl,
    quality: entry.qualityLabel,
    outputPath: entry.outputPath,
    fileSize: entry.fileSize,
    artifacts: entry.artifacts?.map(artifactToDTO),
    artifactCoverage: entry.artifactCoverage,
    status: entry.status,
    error: entry.error,
    finishedAt: entry.finishedAt,
    owner: entry.uploader,
    fileExists: entry.fileExists,
  };
}

function qualityFromDTO(option: QualityOptionDTO): QualityOption {
  return { ...option };
}

function codecFromDTO(option: CodecOptionDTO): { id: CodecId; label: string; available: boolean } {
  const id: CodecId = option.id === 'hevc' ? 'hevc' : option.id === 'av1' ? 'av1' : 'avc';
  return { id, label: option.label, available: option.available };
}

function audioFromDTO(option: AudioFormatOptionDTO): { id: AudioFormatId; label: string; available: boolean; hint?: string } {
  return {
    id: option.id === 'mp3' ? 'mp3' : 'original',
    label: option.label,
    available: option.available,
    hint: option.hint,
  };
}

function normalizeTimestamp(value: number | null | undefined): number | null {
  if (value == null || value <= 0) return null;
  // 契约里发布时间可能来自 core 的秒级时间戳；统一转为毫秒
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

export function parseItemFromDTO(dto: ParseResultDTO, account: AccountState | null): ParseItem {
  const qualities = dto.qualities?.length ? dto.qualities.map(qualityFromDTO) : qualityOptionsForAccount(account);
  const codecs = dto.codecs?.length ? dto.codecs.map(codecFromDTO) : VIDEO_CODECS;
  const audioFormats = dto.audioFormats?.length ? dto.audioFormats.map(audioFromDTO) : AUDIO_FORMATS;
  const audioQualities = dto.audioQualities?.length ? dto.audioQualities : [];
  return {
    id: `${dto.kind}:${dto.sourceId}`,
    rawInput: dto.rawInput,
    kind: dto.kind,
    title: dto.title,
    uploader: dto.owner,
    coverUrl: dto.coverUrl,
    pageUrl: dto.pageUrl,
    durationSec: dto.durationSecs ?? null,
    publishedAt: normalizeTimestamp(dto.publishedAt),
    entries: dto.entries.map((entry: ParseEntryDTO) => ({
      id: `entry-${entry.index}`,
      title: entry.title,
      durationSec: entry.durationSecs,
      sizeBytes: entry.sizeBytes ?? null,
      selectedByDefault: entry.selectedDefault,
      unavailableReason: entry.unavailableReason,
    })),
    qualities,
    codecs,
    audioFormats,
    audioQualities,
  };
}

export function parseResultFromDTO(dto: ParseResultDTO, account: AccountState | null): ParseResult {
  const item = parseItemFromDTO(dto, account);
  return {
    items: [item],
    failures: [],
    requiresConfirmation:
      item.entries.length > 1 || item.entries.filter((entry) => entry.selectedByDefault).length > 1,
  };
}

export function parseResultToDTO(item: ParseItem): ParseResultDTO {
  return {
    kind: item.kind,
    sourceId: item.id,
    title: item.title,
    coverUrl: item.coverUrl,
    pageUrl: item.pageUrl,
    owner: item.uploader,
    rawInput: item.rawInput ?? item.pageUrl,
    entries: item.entries.map((entry, index) => ({
      index,
      title: entry.title,
      durationSecs: entry.durationSec,
      selectedDefault: entry.selectedByDefault,
      sizeBytes: entry.sizeBytes ?? undefined,
      unavailableReason: entry.unavailableReason,
    })),
    durationSecs: item.durationSec,
    publishedAt: item.publishedAt,
    qualities: item.qualities.length ? item.qualities : undefined,
    codecs: item.codecs.length ? item.codecs : undefined,
    audioFormats: item.audioFormats.length ? item.audioFormats : undefined,
  };
}

export function loginChallengeFromDTO(dto: LoginStartDTO): LoginChallenge {
  const map: Record<string, LoginState> = {
    waiting: 'waiting',
    scanned: 'scanned',
    expired: 'expired',
    success: 'success',
    error: 'failed',
  };
  return {
    sessionId: dto.key,
    modules: dto.qrMatrix,
    expiresInSec: dto.expiresInSecs,
    state: map[dto.status ?? 'waiting'] ?? 'waiting',
  };
}

export function loginPollFromDTO(dto: LoginPollDTO): LoginPollResult {
  const map: Record<LoginPollDTO['status'], LoginState> = {
    waiting: 'waiting',
    scanned: 'scanned',
    expired: 'expired',
    success: 'success',
    error: 'failed',
  };
  return {
    state: map[dto.status] ?? 'failed',
    message: dto.message,
    account: dto.account ? accountFromDTO(dto.account) : null,
  };
}

// ---- 设置 ----

export function defaultInternalSettings(): Settings {
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
      appearance: 'system',
      dockBadge: true,
      reduceMotion: false,
      showSpeedInStatus: true,
      glassBackground: true,
      accentColor: '#3B82F6',
      motion: 'system',
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
      ffmpegVersion: null,
    },
    network: {
      proxyMode: 'direct',
      proxyHost: '',
      proxyPort: 7890,
      proxyUser: '',
      proxyPass: '',
    },
  };
}

function codecPreferenceFrom(value: string): Settings['quality']['codecPreference'] {
  if (value === 'auto') return 'auto';
  if (value === 'hevc') return 'hevc';
  if (value === 'av1') return 'av1';
  return 'avc';
}

export function settingsFromDTO(dto: SettingsDTO, prev?: Settings | null): Settings {
  const base = prev ?? defaultInternalSettings();
  const fixedQuality =
    dto.quality === 'best' ? base.quality.fixedQuality : qualityLabelToQn(dto.quality);
  return {
    general: {
      ...base.general,
      notifyOnComplete: dto.notifications,
      soundOnComplete: dto.soundOnComplete ?? base.general.soundOnComplete,
      revealInFinderOnComplete:
        dto.revealInFinderOnComplete ?? base.general.revealInFinderOnComplete,
      keepQueueOnExit: dto.keepQueueOnExit ?? base.general.keepQueueOnExit,
      historyLimit: dto.historyLimit ?? base.general.historyLimit,
      historyClearPolicy: dto.historyClearPolicy ?? base.general.historyClearPolicy,
    },
    interface: {
      appearance: dto.theme,
      dockBadge: dto.dockBadge ?? base.interface.dockBadge,
      reduceMotion: dto.reduceMotion ?? base.interface.reduceMotion,
      showSpeedInStatus: dto.showSpeedInStatus ?? base.interface.showSpeedInStatus,
      glassBackground: dto.glassBackground ?? base.interface.glassBackground,
      accentColor: dto.accentColor ?? base.interface.accentColor,
      motion: dto.motion ?? base.interface.motion ?? 'system',
    },
    download: {
      outputDir: dto.downloadDir,
      maxConcurrent: dto.maxConcurrent,
      parseConcurrency: dto.parseConcurrency ?? base.download.parseConcurrency,
      clipboardWatch: dto.watchClipboard,
      downloadSubtitle: dto.downloadSubtitle ?? base.download.downloadSubtitle ?? false,
      downloadCover: dto.downloadCover ?? base.download.downloadCover ?? false,
      checkUpdatesOnStartup:
        dto.checkUpdatesOnStartup ?? base.download.checkUpdatesOnStartup ?? true,
    },
    quality: {
      strategy: dto.quality === 'best' ? 'best' : 'fixed',
      fixedQuality,
      codecPreference: codecPreferenceFrom(dto.codec),
      audioFormat: dto.audioFormat === 'mp3' ? 'mp3' : 'original',
      fallback: dto.qualityFallback ?? base.quality.fallback,
      writeMetadata: dto.writeMetadata ?? base.quality.writeMetadata,
      ffmpegPath: dto.ffmpegPath,
      ffmpegDetected: dto.ffmpegDetected ?? base.quality.ffmpegDetected,
      ffmpegVersion: dto.ffmpegVersion ?? base.quality.ffmpegVersion,
    },
    network: {
      proxyMode: dto.proxy.mode,
      proxyHost: dto.proxy.host ?? '',
      proxyPort: dto.proxy.port ?? 7890,
      proxyUser: dto.proxy.user ?? '',
      proxyPass: dto.proxy.pass ?? '',
    },
  };
}

export function settingsToDTO(settings: Settings): SettingsDTO {
  const quality =
    settings.quality.strategy === 'best'
      ? 'best'
      : (QUALITY_TIERS.find((tier) => tier.id === settings.quality.fixedQuality)?.label ?? '1080P');
  return {
    downloadDir: settings.download.outputDir,
    maxConcurrent: settings.download.maxConcurrent,
    quality,
    codec: settings.quality.codecPreference,
    audioFormat: settings.quality.audioFormat,
    ffmpegPath: settings.quality.ffmpegPath,
    proxy: {
      mode: settings.network.proxyMode,
      host: settings.network.proxyHost,
      port: settings.network.proxyPort,
      user: settings.network.proxyUser,
      pass: settings.network.proxyPass,
    },
    theme: settings.interface.appearance,
    notifications: settings.general.notifyOnComplete,
    watchClipboard: settings.download.clipboardWatch,
    parseConcurrency: settings.download.parseConcurrency,
    historyLimit: settings.general.historyLimit,
    historyClearPolicy: settings.general.historyClearPolicy,
    dockBadge: settings.interface.dockBadge,
    reduceMotion: settings.interface.reduceMotion,
    showSpeedInStatus: settings.interface.showSpeedInStatus,
    writeMetadata: settings.quality.writeMetadata,
    soundOnComplete: settings.general.soundOnComplete,
    revealInFinderOnComplete: settings.general.revealInFinderOnComplete,
    keepQueueOnExit: settings.general.keepQueueOnExit,
    downloadSubtitle: settings.download.downloadSubtitle ?? false,
    downloadCover: settings.download.downloadCover ?? false,
    checkUpdatesOnStartup: settings.download.checkUpdatesOnStartup ?? true,
    accentColor: settings.interface.accentColor,
    motion: settings.interface.motion ?? 'system',
    qualityFallback: settings.quality.fallback,
    glassBackground: settings.interface.glassBackground,
    ffmpegDetected: settings.quality.ffmpegDetected,
    ffmpegVersion: settings.quality.ffmpegVersion,
  };
}

/** 前端侧校验（契约 set_settings 只返回 invalid-argument，这里提前给出字段级错误） */
export function validateSettings(settings: Settings): SettingsValidationIssue[] {
  const issues: SettingsValidationIssue[] = [];
  const int = (value: number, min: number, max: number, path: string, message: string) => {
    if (!Number.isInteger(value) || value < min || value > max) issues.push({ path, message });
  };
  int(settings.download.maxConcurrent, 1, 8, 'download.maxConcurrent', '需在 1–8 之间');
  int(settings.download.parseConcurrency, 1, 4, 'download.parseConcurrency', '需在 1–4 之间');
  int(settings.general.historyLimit, 1, 5000, 'general.historyLimit', '需在 1–5000 之间');
  if (!settings.download.outputDir.trim()) {
    issues.push({ path: 'download.outputDir', message: '下载目录不能为空' });
  }
  if (settings.network.proxyMode === 'manual') {
    int(settings.network.proxyPort, 1, 65535, 'network.proxyPort', '端口需在 1–65535 之间');
    if (!settings.network.proxyHost.trim()) {
      issues.push({ path: 'network.proxyHost', message: '请输入代理地址' });
    }
  }
  if (!Number.isFinite(settings.quality.fixedQuality) || settings.quality.fixedQuality <= 0) {
    issues.push({ path: 'quality.fixedQuality', message: '请选择有效档位' });
  }
  return issues;
}

export function mergeSettings(settings: Settings, patch: SettingsPatch): Settings {
  return {
    general: { ...settings.general, ...patch.general },
    interface: { ...settings.interface, ...patch.interface },
    download: { ...settings.download, ...patch.download },
    quality: { ...settings.quality, ...patch.quality },
    network: { ...settings.network, ...patch.network },
  };
}
