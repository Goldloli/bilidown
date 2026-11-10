import { useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { t } from '../i18n/zh-CN';
import { cancelQuit, forceQuit, retryQuitSave } from '../lib/quit';
import { useUiStore } from '../stores/useUiStore';
import { Button } from './Button';
import { Icon } from './Icon';

interface QuitFailureDialogProps {
  message: string;
  busy: 'retry' | 'force' | null;
  onRetry: () => void;
  onForce: () => void;
  onCancel: () => void;
}

/** 退出前保存失败对话框（纯展示，便于测试与复用） */
export function QuitFailureDialog({ message, busy, onRetry, onForce, onCancel }: QuitFailureDialogProps) {
  return (
    <motion.div
      className="fixed inset-0 z-[96] grid place-items-center p-6"
      style={{ background: 'var(--scrim)' }}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.14 }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <motion.div
        role="alertdialog"
        aria-modal="true"
        aria-label={t('quit.saveFailed.title')}
        initial={{ opacity: 0, scale: 0.98 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.99 }}
        transition={{ duration: 0.16 }}
        className="glass-surface w-[340px] overflow-hidden rounded-sheet border border-line bg-panel shadow-[var(--shadow-pop)]"
      >
        <div className="px-5 pt-5 pb-4">
          <div className="flex items-start gap-2">
            <Icon name="warning" size={16} className="mt-0.5 shrink-0 text-warning" />
            <div className="min-w-0 flex-1">
              <h2 className="text-sm font-semibold text-fg">{t('quit.saveFailed.title')}</h2>
              <p className="mt-2 break-all text-xs leading-[17px] text-fg2">
                {t('quit.saveFailed.body', { message })}
              </p>
            </div>
          </div>
        </div>
        <div className="flex justify-end gap-2 border-t border-line px-4 py-3">
          <Button autoFocus variant="secondary" size="md" onClick={onCancel}>
            {t('quit.saveFailed.cancel')}
          </Button>
          <Button variant="danger" size="md" loading={busy === 'force'} onClick={onForce}>
            {t('quit.saveFailed.force')}
          </Button>
          <Button variant="primary" size="md" loading={busy === 'retry'} onClick={onRetry}>
            {t('quit.saveFailed.retry')}
          </Button>
        </div>
      </motion.div>
    </motion.div>
  );
}

/**
 * 退出前保存失败对话框宿主（design D8/3.9）：
 * 仅在保存失败时出现；正常保存成功由后端直接退出，不增加额外确认步骤。
 */
export function QuitFailureDialogHost() {
  const message = useUiStore((s) => s.quitFailure);
  const [busy, setBusy] = useState<'retry' | 'force' | null>(null);

  const retry = async () => {
    if (busy) return;
    setBusy('retry');
    try {
      await retryQuitSave();
    } finally {
      setBusy(null);
    }
  };

  const force = async () => {
    if (busy) return;
    setBusy('force');
    try {
      await forceQuit();
    } finally {
      setBusy(null);
    }
  };

  return (
    <AnimatePresence>
      {message !== null && (
        <QuitFailureDialog
          message={message}
          busy={busy}
          onRetry={() => void retry()}
          onForce={() => void force()}
          onCancel={cancelQuit}
        />
      )}
    </AnimatePresence>
  );
}
