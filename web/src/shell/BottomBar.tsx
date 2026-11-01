import { useRef, useState } from 'react';
import { t } from '../i18n/zh-CN';
import { api } from '../lib/ipc';
import { showsSettingsShortcut } from '../lib/platform';
import { addFromClipboard } from '../lib/actions';
import { statusSummary } from '../lib/taskDisplay';
import { useHistoryStore } from '../stores/useHistoryStore';
import { useQueueStore } from '../stores/useQueueStore';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useUiStore } from '../stores/useUiStore';
import { Icon, type IconName } from '../components/Icon';
import { IconButton } from '../components/IconButton';
import { Popover } from '../components/Popover';
import { TextField } from '../components/TextField';
import { Button } from '../components/Button';

const SHORTCUTS: Array<[string, string]> = [
  ['⌘O', t('help.openLink')],
  ['⌃⌘V', t('help.clipboard')],
  ['⌘S', t('help.startAll')],
  ['⌃⌘S', t('help.pauseAll')],
  ['⇧⌘R', t('help.retryAll')],
  ['⌘K', t('help.clearDone')],
  ['⇧⌘K', t('help.clearIdle')],
  ['⌘,', t('help.settings')],
  ['⌘W', t('help.close')],
  ['⌘Q', t('help.quit')],
];

/** 底部悬浮胶囊操作条：左（添加/剪贴板）· 中（全局状态）· 右（搜索/清空/历史/帮助） */
export function BottomBar() {
  const stats = useQueueStore((s) => s.snapshot?.stats);
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const openPanel = useUiStore((s) => s.openPanel);
  const setSearchOpen = useUiStore((s) => s.setSearchOpen);
  const confirm = useUiStore((s) => s.confirm);
  const showSpeed = useSettingsStore((s) => s.settings?.interface.showSpeedInStatus ?? true);
  const clearCompleted = useQueueStore((s) => s.clearCompleted);

  const searchAnchor = useRef<HTMLButtonElement>(null);
  const helpAnchor = useRef<HTMLButtonElement>(null);
  const [searchOpen, setQueueSearchOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const keyword = useQueueStore((s) => s.keyword);
  const setKeyword = useQueueStore((s) => s.setKeyword);

  const completed = stats?.completed ?? 0;
  const renderedRows = useUiStore((s) => s.renderedQueueRows);
  const historyEntries = useHistoryStore((s) => s.entries);
  const historyKeyword = useHistoryStore((s) => s.keyword);
  const historyStatus = useHistoryStore((s) => s.status);
  const historyFiltered = historyEntries.filter((entry) => {
    const keywordMatches = historyKeyword.trim()
      ? entry.title.toLowerCase().includes(historyKeyword.trim().toLowerCase()) ||
        entry.uploader.toLowerCase().includes(historyKeyword.trim().toLowerCase())
      : true;
    const statusMatches = historyStatus === 'all' || entry.status === historyStatus;
    return keywordMatches && statusMatches;
  }).length;
  const filtered = Boolean(historyKeyword.trim()) || historyStatus !== 'all';

  const centerText =
    view === 'history'
      ? filtered
        ? `${historyFiltered} / ${historyEntries.length} 条历史`
        : `共 ${historyEntries.length} 条历史`
      : stats
        ? statusSummary(stats, showSpeed, stats.total > renderedRows)
        : '—';

  return (
    <div className="glass-surface absolute inset-x-4 bottom-4 z-40 flex h-12 items-center gap-1 rounded-full border border-line px-2 shadow-[var(--shadow-bottombar)]" style={{ background: 'var(--panel)' }}>
      <button
        type="button"
        onClick={() => openPanel({ type: 'add-link' })}
        className="inline-flex h-7 items-center gap-1.5 rounded-full bg-accent-weak px-2.5 text-xs font-medium text-accent transition-colors duration-[var(--dur-fast)] hover:bg-accent-weak2"
      >
        <Icon name="plus" size={15} />
        <span className="hidden min-[820px]:inline">{t('bottombar.add')}</span>
      </button>
      <IconButton
        icon="clipboard"
        label={t('bottombar.clipboard')}
        onClick={() => void addFromClipboard()}
      />
      {showsSettingsShortcut() && (
        <IconButton
          icon="gear"
          label={t('settings.title')}
          onClick={() => void api.openPreferencesWindow()}
        />
      )}

      <div className="min-w-0 flex-1 px-2 text-center">
        <span className="num truncate text-2xs text-fg2">{centerText}</span>
      </div>

      <div className="flex items-center gap-0.5">
        <button
          ref={searchAnchor}
          type="button"
          aria-label={t('bottombar.search')}
          onClick={() => {
            if (view === 'history') setSearchOpen(true);
            else setQueueSearchOpen((v) => !v);
          }}
          className={`grid h-7 w-7 place-items-center rounded-tile transition-colors duration-[var(--dur-fast)] ${
            view === 'history' ? 'text-fg2 hover:bg-hover' : 'text-fg2 hover:bg-hover hover:text-fg'
          }`}
        >
          <Icon name="search" size={18} />
        </button>
        <span className="grid place-items-center">
          <IconButton
            icon="trash"
            label={t('bottombar.clear')}
            disabled={completed === 0}
            onClick={() => {
              void confirm({
                title: t('bottombar.clear'),
                body: '已完成的任务将从队列移除并保留在历史中，磁盘文件不受影响。',
                confirmLabel: '清空',
              }).then((ok) => {
                if (ok) void clearCompleted();
              });
            }}
          />
        </span>
        <IconButton
          icon="history"
          label={t('bottombar.history')}
          active={view === 'history'}
          onClick={() => setView(view === 'history' ? 'queue' : 'history')}
        />
        <button
          ref={helpAnchor}
          type="button"
          aria-label={t('bottombar.help')}
          onClick={() => setHelpOpen((v) => !v)}
          className="grid h-7 w-7 place-items-center rounded-tile text-fg2 transition-colors duration-[var(--dur-fast)] hover:bg-hover hover:text-fg"
        >
          <Icon name="help" size={18} />
        </button>
      </div>

      <Popover
        open={searchOpen}
        anchorRef={searchAnchor}
        onClose={() => setQueueSearchOpen(false)}
        align="end"
        side="top"
        width={248}
      >
        <div className="p-1">
          <TextField
            value={keyword}
            onChange={setKeyword}
            placeholder={t('queue.search.placeholder')}
            width="100%"
            ariaLabel={t('bottombar.search')}
            trailing={
              keyword ? (
                <button
                  type="button"
                  aria-label={t('queue.search.clear')}
                  onClick={() => setKeyword('')}
                  className="text-fg3 hover:text-fg"
                >
                  <Icon name="close" size={12} />
                </button>
              ) : undefined
            }
          />
          <p className="px-1 pt-1.5 text-2xs text-fg3">
            {stats ? `${stats.total} 个任务` : ''} · 按标题或 UP 主过滤
          </p>
        </div>
      </Popover>

      <Popover
        open={helpOpen}
        anchorRef={helpAnchor}
        onClose={() => setHelpOpen(false)}
        align="end"
        side="top"
        width={272}
      >
        <div className="p-1">
          <div className="px-2 py-1 text-2xs font-semibold text-fg3">{t('help.title')}</div>
          {SHORTCUTS.map(([key, label]) => (
            <div key={key} className="flex items-center justify-between rounded-[6px] px-2 py-[3px]">
              <span className="text-2xs text-fg2">{label}</span>
              <span className="font-mono text-[10px] text-fg3">{key}</span>
            </div>
          ))}
          <div className="my-1 h-px bg-line" />
          <div className="flex gap-1.5 px-1 pb-1">
            <Button
              size="sm"
              variant="secondary"
              icon="file"
              onClick={() => {
                void api.revealLogs();
                setHelpOpen(false);
              }}
            >
              {t('bottombar.menu.logs')}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon="settings"
              onClick={() => {
                void api.openPreferencesWindow();
                setHelpOpen(false);
              }}
            >
              偏好设置
            </Button>
          </div>
        </div>
      </Popover>
    </div>
  );
}

export const BOTTOM_ICONS: IconName[] = ['plus', 'clipboard', 'star', 'search', 'trash', 'history', 'help'];
