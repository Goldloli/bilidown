import { t } from '../i18n/zh-CN';
import { api } from './ipc';
import { addFromClipboard, handleParseText } from './actions';
import { useAccountStore } from '../stores/useAccountStore';
import { useQueueStore } from '../stores/useQueueStore';
import { useUiStore } from '../stores/useUiStore';

/** 原生菜单动作 id（COMMANDS.md §菜单动作）→ 与界面按钮同一套 action */
export async function handleMenuAction(id: string): Promise<void> {
  const ui = useUiStore.getState();
  const queue = useQueueStore.getState();
  switch (id) {
    case 'open-link':
      ui.openPanel({ type: 'add-link' });
      break;
    case 'clipboard-add':
      await addFromClipboard();
      break;
    case 'start-all':
      await queue.resumeAll();
      break;
    case 'pause-all':
      await queue.pauseAll();
      break;
    case 'retry-all':
      await queue.retryFailed();
      break;
    case 'clear-finished': {
      const ok = await ui.confirm({
        title: t('bottombar.clear'),
        body: '已完成的任务将从队列移除并保留在历史中，磁盘文件不受影响。',
        confirmLabel: '清空',
      });
      if (ok) await queue.clearCompleted();
      break;
    }
    case 'clear-inactive': {
      const ok = await ui.confirm({
        title: '清空未激活的任务？',
        body: '排队、暂停与已取消的任务将从队列移除，已下载文件与历史记录不受影响。',
        confirmLabel: '清空',
        danger: true,
      });
      if (ok) await queue.cancelAllQueued();
      break;
    }
    case 'preferences':
      await api.openPreferencesWindow();
      break;
    case 'login':
      ui.openPanel({ type: 'login' });
      break;
    case 'logout': {
      const ok = await ui.confirm({
        title: t('account.logoutConfirm.title'),
        body: t('account.logoutConfirm.body'),
        confirmLabel: t('account.logoutConfirm.ok'),
        danger: true,
      });
      if (ok) {
        await useAccountStore.getState().logout();
        ui.pushToast({ kind: 'info', message: '已退出登录' });
      }
      break;
    }
    case 'refresh-account':
      await useAccountStore.getState().refreshAccount();
      ui.pushToast({ kind: 'success', message: '登录态已刷新' });
      break;
    case 'show-logs':
      await api.revealLogs();
      break;
    case 'copy-diagnostics':
      await api.copyDiagnostics();
      ui.pushToast({ kind: 'success', message: '诊断信息已复制' });
      break;
    case 'quit': {
      const hasActive = await api.requestQuitCheck();
      if (!hasActive) await api.quitApp();
      break;
    }
    case 'open-link-from-menu': {
      const text = await api.readClipboardText();
      if (text.trim()) await handleParseText(text);
      break;
    }
    default:
      break;
  }
}
