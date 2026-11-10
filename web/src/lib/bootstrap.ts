/**
 * 启动装配与分阶段协调（design D5 / tasks 5.1/5.3/5.4）。
 *
 * 首屏先渲染骨架，随后在后台完成：选择后端 → 一次性订阅事件 → 并行加载
 * 设置/队列/历史/账号/应用信息。任何单一分区失败只影响该分区，并提供局部重试；
 * 核心初始化失败复用同一个初始化任务与同一组事件订阅重试，网络阶段按代次丢弃旧结果。
 */
import { api, type Backend } from './ipc';
import { CONTRACT_EVENTS } from './contract';
import type {
  AccountInfoDTO,
  AppInfoDTO,
  CoreStatusDTO,
  DropPayloadDTO,
  HistoryEntryDTO,
  MenuActionDTO,
  NoticeDTO,
  QueueSnapshotDTO,
  QueueStatsDTO,
  SettingsDTO,
  TaskEventDTO,
} from './contract';
import { devParams } from '../dev/scenario';
import {
  accountFromDTO,
  historyFromDTO,
  settingsFromDTO,
  snapshotFromDTO,
} from './adapt';
import { errorMessageOf } from './errors';
import {
  applyTheme,
  readThemeOverride,
  systemTheme,
  watchSystemTheme,
} from './theme';
import { applyPlatform, applyCapabilities } from './platform';
import { applyGlass, readGlassPref } from './glass';
import { useQueueStore } from '../stores/useQueueStore';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useAccountStore } from '../stores/useAccountStore';
import { useHistoryStore } from '../stores/useHistoryStore';
import { useUiStore } from '../stores/useUiStore';
import {
  STARTUP_SECTIONS,
  useStartupStore,
  type StartupSectionKey,
} from '../stores/useStartupStore';
import { clearCoverCache } from './cover';
import { handleMenuAction } from './menu';
import { addFromClipboard, handleParseText } from './actions';
import { attemptQuit } from './quit';

/** `core 初始化失败: …` / `引擎尚未就绪` 等表示整个 core 不可用，需走统一初始化重试。 */
export function isCoreInitError(message: string | null | undefined): boolean {
  if (!message) return false;
  return /core\s*初始化失败|引擎初始化失败|引擎尚未就绪|初始化失败/i.test(message);
}

interface StartupOptions {
  /** 测试注入；生产走 mock/Tauri 动态加载 */
  backend?: Backend;
}

interface ToastOnlyNotice {
  justUpdated?: string;
}

export class StartupCoordinator {
  private backend: Backend | null = null;
  private starting: Promise<void> | null = null;
  private subscribed = false;
  private disposers: Array<() => void> = [];
  private sectionPromises = new Map<StartupSectionKey, Promise<boolean>>();
  private retryCorePromise: Promise<void> | null = null;
  private retryNetworkPromise: Promise<void> | null = null;
  private fileRefreshPromise: Promise<void> | null = null;
  private fileRefreshQueued = false;
  private scenarioStarted = false;
  private themeWatcherInstalled = false;
  private appInfoNotice: ToastOnlyNotice = {};

  constructor(private readonly options: StartupOptions = {}) {}

  start(): Promise<void> {
    this.starting ??= this.run();
    return this.starting;
  }

  dispose(): void {
    for (const dispose of this.disposers) dispose();
    this.disposers = [];
    this.subscribed = false;
    this.starting = null;
    this.sectionPromises.clear();
    this.retryCorePromise = null;
    this.retryNetworkPromise = null;
    this.fileRefreshPromise = null;
    this.fileRefreshQueued = false;
    this.scenarioStarted = false;
    this.themeWatcherInstalled = false;
  }

  /** 跟随系统外观（仅跟随系统且无截图覆盖时）。 */
  private installSystemThemeWatcher(): void {
    if (this.themeWatcherInstalled) return;
    this.themeWatcherInstalled = true;
    watchSystemTheme((theme) => {
      const appearance = useSettingsStore.getState().settings?.interface.appearance;
      if (appearance === 'system' && !readThemeOverride()) applyTheme(theme);
    });
  }

  /** 局部重试：核心失败时复用单一初始化任务，否则只重载该分区。 */
  async retrySection(key: StartupSectionKey): Promise<void> {
    const state = useStartupStore.getState();
    // 只有整个 core 失败时才走统一初始化重试；否则只重载该分区
    // （核心已就绪但单条命令短暂失败时，命令本身会等待/重试）。
    if (state.corePhase === 'failed') {
      await this.retryCore();
      return;
    }
    await this.loadSection(key);
  }

  /** 核心初始化重试：只有一个在途初始化任务，事件订阅不重复安装。 */
  retryCore(): Promise<void> {
    if (this.retryCorePromise) return this.retryCorePromise;
    this.retryCorePromise = (async () => {
      const store = useStartupStore.getState();
      if (store.corePhase !== 'failed') return;
      store.beginCoreRetry();
      try {
        await api.retryInit();
        // retry_init 只触发后台初始化；重新发起失败分区读取（命令内部等待 core 就绪）。
        useStartupStore.getState().resetFailedSections();
        await Promise.all(STARTUP_SECTIONS.map((key) => this.loadSection(key)));
        // 命令已成功说明 core 本地阶段恢复；若状态事件迟到，界面也不停留在失败态。
        const sections = useStartupStore.getState().sections;
        if (!STARTUP_SECTIONS.some((key) => isCoreInitError(sections[key].error))) {
          useStartupStore.getState().applyCoreStatus({
            stage: 'local-ready',
            generation: useStartupStore.getState().networkGeneration,
            degraded: false,
          });
          void this.applyScenarioWhenSettled();
        }
      } catch (error) {
        useStartupStore.getState().markCoreFailure(errorMessageOf(error));
      } finally {
        useStartupStore.getState().endCoreRetry();
      }
    })().finally(() => {
      this.retryCorePromise = null;
    });
    return this.retryCorePromise;
  }

  /** 网络阶段重试：进入校验中，旧代次结果会被后续状态与账号守卫丢弃。 */
  retryNetwork(): Promise<void> {
    if (this.retryNetworkPromise) return this.retryNetworkPromise;
    this.retryNetworkPromise = (async () => {
      const store = useStartupStore.getState();
      if (store.corePhase === 'failed' || store.corePhase === 'booting') {
        await this.retryCore();
        return;
      }
      store.beginNetworkRetry();
      useAccountStore.getState().beginValidation();
      try {
        await api.retryNetwork();
      } catch (error) {
        useAccountStore
          .getState()
          .finishValidation(store.networkGeneration, true, errorMessageOf(error));
        useStartupStore.getState().endNetworkRetry();
      }
    })().finally(() => {
      this.retryNetworkPromise = null;
    });
    return this.retryNetworkPromise;
  }

  /** `files://invalidated`：重新拉取队列/历史文件状态；同一时间只跑一批，期间再次失效会补跑。 */
  refreshFiles(): Promise<void> {
    if (this.fileRefreshPromise) {
      this.fileRefreshQueued = true;
      return this.fileRefreshPromise;
    }
    this.fileRefreshPromise = (async () => {
      do {
        this.fileRefreshQueued = false;
        await this.performFileRefresh();
      } while (this.fileRefreshQueued);
    })().finally(() => {
      this.fileRefreshPromise = null;
    });
    return this.fileRefreshPromise;
  }

  // ------------------------------------------------------------------
  // 装配
  // ------------------------------------------------------------------

  private async run(): Promise<void> {
    useStartupStore.getState().clearFatalError();
    try {
      // 1) 尽早应用主题，避免白闪；settings 到达后再以设置为准。
      applyTheme(readThemeOverride() ?? systemTheme());
      this.installSystemThemeWatcher();
      await this.ensureBackend();
      this.installSubscriptions();
      this.kickOffSections();
      void this.applyScenarioWhenSettled();
    } catch (error) {
      // 后端装配失败：窗口仍渲染（BootError 可读错误 + 重试），允许再次 start。
      this.starting = null;
      useStartupStore.getState().setFatalError(errorMessageOf(error));
    }
  }

  private async ensureBackend(): Promise<void> {
    if (this.backend) {
      api.setBackend(this.backend);
      return;
    }
    if (this.options.backend) {
      this.backend = this.options.backend;
      api.setBackend(this.backend);
      return;
    }
    const params = devParams();
    if (params.mock) {
      const { MockBackend } = await import('../dev/MockBackend');
      this.backend = new MockBackend();
    } else {
      const { createTauriBackend } = await import('./tauri-backend');
      this.backend = createTauriBackend();
    }
    api.setBackend(this.backend);
  }

  /** 只安装一次事件订阅；重试只复用这些订阅（tasks 5.4）。 */
  private installSubscriptions(): void {
    if (this.subscribed) return;
    this.subscribed = true;
    const keep = (dispose: () => void) => this.disposers.push(dispose);

    keep(
      api.on<QueueSnapshotDTO>(CONTRACT_EVENTS.queueSnapshot, (dto) => {
        const snapshot = snapshotFromDTO(dto);
        useQueueStore.getState().setSnapshot(snapshot);
        const generation = useStartupStore.getState().networkGeneration;
        useAccountStore
          .getState()
          .applyServerAccount(snapshot.account, { source: 'snapshot', generation });
      }),
    );
    keep(
      api.on<TaskEventDTO>(CONTRACT_EVENTS.queueEvents, (event) => {
        if (event.kind === 'removed') {
          useQueueStore.getState().handleQueueEvent({ type: 'task-removed', taskId: event.taskId });
        } else if (event.kind === 'failed') {
          useUiStore.getState().pushToast({
            kind: 'error',
            message: event.message ?? '任务失败',
          });
        }
      }),
    );
    keep(
      api.on<AccountInfoDTO | null>(CONTRACT_EVENTS.accountChanged, (dto) => {
        useAccountStore.getState().applyServerAccount(accountFromDTO(dto), {
          source: 'event',
          generation: useStartupStore.getState().networkGeneration,
        });
      }),
    );
    keep(
      api.on<SettingsDTO>(CONTRACT_EVENTS.settingsChanged, (dto) => {
        const store = useSettingsStore.getState();
        store.setSettings(settingsFromDTO(dto, store.settings));
      }),
    );
    keep(
      api.on<HistoryEntryDTO[]>(CONTRACT_EVENTS.historyChanged, (entries) => {
        useHistoryStore.getState().setEntries(entries.map(historyFromDTO));
      }),
    );
    keep(
      api.on<NoticeDTO>(CONTRACT_EVENTS.notice, (notice) => {
        if (notice.code === 'clipboard-link') {
          useUiStore.getState().pushToast({
            kind: notice.level,
            message: notice.message,
            actionLabel: '一键添加',
            durationMs: 8000,
            action: () => void addFromClipboard(),
          });
          return;
        }
        useUiStore.getState().pushToast({ kind: notice.level, message: notice.message });
      }),
    );
    keep(
      api.on<MenuActionDTO>(CONTRACT_EVENTS.shellMenu, (action) => {
        void handleMenuAction(action.id);
      }),
    );
    keep(
      api.on<DropPayloadDTO>(CONTRACT_EVENTS.shellDrop, (payload) => {
        const text = payload.text || payload.paths.join('\n');
        if (text.trim()) void handleParseText(text);
      }),
    );
    keep(
      api.on<QueueStatsDTO>(CONTRACT_EVENTS.shellQuitRequested, (stats) => {
        void (async () => {
          const pending = stats.queued + stats.active + stats.paused;
          const confirmed = await useUiStore.getState().confirm({
            title: '退出 bilidown？',
            body: `有 ${pending} 个未完成任务。退出后将暂停传输并保存进度，下次启动时可恢复。`,
            confirmLabel: '退出',
          });
          // 保存失败时由 attemptQuit 显示「重试保存 / 仍然退出」；正常退出不额外确认。
          if (confirmed) await attemptQuit();
        })();
      }),
    );
    // 启动分阶段与失效事件（design D5/D9/D11，tasks 5.4/7.2/7.6）
    keep(
      api.on<CoreStatusDTO>(CONTRACT_EVENTS.coreStatus, (status) => {
        this.handleCoreStatus(status);
      }),
    );
    keep(
      api.on(CONTRACT_EVENTS.coversCleared, () => {
        clearCoverCache();
      }),
    );
    keep(
      api.on(CONTRACT_EVENTS.filesInvalidated, () => {
        void this.refreshFiles();
      }),
    );
  }

  private handleCoreStatus(status: CoreStatusDTO): void {
    const store = useStartupStore.getState();
    store.applyCoreStatus(status);
    // 旧代次状态已被 store 丢弃；这里只处理被接受的状态。
    if (status.generation < store.networkGeneration) return;
    const account = useAccountStore.getState();
    switch (status.stage) {
      case 'local-ready':
        // 本地就绪：凭据已装载，账号等待网络校验。
        account.beginValidation(status.generation);
        api.setAccountTrusted(false);
        void this.reloadFailedSections();
        return;
      case 'network-ready':
        api.setAccountTrusted(true);
        account.finishValidation(status.generation, false);
        useStartupStore.getState().endNetworkRetry();
        return;
      case 'network-degraded':
        api.setAccountTrusted(false);
        account.finishValidation(status.generation, true, status.error ?? null);
        useStartupStore.getState().endNetworkRetry();
        return;
    }
  }

  private kickOffSections(): void {
    for (const key of STARTUP_SECTIONS) void this.loadSection(key);
  }

  /** 核心重试成功后，只重载此前失败的分区；已成功分区不重复请求。 */
  private async reloadFailedSections(): Promise<void> {
    const failed = STARTUP_SECTIONS.filter(
      (key) => useStartupStore.getState().sections[key].status === 'error',
    );
    if (failed.length === 0) return;
    await Promise.all(failed.map((key) => this.loadSection(key)));
  }

  private loadSection(key: StartupSectionKey): Promise<boolean> {
    const existing = this.sectionPromises.get(key);
    if (existing) return existing;
    const promise = this.runSection(key).finally(() => {
      this.sectionPromises.delete(key);
    });
    this.sectionPromises.set(key, promise);
    return promise;
  }

  private async runSection(key: StartupSectionKey): Promise<boolean> {
    useStartupStore.getState().markSectionLoading(key);
    try {
      await this.loaders[key]();
      useStartupStore.getState().markSectionReady(key);
      return true;
    } catch (error) {
      const message = errorMessageOf(error);
      useStartupStore.getState().markSectionError(key, message);
      if (isCoreInitError(message)) {
        useStartupStore.getState().markCoreFailure(message);
      } else if (key === 'appInfo') {
        // 应用信息独立失败：提示一次并允许局部重试，不阻塞主窗口。
        useUiStore.getState().pushToast({
          kind: 'warning',
          message: `应用信息加载失败：${message}`,
          actionLabel: '重试',
          durationMs: 8000,
          action: () => void this.retrySection('appInfo'),
        });
      }
      return false;
    }
  }

  private readonly loaders: Record<StartupSectionKey, () => Promise<void>> = {
    settings: async () => {
      const settings = await api.getSettings();
      useSettingsStore.getState().setSettings(settings);
    },
    queue: async () => {
      const ok = await useQueueStore.getState().refresh();
      if (!ok) throw new Error(useQueueStore.getState().error ?? '队列加载失败');
    },
    history: async () => {
      const ok = await useHistoryStore.getState().load();
      if (!ok) throw new Error(useHistoryStore.getState().error ?? '历史记录加载失败');
    },
    account: async () => {
      const generation = useStartupStore.getState().networkGeneration;
      const account = await api.getAccount();
      useAccountStore.getState().applyServerAccount(account, {
        source: 'initial',
        generation,
      });
    },
    appInfo: async () => {
      const info = await api.getAppInfo();
      this.applyAppInfo(info);
    },
  };

  private applyAppInfo(info: AppInfoDTO): void {
    applyPlatform(info.os);
    applyCapabilities({
      glassSupported: info.glassSupported,
      glassUnavailableReason: info.glassUnavailableReason,
      notificationsSupported: info.notificationsSupported,
    });
    // 能力确定后重新应用玻璃偏好：不支持原生材质时强制不透明。
    const settings = useSettingsStore.getState().settings;
    applyGlass(
      Boolean(info.glassSupported && (settings?.interface.glassBackground ?? readGlassPref())),
    );
    if (info.justUpdated && this.appInfoNotice.justUpdated !== info.version) {
      this.appInfoNotice.justUpdated = info.version;
      useUiStore.getState().pushToast({
        kind: 'success',
        message: `已更新到 ${info.version}`,
        durationMs: 8000,
      });
    }
  }

  /** mock 场景预置（面板/视图/toast）：等分区加载结束后执行，不阻塞首屏。 */
  private async applyScenarioWhenSettled(): Promise<void> {
    if (this.scenarioStarted) return;
    // 核心失败时先展示错误与重试；重试成功后再预置演示面板。
    if (useStartupStore.getState().corePhase === 'failed') return;
    this.scenarioStarted = true;
    await Promise.allSettled(
      STARTUP_SECTIONS.map((key) => this.sectionPromises.get(key) ?? Promise.resolve(false)),
    );
    try {
      const scenario = await api.scenarioBootstrap();
      if (!scenario) return;
      const ui = useUiStore.getState();
      if (scenario.view) ui.setView(scenario.view);
      switch (scenario.panel) {
        case 'add-link':
          ui.openPanel({ type: 'add-link' });
          break;
        case 'add':
          if (scenario.parseResult) ui.openPanel({ type: 'add', parse: scenario.parseResult });
          break;
        case 'detail':
          if (scenario.taskId) ui.openPanel({ type: 'detail', taskId: scenario.taskId });
          break;
        case 'login':
          ui.openPanel({ type: 'login' });
          break;
        default:
          break;
      }
      for (const notice of scenario.notices ?? []) ui.pushNotice(notice);
      if (scenario.quitFailure) ui.showQuitFailure(scenario.quitFailure);
    } catch {
      // 场景预置失败不影响正常界面。
    }
  }

  private async performFileRefresh(): Promise<void> {
    useStartupStore.getState().setFilesPending(true);
    useQueueStore.getState().markFilesPending();
    useHistoryStore.getState().markFilesPending();
    try {
      const [queueOk, historyOk] = await Promise.all([
        useQueueStore.getState().refreshFiles(),
        useHistoryStore.getState().load(),
      ]);
      const store = useStartupStore.getState();
      if (queueOk) store.markSectionReady('queue');
      if (historyOk) store.markSectionReady('history');
    } finally {
      useStartupStore.getState().setFilesPending(false);
    }
  }
}

// ----------------------------------------------------------------------
// 单例与界面重试入口
// ----------------------------------------------------------------------

let coordinator: StartupCoordinator | null = null;

export function startupCoordinator(): StartupCoordinator {
  coordinator ??= new StartupCoordinator();
  return coordinator;
}

/** 安装后端/订阅并启动分区加载；首屏不等待它完成。 */
export function bootstrapApp(): Promise<void> {
  return startupCoordinator().start();
}

export function retryStartupSection(key: StartupSectionKey): Promise<void> {
  return startupCoordinator().retrySection(key);
}

export function retryStartupCore(): Promise<void> {
  return startupCoordinator().retryCore();
}

export function retryStartupNetwork(): Promise<void> {
  return startupCoordinator().retryNetwork();
}

/** files://invalidated / 打开历史列表时刷新文件状态展示（tasks 7.2）。 */
export function refreshStartupFiles(): Promise<void> {
  return startupCoordinator().refreshFiles();
}

/** 测试用：释放订阅并丢弃单例（下一个 bootstrapApp 会重新装配）。 */
export function resetStartupForTests(): void {
  coordinator?.dispose();
  coordinator = null;
}

/** 测试用：注入后端但不安装任何真实动态导入。 */
export function createStartupCoordinatorForTests(backend: Backend): StartupCoordinator {
  return new StartupCoordinator({ backend });
}

// 重试入口需要能覆盖「窗口已渲染但 bootstrap 尚未开始」的极早期点击。
export { STARTUP_SECTIONS };
