import { useEffect, useRef, useState } from 'react';
import { SaveFeedbackController } from './saveFeedback';

/**
 * 消费 Store 的 `lastSavedAt`：每次保存成功只显示一次「已保存」，
 * 用一次性到期计时关闭；窗口隐藏/组件卸载时释放计时器（tasks 4.7）。
 */
export function useSavedFeedback(lastSavedAt: number | null): boolean {
  const [visible, setVisible] = useState(false);
  const controllerRef = useRef<SaveFeedbackController | null>(null);
  if (!controllerRef.current) {
    controllerRef.current = new SaveFeedbackController({ onChange: setVisible });
  }

  useEffect(() => {
    if (lastSavedAt) controllerRef.current?.markSaved();
  }, [lastSavedAt]);

  useEffect(() => {
    const controller = controllerRef.current;
    if (!controller) return undefined;
    const syncSuspension = () => {
      const hidden = typeof document !== 'undefined' && document.visibilityState === 'hidden';
      controller.setSuspended(hidden);
    };
    syncSuspension();
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', syncSuspension);
    }
    return () => {
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', syncSuspension);
      }
      controller.dispose();
    };
  }, []);

  return visible;
}
