import { create } from 'zustand';
import type { CoreStatusDTO } from '../lib/contract';

/**
 * 启动分阶段与局部加载状态（design D5 / tasks 5.1/5.3/5.4）。
 *
 * 不再用一个整体 Promise 决定全部成败：
 * · 设置、队列、历史、账号、应用信息各自有 loading/ready/error；
 * · core 有独立的本地就绪/网络 ready/degraded/failed 阶段；
 * · 网络阶段带代次，旧代次状态与迟到结果被丢弃。
 */
export type StartupSectionKey = 'settings' | 'queue' | 'history' | 'account' | 'appInfo';

export type StartupSectionStatus = 'idle' | 'loading' | 'ready' | 'error';

export type CorePhase =
  | 'booting'
  | 'local-ready'
  | 'network-ready'
  | 'network-degraded'
  | 'failed';

export interface StartupSectionState {
  status: StartupSectionStatus;
  error: string | null;
}

export type StartupSections = Record<StartupSectionKey, StartupSectionState>;

export const STARTUP_SECTIONS: StartupSectionKey[] = [
  'settings',
  'queue',
  'history',
  'account',
  'appInfo',
];

function initialSections(): StartupSections {
  return {
    settings: { status: 'idle', error: null },
    queue: { status: 'idle', error: null },
    history: { status: 'idle', error: null },
    account: { status: 'idle', error: null },
    appInfo: { status: 'idle', error: null },
  };
}

interface StartupStore {
  /** 后端装配（IPC 层）致命失败；窗口仍可见并可重试 */
  fatalError: string | null;
  corePhase: CorePhase;
  /** 本地初始化失败的可读原因（数据库迁移/读取等） */
  coreError: string | null;
  /** 已应用的最大网络准备代次 */
  networkGeneration: number;
  networkError: string | null;
  retryingCore: boolean;
  retryingNetwork: boolean;
  sections: StartupSections;
  /** 文件状态展示缓存刚失效、正在重新拉取（tasks 7.2） */
  filesPending: boolean;

  setFatalError: (message: string) => void;
  clearFatalError: () => void;
  markSectionLoading: (key: StartupSectionKey) => void;
  markSectionReady: (key: StartupSectionKey) => void;
  markSectionError: (key: StartupSectionKey, message: string) => void;
  /** 核心重试：把失败分区回到 loading，便于复用同一加载流程 */
  resetFailedSections: () => void;
  applyCoreStatus: (status: CoreStatusDTO) => void;
  markCoreFailure: (message: string) => void;
  beginCoreRetry: () => void;
  endCoreRetry: () => void;
  beginNetworkRetry: () => void;
  endNetworkRetry: () => void;
  setFilesPending: (pending: boolean) => void;
  reset: () => void;
}

/** 所有分区回到初始状态（测试/后端切换复用） */
function initialState() {
  return {
    fatalError: null as string | null,
    corePhase: 'booting' as CorePhase,
    coreError: null as string | null,
    networkGeneration: 0,
    networkError: null as string | null,
    retryingCore: false,
    retryingNetwork: false,
    sections: initialSections(),
    filesPending: false,
  };
}

export const useStartupStore = create<StartupStore>()((set, get) => ({
  ...initialState(),

  setFatalError: (fatalError) => set({ fatalError }),
  clearFatalError: () => set({ fatalError: null }),

  markSectionLoading: (key) =>
    set((state) => ({
      sections: { ...state.sections, [key]: { status: 'loading', error: null } },
    })),

  markSectionReady: (key) =>
    set((state) => ({
      sections: { ...state.sections, [key]: { status: 'ready', error: null } },
    })),

  markSectionError: (key, message) =>
    set((state) => ({
      sections: { ...state.sections, [key]: { status: 'error', error: message } },
    })),

  resetFailedSections: () =>
    set((state) => {
      const sections = { ...state.sections };
      for (const key of STARTUP_SECTIONS) {
        if (sections[key].status === 'error') {
          sections[key] = { status: 'loading', error: null };
        }
      }
      return { sections };
    }),

  applyCoreStatus: (status) => {
    const state = get();
    // 迟到旧代次状态不得覆盖新状态（tasks 5.4）。
    if (status.generation < state.networkGeneration) return;
    switch (status.stage) {
      case 'local-ready':
        set({
          corePhase: 'local-ready',
          coreError: null,
          networkGeneration: status.generation,
        });
        return;
      case 'network-ready':
        set({
          corePhase: 'network-ready',
          networkGeneration: status.generation,
          networkError: null,
        });
        return;
      case 'network-degraded':
        set({
          corePhase: 'network-degraded',
          networkGeneration: status.generation,
          networkError: status.error ?? '网络准备未完成',
        });
        return;
      default:
        return;
    }
  },

  markCoreFailure: (message) =>
    set({ corePhase: 'failed', coreError: message, retryingCore: false }),

  beginCoreRetry: () => set({ retryingCore: true, coreError: null }),
  endCoreRetry: () => set({ retryingCore: false }),

  beginNetworkRetry: () => set({ retryingNetwork: true }),
  endNetworkRetry: () => set({ retryingNetwork: false }),

  setFilesPending: (filesPending) => set({ filesPending }),

  reset: () => set(initialState()),
}));
