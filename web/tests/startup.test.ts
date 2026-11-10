import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Backend } from '../src/lib/ipc';
import { CONTRACT_EVENTS } from '../src/lib/contract';
import type {
  AccountInfoDTO,
  AppInfoDTO,
  HistoryEntryDTO,
  QueueSnapshotDTO,
  SettingsDTO,
} from '../src/lib/contract';
import * as A from '../src/lib/adapt';
import {
  createStartupCoordinatorForTests,
  isCoreInitError,
  resetStartupForTests,
} from '../src/lib/bootstrap';
import { useStartupStore } from '../src/stores/useStartupStore';
import { useQueueStore } from '../src/stores/useQueueStore';
import { useHistoryStore } from '../src/stores/useHistoryStore';
import { useSettingsStore } from '../src/stores/useSettingsStore';
import { useAccountStore } from '../src/stores/useAccountStore';
import { useUiStore } from '../src/stores/useUiStore';
import type { QueueTask } from '../src/lib/types';
import {
  coverCacheStats,
  resetCoverCacheForTests,
  resolveRemoteImage,
} from '../src/lib/cover';

const APP_INFO: AppInfoDTO = {
  productName: 'bilidown',
  version: '2.0.0',
  tauriVersion: '2.0.0',
  os: 'macos',
  arch: 'arm64',
  identifier: 'app.bilidown',
  dataDir: '/tmp/bilidown',
  logDir: '/tmp/bilidown/logs',
  ffmpegFound: true,
  ffmpegPath: '/usr/local/bin/ffmpeg',
  glassSupported: true,
  glassUnavailableReason: null,
  notificationsSupported: true,
  justUpdated: false,
};

function coreError(): { kind: string; message: string } {
  return { kind: 'unknown', message: 'core 初始化失败: database is locked' };
}

const SIGNED_ACCOUNT_DTO: AccountInfoDTO = {
  mid: 42,
  uname: '示例用户',
  avatarUrl: null,
  vip: 'none',
  level: 6,
};

class FakeBackend {
  readonly listeners = new Map<string, Set<(payload: unknown) => void>>();
  subscriptionCount = 0;
  settingsCalls = 0;
  queueCalls = 0;
  historyCalls = 0;
  accountCalls = 0;
  fetchCoverCalls = 0;
  revision = 0;
  settingsRevision = 0;
  /** 模拟本地初始化失败：core 依赖命令 reject，直到 resetCore 成功 */
  failCore = false;
  settings = A.defaultInternalSettings();
  tasks: QueueTask[] = [];
  history: HistoryEntryDTO[] = [];
  account: AccountInfoDTO | null = null;
  historyFailures = 0;
  retryInit = vi.fn(async () => {
    this.failCore = false;
  });
  retryNetwork = vi.fn(async () => {});

  on = <T>(event: string, handler: (payload: T) => void): (() => void) => {
    this.subscriptionCount += 1;
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    const wrapped = handler as (payload: unknown) => void;
    set.add(wrapped);
    return () => set?.delete(wrapped);
  };

  emit = (event: string, payload: unknown): void => {
    const set = this.listeners.get(event);
    if (!set) return;
    for (const handler of set) handler(payload);
  };

  private ensureCore(): void {
    if (this.failCore) throw coreError();
  }

  getSettings = async (): Promise<SettingsDTO> => {
    this.settingsCalls += 1;
    this.ensureCore();
    this.settingsRevision += 1;
    return A.settingsToDTO(this.settings, this.settingsRevision);
  };
  listQueue = async (): Promise<QueueSnapshotDTO> => {
    this.queueCalls += 1;
    this.ensureCore();
    this.revision += 1;
    return A.snapshotToDTO(this.tasks, A.ANONYMOUS_ACCOUNT, this.revision);
  };
  listHistory = async (): Promise<HistoryEntryDTO[]> => {
    this.historyCalls += 1;
    this.ensureCore();
    if (this.historyFailures > 0) {
      this.historyFailures -= 1;
      throw { kind: 'disk', message: '历史记录读取失败：database is locked' };
    }
    return this.history;
  };
  getAccount = async (): Promise<AccountInfoDTO | null> => {
    this.accountCalls += 1;
    this.ensureCore();
    return this.account;
  };
  fetchCover = async (): Promise<string> => {
    this.fetchCoverCalls += 1;
    return 'data:image/jpeg;base64,AA==';
  };
  getAppInfo = async (): Promise<AppInfoDTO> => APP_INFO;
}

function asBackend(fake: FakeBackend): Backend {
  return fake as unknown as Backend;
}

async function readySections(): Promise<void> {
  await vi.waitFor(() => {
    const { sections } = useStartupStore.getState();
    const done = (status: string) => status === 'ready' || status === 'error';
    const settled =
      done(sections.settings.status) &&
      done(sections.queue.status) &&
      done(sections.history.status) &&
      done(sections.account.status) &&
      done(sections.appInfo.status);
    expect(settled).toBe(true);
  });
}

function installDomStubs(): void {
  const dataset: Record<string, string> = {};
  const style = { setProperty: vi.fn(), colorScheme: '' };
  vi.stubGlobal('document', { documentElement: { dataset, style } });
  vi.stubGlobal('window', {
    location: { search: '' },
    sessionStorage: { getItem: () => null, setItem: () => {} },
    localStorage: { getItem: () => null, setItem: () => {} },
    matchMedia: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
    setTimeout: globalThis.setTimeout.bind(globalThis),
    clearTimeout: globalThis.clearTimeout.bind(globalThis),
  });
}

beforeEach(() => {
  installDomStubs();
  resetStartupForTests();
  resetCoverCacheForTests();
  // 清空切面状态，避免测试间串扰。
  useStartupStore.getState().reset();
  useQueueStore.setState({
    snapshot: null,
    loading: true,
    error: null,
    filesVerified: false,
    keyword: '',
    selectedTaskId: null,
  });
  useHistoryStore.setState({
    entries: [],
    loading: false,
    error: null,
    filesVerified: false,
    keyword: '',
    status: 'all',
  });
  useSettingsStore.getState().reset();
  useAccountStore.setState({
    account: null,
    validation: 'idle',
    validationGeneration: 0,
    validationError: null,
    challenge: null,
    loginState: 'idle',
    loginMessage: '',
    busy: false,
  });
  useUiStore.setState({ toasts: [], view: 'queue', panel: null, quitFailure: null });
});

afterEach(() => {
  resetStartupForTests();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('启动局部失败独立恢复（tasks 5.1/5.3）', () => {
  it('历史加载失败不影响设置/队列，历史返回错误而非空列表，可局部重试', async () => {
    const fake = new FakeBackend();
    fake.historyFailures = 1;
    fake.history = [
      {
        id: 'h1',
        taskId: null,
        title: '已完成任务',
        coverUrl: null,
        sourceKind: 'video',
        sourceId: 'BV1',
        pageUrl: 'https://www.bilibili.com/video/BV1',
        quality: '1080P',
        outputPath: '/tmp/a.mp4',
        fileSize: 1024,
        status: 'completed',
        error: null,
        finishedAt: 1,
        owner: 'UP',
        fileExists: true,
        artifacts: [],
        artifactCoverage: 'complete',
      },
    ];
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await readySections();

    const startup = useStartupStore.getState();
    expect(startup.sections.settings.status).toBe('ready');
    expect(startup.sections.queue.status).toBe('ready');
    expect(startup.sections.history.status).toBe('error');
    expect(startup.sections.history.error).toContain('database is locked');
    // 历史读取失败不伪装成空列表成功
    expect(useHistoryStore.getState().entries).toHaveLength(0);
    expect(useHistoryStore.getState().error).toContain('database is locked');
    expect(useQueueStore.getState().snapshot).not.toBeNull();
    expect(useSettingsStore.getState().settings).not.toBeNull();

    const subscriptions = fake.subscriptionCount;
    await coordinator.retrySection('history');
    expect(useStartupStore.getState().sections.history.status).toBe('ready');
    expect(useHistoryStore.getState().entries).toHaveLength(1);
    // 局部重试不重复安装事件订阅
    expect(fake.subscriptionCount).toBe(subscriptions);
  });

  it('核心初始化失败给可读错误；重试复用单一初始化任务且只保留一组订阅', async () => {
    const fake = new FakeBackend();
    fake.failCore = true;
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await vi.waitFor(() =>
      expect(useStartupStore.getState().corePhase).toBe('failed'),
    );
    expect(useStartupStore.getState().coreError).toContain('core 初始化失败');
    expect(useStartupStore.getState().sections.queue.status).toBe('error');
    expect(isCoreInitError(useStartupStore.getState().sections.queue.error)).toBe(true);

    const subscriptions = fake.subscriptionCount;
    const first = coordinator.retryCore();
    const second = coordinator.retryCore();
    expect(first).toBe(second);
    await first;
    expect(fake.retryInit).toHaveBeenCalledTimes(1);
    expect(fake.subscriptionCount).toBe(subscriptions);
    await readySections();
    const startup = useStartupStore.getState();
    expect(startup.corePhase).not.toBe('failed');
    expect(startup.coreError).toBeNull();
    expect(startup.sections.settings.status).toBe('ready');
    expect(startup.sections.queue.status).toBe('ready');
  });

  it('应用信息独立失败：不阻塞其余分区，提示并可局部重试', async () => {
    const fake = new FakeBackend();
    let failures = 1;
    fake.getAppInfo = async (): Promise<AppInfoDTO> => {
      if (failures > 0) {
        failures -= 1;
        throw new Error('app info unavailable');
      }
      return APP_INFO;
    };
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await readySections();

    const startup = useStartupStore.getState();
    expect(startup.sections.appInfo.status).toBe('error');
    // 其余分区不受影响
    expect(startup.sections.settings.status).toBe('ready');
    expect(startup.sections.queue.status).toBe('ready');
    expect(useQueueStore.getState().snapshot).not.toBeNull();
    const toast = useUiStore.getState().toasts.at(-1);
    expect(toast?.message).toContain('应用信息加载失败');

    await coordinator.retrySection('appInfo');
    expect(useStartupStore.getState().sections.appInfo.status).toBe('ready');
  });

  it('核心初始化错误可被识别，普通读取错误不触发核心重试', () => {
    expect(isCoreInitError('core 初始化失败: database is locked')).toBe(true);
    expect(isCoreInitError('引擎尚未就绪')).toBe(true);
    expect(isCoreInitError('历史记录读取失败：database is locked')).toBe(false);
    expect(isCoreInitError(null)).toBe(false);
  });
});

describe('网络阶段代次与账号校验（tasks 5.4）', () => {
  it('网络未就绪不把匿名结果当最终账号，已登录账号不被误清除或降级', async () => {
    const fake = new FakeBackend();
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await readySections();

    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'local-ready',
      generation: 0,
      degraded: false,
    });
    expect(useAccountStore.getState().validation).toBe('pending');

    // 本地阶段匿名快照/事件不能把界面判成「未登录」
    fake.emit(CONTRACT_EVENTS.accountChanged, null);
    expect(useAccountStore.getState().account).toBeNull();

    fake.emit(CONTRACT_EVENTS.accountChanged, SIGNED_ACCOUNT_DTO);
    expect(useAccountStore.getState().account?.signedIn).toBe(true);

    // 校验中的迟到匿名结果不得清除已登录账号
    fake.emit(CONTRACT_EVENTS.accountChanged, null);
    expect(useAccountStore.getState().account?.signedIn).toBe(true);
    expect(useAccountStore.getState().validation).toBe('pending');

    // 网络降级：保留登录态，仅标记校验未完成
    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'network-degraded',
      generation: 1,
      degraded: true,
      error: '网络准备超时',
    });
    expect(useAccountStore.getState().validation).toBe('degraded');
    expect(useAccountStore.getState().account?.signedIn).toBe(true);
    expect(useStartupStore.getState().corePhase).toBe('network-degraded');

    // 后续成功：账号可被采信；显式匿名结果此时才作为最终状态
    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'network-ready',
      generation: 2,
      degraded: false,
    });
    fake.emit(CONTRACT_EVENTS.accountChanged, null);
    expect(useAccountStore.getState().validation).toBe('ready');
    expect(useAccountStore.getState().account?.signedIn).toBe(false);
  });

  it('旧代次网络状态与迟到账号结果被丢弃', async () => {
    const fake = new FakeBackend();
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await readySections();

    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'network-ready',
      generation: 5,
      degraded: false,
    });
    // 旧代次降级状态不得回退 ready
    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'network-degraded',
      generation: 4,
      degraded: true,
      error: 'stale',
    });
    expect(useStartupStore.getState().corePhase).toBe('network-ready');
    expect(useStartupStore.getState().networkGeneration).toBe(5);

    // 迟到账号结果（旧代次）被丢弃
    useAccountStore
      .getState()
      .beginValidation(0);
    useAccountStore.getState().finishValidation(5, false);
    useAccountStore
      .getState()
      .applyServerAccount(A.accountFromDTO(SIGNED_ACCOUNT_DTO), { generation: 0 });
    expect(useAccountStore.getState().account).toBeNull();
  });

  it('初始账号读取晚于网络结果时，旧代次结果不覆盖新状态', async () => {
    const fake = new FakeBackend();
    let resolveAccount!: (account: AccountInfoDTO | null) => void;
    fake.getAccount = () =>
      new Promise<AccountInfoDTO | null>((resolve) => {
        resolveAccount = resolve;
      });
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();

    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'local-ready',
      generation: 0,
      degraded: false,
    });
    fake.emit(CONTRACT_EVENTS.coreStatus, {
      stage: 'network-ready',
      generation: 2,
      degraded: false,
    });
    // 初始 getAccount 属于代次 0，晚到时必须丢弃
    resolveAccount(SIGNED_ACCOUNT_DTO);
    await vi.waitFor(() =>
      expect(useStartupStore.getState().sections.account.status).toBe('ready'),
    );
    expect(useAccountStore.getState().account).toBeNull();
    expect(useAccountStore.getState().validation).toBe('ready');
  });
});

describe('文件状态失效刷新与图片缓存清理（tasks 7.2 / 7.6）', () => {
  it('files://invalidated 触发队列与历史重新拉取，同一时间只跑一批', async () => {
    const fake = new FakeBackend();
    let releaseQueue!: () => void;
    const gate = new Promise<void>((resolve) => {
      releaseQueue = resolve;
    });
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await readySections();
    const queueCalls = fake.queueCalls;
    const historyCalls = fake.historyCalls;

    fake.listQueue = async (): Promise<QueueSnapshotDTO> => {
      await gate;
      fake.queueCalls += 1;
      fake.revision += 1;
      return A.snapshotToDTO(fake.tasks, A.ANONYMOUS_ACCOUNT, fake.revision);
    };
    fake.emit(CONTRACT_EVENTS.filesInvalidated, undefined);
    fake.emit(CONTRACT_EVENTS.filesInvalidated, undefined);
    // 在途期间不并发第二批
    expect(fake.queueCalls).toBe(queueCalls);
    expect(useStartupStore.getState().filesPending).toBe(true);
    expect(useQueueStore.getState().filesVerified).toBe(false);

    releaseQueue();
    await vi.waitFor(() => expect(useQueueStore.getState().filesVerified).toBe(true));
    expect(fake.historyCalls).toBeGreaterThan(historyCalls);
    expect(useQueueStore.getState().filesVerified).toBe(true);
    expect(useHistoryStore.getState().filesVerified).toBe(true);
  });

  it('covers://cleared 失效内存图片缓存', async () => {
    const fake = new FakeBackend();
    const coordinator = createStartupCoordinatorForTests(asBackend(fake));
    await coordinator.start();
    await readySections();
    await resolveRemoteImage('https://i.example/cleared.jpg');
    expect(coverCacheStats().entries).toBe(1);
    fake.emit(CONTRACT_EVENTS.coversCleared, undefined);
    expect(coverCacheStats().entries).toBe(0);
  });
});
