import { useState } from 'react';
import { errorKindLabel } from '../lib/media';
import type { ErrorAction, TaskError } from '../lib/types';
import { Button } from './Button';
import { Icon } from './Icon';

interface ErrorBannerProps {
  error: TaskError;
  onAction?: (action: ErrorAction) => void;
  onRetry?: () => void;
  compact?: boolean;
}

/** 行内错误提示条：分类原因 + 建议操作 + 可折叠底层摘要（不使用浏览器 alert） */
export function ErrorBanner({ error, onAction, onRetry, compact }: ErrorBannerProps) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div
      className="rounded-card border border-line p-3"
      style={{ background: 'var(--danger-weak)' }}
      role="alert"
    >
      <div className="flex items-start gap-2">
        <Icon name="warning" size={16} className="mt-0.5 shrink-0 text-danger" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium text-fg">
            {errorKindLabel(error.kind)} · {error.message}
          </p>
          {error.hint && <p className="mt-1 text-2xs leading-[15px] text-fg2">{error.hint}</p>}
          {!compact && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              {error.actions.map((action) => (
                <Button
                  key={action}
                  size="sm"
                  variant={action === 'retry' ? 'primary' : 'secondary'}
                  onClick={() => (action === 'retry' && onRetry ? onRetry() : onAction?.(action))}
                >
                  {labelOf(action)}
                </Button>
              ))}
              {error.detail && (
                <button
                  type="button"
                  onClick={() => setExpanded((v) => !v)}
                  className="ml-auto inline-flex items-center gap-1 rounded-[5px] px-1.5 py-0.5 text-2xs text-fg2 hover:bg-hover"
                >
                  {expanded ? '收起错误详情' : '展开错误详情'}
                  <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={11} />
                </button>
              )}
            </div>
          )}
          {expanded && error.detail && (
            <pre className="mt-2 max-h-32 overflow-auto rounded-[6px] border border-line bg-field p-2 font-mono text-[10px] leading-[15px] text-fg2">
              {error.detail}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}

function labelOf(action: ErrorAction): string {
  const labels: Record<ErrorAction, string> = {
    retry: '重试',
    relogin: '重新登录',
    'open-settings': '打开设置',
    'copy-diagnostics': '复制诊断信息',
    'open-log': '显示日志',
    'choose-dir': '更换目录',
    'install-ffmpeg': '安装指引',
  };
  return labels[action];
}
