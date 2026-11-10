import { CONTRACT_EVENTS, type ContractEventName } from '../lib/contract';
import type {
  AccountInfoDTO,
  AddOptionsDTO,
  AddTasksResultDTO,
  AppInfoDTO,
  ClearOutcomeDTO,
  ContractLoginStatus,
  HistoryEntryDTO,
  HistoryFilterDTO,
  LoginPollDTO,
  LoginStartDTO,
  MenuStateDTO,
  ParseResultDTO,
  QueueSnapshotDTO,
  SettingsDTO,
  SettingsPatchDTO,
  TaskEventDTO,
} from '../lib/contract';
import type { Backend, ScenarioBootstrap } from '../lib/ipc';
import type { AccountState, HistoryEntry, ParseItem, QueueTask, Settings, TaskStatus } from '../lib/types';
import * as A from '../lib/adapt';
import { devParams } from './scenario';
import {
  ANONYMOUS_ACCOUNT,
  SIGNED_IN_ACCOUNT,
  VIP_ACCOUNT,
  addPanelParseResult,
  bulkTasks,
  defaultSettings,
  errorTasks,
  historyEntries,
  loginChallenge,
  makeTask,
  queueTasks,
  statesTasks,
} from './mockData';

const TERMINAL: TaskStatus[] = ['completed', 'failed', 'cancelled'];
const MB = 1024 * 1024;

export class MockBackend implements Backend {
  private tasks: QueueTask[];
  private account: AccountState;
  private settings: Settings;
  private history: HistoryEntry[];
  private listeners = new Map<string, Set<(payload: unknown) => void>>();
  private parseCache = new Map<string, ParseResultDTO>();
  private loginStatus: ContractLoginStatus = 'waiting';
  private revision = 0;
  /** 设置修订号（design D3）：get/patch 响应与 settings://changed 事件一致携带 */
  private settingsRevision = 0;
  /** dev QA：?settingsFail=N 让前 N 次保存失败，用于验证保存失败/重试界面 */
  private settingsPatchFailures = 0;
  private scenario = devParams().scenario;
  private quitAttempts = 0;
  /**
   * dev QA 启动模拟（?startup=...）：默认正常分阶段；可模拟本地初始化失败、
   * 网络阶段挂起、历史读取失败与外部文件失效（tasks 5.1/5.3/7.2）。
   */
  private startupMode: string | null;
  private coreReady: Promise<void> = Promise.resolve();
  private resolveCore: (() => void) | null = null;
  private rejectCore: ((error: unknown) => void) | null = null;
  private localReady = false;
  private networkReady = false;
  private networkGeneration = 0;
  private historyFailures = 0;
  private startupAttempts = 0;
  private filesInvalidatedEmitted = false;

  constructor() {
    this.settingsPatchFailures = Math.max(
      0,
      Number.parseInt(
        new URLSearchParams(window.location.search).get('settingsFail') ?? '0',
        10,
      ) || 0,
    );
    this.tasks = this.seedTasks();
    this.account = this.seedAccount();
    this.settings = defaultSettings(devParams().theme);
    if (this.scenario === 'add-panel-preferences') {
      this.settings.quality.strategy = 'fixed';
      this.settings.quality.fixedQuality = 64;
      this.settings.quality.codecPreference = 'hevc';
    }
    if (this.scenario === 'add-panel-no-fallback') {
      this.settings.quality.strategy = 'fixed';
      this.settings.quality.fixedQuality = 125;
      this.settings.quality.fallback = false;
    }
    this.history = this.seedHistory();
    if (this.scenario === 'login-scanned') this.loginStatus = 'scanned';
    if (this.scenario === 'login-expired') this.loginStatus = 'expired';
    if (this.scenario === 'login-failed') this.loginStatus = 'error';
    if (this.scenario === 'login-success') this.loginStatus = 'success';
    this.startupMode = new URLSearchParams(window.location.search).get('startup');
    // 首次启动读取与打开历史列表的自动刷新各失败一次，便于演示错误与手动重试。
    if (this.startupMode === 'history-fail') this.historyFailures = 2;
    this.resetCoreGate();
    this.scheduleStartup();
  }

  // ---- 启动分阶段模拟（tasks 5.1/5.3/5.4）----
  private resetCoreGate(): void {
    this.coreReady = new Promise<void>((resolve, reject) => {
      this.resolveCore = resolve;
      this.rejectCore = reject;
    });
  }

  /** core 依赖命令在本地阶段完成前等待（与 Rust `ShellState::core()` 一致）。 */
  private awaitCore(): Promise<void> {
    return this.coreReady;
  }

  private scheduleStartup(): void {
    if (this.startupMode === 'core-fail' && this.startupAttempts === 0) {
      // 用宏任务而非微任务：给启动器留出安装事件订阅的时间（与 Rust 异步初始化时序一致）。
      window.setTimeout(() => {
        this.rejectCore?.({
          kind: 'unknown',
          message: '引擎初始化失败：数据库迁移失败（dev 注入）',
        });
        this.emitNotice('error', '引擎初始化失败：数据库迁移失败（dev 注入）');
      });
      return;
    }
    window.setTimeout(() => this.completeLocalPhase(), 0);
  }

  private completeLocalPhase(): void {
    if (this.localReady) return;
    this.localReady = true;
    this.resolveCore?.();
    this.resolveCore = null;
    this.rejectCore = null;
    // 与 Rust 本地阶段一致：先发布本地数据事件，再发布 local-ready。
    // 账号按未校验的本地匿名态发布，不虚报已登录（design D5/tasks 5.4）。
    this.emitSnapshot();
    this.emit(CONTRACT_EVENTS.accountChanged, A.accountToDTO(this.visibleAccount()));
    this.emitSettings();
    // 历史读取失败模拟：本地阶段也没有可发布的成功历史（不伪造空列表为成功）。
    if (this.historyFailures === 0) this.emitHistory();
    this.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'local-ready',
      generation: this.networkGeneration,
      degraded: false,
    });
    if (this.startupMode === 'network-hang') return;
    window.setTimeout(() => this.completeNetworkPhase(), 0);
  }

  private completeNetworkPhase(): void {
    if (this.networkReady) return;
    this.networkGeneration += 1;
    if (this.startupMode === 'network-degraded') {
      // 首次网络准备超时：本地可用、账号校验未完成，可点击重试（tasks 5.4）。
      this.emit(CONTRACT_EVENTS.coreStatus, {
        stage: 'network-degraded',
        generation: this.networkGeneration,
        degraded: true,
        error: '网络准备超时，本地内容可继续使用（dev 注入）',
      });
      return;
    }
    this.networkReady = true;
    this.emitAccount();
    this.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'network-ready',
      generation: this.networkGeneration,
      degraded: false,
    });
    this.scheduleFilesInvalidated();
  }

  private scheduleFilesInvalidated(): void {
    if (this.startupMode !== 'files-invalidated' || this.filesInvalidatedEmitted) return;
    this.filesInvalidatedEmitted = true;
    window.setTimeout(() => {
      const done = this.tasks.find((task) => task.status === 'completed');
      if (done) {
        if (done.artifacts?.length) {
          done.artifacts = done.artifacts.map((artifact) =>
            artifact.entryIndex === 0
              ? { ...artifact, exists: false, sizeMatches: false, valid: false }
              : artifact,
          );
        } else {
          done.artifacts = [
            {
              path: done.outputPath ?? '~/Downloads/bilidown/已删除.mp4',
              size: done.progress.totalBytes,
              entryIndex: 0,
              entryTitle: null,
              kind: 'media',
              exists: false,
              sizeMatches: false,
              valid: false,
            },
          ];
        }
      }
      const entry = this.history[0];
      if (entry) entry.fileExists = false;
      this.emit(CONTRACT_EVENTS.filesInvalidated, undefined);
      this.emitSnapshot();
      this.emitHistory();
    }, 600);
  }

  /** 网络阶段未完成的本地视图：账号按未校验匿名态处理，不虚报登录成功。 */
  private visibleAccount(): AccountState {
    return this.networkReady ? this.account : { ...ANONYMOUS_ACCOUNT };
  }

  private seedTasks(): QueueTask[] {
    switch (this.scenario) {
      case 'empty':
      case 'history':
      case 'history-artifacts':
      case 'history-legacy':
      case 'history-empty':
        return [];
      case 'states':
        return statesTasks();
      case 'bulk':
        return bulkTasks(187);
      case 'task-detail-artifacts': {
        const tasks = queueTasks();
        const done = tasks.find((task) => task.id === 'task-done');
        if (done) {
          done.artifacts = [
            {
              path: '~/Downloads/bilidown/城市夜景[1080P] P1.mp4',
              size: 300 * MB,
              entryIndex: 0,
              entryTitle: 'P1',
              kind: 'media',
              exists: true,
              sizeMatches: true,
              valid: true,
            },
            {
              path: '~/Downloads/bilidown/城市夜景[1080P] P2.mp4',
              size: 438 * MB,
              entryIndex: 1,
              entryTitle: 'P2',
              kind: 'media',
              exists: true,
              sizeMatches: true,
              valid: true,
            },
          ];
          done.artifactCoverage = 'complete';
        }
        return tasks;
      }
      case 'errors':
        return errorTasks();
      case 'persist-error': {
        const tasks = queueTasks();
        const downloading = tasks.find((task) => task.id === 'task-dl');
        if (downloading) {
          downloading.persistError = '任务记录保存失败：database is locked';
        }
        const done = tasks.find((task) => task.id === 'task-done');
        if (done) {
          done.persistError = '历史记录写入失败：disk I/O error';
        }
        return tasks;
      }
      default:
        return queueTasks();
    }
  }

  private seedAccount(): AccountState {
    if (
      this.scenario === 'login' ||
      this.scenario === 'login-scanned' ||
      this.scenario === 'login-expired'
    ) {
      return { ...ANONYMOUS_ACCOUNT };
    }
    if (this.scenario === 'login-success') return { ...VIP_ACCOUNT };
    return { ...SIGNED_IN_ACCOUNT };
  }

  private seedHistory(): HistoryEntry[] {
    if (this.scenario === 'history-empty') return [];
    const entries = historyEntries();
    if (this.scenario === 'history-artifacts' && entries[0]) {
      entries[0].artifacts = [
        {
          path: '~/Downloads/bilidown/历史 P1.mp4',
          size: 320 * MB,
          entryIndex: 0,
          entryTitle: 'P1',
          kind: 'media',
          exists: true,
          sizeMatches: true,
          valid: true,
        },
        {
          path: '~/Downloads/bilidown/历史 P2.mp4',
          size: 280 * MB,
          entryIndex: 1,
          entryTitle: 'P2',
          kind: 'media',
          exists: true,
          sizeMatches: true,
          valid: true,
        },
      ];
      entries[0].artifactCoverage = 'complete';
    }
    if (this.scenario === 'history-legacy' && entries[0]) {
      entries[0].artifacts = [
        {
          path: entries[0].outputPath,
          size: entries[0].fileSize,
          entryIndex: null,
          entryTitle: null,
          kind: 'media',
          exists: entries[0].fileExists,
          sizeMatches: entries[0].fileExists,
          valid: entries[0].fileExists,
        },
      ];
      entries[0].artifactCoverage = 'legacy-single';
    }
    return entries;
  }

  // ---- 事件 ----
  private emit(event: string, payload: unknown): void {
    const set = this.listeners.get(event);
    if (!set) return;
    for (const handler of set) handler(payload);
  }

  private snapshotDTO(): QueueSnapshotDTO {
    this.revision += 1;
    return A.snapshotToDTO(this.tasks, this.visibleAccount(), this.revision);
  }

  private emitSnapshot(): void {
    this.emit(CONTRACT_EVENTS.queueSnapshot, this.snapshotDTO());
  }

  private emitTaskEvent(taskId: string, kind: TaskEventDTO['kind'], message?: string): void {
    this.emit(CONTRACT_EVENTS.queueEvents, {
      taskId,
      kind,
      message,
      at: Date.now(),
    } satisfies TaskEventDTO);
  }

  private emitNotice(level: 'info' | 'success' | 'warning' | 'error', message: string): void {
    this.emit(CONTRACT_EVENTS.notice, { level, message });
  }

  private emitAccount(): void {
    this.emit(CONTRACT_EVENTS.accountChanged, A.accountToDTO(this.account));
  }

  private settingsDTO(): SettingsDTO {
    return A.settingsToDTO(this.settings, this.settingsRevision);
  }

  private emitSettings(): void {
    this.emit(CONTRACT_EVENTS.settingsChanged, this.settingsDTO());
  }

  private emitHistory(): void {
    this.emit(CONTRACT_EVENTS.historyChanged, this.history.map(A.historyToDTO));
  }

  private syncHistoryFromTasks(): void {
    const existing = new Set(this.history.map((entry) => entry.taskId).filter(Boolean) as string[]);
    for (const task of this.tasks) {
      if (TERMINAL.includes(task.status) && !existing.has(task.id)) {
        this.history.unshift({
          id: `h-${task.id}`,
          taskId: task.id,
          title: task.title,
          uploader: task.uploader,
          coverUrl: task.coverUrl,
          kind: task.kind,
          qualityLabel: task.params.qualityLabel,
          outputPath: task.outputPath ?? '',
          fileSize: task.progress.totalBytes,
          artifacts: task.artifacts,
          artifactCoverage: task.artifactCoverage,
          status: task.status as HistoryEntry['status'],
          error: task.error?.message ?? null,
          finishedAt: Date.now(),
          pageUrl: task.pageUrl,
          fileExists: Boolean(task.outputPath),
        });
        existing.add(task.id);
      }
    }
    this.history.sort((a, b) => b.finishedAt - a.finishedAt);
    this.emitHistory();
  }

  private findTask(id: string): QueueTask | undefined {
    return this.tasks.find((task) => task.id === id);
  }

  private updateTask(id: string, updater: (task: QueueTask) => void, kind: TaskEventDTO['kind']): void {
    const task = this.findTask(id);
    if (!task) return;
    updater(task);
    task.updatedAt = Date.now();
    this.emitSnapshot();
    this.emitTaskEvent(task.id, kind);
    if (TERMINAL.includes(task.status)) this.syncHistoryFromTasks();
  }

  // ---- 应用 ----
  getAppInfo = async (): Promise<AppInfoDTO> => {
    const windows = devParams().os === 'windows';
    return {
      productName: 'bilidown',
      version: '2.0.0',
      tauriVersion: '2.0.0',
      os: windows ? 'windows' : 'macOS',
      arch: windows ? 'x64' : 'arm64',
      identifier: 'app.bilidown',
      dataDir: windows
        ? '%LOCALAPPDATA%\\bilidown'
        : '~/Library/Application Support/bilidown',
      logDir: windows
        ? '%LOCALAPPDATA%\\bilidown\\logs'
        : '~/Library/Application Support/bilidown/logs',
      ffmpegFound: this.settings.quality.ffmpegDetected,
      ffmpegPath: this.settings.quality.ffmpegPath,
      glassSupported: !windows,
      glassUnavailableReason: windows
        ? '当前窗口模式使用不透明背景，以保证圆角与边缘显示正常。'
        : null,
      notificationsSupported: true,
      justUpdated: false,
    };
  };

  openPreferences = async (): Promise<void> => {
    window.location.hash = '#/preferences';
  };

  quitApp = async (): Promise<void> => {
    // 开发场景演示「退出前保存失败」：第一次失败，重试后成功（真实后端由 shell 决定）
    if (this.scenario === 'quit-failure' && this.quitAttempts === 0) {
      this.quitAttempts += 1;
      throw { kind: 'disk', message: '任务状态保存失败：database is locked', actions: ['retry'] };
    }
  };

  forceQuitApp = async (): Promise<void> => {};

  retryInit = async (): Promise<void> => {
    if (this.localReady) return;
    this.startupAttempts += 1;
    this.resetCoreGate();
    this.scheduleStartup();
  };

  retryNetwork = async (): Promise<void> => {
    await this.awaitCore();
    if (this.startupMode === 'network-hang' || this.startupMode === 'network-degraded') {
      this.startupMode = '';
    }
    this.completeNetworkPhase();
  };

  getDiagnostics = async (): Promise<string> =>
    `bilidown 2.0.0 (mock)\nmacOS 27.0.1 arm64\n队列：${this.tasks.length} 项\n最近错误：登录已失效`;

  // ---- 解析 / 队列 ----
  parseInput = async (input: string): Promise<ParseResultDTO> => {
    const trimmed = input.trim();
    const looksLikeBili =
      /bilibili\.com|b23\.tv|^BV[0-9A-Za-z]+$|^av\d+$/i.test(trimmed) || /^ep\d+$/i.test(trimmed);
    if (!looksLikeBili) {
      throw { kind: 'invalid-argument', message: '未识别到有效的 B 站链接', actions: ['retry'] };
    }
    if (
      this.scenario === 'add-panel' ||
      this.scenario === 'add-panel-preferences' ||
      this.scenario === 'add-panel-no-fallback'
    ) {
      const item = addPanelParseResult().items[0];
      const dto = { ...A.parseResultToDTO(item), rawInput: trimmed };
      this.parseCache.set(trimmed, dto);
      return dto;
    }
    if (this.scenario === 'add-multi-p') {
      // 多 P/番剧：实际 12 个条目，但默认只勾选当前集（第 1 集）
      const dto: ParseResultDTO = {
        kind: 'bangumi',
        sourceId: 'ss-multi-p',
        title: '《孤独摇滚！》全 12 话',
        coverUrl: '/covers/cover-02.svg',
        pageUrl: trimmed.startsWith('http') ? trimmed : 'https://www.bilibili.com/bangumi/play/ss41410',
        owner: '孤独摇滚制作委员会',
        rawInput: trimmed,
        entries: Array.from({ length: 12 }, (_, index) => ({
          index,
          title: `第 ${String(index + 1).padStart(2, '0')} 话`,
          durationSecs: 1420,
          selectedDefault: index === 0,
        })),
      };
      this.parseCache.set(trimmed, dto);
      return dto;
    }
    const item: ParseItem = {
      id: `parse-${Date.now()}`,
      rawInput: trimmed,
      kind: 'video',
      title: '示例视频 · 来自 B 站链接',
      uploader: '示例 UP 主',
      coverUrl: '/covers/cover-05.svg',
      pageUrl: trimmed.startsWith('http') ? trimmed : 'https://www.bilibili.com/video/BV1mock0001',
      durationSec: 642,
      publishedAt: Date.now() - 86_400_000,
      entries: [
        { id: 'entry-0', title: 'P1', durationSec: 642, sizeBytes: 640 * MB, selectedByDefault: true },
      ],
      qualities: A.qualityOptionsForAccount(this.account),
      codecs: [
        { id: 'avc', label: 'AVC', available: true },
        { id: 'hevc', label: 'HEVC', available: true },
      ],
      audioFormats: [
        { id: 'original', label: '原格式', available: true, hint: 'm4a / flac' },
        { id: 'mp3', label: 'MP3', available: true, hint: '需 ffmpeg' },
      ],
      audioQualities: [
        { id: 30251, label: 'FLAC' },
        { id: 30250, label: '杜比全景声' },
        { id: 30280, label: '192K' },
      ],
    };
    const dto = A.parseResultToDTO(item);
    this.parseCache.set(trimmed, dto);
    return dto;
  };

  addTasks = async (input: string, options: AddOptionsDTO): Promise<AddTasksResultDTO> => {
    const dto = this.parseCache.get(input) ?? (await this.parseInput(input));
    if (options.entryIndexes?.length === 0) {
      throw new Error('至少选择一个条目');
    }
    const indexes =
      options.entryIndexes === undefined
        ? dto.entries.filter((entry) => entry.selectedDefault).map((entry) => entry.index)
        : options.entryIndexes;
    const selected = dto.entries.filter((entry) => indexes.includes(entry.index));
    if (selected.length === 0) throw new Error('至少选择一个有效条目');
    const list = selected;
    const taskIds: string[] = [];
    for (const entry of list) {
      const totalBytes = entry.sizeBytes ?? 480 * MB;
      const title =
        dto.entries.length > 1 && list.length === 1 ? `${dto.title} · ${entry.title}` : dto.title;
      const task = makeTask({
        id: `task-${Date.now()}-${taskIds.length}`,
        title,
        uploader: dto.owner,
        kind: dto.kind,
        coverIndex: 3,
        status: 'queued',
        percent: 0,
        totalBytes,
        quality: options.quality,
        entryCount: list.length,
        entryLabel: entry.title,
      });
      task.coverUrl = dto.coverUrl;
      task.pageUrl = dto.pageUrl;
      task.params.codec = options.codec === 'hevc' ? 'hevc' : options.codec === 'av1' ? 'av1' : 'avc';
      task.params.audio = options.audioFormat === 'mp3' ? 'mp3' : 'original';
      task.params.outputDir = options.outputDir;
      if (options.note) task.params.note = options.note;
      this.tasks.unshift(task);
      taskIds.push(task.id);
    }
    this.emitSnapshot();
    if (taskIds.length > 0) this.emitNotice('success', `已添加 ${taskIds.length} 个任务`);
    return { taskIds, rejected: [] };
  };

  listQueue = async (): Promise<QueueSnapshotDTO> => {
    await this.awaitCore();
    return this.snapshotDTO();
  };

  pauseTask = async (taskId: string): Promise<void> => {
    this.updateTask(
      taskId,
      (task) => {
        if (TERMINAL.includes(task.status)) return;
        task.status = 'paused';
        task.progress.speedBps = 0;
        task.progress.etaSec = null;
        task.phase = null;
      },
      'paused',
    );
  };

  resumeTask = async (taskId: string): Promise<void> => {
    this.updateTask(
      taskId,
      (task) => {
        if (task.status !== 'paused') return;
        task.status = 'downloading';
        task.restored = false;
        task.phase = 'video';
        task.progress.speedBps = 5.4 * MB;
        task.progress.etaSec = 160;
      },
      'resumed',
    );
  };

  cancelTask = async (taskId: string): Promise<void> => {
    this.updateTask(
      taskId,
      (task) => {
        if (TERMINAL.includes(task.status)) return;
        task.status = 'cancelled';
        task.progress.speedBps = 0;
        task.progress.etaSec = null;
        task.phase = null;
      },
      'cancelled',
    );
  };

  retryTask = async (taskId: string): Promise<void> => {
    this.updateTask(
      taskId,
      (task) => {
        task.status = 'queued';
        task.error = null;
        task.progress = {
          doneBytes: 0,
          totalBytes: task.progress.totalBytes,
          percent: 0,
          speedBps: 0,
          etaSec: null,
        };
        task.restored = false;
        task.phase = null;
      },
      'enqueued',
    );
  };

  removeTask = async (taskId: string): Promise<void> => {
    this.tasks = this.tasks.filter((task) => task.id !== taskId);
    this.emitSnapshot();
    this.emitTaskEvent(taskId, 'removed');
  };

  startAll = async (): Promise<number> => {
    let count = 0;
    for (const task of this.tasks) {
      if (task.status === 'paused') {
        task.status = 'downloading';
        task.restored = false;
        task.phase = 'video';
        task.progress.speedBps = 4.8 * MB;
        count += 1;
      }
    }
    this.emitSnapshot();
    return count;
  };

  pauseAll = async (): Promise<number> => {
    let count = 0;
    for (const task of this.tasks) {
      if (!TERMINAL.includes(task.status) && task.status !== 'paused') {
        task.status = 'paused';
        task.progress.speedBps = 0;
        task.phase = null;
        count += 1;
      }
    }
    this.emitSnapshot();
    return count;
  };

  retryAll = async (): Promise<number> => {
    let count = 0;
    for (const task of this.tasks) {
      if (task.status === 'failed') {
        task.status = 'queued';
        task.error = null;
        task.progress.percent = 0;
        task.progress.doneBytes = 0;
        count += 1;
      }
    }
    this.emitSnapshot();
    return count;
  };

  clearFinished = async (): Promise<ClearOutcomeDTO> => {
    const removed = this.tasks.filter((task) => task.status === 'completed').map((task) => task.id);
    this.tasks = this.tasks.filter((task) => task.status !== 'completed');
    this.emitSnapshot();
    for (const id of removed) this.emitTaskEvent(id, 'removed');
    return { removed: removed.length, failed: [] };
  };

  clearInactive = async (): Promise<ClearOutcomeDTO> => {
    const inactive: TaskStatus[] = ['queued', 'paused', 'cancelled'];
    const removed = this.tasks.filter((task) => inactive.includes(task.status)).map((task) => task.id);
    this.tasks = this.tasks.filter((task) => !inactive.includes(task.status));
    this.emitSnapshot();
    for (const id of removed) this.emitTaskEvent(id, 'removed');
    return { removed: removed.length, failed: [] };
  };

  retryPersistence = async (): Promise<void> => {
    let cleared = 0;
    for (const task of this.tasks) {
      if (task.persistError) {
        task.persistError = null;
        cleared += 1;
      }
    }
    if (cleared > 0) this.emitSnapshot();
  };

  // ---- 账户 / 登录 ----
  getAccount = async (): Promise<AccountInfoDTO | null> => {
    await this.awaitCore();
    return A.accountToDTO(this.visibleAccount());
  };

  loginStart = async (): Promise<LoginStartDTO> => {
    const state =
      this.loginStatus === 'scanned'
        ? 'scanned'
        : this.loginStatus === 'expired'
          ? 'expired'
          : this.loginStatus === 'success'
            ? 'success'
            : 'waiting';
    const challenge = loginChallenge(state);
    if (state === 'success') {
      this.account = { ...VIP_ACCOUNT };
      this.emitAccount();
    }
    return {
      key: challenge.sessionId,
      qrMatrix: challenge.modules,
      qrUrl: '',
      expiresInSecs: challenge.expiresInSec,
      status: this.loginStatus,
    };
  };

  loginPoll = async (): Promise<LoginPollDTO> => {
    const messages: Record<ContractLoginStatus, string> = {
      waiting: '使用 B 站 App 扫码登录',
      scanned: '已扫码，请在手机上确认',
      success: '登录成功',
      expired: '二维码已过期',
      error: '登录失败，请重试',
    };
    if (this.loginStatus === 'success') {
      this.account = { ...VIP_ACCOUNT };
      this.emitAccount();
    }
    return {
      status: this.loginStatus,
      message: messages[this.loginStatus],
      account: this.loginStatus === 'success' ? A.accountToDTO(this.account) : null,
    };
  };

  loginLogout = async (): Promise<void> => {
    this.account = { ...ANONYMOUS_ACCOUNT };
    this.emitAccount();
  };

  refreshAccount = async (): Promise<AccountInfoDTO | null> => {
    this.emitAccount();
    return A.accountToDTO(this.account);
  };

  // ---- 设置 ----
  getSettings = async (): Promise<SettingsDTO> => {
    await this.awaitCore();
    return this.settingsDTO();
  };

  setSettings = async (settings: SettingsDTO): Promise<SettingsDTO> => {
    this.settings = A.settingsFromDTO(settings, this.settings);
    this.settingsRevision += 1;
    this.emitSettings();
    return this.settingsDTO();
  };

  /** 增量提交（design D3）：省略字段保持不变、null 清空可空字段；Mock 与 Rust 同语义。 */
  patchSettings = async (patch: SettingsPatchDTO): Promise<SettingsDTO> => {
    if (this.settingsPatchFailures > 0) {
      this.settingsPatchFailures -= 1;
      throw {
        kind: 'disk',
        message: '设置保存失败：无法写入设置文件（dev 注入）',
        actions: ['retry'],
      };
    }
    this.settings = A.applySettingsPatchDTO(this.settings, patch);
    this.settingsRevision += 1;
    this.emitSettings();
    return this.settingsDTO();
  };

  // ---- 历史 ----
  listHistory = async (filter?: HistoryFilterDTO): Promise<HistoryEntryDTO[]> => {
    await this.awaitCore();
    if (this.historyFailures > 0) {
      this.historyFailures -= 1;
      throw { kind: 'disk', message: '历史记录读取失败：database is locked（dev 注入）' };
    }
    let items = [...this.history];
    if (filter?.status) items = items.filter((entry) => entry.status === filter.status);
    if (filter?.keyword?.trim()) {
      const keyword = filter.keyword.trim().toLowerCase();
      items = items.filter(
        (entry) =>
          entry.title.toLowerCase().includes(keyword) || entry.uploader.toLowerCase().includes(keyword),
      );
    }
    items.sort((a, b) => b.finishedAt - a.finishedAt);
    return items.map(A.historyToDTO);
  };

  removeHistory = async (id: string): Promise<void> => {
    this.history = this.history.filter((entry) => entry.id !== id);
    this.emitHistory();
  };

  clearHistory = async (): Promise<number> => {
    const count = this.history.length;
    this.history = [];
    this.emitHistory();
    return count;
  };

  // ---- 系统集成 ----
  pickDirectory = async (): Promise<string | null> => '~/Downloads/bilidown/备份';
  revealOutput = async (): Promise<void> => {};
  revealPath = async (path: string): Promise<void> => {
    this.emitNotice('info', `定位文件：${path}`);
  };
  openFile = async (path: string): Promise<void> => {
    this.emitNotice('info', `打开文件：${path}`);
  };
  showLogs = async (): Promise<void> => {};
  getMenuState = async (): Promise<MenuStateDTO> => ({
    enabled: {
      'start-all': this.tasks.some((task) => task.status === 'paused'),
      'pause-all': this.tasks.some((task) =>
        ['queued', 'downloading', 'resolving', 'merging'].includes(task.status),
      ),
      'retry-all': this.tasks.some((task) => task.status === 'failed'),
      'clear-finished': this.tasks.some((task) => task.status === 'completed'),
      'clear-inactive': this.tasks.some((task) =>
        ['queued', 'paused', 'cancelled'].includes(task.status),
      ),
    },
  });
  requestQuitCheck = async (): Promise<boolean> =>
    this.tasks.some((task) => !TERMINAL.includes(task.status));

  copyText = async (text: string): Promise<void> => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* mock 环境可能无剪贴板权限 */
    }
  };
  readClipboardText = async (): Promise<string> =>
    'https://www.bilibili.com/video/BV1mock0001?p=3';
  openExternal = async (): Promise<void> => {};
  detectFfmpeg = async (): Promise<{ detected: boolean; version: string | null; path: string | null }> => ({
    detected: this.settings.quality.ffmpegDetected,
    version: this.settings.quality.ffmpegVersion,
    path: this.settings.quality.ffmpegPath,
  });
  fetchCover = async (url: string): Promise<string> => url;

  testProxy = async (proxy: SettingsDTO['proxy']): Promise<{ ok: boolean; message: string }> => {
    await new Promise((resolve) => setTimeout(resolve, 400));
    if (proxy.mode === 'direct') return { ok: true, message: '直连可用' };
    return proxy.host
      ? { ok: true, message: `已连接 ${proxy.host}:${proxy.port ?? 0}` }
      : { ok: false, message: '请输入代理地址' };
  };

  // ---- 事件 ----
  on<T>(event: ContractEventName, handler: (payload: T) => void): () => void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    const wrapped = handler as (payload: unknown) => void;
    set.add(wrapped);
    return () => {
      set?.delete(wrapped);
    };
  }

  // ---- 场景预置（仅 mock） ----
  scenarioBootstrap = async (): Promise<ScenarioBootstrap | null> => {
    const params = devParams();
    const base: ScenarioBootstrap = {};
    const notices = [] as ScenarioBootstrap['notices'];
    switch (this.scenario) {
      case 'history':
      case 'history-artifacts':
      case 'history-legacy':
      case 'history-empty':
        base.view = 'history';
        break;
      case 'add-link':
        base.panel = 'add-link';
        break;
      case 'add-multi-p':
        // 默认只勾选当前集的多条目内容：解析后仍应进入确认面板（design D6/6.2）
        base.panel = 'add-link';
        break;
      case 'add-panel':
      case 'add-panel-preferences':
      case 'add-panel-no-fallback':
        base.panel = 'add';
        base.parseResult = addPanelParseResult();
        break;
      case 'task-detail':
        base.panel = 'detail';
        base.taskId = 'task-dl';
        break;
      case 'task-detail-failed':
        base.panel = 'detail';
        base.taskId = 'task-fail';
        break;
      case 'task-detail-done':
      case 'task-detail-artifacts':
        base.panel = 'detail';
        base.taskId = 'task-done';
        break;
      case 'login':
      case 'login-scanned':
      case 'login-expired':
      case 'login-failed':
      case 'login-success':
        base.panel = 'login';
        break;
      case 'errors':
        notices?.push({
          id: 'notice-login-expired',
          kind: 'error',
          message: '登录已失效，请重新扫码后重试',
          actionLabel: '重新登录',
        });
        break;
      case 'quit-failure':
        // 场景直接展示「保存失败」对话框；把首次失败记为已发生，重试保存即可成功演示
        this.quitAttempts = 1;
        base.quitFailure = '任务状态保存失败：database is locked';
        break;
      default:
        break;
    }
    if (params.category) base.category = params.category;
    if (notices && notices.length > 0) base.notices = notices;
    return base;
  };
}
