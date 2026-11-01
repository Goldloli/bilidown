import { useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { t } from '../i18n/zh-CN';
import { useQueueStore } from '../stores/useQueueStore';
import { useUiStore } from '../stores/useUiStore';
import { Button } from '../components/Button';
import { EmptyState } from '../components/EmptyState';
import { Icon } from '../components/Icon';
import { QueueRowSkeleton } from '../components/Skeleton';
import { QueueRow } from './QueueRow';

function matches(task: { title: string; uploader: string }, keyword: string): boolean {
  const q = keyword.trim().toLowerCase();
  if (!q) return true;
  return task.title.toLowerCase().includes(q) || task.uploader.toLowerCase().includes(q);
}

export function QueueView() {
  const snapshot = useQueueStore((s) => s.snapshot);
  const loading = useQueueStore((s) => s.loading);
  const error = useQueueStore((s) => s.error);
  const keyword = useQueueStore((s) => s.keyword);
  const refresh = useQueueStore((s) => s.refresh);
  const openPanel = useUiStore((s) => s.openPanel);

  const tasks = useMemo(
    () => (snapshot ? snapshot.tasks.filter((task) => matches(task, keyword)) : []),
    [snapshot, keyword],
  );

  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: tasks.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 112,
    overscan: 8,
  });
  const renderedRows = virtualizer.getVirtualItems().length;
  const setRenderedQueueRows = useUiStore((s) => s.setRenderedQueueRows);
  useEffect(() => {
    setRenderedQueueRows(renderedRows);
  }, [renderedRows, setRenderedQueueRows]);

  if (loading && !snapshot) {
    return (
      <div className="h-full space-y-2 px-4 pt-1">
        {Array.from({ length: 5 }).map((_, index) => (
          <QueueRowSkeleton key={index} />
        ))}
      </div>
    );
  }

  if (error) {
    return (
      <EmptyState
        icon="warning"
        title={t('queue.error.title')}
        subtitle={error}
        hint={undefined}
      >
        <Button variant="secondary" icon="refresh" onClick={() => void refresh()}>
          {t('queue.error.retry')}
        </Button>
      </EmptyState>
    );
  }

  if (snapshot && snapshot.tasks.length === 0) {
    return (
      <EmptyState
        brand
        title={t('empty.title')}
        subtitle={t('empty.subtitle')}
      >
        <Button variant="primary" size="lg" icon="plus" onClick={() => openPanel({ type: 'add-link' })}>
          {t('empty.action')}
        </Button>
      </EmptyState>
    );
  }

  if (tasks.length === 0) {
    return (
      <EmptyState
        compact
        icon="search"
        title={t('queue.search.empty')}
        subtitle={`“${keyword}”`}
      >
        <Button variant="secondary" onClick={() => useQueueStore.getState().setKeyword('')}>
          {t('queue.search.clear')}
        </Button>
      </EmptyState>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {keyword.trim() && (
        <div className="flex h-8 shrink-0 items-center gap-1.5 px-4">
          <span className="inline-flex items-center gap-1 rounded-full bg-accent-weak px-2 py-0.5 text-2xs text-accent">
            <Icon name="search" size={11} />
            {keyword}
            <button
              type="button"
              aria-label={t('queue.search.clear')}
              onClick={() => useQueueStore.getState().setKeyword('')}
              className="ml-0.5 hover:opacity-70"
            >
              <Icon name="close" size={10} />
            </button>
          </span>
          <span className="num text-2xs text-fg3">{tasks.length} 个结果</span>
        </div>
      )}
      <div
        ref={parentRef}
        className="scroll-area min-h-0 flex-1 overflow-y-auto px-4 pt-1"
        style={{
          paddingBottom: 'calc(var(--bottombar-h) + var(--bottombar-inset) + 24px)',
        }}
      >
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative', width: '100%' }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const task = tasks[virtualRow.index];
            return (
              <div
                key={task.id}
                data-index={virtualRow.index}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: 112,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <div className="pb-2">
                  <QueueRow task={task} />
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
