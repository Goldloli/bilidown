import { create } from 'zustand';
import type { AppNotice, NoticeKind, ParseResult } from '../lib/types';

export type PanelState =
  | { type: 'add-link'; initialText?: string }
  | { type: 'add'; parse: ParseResult }
  | { type: 'detail'; taskId: string }
  | { type: 'login' }
  | null;

export interface ToastItem {
  id: string;
  kind: NoticeKind;
  message: string;
  actionLabel?: string;
  action?: () => void;
  /** 自动消失时长（ms），默认 3200 */
  durationMs?: number;
}

interface ConfirmRequest {
  title: string;
  body: string;
  confirmLabel: string;
  cancelLabel?: string;
  danger: boolean;
  resolve: (ok: boolean) => void;
}

export type ContextTarget =
  | { type: 'task'; taskId: string; x: number; y: number }
  | { type: 'history'; historyId: string; x: number; y: number };

interface UiState {
  view: 'queue' | 'history';
  panel: PanelState;
  toasts: ToastItem[];
  confirmRequest: ConfirmRequest | null;
  /** 退出前保存失败（design D8/3.9）：非 null 时显示「重试保存 / 仍然退出」对话框 */
  quitFailure: string | null;
  contextTarget: ContextTarget | null;
  searchOpen: boolean;
  /** 队列视图当前渲染（虚拟滚动）的行数，用于判断是否需要显示总数 */
  renderedQueueRows: number;
  setRenderedQueueRows: (count: number) => void;
  setView: (view: 'queue' | 'history') => void;
  openPanel: (panel: PanelState) => void;
  closePanel: () => void;
  setSearchOpen: (open: boolean) => void;
  pushToast: (toast: {
    kind: NoticeKind;
    message: string;
    actionLabel?: string;
    action?: () => void;
    durationMs?: number;
  }) => void;
  pushNotice: (notice: AppNotice) => void;
  dismissToast: (id: string) => void;
  confirm: (options: {
    title: string;
    body: string;
    confirmLabel: string;
    cancelLabel?: string;
    danger?: boolean;
  }) => Promise<boolean>;
  resolveConfirm: (ok: boolean) => void;
  showQuitFailure: (message: string) => void;
  dismissQuitFailure: () => void;
  openContext: (target: ContextTarget) => void;
  closeContext: () => void;
}

let toastSeq = 0;

export const useUiStore = create<UiState>()((set, get) => ({
  view: 'queue',
  panel: null,
  toasts: [],
  confirmRequest: null,
  quitFailure: null,
  contextTarget: null,
  searchOpen: false,
  renderedQueueRows: 0,

  setRenderedQueueRows: (renderedQueueRows) => set({ renderedQueueRows }),
  setView: (view) => set({ view }),
  openPanel: (panel) => set({ panel }),
  closePanel: () => set({ panel: null }),
  setSearchOpen: (searchOpen) => set({ searchOpen }),

  pushToast: (toast) => {
    const id = `toast-${++toastSeq}`;
    const item: ToastItem = { id, ...toast };
    set((state) => ({ toasts: [...state.toasts, item] }));
    if (typeof window !== 'undefined') {
      window.setTimeout(() => get().dismissToast(id), toast.durationMs ?? 3200);
    }
  },

  pushNotice: (notice) => {
    get().pushToast({
      kind: notice.kind,
      message: notice.message,
      actionLabel: notice.actionLabel,
    });
  },

  dismissToast: (id) =>
    set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })),

  confirm: (options) =>
    new Promise<boolean>((resolve) => {
      set({
        confirmRequest: {
          title: options.title,
          body: options.body,
          confirmLabel: options.confirmLabel,
          cancelLabel: options.cancelLabel,
          danger: options.danger ?? false,
          resolve,
        },
      });
    }),

  resolveConfirm: (ok) => {
    const request = get().confirmRequest;
    set({ confirmRequest: null });
    request?.resolve(ok);
  },

  showQuitFailure: (message) => set({ quitFailure: message }),
  dismissQuitFailure: () => set({ quitFailure: null }),

  openContext: (contextTarget) => set({ contextTarget }),
  closeContext: () => set({ contextTarget: null }),
}));
