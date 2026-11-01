import { t } from '../i18n/zh-CN';
import { api } from '../lib/ipc';
import { isMockMode, isPending, pendingLabel } from '../lib/capabilities';
import { mockTaskExtras } from '../dev/detailExtras';
import { formatBytes, formatDateTime, formatDuration, formatEta, formatPercent, formatSpeed } from '../lib/format';
import { codecLabel, errorActionLabel, kindLabel, phaseLabel, statusLabel } from '../lib/media';
import type { ErrorAction, TaskArtifact } from '../lib/types';
import { useQueueStore } from '../stores/useQueueStore';
import { useUiStore } from '../stores/useUiStore';
import { Button } from '../components/Button';
import { ArtifactList } from '../components/ArtifactList';
import { ErrorBanner } from '../components/ErrorBanner';
import { Icon } from '../components/Icon';
import { ProgressBar } from '../components/ProgressBar';
import { Sheet } from '../components/Sheet';
import { StatusBadge } from '../components/StatusBadge';
import { Thumbnail } from '../components/Thumbnail';

const STAGE_ICON = {
  pending: 'clock',
  active: 'loader',
  done: 'check-circle',
  failed: 'warning',
  skipped: 'minus',
} as const;

export function TaskDetailPanel({ taskId, onClose }: { taskId: string; onClose: () => void }) {
  const task = useQueueStore((s) => s.snapshot?.tasks.find((item) => item.id === taskId) ?? null);
  const pause = useQueueStore((s) => s.pause);
  const resume = useQueueStore((s) => s.resume);
  const cancel = useQueueStore((s) => s.cancel);
  const retry = useQueueStore((s) => s.retry);
  const openPanel = useUiStore((s) => s.openPanel);
  const pushToast = useUiStore((s) => s.pushToast);

  if (!task) {
    return (
      <Sheet open onClose={onClose} width={520} ariaLabel={t('detail.title')}>
        <div className="flex h-40 items-center justify-center text-xs text-fg2">任务不存在或已移除</div>
      </Sheet>
    );
  }

  const percent = task.progress.percent;
  const canPause = ['queued', 'resolving', 'downloading', 'merging'].includes(task.status);
  const canResume = task.status === 'paused';
  const canCancel = ['queued', 'resolving', 'downloading', 'merging', 'paused'].includes(task.status);
  const canRetry = ['failed', 'cancelled'].includes(task.status);
  const completed = task.status === 'completed';
  const artifacts: TaskArtifact[] =
    task.artifacts ??
    (task.outputPath
      ? [
          {
            path: task.outputPath,
            size: task.progress.doneBytes || task.progress.totalBytes,
            entryIndex: null,
            entryTitle: null,
            kind: 'media',
            exists: true,
            sizeMatches: true,
            valid: true,
          },
        ]
      : []);
  // 完成态由 core 提供 extras（经 adapt 映射）；mock 下用只读假数据展示布局
  const extras = task.extras ?? (isMockMode() ? mockTaskExtras(task) : null);
  const hasExtras = Boolean(
    extras && (extras.subtitles.length > 0 || extras.cover || extras.subtitleError || extras.coverError),
  );

  const onErrorAction = (action: ErrorAction) => {
    switch (action) {
      case 'relogin':
        openPanel({ type: 'login' });
        break;
      case 'open-settings':
        void api.openPreferencesWindow();
        break;
      case 'copy-diagnostics':
        void api.copyDiagnostics().then(() => pushToast({ kind: 'success', message: '诊断信息已复制' }));
        break;
      case 'open-log':
        void api.revealLogs();
        break;
      case 'choose-dir':
        void api.chooseDirectory(task.params.outputDir);
        break;
      case 'install-ffmpeg':
        void api.openExternal('https://ffmpeg.org/download.html');
        break;
      case 'retry':
        void retry(task.id);
        break;
    }
  };

  return (
    <Sheet open onClose={onClose} width={560} ariaLabel={t('detail.title')}>
      <div className="scroll-area min-h-0 flex-1 overflow-y-auto px-5 pb-2 pt-4">
        <div className="flex items-start gap-3">
          <Thumbnail src={task.coverUrl} alt={task.title} className="h-[99px] w-[176px] shrink-0" iconSize={22} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <StatusBadge
                tone={
                  task.status === 'completed'
                    ? 'success'
                    : task.status === 'failed'
                      ? 'danger'
                      : task.status === 'paused' || task.status === 'cancelled'
                        ? 'neutral'
                        : 'accent'
                }
                dot
              >
                {statusLabel(task.status)}
              </StatusBadge>
              <span className="text-2xs text-fg3">{kindLabel(task.kind)}</span>
            </div>
            <h2 className="mt-1.5 line-clamp-2 text-md font-semibold leading-[20px] text-fg">
              {task.title}
            </h2>
            <p className="mt-1 truncate text-2xs text-fg2">{task.uploader}</p>
            <div className="mt-1.5 flex items-center gap-1.5">
              <button
                type="button"
                disabled={isPending('copy-link')}
                title={pendingLabel('copy-link', t('detail.copyLink'))}
                onClick={() => {
                  if (isPending('copy-link')) return;
                  void api.copyText(task.pageUrl);
                  pushToast({ kind: 'success', message: t('toast.copied') });
                }}
                className="inline-flex items-center gap-1 rounded-[5px] px-1.5 py-0.5 text-2xs text-accent hover:bg-accent-weak disabled:pointer-events-none disabled:opacity-40"
              >
                <Icon name="link" size={11} />
                {t('detail.copyLink')}
              </button>
            </div>
          </div>
          <button
            type="button"
            aria-label={t('common.close')}
            onClick={onClose}
            className="grid h-7 w-7 shrink-0 place-items-center rounded-tile text-fg3 hover:bg-hover hover:text-fg"
          >
            <Icon name="close" size={15} />
          </button>
        </div>

        {task.error && (
          <div className="mt-4">
            <ErrorBanner error={task.error} onAction={onErrorAction} onRetry={() => void retry(task.id)} />
          </div>
        )}

        <div className="mt-4 rounded-card border border-line p-3.5" style={{ background: 'var(--card-inset)' }}>
          <div className="flex items-baseline justify-between">
            <span className="text-xs font-medium text-fg">{t('detail.totalProgress')}</span>
            <span className="num text-md font-semibold text-fg">{formatPercent(percent, 1)}</span>
          </div>
          <div className="mt-2">
            <ProgressBar
              value={percent}
              tone={
                task.status === 'failed'
                  ? 'danger'
                  : task.status === 'completed'
                    ? 'success'
                    : task.status === 'paused' || task.status === 'cancelled' || task.status === 'queued'
                      ? 'neutral'
                      : 'accent'
              }
              height={8}
              ariaLabel={t('detail.totalProgress')}
            />
          </div>
          <div className="num mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-2xs text-fg2">
            {task.progress.totalBytes > 0 && (
              <span>
                {t('detail.downloaded')} {formatBytes(task.progress.doneBytes)} /{' '}
                {formatBytes(task.progress.totalBytes)}
              </span>
            )}
            {task.progress.speedBps > 0 && (
              <span>
                {t('detail.speed')} {formatSpeed(task.progress.speedBps)}
              </span>
            )}
            {task.progress.etaSec != null && <span>{formatEta(task.progress.etaSec)}</span>}
          </div>
        </div>

        <div className="mt-4">
          <h3 className="mb-2 text-xs font-semibold text-fg">{t('detail.stages')}</h3>
          <div className="space-y-1.5">
            {task.stages.map((stage) => (
              <div key={stage.phase} className="flex items-center gap-2">
                <Icon
                  name={STAGE_ICON[stage.status]}
                  size={13}
                  className={
                    stage.status === 'active'
                      ? 'spinner shrink-0 text-accent'
                      : stage.status === 'done'
                        ? 'shrink-0 text-success'
                        : stage.status === 'failed'
                          ? 'shrink-0 text-danger'
                          : 'shrink-0 text-fg3'
                  }
                />
                <span className={`w-16 shrink-0 text-2xs ${stage.status === 'active' ? 'text-fg' : 'text-fg2'}`}>
                  {phaseLabel(stage.phase)}
                </span>
                <div className="min-w-0 flex-1">
                  <ProgressBar
                    value={stage.status === 'done' ? 1 : stage.status === 'active' ? percent : 0}
                    tone={stage.status === 'failed' ? 'danger' : stage.status === 'done' ? 'success' : 'accent'}
                    height={4}
                    indeterminate={stage.status === 'active' && task.status === 'merging'}
                    ariaLabel={phaseLabel(stage.phase)}
                  />
                </div>
                <span className="num w-16 shrink-0 text-right text-2xs text-fg3">
                  {stage.status === 'done' ? '完成' : stage.status === 'skipped' ? '跳过' : stage.status === 'pending' ? '等待' : formatBytes(stage.doneBytes)}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="mt-4">
          <h3 className="mb-1.5 text-xs font-semibold text-fg">{t('detail.params')}</h3>
          <dl className="grid grid-cols-[88px_1fr] gap-x-3 gap-y-1.5 text-2xs">
            <dt className="text-fg3">{t('detail.entries')}</dt>
            <dd className="num text-fg2">
              {task.params.entryCount} 项
              {task.params.entries[0] ? ` · ${task.params.entries[0]}` : ''}
            </dd>
            <dt className="text-fg3">{t('detail.quality')}</dt>
            <dd className="text-fg2">{task.params.qualityLabel}</dd>
            <dt className="text-fg3">{t('detail.codec')}</dt>
            <dd className="text-fg2">{codecLabel(task.params.codec)}</dd>
            {task.kind === 'audio' && (
              <>
                <dt className="text-fg3">{t('detail.audio')}</dt>
                <dd className="text-fg2">{task.params.audio === 'mp3' ? 'MP3' : '原格式'}</dd>
              </>
            )}
            <dt className="text-fg3">{t('detail.output')}</dt>
            <dd className="break-all text-fg2">{task.outputPath ?? '—'}</dd>
            <dt className="text-fg3">{t('detail.createdAt')}</dt>
            <dd className="text-fg2">{formatDateTime(task.createdAt)}</dd>
            {task.durationSec != null && (
              <>
                <dt className="text-fg3">时长</dt>
                <dd className="text-fg2">{formatDuration(task.durationSec)}</dd>
              </>
            )}
          </dl>
        </div>

        {artifacts.length > 0 && (
          <div className="mt-4">
            <div className="mb-2 flex items-center justify-between gap-2">
              <h3 className="text-xs font-semibold text-fg">{t('detail.files')}</h3>
              <span className="num text-2xs text-fg3">
                {t('artifacts.total', {
                  n: artifacts.length,
                  size: formatBytes(artifacts.reduce((sum, artifact) => sum + artifact.size, 0)),
                })}
              </span>
            </div>
            {task.artifactCoverage === 'legacy-single' && (
              <p className="mb-2 text-2xs text-warning">{t('artifacts.legacy')}</p>
            )}
            <ArtifactList artifacts={artifacts} />
          </div>
        )}

        {hasExtras && extras && (
          <div className="mt-4">
            <h3 className="mb-1.5 text-xs font-semibold text-fg">{t('detail.extras')}</h3>
            <dl className="grid grid-cols-[88px_1fr] gap-x-3 gap-y-1.5 text-2xs">
              <dt className="text-fg3">{t('detail.subtitles')}</dt>
              <dd className="min-w-0 break-all text-fg2">
                {extras.subtitles.length > 0 ? (
                  <ul className="space-y-0.5">
                    {extras.subtitles.map((file) => (
                      <li key={file}>{file}</li>
                    ))}
                  </ul>
                ) : (
                  '—'
                )}
              </dd>
              {extras.cover && (
                <>
                  <dt className="text-fg3">{t('detail.coverFile')}</dt>
                  <dd className="break-all text-fg2">{extras.cover}</dd>
                </>
              )}
              {extras.subtitleError && (
                <>
                  <dt className="text-fg3">{t('detail.subtitleError')}</dt>
                  <dd className="break-all text-danger">{extras.subtitleError}</dd>
                </>
              )}
              {extras.coverError && (
                <>
                  <dt className="text-fg3">{t('detail.coverError')}</dt>
                  <dd className="break-all text-danger">{extras.coverError}</dd>
                </>
              )}
            </dl>
          </div>
        )}
      </div>

      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-line px-5 py-3">
        <div className="flex flex-wrap gap-1.5">
          {canPause && (
            <Button size="sm" variant="secondary" icon="pause" onClick={() => void pause(task.id)}>
              {t('detail.pause')}
            </Button>
          )}
          {canResume && (
            <Button size="sm" variant="secondary" icon="play" onClick={() => void resume(task.id)}>
              {t('detail.resume')}
            </Button>
          )}
          {canRetry && (
            <Button size="sm" variant="primary" icon="refresh" onClick={() => void retry(task.id)}>
              {t('detail.retry')}
            </Button>
          )}
          {completed && (
            <>
              <Button
                size="sm"
                variant="secondary"
                icon="reveal"
                disabled={isPending('reveal')}
                title={pendingLabel('reveal', t('detail.reveal'))}
                onClick={() => {
                  if (!isPending('reveal')) void api.openInFinder(task.outputPath ?? '');
                }}
              >
                {t('detail.reveal')}
              </Button>
              <Button
                size="sm"
                variant="secondary"
                icon="external"
                disabled={isPending('open-file')}
                title={pendingLabel('open-file', t('detail.openFile'))}
                onClick={() => {
                  if (!isPending('open-file')) void api.openFile(task.outputPath ?? '');
                }}
              >
                {t('detail.openFile')}
              </Button>
            </>
          )}
          {canCancel && (
            <Button
              size="sm"
              variant="ghost"
              icon="close"
              className="text-danger hover:bg-danger-weak"
              onClick={() => void cancel(task.id)}
            >
              取消任务
            </Button>
          )}
        </div>
        <Button size="sm" variant="secondary" onClick={onClose}>
          {t('detail.close')}
        </Button>
      </div>
    </Sheet>
  );
}

export const DETAIL_ACTION_LABELS = errorActionLabel;
