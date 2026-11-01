import type { ReactNode } from 'react';
import { Icon, type IconName } from './Icon';

export type BadgeTone = 'accent' | 'success' | 'danger' | 'warning' | 'neutral' | 'vip';

const TONES: Record<BadgeTone, string> = {
  accent: 'bg-accent-weak text-accent',
  success: 'bg-success-weak text-success',
  danger: 'bg-danger-weak text-danger',
  warning: 'bg-warning/12 text-warning',
  neutral: 'bg-hover text-fg2',
  vip: 'bg-accent-weak text-accent',
};

interface StatusBadgeProps {
  tone: BadgeTone;
  children: ReactNode;
  icon?: IconName;
  dot?: boolean;
  className?: string;
}

export function StatusBadge({ tone, children, icon, dot, className }: StatusBadgeProps) {
  return (
    <span
      className={`inline-flex h-[20px] items-center gap-1 rounded-tag px-1.5 text-2xs font-medium ${TONES[tone]} ${className ?? ''}`}
    >
      {dot && <span className="h-1.5 w-1.5 rounded-full bg-current" />}
      {icon && <Icon name={icon} size={11} />}
      {children}
    </span>
  );
}
