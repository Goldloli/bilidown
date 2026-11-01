import { AnimatePresence, motion } from 'motion/react';
import { useUiStore } from '../stores/useUiStore';
import { Icon, type IconName } from './Icon';

const KIND_ICON: Record<string, { icon: IconName; color: string }> = {
  info: { icon: 'info', color: 'var(--text-2)' },
  success: { icon: 'check-circle', color: 'var(--success)' },
  warning: { icon: 'warning', color: 'var(--warning)' },
  error: { icon: 'warning', color: 'var(--danger)' },
};

/** Toast 视口：右下角、底栏上方、3.2s 自动消失 */
export function ToastViewport() {
  const toasts = useUiStore((s) => s.toasts);
  const dismiss = useUiStore((s) => s.dismissToast);

  return (
    <div
      className="pointer-events-none fixed right-4 z-[90] flex flex-col items-end gap-2"
      style={{ bottom: 'calc(var(--bottombar-inset) + var(--bottombar-h) + 14px)' }}
    >
      <AnimatePresence initial={false}>
        {toasts.map((toast) => {
          const meta = KIND_ICON[toast.kind] ?? KIND_ICON.info;
          return (
            <motion.div
              key={toast.id}
              layout
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              transition={{ duration: 0.16, ease: [0.22, 1, 0.36, 1] }}
              role="status"
              className="glass-surface pointer-events-auto flex min-w-[240px] max-w-[380px] items-center gap-2 rounded-pop border border-line bg-panel px-3 py-2 shadow-[var(--shadow-toast)]"
            >
              <Icon name={meta.icon} size={15} style={{ color: meta.color }} />
              <span className="flex-1 text-xs text-fg">{toast.message}</span>
              {toast.actionLabel && (
                <button
                  type="button"
                  onClick={() => {
                    toast.action?.();
                    dismiss(toast.id);
                  }}
                  className="shrink-0 rounded-[5px] px-1.5 py-0.5 text-2xs font-medium text-accent hover:bg-accent-weak"
                >
                  {toast.actionLabel}
                </button>
              )}
              <button
                type="button"
                aria-label="关闭提示"
                onClick={() => dismiss(toast.id)}
                className="shrink-0 rounded-[5px] p-0.5 text-fg3 hover:bg-hover hover:text-fg"
              >
                <Icon name="close" size={12} />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
