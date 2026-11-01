import { useEffect, useState } from 'react';
import { t } from '../i18n/zh-CN';
import { handleParseText } from '../lib/actions';
import { devParams } from '../dev/scenario';
import { mockUgcSeason } from '../lib/collection';
import { useQueueStore } from '../stores/useQueueStore';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useUiStore } from '../stores/useUiStore';
import { ConfirmDialogHost } from '../components/ConfirmDialog';
import { Icon } from '../components/Icon';
import { QueueRowSkeleton } from '../components/Skeleton';
import { ToastViewport } from '../components/Toast';
import { BottomBar } from './BottomBar';
import { ContextMenuHost } from './ContextMenuHost';
import { PanelHost } from './PanelHost';
import { TitleBar } from './TitleBar';
import { HistoryView } from '../views/HistoryView';
import { QueueView } from '../views/QueueView';

export function MainWindow() {
  const snapshot = useQueueStore((s) => s.snapshot);
  const settings = useSettingsStore((s) => s.settings);
  const view = useUiStore((s) => s.view);
  const panel = useUiStore((s) => s.panel);
  const [dragOver, setDragOver] = useState(false);

  const ready = Boolean(snapshot && settings);

  // dev QA 场景：自动弹出 UGC 合集确认，供截图流水线（正常用户不会命中）
  useEffect(() => {
    if (!ready || devParams().scenario !== 'season-confirm') return;
    const ui = useUiStore.getState();
    const season = mockUgcSeason();
    void ui.confirm({
      title: t('collection.seasonTitle'),
      body: t('collection.seasonBody', { title: season.title, n: season.episodes.length }),
      confirmLabel: t('collection.seasonConfirm'),
      cancelLabel: t('collection.seasonCurrent'),
    });
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
          if (text.trim()) void handleParseText(text);
        }}
      >
        {view === 'queue' ? <QueueView /> : <HistoryView />}
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
      <ContextMenuHost />
      <ToastViewport />
      <ConfirmDialogHost />
    </div>
  );
}
