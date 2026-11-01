export type ProgressTone = 'accent' | 'success' | 'danger' | 'neutral' | 'warning';

const FILL: Record<ProgressTone, string> = {
  accent: 'bg-accent',
  success: 'bg-success',
  danger: 'bg-danger',
  neutral: 'bg-fg3',
  warning: 'bg-warning',
};

interface ProgressBarProps {
  value: number;
  tone?: ProgressTone;
  indeterminate?: boolean;
  height?: number;
  className?: string;
  ariaLabel?: string;
}

/** 通用进度条（行内胶囊与详情阶段共用 fill/tone 规则） */
export function ProgressBar({
  value,
  tone = 'accent',
  indeterminate,
  height = 6,
  className,
  ariaLabel,
}: ProgressBarProps) {
  const percent = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
  return (
    <div
      role="progressbar"
      aria-label={ariaLabel}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(percent * 100)}
      className={`relative w-full overflow-hidden rounded-full ${indeterminate ? 'indeterminate' : ''} ${className ?? ''}`}
      style={{ height, background: 'var(--rail-bg)' }}
    >
      <div
        className={`progress-fill absolute inset-y-0 left-0 rounded-full ${FILL[tone]}`}
        style={{ width: indeterminate ? '35%' : `${percent * 100}%` }}
      />
    </div>
  );
}
