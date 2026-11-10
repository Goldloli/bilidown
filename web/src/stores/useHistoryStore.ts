import { create } from 'zustand';
import { api } from '../lib/ipc';
import { errorMessageOf } from '../lib/errors';
import type { HistoryEntry, HistoryQuery, HistoryStatus } from '../lib/types';

interface HistoryStore {
  entries: HistoryEntry[];
  loading: boolean;
  error: string | null;
  /** 文件状态展示缓存是否已核实（design D9/7.2）；失效后重新拉取，不把未知当有效 */
  filesVerified: boolean;
  keyword: string;
  status: HistoryStatus | 'all';
  setEntries: (entries: HistoryEntry[]) => void;
  setError: (error: string | null) => void;
  /** 文件状态失效：等待重新拉取 */
  markFilesPending: () => void;
  setKeyword: (keyword: string) => void;
  setStatus: (status: HistoryStatus | 'all') => void;
  /** 重新读取历史；失败保留现有条目并把错误交给界面重试（不伪造空列表） */
  load: () => Promise<boolean>;
  deleteEntry: (id: string) => Promise<void>;
  clearAll: () => Promise<void>;
  filtered: () => HistoryEntry[];
  query: () => HistoryQuery;
}

export const useHistoryStore = create<HistoryStore>()((set, get) => ({
  entries: [],
  loading: false,
  error: null,
  filesVerified: false,
  keyword: '',
  status: 'all',

  setEntries: (entries) =>
    set({ entries, loading: false, error: null, filesVerified: true }),
  setError: (error) => set({ error, loading: false }),
  markFilesPending: () => set({ filesVerified: false }),
  setKeyword: (keyword) => set({ keyword }),
  setStatus: (status) => set({ status }),

  load: async () => {
    set({ loading: true });
    try {
      const entries = await api.getHistory({ keyword: '', status: 'all' });
      set({ entries, loading: false, error: null, filesVerified: true });
      return true;
    } catch (error) {
      set({
        loading: false,
        error: errorMessageOf(error),
      });
      return false;
    }
  },

  deleteEntry: async (id) => {
    await api.deleteHistory(id);
    set({ entries: get().entries.filter((e) => e.id !== id) });
  },

  clearAll: async () => {
    await api.clearHistory();
    set({ entries: [] });
  },

  filtered: () => {
    const { entries, keyword, status } = get();
    const q = keyword.trim().toLowerCase();
    return entries
      .filter((e) => (status === 'all' ? true : e.status === status))
      .filter((e) =>
        q ? e.title.toLowerCase().includes(q) || e.uploader.toLowerCase().includes(q) : true,
      )
      .sort((a, b) => b.finishedAt - a.finishedAt);
  },

  query: () => ({ keyword: get().keyword, status: get().status }),
}));
