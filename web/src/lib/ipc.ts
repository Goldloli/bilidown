import { CONTRACT_EVENTS, type ContractEventName } from './contract';
import type {
  AccountInfoDTO,
  AddOptionsDTO,
  AddTasksResultDTO,
  AppInfoDTO,
  ClearOutcomeDTO,
  ContractHistoryStatus,
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
} from './contract';
import type {
  AccountState,
  ClearOutcome,
  EnqueueFailure,
  EnqueueOutcome,
  EnqueueSelection,
  HistoryEntry,
  HistoryQuery,
  LoginChallenge,
  LoginPollResult,
  ParseFailure,
  ParseResult,
  QueueSnapshot,
  QueueTask,
  Settings,
  SettingsPatch,
} from './types';
import {
  ANONYMOUS_ACCOUNT,
  accountFromDTO,
  historyFromDTO,
  loginChallengeFromDTO,
  loginPollFromDTO,
  parseResultFromDTO,
  settingsFromDTO,
  settingsPatchToDTO,
  settingsRevisionOf,
  snapshotFromDTO,
  type AdaptedSettings,
} from './adapt';
import { requiresAddConfirmation } from './addSelection';
import { errorMessageOf } from './errors';

export type { ContractEventName };

/**
 * Tauri IPC 契约后端（与 src-tauri/COMMANDS.md 一一对应）。
 * mock 与真实后端都实现这一层；UI 只使用下方 ApiClient 的高层方法（内部经 adapt.ts 转换）。
 */
export interface Backend {
  getAppInfo(): Promise<AppInfoDTO>;
  openPreferences(): Promise<void>;
  /** 保存失败时 reject（AppError），不再总是退出；前端提供重试/仍然退出（design D8/3.9） */
  quitApp(): Promise<void>;
  /** 用户明确放弃未保存状态后强制退出 */
  forceQuitApp(): Promise<void>;
  /** 本地初始化失败后重试；仅在 core 未就绪时生效（design D5/5.4） */
  retryInit(): Promise<void>;
  /** 本地已就绪时重新尝试设备激活/账号恢复（design D5/5.4） */
  retryNetwork(): Promise<void>;
  getDiagnostics(): Promise<string>;

  parseInput(input: string): Promise<ParseResultDTO>;
  addTasks(input: string, options: AddOptionsDTO): Promise<AddTasksResultDTO>;
  listQueue(): Promise<QueueSnapshotDTO>;
  pauseTask(taskId: string): Promise<void>;
  resumeTask(taskId: string): Promise<void>;
  cancelTask(taskId: string): Promise<void>;
  retryTask(taskId: string): Promise<void>;
  removeTask(taskId: string): Promise<void>;
  startAll(): Promise<number>;
  pauseAll(): Promise<number>;
  retryAll(): Promise<number>;
  clearFinished(): Promise<ClearOutcomeDTO>;
  clearInactive(): Promise<ClearOutcomeDTO>;
  /** 仅重试待保存动作（记录/完成事务/历史），不清空产物、不重新下载 */
  retryPersistence(): Promise<void>;

  getAccount(): Promise<AccountInfoDTO | null>;
  loginStart(): Promise<LoginStartDTO>;
  loginPoll(): Promise<LoginPollDTO>;
  loginLogout(): Promise<void>;
  refreshAccount(): Promise<AccountInfoDTO | null>;

  getSettings(): Promise<SettingsDTO>;
  setSettings(settings: SettingsDTO): Promise<SettingsDTO>;
  /** 增量提交（design D3）；省略字段=保持不变，null=清空可空字段 */
  patchSettings(patch: SettingsPatchDTO): Promise<SettingsDTO>;

  listHistory(filter?: HistoryFilterDTO): Promise<HistoryEntryDTO[]>;
  removeHistory(id: string): Promise<void>;
  clearHistory(): Promise<number>;

  pickDirectory(): Promise<string | null>;
  revealOutput(taskId: string): Promise<void>;
  revealPath(path: string): Promise<void>;
  openFile(path: string): Promise<void>;
  showLogs(): Promise<void>;
  getMenuState(): Promise<MenuStateDTO>;
  requestQuitCheck(): Promise<boolean>;

  /** EXTENSION（契约表未列，已同步 Lead）：剪贴板/外链/ffmpeg/代理 */
  copyText(text: string): Promise<void>;
  readClipboardText(): Promise<string>;
  openExternal(url: string): Promise<void>;
  detectFfmpeg(): Promise<{ detected: boolean; version: string | null; path: string | null }>;
  testProxy(proxy: SettingsDTO['proxy']): Promise<{ ok: boolean; message: string }>;
  fetchCover(url: string): Promise<string>;

  on<T>(event: ContractEventName, handler: (payload: T) => void): () => void;

  /** 仅 mock 后端：按 URL scenario 预置 UI 状态 */
  scenarioBootstrap?(): Promise<ScenarioBootstrap | null>;
}

export interface ScenarioBootstrap {
  view?: 'queue' | 'history';
  panel?: 'add-link' | 'add' | 'detail' | 'login';
  parseResult?: ParseResult;
  taskId?: string;
  category?: string;
  notices?: import('./types').AppNotice[];
  /** 退出保存失败弹窗（dev QA 场景） */
  quitFailure?: string;
}

export type { EnqueueSelection };

function clearOutcome(dto: ClearOutcomeDTO): ClearOutcome {
  return { removed: dto?.removed ?? 0, failed: dto?.failed ?? [] };
}

/** 高层客户端：契约命令 + UI 适配（stores 只依赖这一层） */
class ApiClient {
  private backend: Backend | null = null;
  /** 最近一次被接受的设置（含服务端 revision），仅作前端缓存与补丁基底 */
  private lastSettings: AdaptedSettings | null = null;
  /** 已接受的最大设置修订号；-1 表示尚未建立/已重置 */
  private lastRevision = -1;
  /**
   * 账号网络校验是否可采信（tasks 5.4）：未校验时不拿临时匿名态限制清晰度兜底。
   * 由启动协调器在 `shell://core-status` 后更新。
   */
  private accountTrusted = false;

  setBackend(backend: Backend): void {
    this.backend = backend;
    // 重新建立后端会话时重置本地修订状态（design D3）。
    this.lastSettings = null;
    this.lastRevision = -1;
    this.accountTrusted = false;
  }

  /** 账号校验阶段变化（ready/degraded 后可采信网络/账号结果） */
  setAccountTrusted(trusted: boolean): void {
    this.accountTrusted = trusted;
  }

  get isAccountTrusted(): boolean {
    return this.accountTrusted;
  }

  /** 已接受设置的服务端修订号（测试与调试用） */
  get settingsRevision(): number {
    return this.lastRevision;
  }

  get isReady(): boolean {
    return this.backend !== null;
  }

  private b(): Backend {
    if (!this.backend) throw new Error('IPC backend 尚未初始化');
    return this.backend;
  }

  // ---- 队列 ----
  getQueueSnapshot = async (): Promise<QueueSnapshot> =>
    snapshotFromDTO(await this.b().listQueue());

  getTask = async (id: string): Promise<QueueTask | null> => {
    const snapshot = await this.getQueueSnapshot();
    return snapshot.tasks.find((task) => task.id === id) ?? null;
  };

  pauseTask = (taskId: string) => this.b().pauseTask(taskId);
  resumeTask = (taskId: string) => this.b().resumeTask(taskId);
  cancelTask = (taskId: string) => this.b().cancelTask(taskId);
  retryTask = (taskId: string) => this.b().retryTask(taskId);
  removeTask = (taskId: string) => this.b().removeTask(taskId);
  retryPersistence = () => this.b().retryPersistence();
  pauseAll = () => this.b().pauseAll();
  resumeAll = () => this.b().startAll();
  retryFailed = () => this.b().retryAll();
  clearCompleted = async (): Promise<ClearOutcome> => clearOutcome(await this.b().clearFinished());
  clearInactive = async (): Promise<ClearOutcome> => clearOutcome(await this.b().clearInactive());

  // ---- 解析 / 入队 ----
  parseLinks = async (text: string): Promise<ParseResult> => {
    const lines = text
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    const inputs = lines.length > 0 ? lines : [text.trim()];
    const account = await this.getAccount();
    // 网络校验未完成时账号结果只用于展示，不用它限制清晰度兜底（tasks 5.4）。
    const accountTrusted = this.accountTrusted || account.signedIn;
    const items: ParseResult['items'] = [];
    const failures: ParseFailure[] = [];
    for (const input of inputs) {
      try {
        const dto = await this.b().parseInput(input);
        items.push(...parseResultFromDTO(dto, account, accountTrusted).items);
      } catch (error) {
        failures.push({ raw: input, reason: errorMessageOf(error) });
      }
    }
    return {
      items,
      failures,
      requiresConfirmation: requiresAddConfirmation(items),
    };
  };

  /**
   * 按内容分别入队（design D8/6.4）：一次确认可能拆成多个独立 add_tasks 请求。
   * 单个请求失败不影响其他内容的结果；返回逐内容成功/失败，供 UI 只重试失败内容。
   */
  enqueue = async (parse: ParseResult, selection: EnqueueSelection): Promise<EnqueueOutcome> => {
    const taskIds: string[] = [];
    const succeededItemIds: string[] = [];
    const failures: EnqueueFailure[] = [];
    for (const item of parse.items) {
      if (!selection.itemIds.includes(item.id)) continue;
      const selectedIds = selection.entries[item.id];
      // 明确清空的内容不创建任务，也不回落到默认选择
      if (selectedIds?.length === 0) continue;
      const entryIndexes =
        selectedIds === undefined
          ? undefined
          : item.entries
              .map((entry, index) => (selectedIds.includes(entry.id) ? index : -1))
              .filter((index) => index >= 0);
      const qualityQn = selection.quality[item.id] ?? 0;
      const options: AddOptionsDTO = {
        quality: '',
        qualityQn,
        codec: selection.codec[item.id] ?? 'avc',
        audioFormat: selection.audio[item.id] ?? 'original',
        audioQn: selection.audioQn?.[item.id],
        outputDir: selection.outputDir,
        entryIndexes,
        note: selection.note,
      };
      try {
        const result = await this.b().addTasks(item.rawInput ?? item.pageUrl, options);
        taskIds.push(...result.taskIds);
        if (result.taskIds.length > 0 || result.rejected.length === 0) {
          succeededItemIds.push(item.id);
        }
        if (result.rejected.length > 0 && result.taskIds.length === 0) {
          failures.push({
            itemId: item.id,
            title: item.title,
            message: result.rejected.map((rejected) => rejected.reason).join('；'),
          });
        }
      } catch (error) {
        failures.push({ itemId: item.id, title: item.title, message: errorMessageOf(error) });
      }
    }
    return { taskIds, succeededItemIds, failures };
  };

  // ---- 设置 ----

  /** 按修订号接收设置：更旧的响应/事件不得覆盖较新的已接受值（design D3）。 */
  private acceptSettings(settings: AdaptedSettings): AdaptedSettings {
    const revision = settingsRevisionOf(settings);
    if (revision !== undefined && revision < this.lastRevision) {
      return this.lastSettings ?? settings;
    }
    this.lastSettings = settings;
    if (revision !== undefined && revision > this.lastRevision) {
      this.lastRevision = revision;
    }
    return settings;
  }

  /**
   * 事件/启动读取路径：Store 判定接收后再同步 ApiClient 缓存，
   * 保证 `lastSettings` 与 Store 始终一致。
   */
  applySettingsSnapshot(settings: AdaptedSettings): void {
    this.acceptSettings(settings);
  }

  getSettings = async (): Promise<AdaptedSettings> => {
    const dto = await this.b().getSettings();
    return this.acceptSettings(settingsFromDTO(dto, this.lastSettings));
  };

  /** 增量提交一个设置补丁；后端已按 patch_settings 原子合并并返回新修订号。 */
  patchSettings = async (patch: SettingsPatch): Promise<AdaptedSettings> => {
    const base = this.lastSettings ?? (await this.getSettings());
    const dto = settingsPatchToDTO(patch, base);
    const saved = settingsFromDTO(await this.b().patchSettings(dto), base);
    return this.acceptSettings(saved);
  };

  // ---- 账户 / 登录 ----
  getAccount = async (): Promise<AccountState> => {
    const dto = await this.b().getAccount();
    return accountFromDTO(dto);
  };

  createLoginSession = async (): Promise<LoginChallenge> =>
    loginChallengeFromDTO(await this.b().loginStart());

  pollLogin = async (_sessionId: string): Promise<LoginPollResult> =>
    loginPollFromDTO(await this.b().loginPoll());

  cancelLogin = async (_sessionId: string): Promise<void> => {
    // 契约无「取消登录」命令：关闭面板即停止轮询，登录态不变
  };

  refreshAccount = async (): Promise<AccountState> => {
    const dto = await this.b().refreshAccount();
    return accountFromDTO(dto);
  };

  logout = async (): Promise<void> => {
    await this.b().loginLogout();
  };

  // ---- 历史 ----
  getHistory = async (query: HistoryQuery): Promise<HistoryEntry[]> => {
    const filter: HistoryFilterDTO = {};
    if (query.status !== 'all') filter.status = query.status as ContractHistoryStatus;
    if (query.keyword.trim()) filter.keyword = query.keyword.trim();
    const entries = await this.b().listHistory(filter);
    return entries.map(historyFromDTO).sort((a, b) => b.finishedAt - a.finishedAt);
  };

  deleteHistory = (id: string) => this.b().removeHistory(id);
  clearHistory = async (): Promise<void> => {
    await this.b().clearHistory();
  };

  fetchCover = (url: string): Promise<string> => this.b().fetchCover(url);

  // ---- 系统 ----
  readClipboardText = () => this.b().readClipboardText();
  chooseDirectory = async (_current: string): Promise<string | null> => this.b().pickDirectory();
  openInFinder = (path: string) => this.b().revealPath(path);
  openFile = (path: string) => this.b().openFile(path);
  openExternal = (url: string) => this.b().openExternal(url);
  copyText = (text: string) => this.b().copyText(text);
  openPreferencesWindow = () => this.b().openPreferences();
  revealLogs = () => this.b().showLogs();
  copyDiagnostics = async (): Promise<string> => {
    const diagnostics = await this.b().getDiagnostics();
    await this.b().copyText(diagnostics);
    return diagnostics;
  };
  detectFfmpeg = () => this.b().detectFfmpeg();
  testProxy = (network: Settings['network']) =>
    this.b().testProxy({
      mode: network.proxyMode,
      host: network.proxyHost,
      port: network.proxyPort,
      user: network.proxyUser,
      pass: network.proxyPass,
    });
  quitApp = () => this.b().quitApp();
  forceQuitApp = () => this.b().forceQuitApp();
  retryInit = () => this.b().retryInit();
  retryNetwork = () => this.b().retryNetwork();
  requestQuitCheck = () => this.b().requestQuitCheck();
  getAppInfo = () => this.b().getAppInfo();
  getMenuState = () => this.b().getMenuState();

  on<T>(event: ContractEventName, handler: (payload: T) => void): () => void {
    return this.b().on(event, handler);
  }

  onQueueEvent(handler: (event: TaskEventDTO) => void): () => void {
    return this.b().on(CONTRACT_EVENTS.queueEvents, handler);
  }

  scenarioBootstrap = async (): Promise<ScenarioBootstrap | null> => {
    const backend = this.b();
    return backend.scenarioBootstrap ? backend.scenarioBootstrap() : null;
  };
}

export function createApiClient(): ApiClient {
  return new ApiClient();
}

export const api = new ApiClient();
export { CONTRACT_EVENTS };
export const EMPTY_ACCOUNT = ANONYMOUS_ACCOUNT;
export type { QueueSnapshotDTO };
