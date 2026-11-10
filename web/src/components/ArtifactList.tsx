import { api } from '../lib/ipc';
import { errorMessageOf } from '../lib/errors';
import { artifactFileTarget } from '../lib/fileAccess';
import { formatBytes } from '../lib/format';
import { t } from '../i18n/zh-CN';
import type { TaskArtifact } from '../lib/types';
import { useUiStore } from '../stores/useUiStore';
import { Icon } from './Icon';

function fileLabel(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

function artifactState(artifact: TaskArtifact): { label: string; tone: string } | null {
  if (!artifact.exists) return { label: '文件缺失', tone: 'text-danger' };
  if (!artifact.sizeMatches) return { label: '大小不匹配', tone: 'text-danger' };
  if (!artifact.valid) return { label: '记录异常', tone: 'text-warning' };
  return null;
}

export function ArtifactList({
  artifacts,
  className,
}: {
  artifacts: TaskArtifact[];
  className?: string;
}) {
  const pushToast = useUiStore((state) => state.pushToast);

  if (artifacts.length === 0) return null;

  return (
    <ul className={`space-y-1.5 ${className ?? ''}`}>
      {artifacts.map((artifact, index) => {
        const state = artifactState(artifact);
        const target = artifactFileTarget(artifact);
        const open = () => {
          // 缺失/大小不符/记录异常时给出可读提示，不把失败当作打开成功
          if (!target.available) {
            pushToast({ kind: 'warning', message: target.reason ?? t('file.missing') });
            return;
          }
          void api.openFile(target.path).catch((error) => {
            pushToast({
              kind: 'error',
              message: t('file.openFailed', { message: errorMessageOf(error) }),
            });
          });
        };
        const reveal = () => {
          if (!artifact.exists) {
            pushToast({ kind: 'warning', message: t('file.missing') });
            return;
          }
          void api.openInFinder(artifact.path).catch((error) => {
            pushToast({
              kind: 'error',
              message: t('file.revealFailed', { message: errorMessageOf(error) }),
            });
          });
        };
        return (
          <li
            key={`${artifact.path}:${artifact.kind}:${index}`}
            className="rounded-ctrl border border-line px-2.5 py-2"
            style={{ background: 'var(--card-inset)' }}
          >
            <div className="flex min-w-0 items-start gap-2">
              <Icon name="external" size={12} className="mt-0.5 shrink-0 text-fg3" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-xs text-fg" title={artifact.path}>
                  {fileLabel(artifact.path)}
                </div>
                <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-2xs text-fg3">
                  <span className="num">{formatBytes(artifact.size)}</span>
                  {artifact.entryTitle && <span>· {artifact.entryTitle}</span>}
                  {artifact.entryIndex != null && <span>· 条目 #{artifact.entryIndex + 1}</span>}
                  {state && <span className={state.tone}>· {state.label}</span>}
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <button
                  type="button"
                  aria-label={`打开第 ${index + 1} 个文件`}
                  title={target.available ? '打开文件' : (target.reason ?? t('file.missing'))}
                  onClick={(event) => {
                    event.stopPropagation();
                    open();
                  }}
                  className={`grid h-6 w-6 place-items-center rounded-[6px] text-fg2 hover:bg-hover hover:text-fg ${
                    target.available ? '' : 'opacity-45'
                  }`}
                >
                  <Icon name="external" size={13} />
                </button>
                <button
                  type="button"
                  aria-label={`定位第 ${index + 1} 个文件`}
                  title={artifact.exists ? '定位文件' : t('file.missing')}
                  onClick={(event) => {
                    event.stopPropagation();
                    reveal();
                  }}
                  className={`grid h-6 w-6 place-items-center rounded-[6px] text-fg2 hover:bg-hover hover:text-fg ${
                    artifact.exists ? '' : 'opacity-45'
                  }`}
                >
                  <Icon name="reveal" size={13} />
                </button>
              </div>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
