import '../i18n/zh-CN.dynamic';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { t } from '../i18n/zh-CN';
import { copyLink, handleParseText } from '../lib/actions';
import { formatBytes, formatDateTime, formatDateGroup } from '../lib/format';
import { kindIcon } from '../lib/media';
import { api } from '../lib/ipc';
import { retryStartupSection } from '../lib/bootstrap';
import { errorMessageOf } from '../lib/errors';
import { isPending, pendingLabel, type PendingFeature } from '../lib/capabilities';
import type { HistoryEntry, HistoryStatus, TaskArtifact } from '../lib/types';
import { useHistoryStore } from '../stores/useHistoryStore';
import { useUiStore } from '../stores/useUiStore';
import { Button } from '../components/Button';
import { ArtifactList } from '../components/ArtifactList';
import { EmptyState } from '../components/EmptyState';
import { Icon, type IconName } from '../components/Icon';
import { IconButton } from '../components/IconButton';
import { SegmentedControl } from '../components/SegmentedControl';
import { StatusBadge } from '../components/StatusBadge';
import { TextField } from '../components/TextField';
import { Thumbnail } from '../components/Thumbnail';
import { Tooltip } from '../components/Tooltip';

type FlatItem =
  | { type: 'header'; key: string; label: string }
  | { type: 'entry'; key: string; entry: HistoryEntry };

const STATUS_BADGE: Record<HistoryStatus, { tone: 'success' | 'danger' | 'neutral'; label: string }> = {
  completed: { tone: 'success', label: t('history.filter.completed') },
  failed: { tone: 'danger', label: t('history.filter.failed') },
  cancelled: { tone: 'neutral', label: t('history.filter.cancelled') },
};

function HistoryRow({ entry }: { entry: HistoryEntry }) {
  const filesVerified = useHistoryStore((s) => s.filesVerified);
  const pushToast = useUiStore((s) => s.pushToast);
  const confirm = useUiStore((s) => s.confirm);
  const openContext = useUiStore((s) => s.openContext);
  const deleteEntry = useHistoryStore((s) => s.deleteEntry);
  const badge = STATUS_BADGE[entry.status];
  const [showFiles, setShowFiles] = useState(false);
  const artifacts: TaskArtifact[] =
    entry.artifacts ??
    (entry.outputPath
      ? [
          {
            path: entry.outputPath,
            size: entry.fileSize,
            entryIndex: null,
            entryTitle: null,
            kind: 'media',
            exists: entry.fileExists,
            sizeMatches: entry.fileExists,
            valid: entry.fileExists,
          },
        ]
      : []);

  const reveal = () => {
    if (!entry.fileExists) {
      pushToast({ kind: 'warning', message: t('file.missing') });
      return;
    }
    void api.openInFinder(entry.outputPath).catch((error) => {
      pushToast({ kind: 'error', message: t('file.revealFailed', { message: errorMessageOf(error) }) });
    });
  };

  const open = () => {
    if (!entry.fileExists) {
      pushToast({ kind: 'warning', message: t('history.missing') });
      return;
    }
    void api.openFile(entry.outputPath).catch((error) => {
      pushToast({ kind: 'error', message: t('file.openFailed', { message: errorMessageOf(error) }) });
    });
  };

  const redownload = () => {
    void handleParseText(entry.pageUrl);
  };

  const del = () => {
    void confirm({
      title: t('history.deleteConfirm.title'),
      body: t('history.deleteConfirm.body'),
      confirmLabel: t('common.remove'),
      danger: true,
    }).then((ok) => {
      if (ok)
        void deleteEntry(entry.id).then(() =>
          pushToast({ kind: 'info', message: t('toast.historyDeleted') }),
        );
    });
  };

  const action = (
    icon: IconName,
    label: string,
    onClick: () => void,
    disabled?: boolean,
    pending?: PendingFeature,
  ) => {
    const blocked = Boolean(disabled) || (pending ? isPending(pending) : false);
    return (
      <Tooltip label={pending ? pendingLabel(pending, label) : label}>
        <button
          type="button"
          aria-label={pending ? pendingLabel(pending, label) : label}
          disabled={blocked}
          onClick={(event) => {
            event.stopPropagation();
            onClick();
          }}
          className="grid h-6 w-6 place-items-center rounded-[6px] text-fg2 transition-colors duration-[var(--dur-fast)] hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-30"
        >
          <Icon name={icon} size={15} />
        </button>
      </Tooltip>
    );
  };

  return (
    <div
      role="listitem"
      tabIndex={0}
      aria-label={`${entry.title}，${badge.label}`}
      onDoubleClick={open}
      onContextMenu={(event) => {
        event.preventDefault();
        openContext({ type: 'history', historyId: entry.id, x: event.clientX, y: event.clientY });
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') open();
      }}
      className="group flex min-h-[64px] flex-wrap items-center gap-3 rounded-card px-2 py-1 transition-colors duration-[var(--dur-fast)] hover:bg-hover focus-visible:bg-hover"
    >
      <Thumbnail src={entry.coverUrl} alt={entry.title} className="h-[54px] w-[96px] shrink-0" iconSize={16} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <Icon name={kindIcon(entry.kind) as IconName} size={12} className="shrink-0 text-fg3" />
          <span className="truncate text-sm font-medium text-fg">{entry.title}</span>
        </div>
        <div className="mt-1 flex items-center gap-1.5 text-2xs text-fg2">
          <StatusBadge tone={badge.tone}>{badge.label}</StatusBadge>
          <span className="truncate">{entry.qualityLabel}</span>
          {!filesVerified && entry.status === 'completed' && (
            <StatusBadge tone="neutral">{t('files.checking')}</StatusBadge>
          )}
          {filesVerified && !entry.fileExists && entry.status === 'completed' && (
            <StatusBadge tone="neutral">{t('history.missing')}</StatusBadge>
          )}
          {entry.artifactCoverage === 'legacy-single' && (
            <StatusBadge tone="neutral">旧记录</StatusBadge>
          )}
          {(artifacts.length > 1 || entry.artifactCoverage === 'legacy-single') && (
            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                setShowFiles((shown) => !shown);
              }}
              className="shrink-0 rounded-[5px] px-1 text-accent hover:bg-accent-weak"
            >
              {artifacts.length} 个文件 · {showFiles ? '收起' : '展开'}
            </button>
          )}
        </div>
      </div>
      <div className="relative flex h-6 shrink-0 items-center">
        <span className="num text-2xs text-fg3 transition-opacity duration-[var(--dur-fast)] group-hover:opacity-0 group-focus-within:opacity-0">
          {formatBytes(entry.fileSize)} · {formatDateTime(entry.finishedAt)}
        </span>
        <div
          className="absolute right-0 top-1/2 flex -translate-y-1/2 items-center gap-0.5 rounded-[6px] pl-1 opacity-0 transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100"
          style={{ background: 'var(--bg-content)' }}
        >
          {action('reveal', t('history.reveal'), reveal, false, 'reveal')}
          {action('external', t('history.open'), open, false, 'open-file')}
          {action('refresh', t('history.redownload'), redownload, false, 'redownload')}
          {action('copy', t('history.copy'), () => void copyLink(entry.pageUrl), false, 'copy-link')}
          {action('trash', t('history.delete'), del)}
        </div>
      </div>
      {showFiles && artifacts.length > 0 && (
        <div className="basis-full pb-1 pl-[108px] pr-2">
          {entry.artifactCoverage === 'legacy-single' && (
            <p className="mb-1.5 text-2xs text-warning">{t('artifacts.legacy')}</p>
          )}
          <ArtifactList artifacts={artifacts} filesVerified={filesVerified} />
        </div>
      )}
    </div>
  );
}

export function HistoryView() {
  const entries = useHistoryStore((s) => s.entries);
  const keyword = useHistoryStore((s) => s.keyword);
  const status = useHistoryStore((s) => s.status);
  const loading = useHistoryStore((s) => s.loading);
  const error = useHistoryStore((s) => s.error);
  const setKeyword = useHistoryStore((s) => s.setKeyword);
  const setStatus = useHistoryStore((s) => s.setStatus);
  const clearAll = useHistoryStore((s) => s.clearAll);
  const setView = useUiStore((s) => s.setView);
  const searchOpen = useUiStore((s) => s.searchOpen);
  const setSearchOpen = useUiStore((s) => s.setSearchOpen);
  const confirm = useUiStore((s) => s.confirm);
  const pushToast = useUiStore((s) => s.pushToast);

  const filtered = useMemo(
    () => useHistoryStore.getState().filtered(),
    // entries / keyword / status 变化时重算（selector 返回新数组会导致无限循环，故显式依赖）
    [entries, keyword, status],
  );
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (searchOpen) {
      searchRef.current?.focus();
      setSearchOpen(false);
    }
  }, [searchOpen, setSearchOpen]);

  // 打开历史列表时重新拉取文件状态，不信任旧展示缓存（design D9/tasks 7.2）。
  useEffect(() => {
    void useHistoryStore.getState().load();
  }, []);

  const flat = useMemo<FlatItem[]>(() => {
    const items: FlatItem[] = [];
    let lastGroup = '';
    for (const entry of filtered) {
      const group = formatDateGroup(entry.finishedAt);
      if (group !== lastGroup) {
        items.push({ type: 'header', key: `h-${group}`, label: group });
        lastGroup = group;
      }
      items.push({ type: 'entry', key: entry.id, entry });
    }
    return items;
  }, [filtered]);

  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: flat.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (index) => (flat[index]?.type === 'header' ? 32 : 72),
    overscan: 10,
  });

  const clear = () => {
    void confirm({
      title: t('history.clearConfirm.title'),
      body: t('history.clearConfirm.body'),
      confirmLabel: '清空历史',
      danger: true,
    }).then((ok) => {
      if (ok)
        void clearAll().then(() =>
          pushToast({ kind: 'info', message: t('toast.historyCleared') }),
        );
    });
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-[44px] shrink-0 items-center gap-2 px-3">
        <IconButton icon="arrow-left" label={t('history.back')} onClick={() => setView('queue')} />
        <h1 className="text-md font-semibold text-fg">{t('history.title')}</h1>
        <span className="num text-2xs text-fg3">{t('history.resultCount', { n: filtered.length })}</span>
        <div className="ml-auto flex items-center gap-2">
          <TextField
            inputRef={searchRef}
            value={keyword}
            onChange={setKeyword}
            placeholder={t('history.search.placeholder')}
            width={200}
            ariaLabel={t('history.search.placeholder')}
            trailing={<Icon name="search" size={13} className="text-fg3" />}
          />
          <SegmentedControl
            size="sm"
            ariaLabel="历史状态筛选"
            value={status}
            onChange={(value) => setStatus(value)}
            options={[
              { value: 'all', label: t('history.filter.all') },
              { value: 'completed', label: t('history.filter.completed') },
              { value: 'failed', label: t('history.filter.failed') },
              { value: 'cancelled', label: t('history.filter.cancelled') },
            ]}
          />
          <IconButton
            icon="trash"
            label={t('history.clear')}
            disabled={entries.length === 0}
            onClick={clear}
          />
        </div>
      </div>

      {error && entries.length > 0 && (
        <div
          role="alert"
          className="mx-4 mb-1 flex items-center gap-2 rounded-card border border-line px-3 py-1.5"
          style={{ background: 'var(--danger-weak)' }}
        >
          <Icon name="warning" size={13} className="shrink-0 text-danger" />
          <span className="min-w-0 flex-1 truncate text-2xs text-fg">
            {t('history.error.banner', { message: error })}
          </span>
          <button
            type="button"
            className="shrink-0 rounded-[6px] px-2 py-0.5 text-2xs text-accent hover:bg-accent-weak"
            onClick={() => void retryStartupSection('history')}
          >
            {t('history.error.retry')}
          </button>
        </div>
      )}
      {loading && entries.length === 0 ? (
        <div className="px-4 py-2 text-xs text-fg2">正在加载历史…</div>
      ) : error && entries.length === 0 ? (
        <EmptyState icon="warning" title={t('history.error.title')} subtitle={error}>
          <Button variant="secondary" icon="refresh" onClick={() => void retryStartupSection('history')}>
            {t('history.error.retry')}
          </Button>
        </EmptyState>
      ) : entries.length === 0 ? (
        <EmptyState
          icon="history"
          title={t('history.empty.title')}
          subtitle={t('history.empty.subtitle')}
        />
      ) : filtered.length === 0 ? (
        <EmptyState compact icon="search" title={t('history.searchEmpty')}>
          <Button
            variant="secondary"
            onClick={() => {
              setKeyword('');
              setStatus('all');
            }}
          >
            清除筛选
          </Button>
        </EmptyState>
      ) : (
        <div
          ref={parentRef}
          className="scroll-area min-h-0 flex-1 overflow-y-auto px-3"
          style={{ paddingBottom: 'calc(var(--bottombar-h) + var(--bottombar-inset) + 24px)' }}
        >
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative', width: '100%' }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const item = flat[virtualRow.index];
              return (
                <div
                  key={item.key}
                  data-index={virtualRow.index}
                  ref={virtualizer.measureElement}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  {item.type === 'header' ? (
                    <div className="flex h-8 items-end px-1 pb-1 text-2xs font-semibold text-fg3">
                      {item.label}
                    </div>
                  ) : (
                    <div className="pb-1">
                      <HistoryRow entry={item.entry} />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
