import { api } from '../lib/ipc';
import { formatBytes } from '../lib/format';
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

  const run = (action: Promise<void>, failure: string) => {
    void action.catch((error) => {
      pushToast({
        kind: 'error',
        message: `${failure}：${error instanceof Error ? error.message : String(error)}`,
      });
    });
  };

  return (
    <ul className={`space-y-1.5 ${className ?? ''}`}>
      {artifacts.map((artifact, index) => {
        const state = artifactState(artifact);
        const canOpen = artifact.exists && artifact.sizeMatches && artifact.valid;
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
                  disabled={!canOpen}
                  aria-label={`打开第 ${index + 1} 个文件`}
                  title="打开文件"
                  onClick={(event) => {
                    event.stopPropagation();
                    run(api.openFile(artifact.path), '打开文件失败');
                  }}
                  className="grid h-6 w-6 place-items-center rounded-[6px] text-fg2 hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-30"
                >
                  <Icon name="external" size={13} />
                </button>
                <button
                  type="button"
                  disabled={!artifact.exists}
                  aria-label={`定位第 ${index + 1} 个文件`}
                  title="定位文件"
                  onClick={(event) => {
                    event.stopPropagation();
                    run(api.openInFinder(artifact.path), '定位文件失败');
                  }}
                  className="grid h-6 w-6 place-items-center rounded-[6px] text-fg2 hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-30"
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
