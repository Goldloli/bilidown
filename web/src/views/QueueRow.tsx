import { t } from '../i18n/zh-CN';
import { api } from '../lib/ipc';
import { isPending, pendingLabel } from '../lib/capabilities';
import { formatBytes, formatEta, formatSpeed } from '../lib/format';
import { kindIcon, kindLabel, statusLabel } from '../lib/media';
import { CAPSULE_TONE_STYLE, capsuleView, taskSubline } from '../lib/taskDisplay';
import type { QueueTask } from '../lib/types';
import { useQueueStore } from '../stores/useQueueStore';
import { useUiStore } from '../stores/useUiStore';
import { Icon, type IconName } from '../components/Icon';
import { Thumbnail } from '../components/Thumbnail';
import { Tooltip } from '../components/Tooltip';

interface QueueRowProps {
  task: QueueTask;
  'data-index'?: number;
}

function RowAction({
  icon,
  label,
  onClick,
  disabled,
}: {
  icon: IconName;
  label: string;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  disabled?: boolean;
}) {
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        onClick={(event) => {
          event.stopPropagation();
          onClick(event);
        }}
        className="grid h-[24px] w-[24px] place-items-center rounded-[6px] text-fg2 transition-colors duration-[var(--dur-fast)] hover:bg-hover hover:text-fg disabled:opacity-30"
      >
        <Icon name={icon} size={15} />
      </button>
    </Tooltip>
  );
}

export function QueueRow({ task }: QueueRowProps) {
  const openPanel = useUiStore((s) => s.openPanel);
  const openContext = useUiStore((s) => s.openContext);
  const confirm = useUiStore((s) => s.confirm);
  const pause = useQueueStore((s) => s.pause);
  const resume = useQueueStore((s) => s.resume);
  const retry = useQueueStore((s) => s.retry);
  const remove = useQueueStore((s) => s.remove);
  const capsule = capsuleView(task);
  const toneStyle = CAPSULE_TONE_STYLE[capsule.tone];
  const canPause = ['queued', 'resolving', 'downloading', 'merging'].includes(task.status);
  const canResume = task.status === 'paused';
  const canRetry = ['failed', 'cancelled'].includes(task.status);
  const completed = task.status === 'completed';
  const failed = task.status === 'failed';

  const rightMeta = (() => {
    switch (task.status) {
      case 'downloading':
        return task.progress.etaSec != null
          ? formatEta(task.progress.etaSec)
          : formatSpeed(task.progress.speedBps);
      case 'resolving':
        return '';
      case 'merging':
        return `${Math.round(task.progress.percent * 100)}%`;
      case 'paused':
        return task.restored
          ? '已恢复'
          : `${formatBytes(task.progress.doneBytes)} / ${formatBytes(task.progress.totalBytes)}`;
      case 'completed':
        return task.progress.totalBytes > 0
          ? formatBytes(task.progress.totalBytes)
          : task.params.qualityLabel;
      case 'failed':
        if (task.progress.percent >= 0.999) return '合并失败';
        if (task.progress.percent <= 0.001) return '解析失败';
        return `${Math.round(task.progress.percent * 100)}% 后失败`;
      case 'cancelled':
        return `${Math.round(task.progress.percent * 100)}%`;
      case 'queued':
        return '等待中';
      default:
        return '';
    }
  })();

  const openDetail = () => openPanel({ type: 'detail', taskId: task.id });
  const openRowMenu = (x: number, y: number) =>
    openContext({ type: 'task', taskId: task.id, x, y });

  return (
    <div
      role="listitem"
      tabIndex={0}
      aria-label={`${task.title}，${statusLabel(task.status)}`}
      data-task-id={task.id}
      data-status={task.status}
      onClick={openDetail}
      onContextMenu={(event) => {
        event.preventDefault();
        openRowMenu(event.clientX, event.clientY);
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault();
          openDetail();
        } else if (event.key === ' ') {
          event.preventDefault();
          if (canPause) void pause(task.id);
          else if (canResume) void resume(task.id);
        } else if (event.key === 'Delete' || event.key === 'Backspace') {
          event.preventDefault();
          void confirm({
            title: t('confirm.removeTask.title'),
            body: t('queue.row.delete.detail'),
            confirmLabel: t('common.remove'),
            danger: true,
          }).then((ok) => {
            if (ok) void remove(task.id);
          });
        }
      }}
      className="queue-row group flex h-[104px] cursor-default items-stretch gap-3 rounded-card p-2 outline-none transition-colors duration-[var(--dur-fast)] focus-visible:shadow-[0_0_0_2px_var(--ring)]"
      style={{
        background: 'var(--card)',
        boxShadow: '0 0 0 1px var(--hairline)',
      }}
    >
      <div className="relative shrink-0 self-center">
        <Thumbnail
          src={task.coverUrl}
          alt={task.title}
          className="h-[81px] w-[144px]"
          iconSize={20}
        />
        <span
          className="absolute left-1 top-1 inline-flex items-center gap-0.5 rounded-[4px] px-1 py-[1px] text-[10px] leading-[13px] text-white"
          style={{ background: 'rgba(0,0,0,0.52)' }}
        >
          <Icon name={kindIcon(task.kind) as IconName} size={10} />
          {kindLabel(task.kind)}
        </span>
        {(failed || task.status === 'cancelled') && (
          <span
            className="absolute inset-0 grid place-items-center rounded-tile"
            style={{ background: 'rgba(0,0,0,0.28)' }}
          >
            <span className="grid h-7 w-7 place-items-center rounded-full bg-black/45 text-white">
              <Icon name={failed ? 'warning' : 'close'} size={15} />
            </span>
          </span>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col justify-between py-[2px]">
        <div className="flex items-start gap-2">
          <div className="min-w-0 flex-1">
            <h3 className="truncate text-sm font-semibold leading-[18px] text-fg" title={task.title}>
              {task.title}
            </h3>
            <p className="mt-[2px] truncate text-2xs leading-[15px] text-fg2">{taskSubline(task)}</p>
          </div>
          <div className="relative flex h-6 shrink-0 items-center">
            <span className="num text-2xs text-fg3 transition-opacity duration-[var(--dur-fast)] group-hover:opacity-0 group-focus-within:opacity-0">
              {rightMeta}
            </span>
            <div className="absolute right-0 top-1/2 flex -translate-y-1/2 items-center gap-0.5 rounded-[6px] pl-1 opacity-0 transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100" style={{ background: 'var(--card)' }}>
            {canPause && (
              <RowAction icon="pause" label={t('queue.row.pause')} onClick={() => void pause(task.id)} />
            )}
            {canResume && (
              <RowAction icon="play" label={t('queue.row.resume')} onClick={() => void resume(task.id)} />
            )}
            {canRetry && (
              <RowAction icon="refresh" label={t('queue.row.retry')} onClick={() => void retry(task.id)} />
            )}
            {!completed && (
              <RowAction
                icon="more"
                label={t('queue.row.more')}
                onClick={(event) => {
                  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
                  openRowMenu(rect.right - 4, rect.bottom + 4);
                }}
              />
            )}
            </div>
          </div>
        </div>

        <div
          className="relative flex h-[24px] w-full items-center overflow-hidden rounded-full"
          style={{ background: toneStyle.background, color: toneStyle.color }}
        >
          {capsule.showFill && (
            <div
              className="progress-fill absolute inset-y-0 left-0"
              style={{
                width: `${Math.max(0, Math.min(1, capsule.fill)) * 100}%`,
                background: toneStyle.fill,
              }}
            />
          )}
          {capsule.indeterminate && capsule.tone === 'accent' && (
            <div className="indeterminate absolute inset-0 rounded-full opacity-60" />
          )}
          <div className="relative z-10 flex w-full min-w-0 items-center gap-1.5 px-2">
            <Icon
              name={
                capsule.icon === 'merge'
                  ? 'refresh'
                  : capsule.icon === 'loader'
                    ? 'loader'
                    : capsule.icon
              }
              size={13}
              className={capsule.icon === 'loader' || capsule.icon === 'merge' ? 'spinner shrink-0' : 'shrink-0'}
            />
            <span className="truncate text-xs font-medium">{capsule.text}</span>
            {capsule.right && (
              <span className="num ml-auto shrink-0 text-2xs opacity-85">{capsule.right}</span>
            )}
            {completed && (
              <span className="ml-auto flex shrink-0 items-center gap-1">
                <button
                  type="button"
                  aria-label={pendingLabel('reveal', t('queue.row.reveal'))}
                  disabled={isPending('reveal')}
                  title={pendingLabel('reveal', t('queue.row.reveal'))}
                  onClick={(event) => {
                    event.stopPropagation();
                    if (!isPending('reveal')) void api.openInFinder(task.outputPath ?? '');
                  }}
                  className="grid h-5 w-5 place-items-center rounded-full text-white/85 hover:bg-white/15 hover:text-white"
                >
                  <Icon name="reveal" size={12} />
                </button>
                <button
                  type="button"
                  aria-label={t('queue.row.more')}
                  onClick={(event) => {
                    event.stopPropagation();
                    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
                    openRowMenu(rect.right, rect.bottom + 4);
                  }}
                  className="grid h-5 w-5 place-items-center rounded-full text-white/85 hover:bg-white/15 hover:text-white"
                >
                  <Icon name="dots" size={12} />
                </button>
              </span>
            )}
            {failed && (
              <button
                type="button"
                onClick={(event) => {
                  event.stopPropagation();
                  void retry(task.id);
                }}
                className="ml-auto shrink-0 rounded-full bg-white/18 px-2 py-[1px] text-2xs font-medium text-white hover:bg-white/28"
              >
                {t('queue.row.retry')}
              </button>
            )}
            {task.status === 'paused' && (
              <button
                type="button"
                onClick={(event) => {
                  event.stopPropagation();
                  void resume(task.id);
                }}
                className="ml-auto shrink-0 rounded-full px-2 py-[1px] text-2xs font-medium hover:brightness-95"
                style={{ background: 'var(--hover-fill)', color: 'var(--accent-text)' }}
              >
                {t('queue.row.resume')}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
