import { create } from 'zustand';
import { api } from '../lib/ipc';
import type { QueueSnapshot, QueueTask } from '../lib/types';
import type { QueueEvent } from '../lib/types';
import { useUiStore } from './useUiStore';

interface QueueState {
  snapshot: QueueSnapshot | null;
  loading: boolean;
  error: string | null;
  keyword: string;
  selectedTaskId: string | null;
  setSnapshot: (snapshot: QueueSnapshot) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setKeyword: (keyword: string) => void;
  select: (id: string | null) => void;
  handleQueueEvent: (event: QueueEvent) => void;
  refresh: () => Promise<void>;
  getTask: (id: string) => QueueTask | null;
  pause: (id: string) => Promise<void>;
  resume: (id: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  retry: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  clearCompleted: () => Promise<void>;
  pauseAll: () => Promise<void>;
  resumeAll: () => Promise<void>;
  retryFailed: () => Promise<void>;
  cancelAllQueued: () => Promise<void>;
}

function toastError(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  useUiStore.getState().pushToast({ kind: 'error', message });
}

export const useQueueStore = create<QueueState>()((set, get) => ({
  snapshot: null,
  loading: true,
  error: null,
  keyword: '',
  selectedTaskId: null,

  setSnapshot: (snapshot) => set({ snapshot, loading: false, error: null }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error, loading: false }),
  setKeyword: (keyword) => set({ keyword }),
  select: (selectedTaskId) => set({ selectedTaskId }),

  handleQueueEvent: (event) => {
    if (event.type === 'task-removed') {
      const snapshot = get().snapshot;
      if (!snapshot) return;
      set({
        snapshot: {
          ...snapshot,
          tasks: snapshot.tasks.filter((t) => t.id !== event.taskId),
        },
      });
      return;
    }
    if (event.type === 'task-updated') {
      // 快照事件会随后到达，这里仅用于事件先到时的即时反馈
      const snapshot = get().snapshot;
      if (!snapshot) return;
      const exists = snapshot.tasks.some((t) => t.id === event.task.id);
      set({
        snapshot: {
          ...snapshot,
          tasks: exists
            ? snapshot.tasks.map((t) => (t.id === event.task.id ? event.task : t))
            : [event.task, ...snapshot.tasks],
        },
      });
      return;
    }
    if (event.type === 'toast') {
      useUiStore.getState().pushToast(event.notice);
    }
  },

  refresh: async () => {
    try {
      const snapshot = await api.getQueueSnapshot();
      get().setSnapshot(snapshot);
    } catch (error) {
      get().setError(error instanceof Error ? error.message : String(error));
    }
  },

  getTask: (id) => get().snapshot?.tasks.find((t) => t.id === id) ?? null,

  pause: async (id) => {
    try {
      await api.pauseTask(id);
    } catch (error) {
      toastError(error);
    }
  },
  resume: async (id) => {
    try {
      await api.resumeTask(id);
    } catch (error) {
      toastError(error);
    }
  },
  cancel: async (id) => {
    try {
      await api.cancelTask(id);
    } catch (error) {
      toastError(error);
    }
  },
  retry: async (id) => {
    try {
      await api.retryTask(id);
    } catch (error) {
      toastError(error);
    }
  },
  remove: async (id) => {
    try {
      await api.removeTask(id);
      useUiStore.getState().pushToast({ kind: 'info', message: '已从队列移除' });
    } catch (error) {
      toastError(error);
    }
  },
  clearCompleted: async () => {
    try {
      await api.clearCompleted();
      useUiStore.getState().pushToast({ kind: 'info', message: '已清空已完成任务' });
    } catch (error) {
      toastError(error);
    }
  },
  pauseAll: async () => {
    try {
      await api.pauseAll();
    } catch (error) {
      toastError(error);
    }
  },
  resumeAll: async () => {
    try {
      await api.resumeAll();
    } catch (error) {
      toastError(error);
    }
  },
  retryFailed: async () => {
    try {
      await api.retryFailed();
    } catch (error) {
      toastError(error);
    }
  },
  cancelAllQueued: async () => {
    const queued = (get().snapshot?.tasks ?? []).filter((t) => t.status === 'queued');
    for (const task of queued) {
      try {
        await api.cancelTask(task.id);
      } catch (error) {
        toastError(error);
      }
    }
  },
}));
