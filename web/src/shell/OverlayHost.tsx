import { ConfirmDialogHost } from '../components/ConfirmDialog';
import { QuitFailureDialogHost } from '../components/QuitFailureDialog';
import { ToastViewport } from '../components/Toast';

/**
 * 覆盖层宿主（toast / 确认框 / 退出保存失败）：首次渲染后异步加载，
 * 不进入主窗口首屏包（tasks 8.1）。
 */
export function OverlayHost() {
  return (
    <>
      <ToastViewport />
      <ConfirmDialogHost />
      <QuitFailureDialogHost />
    </>
  );
}
