import { create } from 'zustand';
import { api } from '../lib/ipc';
import { errorMessageOf } from '../lib/errors';
import { t } from '../i18n/zh-CN';
import type { ClearOutcome, QueueSnapshot, QueueTask } from '../lib/types';
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
  /** 重试待保存的任务记录（不清产物、不重新下载） */
  retryPersistence: () => Promise<void>;
  clearCompleted: () => Promise<void>;
  pauseAll: () => Promise<void>;
  resumeAll: () => Promise<void>;
  retryFailed: () => Promise<void>;
  /** 清空未激活任务（排队/暂停/已取消）；菜单、快捷键、界面按钮共用 */
  clearInactive: () => Promise<void>;
}

function toastError(error: unknown): void {
  useUiStore.getState().pushToast({ kind: 'error', message: errorMessageOf(error) });
}

/** 批量清理反馈：成功数用真实 removed，failed 非空时明确「部分失败，保留 N 行」 */
function notifyClearOutcome(outcome: ClearOutcome, kind: 'finished' | 'inactive'): void {
  const removed = outcome.removed;
  const failed = outcome.failed.length;
  const push = useUiStore.getState().pushToast;
  const successText =
    kind === 'finished'
      ? t('toast.clearFinished', { n: removed })
      : t('toast.clearInactive', { n: removed });
  if (failed > 0) {
    push({
      kind: 'warning',
      message:
        removed > 0
          ? `${successText}；${t('toast.clearPartial', { n: failed })}`
          : t('toast.clearPartial', { n: failed }),
    });
    return;
  }
  if (removed > 0) {
    push({ kind: 'info', message: successText });
    return;
  }
  push({ kind: 'info', message: t('toast.clearEmpty') });
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
  retryPersistence: async () => {
    try {
      await api.retryPersistence();
      useUiStore.getState().pushToast({
        kind: 'success',
        message: t('toast.persistenceRetried'),
      });
    } catch (error) {
      useUiStore.getState().pushToast({
        kind: 'error',
        message: t('toast.persistenceRetryFailed', { message: errorMessageOf(error) }),
      });
    }
  },
  clearCompleted: async () => {
    try {
      notifyClearOutcome(await api.clearCompleted(), 'finished');
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
  clearInactive: async () => {
    try {
      notifyClearOutcome(await api.clearInactive(), 'inactive');
    } catch (error) {
      toastError(error);
    }
  },
}));
