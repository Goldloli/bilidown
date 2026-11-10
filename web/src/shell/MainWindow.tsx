import { useEffect, useState } from 'react';
import { t } from '../i18n/zh-CN';
import { retryStartupCore, retryStartupSection } from '../lib/bootstrap';
import { devParams } from '../dev/scenario';
import { useAsyncComponent } from '../lib/asyncComponent';
import { useQueueStore } from '../stores/useQueueStore';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useStartupStore } from '../stores/useStartupStore';
import { useUiStore } from '../stores/useUiStore';
import { Icon } from '../components/Icon';
import { QueueRowSkeleton } from '../components/Skeleton';
import { BottomBar } from './BottomBar';
import { PanelHost } from './PanelHost';
import { TitleBar } from './TitleBar';
import { QueueView } from '../views/QueueView';

const loadHistoryView = () =>
  import('../views/HistoryView').then((module) => module.HistoryView);
const loadContextMenu = () =>
  import('./ContextMenuHost').then((module) => module.ContextMenuHost);
const loadOverlayHost = () => import('./OverlayHost').then((module) => module.OverlayHost);
const loadCoreErrorPanel = () =>
  import('../components/CoreErrorPanel').then((module) => module.CoreErrorPanel);

export function MainWindow() {
  const snapshot = useQueueStore((s) => s.snapshot);
  const settings = useSettingsStore((s) => s.settings);
  const view = useUiStore((s) => s.view);
  const panel = useUiStore((s) => s.panel);
  const [dragOver, setDragOver] = useState(false);
  const contextTarget = useUiStore((s) => s.contextTarget);
  // 启动分阶段与局部失败（tasks 5.1/5.3）：核心失败时窗口仍可见并可重试。
  const corePhase = useStartupStore((s) => s.corePhase);
  const coreError = useStartupStore((s) => s.coreError);
  const retryingCore = useStartupStore((s) => s.retryingCore);
  const queueSection = useStartupStore((s) => s.sections.queue);
  const settingsSection = useStartupStore((s) => s.sections.settings);
  const historyView = useAsyncComponent(loadHistoryView, view === 'history');
  const contextMenu = useAsyncComponent(loadContextMenu, contextTarget !== null);
  // 覆盖层在首次渲染后立刻异步加载（不阻塞首屏，事件到达前通常已就绪）。
  const overlayHost = useAsyncComponent(loadOverlayHost, true);
  const coreErrorPanel = useAsyncComponent(loadCoreErrorPanel, coreError !== null);

  // 队列分区就绪即渲染本地内容；设置失败不阻塞队列/历史（设置窗口独立展示错误）。
  const ready = Boolean(snapshot) || queueSection.status === 'error';

  // dev QA 场景：自动弹出 UGC 合集确认，供截图流水线（正常用户不会命中）
  useEffect(() => {
    if (!ready || devParams().scenario !== 'season-confirm') return;
    // 仅截图场景需要的 Mock 数据按需加载，不进入首屏包（tasks 8.1）。
    void (async () => {
      const { mockUgcSeason } = await import('../lib/collection');
      const ui = useUiStore.getState();
      const season = mockUgcSeason();
      void ui.confirm({
        title: t('collection.seasonTitle'),
        body: t('collection.seasonBody', { title: season.title, n: season.episodes.length }),
        confirmLabel: t('collection.seasonConfirm'),
        cancelLabel: t('collection.seasonCurrent'),
      });
    })();
  }, [ready]);

  useEffect(() => {
    if (!ready) return;
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        document.documentElement.dataset.ready = '1';
      }),
    );
    return () => cancelAnimationFrame(raf);
  }, [ready]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      if (event.key === 'Escape') {
        const ui = useUiStore.getState();
        if (ui.panel) ui.closePanel();
        else if (ui.contextTarget) ui.closeContext();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  if (corePhase === 'failed' && coreError) {
    return (
      <div className="flex h-full flex-col" style={{ background: 'var(--bg-window)' }}>
        <TitleBar />
        <main
          className="grid min-h-0 flex-1 place-items-center px-8"
          style={{ background: 'var(--bg-content)' }}
        >
          {coreErrorPanel.Page ? (
            <coreErrorPanel.Page
              message={coreError}
              retrying={retryingCore}
              onRetry={() => void retryStartupCore()}
            />
          ) : (
            <p className="text-[13px]">引擎初始化失败：{coreError}</p>
          )}
        </main>
      </div>
    );
  }

  if (!ready) {
    return (
      <div className="flex h-full flex-col" style={{ background: 'var(--bg-window)' }}>
        <TitleBar />
        <main className="min-h-0 flex-1 px-4 pt-1" style={{ background: 'var(--bg-content)' }}>
          <div className="space-y-2">
            {Array.from({ length: 5 }).map((_, index) => (
              <QueueRowSkeleton key={index} />
            ))}
          </div>
        </main>
        <BottomBar />
      </div>
    );
  }

  return (
    <div className="relative flex h-full flex-col overflow-hidden" style={{ background: 'var(--bg-window)' }}>
      <TitleBar />
      <main
        className="relative min-h-0 flex-1"
        style={{ background: 'var(--bg-content)' }}
        onDragOver={(event) => {
          event.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={(event) => {
          if (event.currentTarget === event.target) setDragOver(false);
        }}
        onDrop={(event) => {
          event.preventDefault();
          setDragOver(false);
          const text =
            event.dataTransfer.getData('text/plain') ||
            event.dataTransfer.getData('text/uri-list');
          if (text.trim()) {
            void import('../lib/actions').then(({ handleParseText }) => handleParseText(text));
          }
        }}
      >
        {settingsSection.status === 'error' && !settings && (
          <div
            role="alert"
            className="mx-4 mt-2 flex items-center gap-2 rounded-card border border-line px-3 py-1.5"
            style={{ background: 'var(--danger-weak)' }}
          >
            <Icon name="warning" size={13} className="shrink-0 text-danger" />
            <span className="min-w-0 flex-1 truncate text-2xs text-fg">
              {t('startup.settingsFailed', { message: settingsSection.error ?? '' })}
            </span>
            <button
              type="button"
              className="shrink-0 rounded-[6px] px-2 py-0.5 text-2xs text-accent hover:bg-accent-weak"
              onClick={() => void retryStartupSection('settings')}
            >
              {t('startup.retry')}
            </button>
          </div>
        )}
        {view === 'queue' ? (
          <QueueView />
        ) : historyView.Page ? (
          <historyView.Page />
        ) : historyView.error ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
            <p className="text-[13px]">历史记录加载失败：{historyView.error}</p>
            <button
              type="button"
              className="rounded-md px-3 py-1 text-[12px]"
              style={{ background: 'var(--accent)', color: 'var(--accent-fg)' }}
              onClick={historyView.retry}
            >
              重新加载
            </button>
          </div>
        ) : (
          <div className="space-y-2 px-4 pt-2">
            {Array.from({ length: 5 }).map((_, index) => (
              <QueueRowSkeleton key={index} />
            ))}
          </div>
        )}
        {dragOver && (
          <div
            className="pointer-events-none absolute inset-3 z-30 grid place-items-center rounded-sheet border-2 border-dashed"
            style={{ borderColor: 'var(--accent)', background: 'var(--selection-fill)' }}
          >
            <div className="flex flex-col items-center gap-2 text-accent">
              <Icon name="download" size={28} />
              <span className="text-sm font-semibold">{t('empty.drag')}</span>
            </div>
          </div>
        )}
        {panel === null && view === 'queue' && snapshot && snapshot.tasks.length > 0 && (
          <span className="sr-only" aria-live="polite">
            {snapshot.stats.total} 个任务
          </span>
        )}
      </main>
      <BottomBar />
      <PanelHost />
      {contextMenu.Page ? <contextMenu.Page /> : null}
      {overlayHost.Page ? <overlayHost.Page /> : null}
    </div>
  );
}
