import { t } from '../i18n/zh-CN';
import { api } from '../lib/ipc';
import { isPending, pendingLabel } from '../lib/capabilities';
import { copyLink, handleParseText } from '../lib/actions';
import { useHistoryStore } from '../stores/useHistoryStore';
import { useQueueStore } from '../stores/useQueueStore';
import { useUiStore } from '../stores/useUiStore';
import { Icon } from '../components/Icon';
import { MenuPanel, type MenuItem } from '../components/Menu';

/** 队列行 / 历史行右键菜单（与界面按钮同一套 action） */
export function ContextMenuHost() {
  const target = useUiStore((s) => s.contextTarget);
  const closeContext = useUiStore((s) => s.closeContext);
  const openPanel = useUiStore((s) => s.openPanel);
  const confirm = useUiStore((s) => s.confirm);
  const pushToast = useUiStore((s) => s.pushToast);
  const tasks = useQueueStore((s) => s.snapshot?.tasks ?? []);
  const pause = useQueueStore((s) => s.pause);
  const resume = useQueueStore((s) => s.resume);
  const cancel = useQueueStore((s) => s.cancel);
  const retry = useQueueStore((s) => s.retry);
  const remove = useQueueStore((s) => s.remove);
  const history = useHistoryStore((s) => s.entries);
  const deleteEntry = useHistoryStore((s) => s.deleteEntry);

  if (!target) return null;

  const items: MenuItem[] = [];

  if (target.type === 'task') {
    const task = tasks.find((item) => item.id === target.taskId);
    if (!task) return null;
    const canPause = ['queued', 'resolving', 'downloading', 'merging'].includes(task.status);
    const canResume = task.status === 'paused';
    const canRetry = ['failed', 'cancelled'].includes(task.status);
    const completed = task.status === 'completed';
    items.push({
      label: t('context.detail'),
      icon: <Icon name="info" size={14} />,
      onSelect: () => openPanel({ type: 'detail', taskId: task.id }),
    });
    items.push({ type: 'separator' });
    if (canPause) {
      items.push({
        label: t('context.pause'),
        icon: <Icon name="pause" size={14} />,
        onSelect: () => void pause(task.id),
      });
    }
    if (canResume) {
      items.push({
        label: t('context.resume'),
        icon: <Icon name="play" size={14} />,
        onSelect: () => void resume(task.id),
      });
    }
    if (canRetry) {
      items.push({
        label: t('context.retry'),
        icon: <Icon name="refresh" size={14} />,
        onSelect: () => void retry(task.id),
      });
    }
    if (completed) {
      items.push({
        label: pendingLabel('reveal', t('context.reveal')),
        icon: <Icon name="reveal" size={14} />,
        disabled: isPending('reveal'),
        onSelect: () => void api.openInFinder(task.outputPath ?? ''),
      });
      items.push({
        label: pendingLabel('open-file', t('context.open')),
        icon: <Icon name="external" size={14} />,
        disabled: isPending('open-file'),
        onSelect: () => void api.openFile(task.outputPath ?? ''),
      });
    }
    items.push({ type: 'separator' });
    items.push({
      label: pendingLabel('copy-link', t('context.copy')),
      icon: <Icon name="copy" size={14} />,
      disabled: isPending('copy-link'),
      onSelect: () => void copyLink(task.pageUrl),
    });
    const cancelling = ['queued', 'resolving', 'downloading', 'merging', 'paused'].includes(task.status);
    if (cancelling) {
      items.push({
        label: t('context.cancel'),
        icon: <Icon name="close" size={14} />,
        onSelect: () => void cancel(task.id),
      });
    }
    items.push({
      label: t('context.remove'),
      icon: <Icon name="trash" size={14} />,
      danger: true,
      onSelect: () => {
        void confirm({
          title: t('confirm.removeTask.title'),
          body: t('queue.row.delete.detail'),
          confirmLabel: t('common.remove'),
          danger: true,
        }).then((ok) => {
          if (ok) void remove(task.id);
        });
      },
    });
  } else {
    const entry = history.find((item) => item.id === target.historyId);
    if (!entry) return null;
    const missing = !entry.fileExists;
    items.push({
      label: pendingLabel('reveal', t('context.reveal')),
      icon: <Icon name="reveal" size={14} />,
      disabled: missing || isPending('reveal'),
      onSelect: () => void api.openInFinder(entry.outputPath),
    });
    items.push({
      label: pendingLabel('open-file', t('context.open')),
      icon: <Icon name="external" size={14} />,
      disabled: missing || isPending('open-file'),
      onSelect: () => void api.openFile(entry.outputPath),
    });
    items.push({
      label: pendingLabel('redownload', t('context.redownload')),
      icon: <Icon name="refresh" size={14} />,
      disabled: isPending('redownload'),
      onSelect: () => void handleParseText(entry.pageUrl),
    });
    items.push({
      label: pendingLabel('copy-link', t('context.copy')),
      icon: <Icon name="copy" size={14} />,
      disabled: isPending('copy-link'),
      onSelect: () => void copyLink(entry.pageUrl),
    });
    items.push({ type: 'separator' });
    items.push({
      label: t('history.delete'),
      icon: <Icon name="trash" size={14} />,
      danger: true,
      onSelect: () => {
        void confirm({
          title: t('history.deleteConfirm.title'),
          body: t('history.deleteConfirm.body'),
          confirmLabel: t('common.remove'),
          danger: true,
        }).then((ok) => {
          if (ok)
            void deleteEntry(entry.id).then(() =>
              pushToast({ kind: 'info', message: t('toast.historyDeleted') }),
            );
        });
      },
    });
  }

  return (
    <MenuPanel
      items={items}
      x={target.x}
      y={target.y}
      onClose={closeContext}
      ariaLabel="条目操作"
    />
  );
}
