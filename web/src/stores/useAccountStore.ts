import { create } from 'zustand';
import { api } from '../lib/ipc';
import type { AccountState, LoginChallenge, LoginState } from '../lib/types';

interface AccountStore {
  account: AccountState | null;
  challenge: LoginChallenge | null;
  loginState: LoginState;
  loginMessage: string;
  busy: boolean;
  setAccount: (account: AccountState) => void;
  startLogin: () => Promise<void>;
  pollLogin: () => Promise<void>;
  cancelLogin: () => Promise<void>;
  refreshAccount: () => Promise<void>;
  logout: () => Promise<void>;
  resetLogin: () => void;
}

export const useAccountStore = create<AccountStore>()((set, get) => ({
  account: null,
  challenge: null,
  loginState: 'idle',
  loginMessage: '',
  busy: false,

  setAccount: (account) => set({ account }),

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
    const account = await api.refreshAccount();
    set({ account });
  },

  logout: async () => {
    await api.logout();
    set({ account: null });
  },

  resetLogin: () => set({ challenge: null, loginState: 'idle', loginMessage: '' }),
}));
