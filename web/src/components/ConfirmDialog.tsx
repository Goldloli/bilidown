import { AnimatePresence, motion } from 'motion/react';
import { useUiStore } from '../stores/useUiStore';
import { t } from '../i18n/zh-CN';
import { Button } from './Button';

/** 系统风格确认框（260 宽居中，危险操作红色按钮） */
export function ConfirmDialogHost() {
  const request = useUiStore((s) => s.confirmRequest);
  const resolve = useUiStore((s) => s.resolveConfirm);

  return (
    <AnimatePresence>
      {request && (
        <motion.div
          className="fixed inset-0 z-[95] grid place-items-center p-6"
          style={{ background: 'var(--scrim)' }}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.14 }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) resolve(false);
          }}
        >
          <motion.div
            role="alertdialog"
            aria-modal="true"
            aria-label={request.title}
            initial={{ opacity: 0, scale: 0.98 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.99 }}
            transition={{ duration: 0.16 }}
            className="glass-surface w-[300px] overflow-hidden rounded-sheet border border-line bg-panel shadow-[var(--shadow-pop)]"
          >
            <div className="px-5 pt-5 pb-4">
              <h2 className="text-sm font-semibold text-fg">{request.title}</h2>
              <p className="mt-2 whitespace-pre-line text-xs leading-[17px] text-fg2">
                {request.body}
              </p>
            </div>
            <div className="flex justify-end gap-2 border-t border-line px-4 py-3">
              <Button autoFocus variant="secondary" size="md" onClick={() => resolve(false)}>
                {request.cancelLabel ?? t('common.cancel')}
              </Button>
              <Button
                variant={request.danger ? 'danger' : 'primary'}
                size="md"
                onClick={() => resolve(true)}
              >
                {request.confirmLabel}
              </Button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
