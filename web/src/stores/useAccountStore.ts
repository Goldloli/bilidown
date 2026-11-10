import { create } from 'zustand';
import { api } from '../lib/ipc';
import type {
  AccountState,
  AccountValidation,
  LoginChallenge,
  LoginState,
} from '../lib/types';

/** 账号结果来源：不同来源的迟到/临时结果可信度不同（tasks 5.4）。 */
export type AccountSource = 'snapshot' | 'event' | 'initial' | 'refresh';

export interface ApplyAccountOptions {
  source?: AccountSource;
  /** 请求发出时的网络准备代次；小于已应用代次的结果被丢弃 */
  generation?: number;
}

interface AccountStore {
  account: AccountState | null;
  /** 网络校验状态（design D5/tasks 5.4） */
  validation: AccountValidation;
  /** 已应用的最大网络准备代次，用于丢弃迟到账号结果 */
  validationGeneration: number;
  validationError: string | null;
  challenge: LoginChallenge | null;
  loginState: LoginState;
  loginMessage: string;
  busy: boolean;
  /** 显式登录/退出结果：本轮会话内可采信（不参与账号校验排队） */
  setAccount: (account: AccountState) => void;
  /** 后端返回的账号结果：按校验阶段与代次决定是否采信 */
  applyServerAccount: (account: AccountState, options?: ApplyAccountOptions) => void;
  /** 网络阶段开始：进入「校验中」，保留已登录账号与凭据 */
  beginValidation: (generation?: number) => void;
  /** 网络阶段结束：ready/degraded；旧代次结果被忽略 */
  finishValidation: (generation: number, degraded: boolean, error?: string | null) => void;
  resetValidation: () => void;
  startLogin: () => Promise<void>;
  pollLogin: () => Promise<void>;
  cancelLogin: () => Promise<void>;
  refreshAccount: () => Promise<void>;
  logout: () => Promise<void>;
  resetLogin: () => void;
}

export const useAccountStore = create<AccountStore>()((set, get) => ({
  account: null,
  validation: 'idle',
  validationGeneration: 0,
  validationError: null,
  challenge: null,
  loginState: 'idle',
  loginMessage: '',
  busy: false,

  setAccount: (account) =>
    set({ account, validation: 'ready', validationError: null }),

  applyServerAccount: (account, options = {}) => {
    const state = get();
    // 迟到结果丢弃：网络重试开始后，旧代次请求的账号结果不得覆盖新状态。
    if (
      options.generation !== undefined &&
      options.generation < state.validationGeneration
    ) {
      return;
    }
    // 本地阶段/校验中/降级：匿名结果不是最终账号状态。保留已登录账号，避免误清除；
    // 没有已登录账号时保持 null，由界面显示「账号校验中」而不是「未登录」。
    if (!account.signedIn && state.validation !== 'ready') return;
    set({ account });
  },

  beginValidation: (generation) => {
    const state = get();
    if (generation !== undefined && generation < state.validationGeneration) return;
    set({
      validation: 'pending',
      validationError: null,
      ...(generation !== undefined && generation > state.validationGeneration
        ? { validationGeneration: generation }
        : {}),
    });
  },

  finishValidation: (generation, degraded, error) => {
    const state = get();
    if (generation < state.validationGeneration) return;
    set({
      validation: degraded ? 'degraded' : 'ready',
      validationGeneration: generation,
      validationError: degraded ? (error ?? '网络准备未完成') : null,
    });
  },

  resetValidation: () =>
    set({ validation: 'idle', validationGeneration: 0, validationError: null }),

  startLogin: async () => {
    set({ busy: true, challenge: null, loginState: 'waiting', loginMessage: '' });
    try {
      const challenge = await api.createLoginSession();
      set({
        challenge,
        loginState: challenge.state,
        busy: false,
        loginMessage: challenge.state === 'waiting' ? '使用 B 站 App 扫码登录' : '',
      });
    } catch {
      set({ loginState: 'failed', busy: false, loginMessage: '无法获取二维码' });
    }
  },

  pollLogin: async () => {
    const challenge = get().challenge;
    if (!challenge) return;
    try {
      const result = await api.pollLogin(challenge.sessionId);
      set({
        loginState: result.state,
        loginMessage: result.message,
        account: result.account ?? get().account,
        // 扫码登录成功是明确的用户动作，结果可直接采信。
        ...(result.account ? { validation: 'ready' as const, validationError: null } : {}),
      });
    } catch {
      set({ loginState: 'failed', loginMessage: '登录失败，请重试' });
    }
  },

  cancelLogin: async () => {
    const challenge = get().challenge;
    if (challenge) {
      try {
        await api.cancelLogin(challenge.sessionId);
      } catch {
        /* 忽略 */
      }
    }
    set({ challenge: null, loginState: 'idle', loginMessage: '' });
  },

  refreshAccount: async () => {
    const generation = get().validationGeneration;
    const account = await api.refreshAccount();
    get().applyServerAccount(account, { source: 'refresh', generation });
  },

  logout: async () => {
    await api.logout();
    set({ account: null, validation: 'ready', validationError: null });
  },

  resetLogin: () => set({ challenge: null, loginState: 'idle', loginMessage: '' }),
}));
