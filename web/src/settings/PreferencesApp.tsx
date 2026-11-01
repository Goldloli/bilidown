import { useEffect, useRef, useState } from 'react';
import { t, type StringKey } from '../i18n/zh-CN';
import { devParams } from '../dev/scenario';
import { useSettingsStore } from '../stores/useSettingsStore';
import { Icon, type IconName } from '../components/Icon';
import { LoginPanel } from '../panels/LoginPanel';
import { ConfirmDialogHost } from '../components/ConfirmDialog';
import { ToastViewport } from '../components/Toast';
import { TrafficLights } from '../shell/TrafficLights';
import { WindowControls } from '../shell/WindowControls';
import { AccountPage } from './pages/AccountPage';
import { AboutPage } from './pages/AboutPage';
import { DownloadPage } from './pages/DownloadPage';
import { GeneralPage } from './pages/GeneralPage';
import { InterfacePage } from './pages/InterfacePage';
import { NetworkPage } from './pages/NetworkPage';
import { QualityPage } from './pages/QualityPage';
import { Skeleton } from '../components/Skeleton';

type CategoryKey = 'general' | 'interface' | 'download' | 'quality' | 'network' | 'account' | 'about';

const CATEGORIES: Array<{ key: CategoryKey; labelKey: StringKey; icon: IconName }> = [
  { key: 'general', labelKey: 'settings.category.general', icon: 'settings' },
  { key: 'interface', labelKey: 'settings.category.interface', icon: 'monitor' },
  { key: 'download', labelKey: 'settings.category.download', icon: 'download' },
  { key: 'quality', labelKey: 'settings.category.quality', icon: 'film' },
  { key: 'network', labelKey: 'settings.category.network', icon: 'wifi' },
  { key: 'account', labelKey: 'settings.category.account', icon: 'user' },
  { key: 'about', labelKey: 'settings.category.about', icon: 'info' },
];

function isCategory(value: string): value is CategoryKey {
  return CATEGORIES.some((category) => category.key === value);
}

/** 独立偏好设置窗口：hash 路由 #/preferences，7 分类 + 即时生效表单 */
export function PreferencesApp() {
  const params = devParams();
  const initial = isCategory(params.category) ? params.category : 'general';
  const [category, setCategory] = useState<CategoryKey>(initial);
  const [loginOpen, setLoginOpen] = useState(false);
  const settings = useSettingsStore((s) => s.settings);
  const lastSavedAt = useSettingsStore((s) => s.lastSavedAt);
  const [, forceTick] = useState(0);
  const contentRef = useRef<HTMLElement>(null);

  useEffect(() => {
    // 打开窗口时不把焦点留在侧栏第一项（避免出现无意的 focus ring）
    contentRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!settings) void useSettingsStore.getState().reload();
  }, [settings]);

  useEffect(() => {
    if (!lastSavedAt) return;
    const timer = window.setInterval(() => forceTick((value) => value + 1), 500);
    return () => window.clearInterval(timer);
  }, [lastSavedAt]);

  useEffect(() => {
    if (!settings) return;
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        document.documentElement.dataset.ready = '1';
      }),
    );
    return () => cancelAnimationFrame(raf);
  }, [settings]);

  const saved = Boolean(lastSavedAt && Date.now() - lastSavedAt < 1600);
  const current = CATEGORIES.find((item) => item.key === category) ?? CATEGORIES[0];

  const page = (() => {
    switch (category) {
      case 'general':
        return <GeneralPage />;
      case 'interface':
        return <InterfacePage />;
      case 'download':
        return <DownloadPage />;
      case 'quality':
        return <QualityPage />;
      case 'network':
        return <NetworkPage />;
      case 'account':
        return <AccountPage onLogin={() => setLoginOpen(true)} />;
      case 'about':
        return <AboutPage />;
      default:
        return null;
    }
  })();

  const windowContent = (
    <div className="relative flex h-full flex-col overflow-hidden" style={{ background: 'var(--bg-window)' }}>
      <header
        data-tauri-drag-region
        className="drag-region titlebar relative flex h-[var(--titlebar-h)] shrink-0 items-center gap-1.5"
      >
        {params.mock && params.os !== 'windows' && <TrafficLights />}
        <span className="text-sm font-semibold text-fg">{t('settings.title')}</span>
        <span className="text-2xs text-fg3">{t(current.labelKey)}</span>
        <span className="ml-auto flex h-5 items-center gap-1 text-2xs text-success">
          {saved && (
            <>
              <Icon name="check-circle" size={12} />
              {t('settings.saved')}
            </>
          )}
        </span>
        <WindowControls />
      </header>

      <div className="flex min-h-0 flex-1 gap-3 px-3 pb-3">
        <aside
          className="glass-surface w-[var(--sidebar-w)] shrink-0 overflow-y-auto rounded-[16px] p-2"
          style={{ background: 'var(--sidebar)' }}
          aria-label="设置分类"
        >
          <nav className="flex flex-col gap-0.5">
            {CATEGORIES.map((item) => {
              const active = item.key === category;
              return (
                <button
                  key={item.key}
                  type="button"
                  aria-current={active ? 'page' : undefined}
                  onClick={() => setCategory(item.key)}
                  className={`flex h-9 w-full items-center gap-2.5 rounded-tile px-2.5 text-left text-sm transition-colors duration-[var(--dur-fast)] ${
                    active
                      ? 'bg-accent font-medium text-on-accent'
                      : 'text-fg hover:bg-hover active:bg-pressed'
                  }`}
                >
                  <Icon name={item.icon} size={18} className={active ? 'text-on-accent' : 'text-fg2'} />
                  <span className="truncate">{t(item.labelKey)}</span>
                </button>
              );
            })}
          </nav>
        </aside>

        <main
          ref={contentRef}
          data-prefs-scroll
          tabIndex={-1}
          className="min-w-0 flex-1 overflow-y-auto rounded-[16px] p-6 outline-none"
          style={{ background: 'var(--bg-content)' }}
        >
          <div className="mx-auto max-w-[600px]">
            {settings ? (
              page
            ) : (
              <div className="space-y-4">
                <Skeleton className="h-4 w-32" />
                <Skeleton className="h-24 w-full" rounded="rounded-card" />
                <Skeleton className="h-4 w-24" />
                <Skeleton className="h-36 w-full" rounded="rounded-card" />
              </div>
            )}
          </div>
        </main>
      </div>

      <ToastViewport />
      <ConfirmDialogHost />
      {loginOpen && <LoginPanel onClose={() => setLoginOpen(false)} />}
    </div>
  );

  if (!params.mock) return windowContent;

  // 浏览器评审：用 880×600 窗口中框模拟独立偏好设置窗口
  return (
    <div className="grid h-full place-items-center" style={{ background: 'var(--desktop-backdrop)' }}>
      <div
        className="h-[600px] w-[880px] overflow-hidden rounded-[12px] border border-black/20 shadow-[0_28px_70px_rgba(0,0,0,0.38)]"
        style={{ background: 'var(--bg-window)' }}
      >
        {windowContent}
      </div>
    </div>
  );
}
