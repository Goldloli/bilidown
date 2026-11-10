import { useEffect, useState } from 'react';
import { Skeleton } from './components/Skeleton';
import { useAsyncComponent } from './lib/asyncComponent';
import { MainWindow } from './shell/MainWindow';

/** 页面加载态：沿用主窗口骨架风格，不出现空白框架（tasks 8.1/8.2）。 */
function PageLoading({ label }: { label: string }) {
  return (
    <div className="flex h-full flex-col" style={{ background: 'var(--bg-window)' }}>
      <div
        className="flex items-center px-4"
        style={{ height: 'var(--titlebar-height, 38px)' }}
      >
        <Skeleton className="h-3 w-24" />
      </div>
      <main className="min-h-0 flex-1 px-4 pt-2" style={{ background: 'var(--bg-content)' }}>
        <p className="mb-3 text-[12px]" style={{ color: 'var(--text-secondary)' }}>
          {label}
        </p>
        <div className="space-y-2">
          {Array.from({ length: 4 }).map((_, index) => (
            <Skeleton key={index} className="h-10 w-full" />
          ))}
        </div>
      </main>
    </div>
  );
}

/** 页面加载失败：可读错误 + 重试（tasks 8.2）。 */
function PageError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-3 px-8 text-center"
      style={{ background: 'var(--bg-window)', color: 'var(--text-primary)' }}
    >
      <p className="text-[13px]">页面加载失败：{message}</p>
      <button
        type="button"
        className="rounded-md px-3 py-1 text-[12px]"
        style={{ background: 'var(--accent)', color: 'var(--accent-fg)' }}
        onClick={onRetry}
      >
        重新加载
      </button>
    </div>
  );
}

const loadPreferences = () =>
  import('./settings/PreferencesApp').then((module) => module.PreferencesApp);
const loadGallery = () => import('./dev/GalleryApp').then((module) => module.GalleryApp);

function useHashRoute(): 'main' | 'preferences' | 'gallery' {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const onChange = () => setHash(window.location.hash);
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  if (hash.startsWith('#/preferences')) return 'preferences';
  if (hash.startsWith('#/gallery')) return 'gallery';
  return 'main';
}

export function App() {
  const route = useHashRoute();
  const preferences = useAsyncComponent(loadPreferences, route === 'preferences');
  const gallery = useAsyncComponent(loadGallery, route === 'gallery');

  // 主窗口就绪后一次性预加载常用设置页面代码，避免首次打开设置时白屏（tasks 8.1）。
  useEffect(() => {
    if (route !== 'main') return;
    const preload = () => {
      void loadPreferences();
    };
    if (typeof window.requestIdleCallback === 'function') {
      const handle = window.requestIdleCallback(preload, { timeout: 3000 });
      return () => window.cancelIdleCallback?.(handle);
    }
    const timer = window.setTimeout(preload, 1200);
    return () => window.clearTimeout(timer);
  }, [route]);

  if (route === 'preferences') {
    if (preferences.error) {
      return <PageError message={preferences.error} onRetry={preferences.retry} />;
    }
    const Page = preferences.Page;
    return Page ? <Page /> : <PageLoading label="正在加载偏好设置…" />;
  }
  if (route === 'gallery') {
    if (gallery.error) {
      return <PageError message={gallery.error} onRetry={gallery.retry} />;
    }
    const Page = gallery.Page;
    return Page ? <Page /> : <PageLoading label="正在加载开发画廊…" />;
  }
  return <MainWindow />;
}
