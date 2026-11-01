import { create } from 'zustand';
import { api } from '../lib/ipc';
import type { HistoryEntry, HistoryQuery, HistoryStatus } from '../lib/types';

interface HistoryStore {
  entries: HistoryEntry[];
  loading: boolean;
  error: string | null;
  keyword: string;
  status: HistoryStatus | 'all';
  setEntries: (entries: HistoryEntry[]) => void;
  setKeyword: (keyword: string) => void;
  setStatus: (status: HistoryStatus | 'all') => void;
  load: () => Promise<void>;
  deleteEntry: (id: string) => Promise<void>;
  clearAll: () => Promise<void>;
  filtered: () => HistoryEntry[];
  query: () => HistoryQuery;
}

export const useHistoryStore = create<HistoryStore>()((set, get) => ({
  entries: [],
  loading: false,
  error: null,
  keyword: '',
  status: 'all',

  setEntries: (entries) => set({ entries, loading: false, error: null }),
  setKeyword: (keyword) => set({ keyword }),
  setStatus: (status) => set({ status }),

  load: async () => {
    set({ loading: true });
    try {
      const entries = await api.getHistory({ keyword: '', status: 'all' });
      set({ entries, loading: false, error: null });
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      });
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
