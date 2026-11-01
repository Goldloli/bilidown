import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauriEnv } from '../lib/glass';

/**
 * Windows 风格窗口按钮（最小化 / 最大化-还原 / 关闭）。
 * macOS 使用系统交通灯（overlay 标题栏），本组件由 CSS `[data-os="macos"]` 隐藏。
 * 浏览器 mock 环境渲染为禁用态占位，便于截图评审。
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  const tauri = isTauriEnv();

  useEffect(() => {
    if (!tauri) return;
    const win = getCurrentWindow();
    let disposed = false;
    void win.isMaximized().then((value) => {
      if (!disposed) setMaximized(value);
    });
    const unlisten = win.onResized(() => {
      void win.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    });
    return () => {
      disposed = true;
      void unlisten.then((fn) => fn());
    };
  }, [tauri]);

  const call = (action: 'minimize' | 'toggleMaximize' | 'close') => () => {
    if (!tauri) return;
    const win = getCurrentWindow();
    if (action === 'minimize') void win.minimize();
    else if (action === 'toggleMaximize') void win.toggleMaximize();
    else void win.close();
  };

  return (
    <div className="win-controls absolute right-0 top-0 h-full" aria-hidden={false}>
      <button
        type="button"
        className="win-control-btn"
        aria-label="最小化"
        title="最小化"
        onClick={call('minimize')}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5h10" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
      <button
        type="button"
        className="win-control-btn"
        aria-label={maximized ? '还原' : '最大化'}
        title={maximized ? '还原' : '最大化'}
        onClick={call('toggleMaximize')}
      >
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M2.5 2.5h5v5h-5z" fill="none" stroke="currentColor" strokeWidth="1" />
            <path d="M4 2.5V1h5v5H7.5" fill="none" stroke="currentColor" strokeWidth="1" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1" />
          </svg>
        )}
      </button>
      <button
        type="button"
        className="win-control-btn win-control-close"
        aria-label="关闭"
        title="关闭"
        onClick={call('close')}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
    </div>
  );
}
