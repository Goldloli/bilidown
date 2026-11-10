/**
 * 退出流程（design D8/3.9）：
 * · 正常退出保存成功由后端直接退出，界面不增加确认步骤；
 * · 保存失败时只显示一次「重试保存 / 仍然退出 / 取消」对话框；
 * · 重试成功继续退出，仍然退出是用户明确放弃未保存状态。
 */
import { api } from './ipc';
import { errorMessageOf } from './errors';
import { useUiStore } from '../stores/useUiStore';
import { t } from '../i18n/zh-CN';

/** 尝试正常退出；保存失败时打开失败对话框并返回 false（不退出）。 */
export async function attemptQuit(): Promise<boolean> {
  const ui = useUiStore.getState();
  try {
    await api.quitApp();
    ui.dismissQuitFailure();
    return true;
  } catch (error) {
    ui.showQuitFailure(errorMessageOf(error));
    return false;
  }
}

/** 失败对话框里的「重试保存」：再次走正常退出，成功即退出。 */
export async function retryQuitSave(): Promise<boolean> {
  return attemptQuit();
}

/** 「仍然退出」：用户明确放弃未保存状态后强制退出。 */
export async function forceQuit(): Promise<void> {
  const ui = useUiStore.getState();
  try {
    await api.forceQuitApp();
    ui.dismissQuitFailure();
  } catch (error) {
    ui.pushToast({
      kind: 'error',
      message: t('quit.saveFailed.forceError', { message: errorMessageOf(error) }),
    });
  }
}

/** 「取消」：留在应用内，未保存状态保持可重试。 */
export function cancelQuit(): void {
  useUiStore.getState().dismissQuitFailure();
}
