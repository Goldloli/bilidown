import type { ReactNode } from 'react';
import { BrandMark } from './BrandMark';
import { Icon, type IconName } from './Icon';

interface EmptyStateProps {
  brand?: boolean;
  icon?: IconName;
  title: string;
  subtitle?: string;
  children?: ReactNode;
  hint?: string;
  compact?: boolean;
}

/** 品牌空态：自有标识 + 标题 + 引导按钮 + 快捷键提示（图标为项目自绘） */
export function EmptyState({ brand, icon = 'inbox', title, subtitle, children, hint, compact }: EmptyStateProps) {
  return (
    <div className={`flex h-full flex-col items-center justify-center ${compact ? 'gap-2 py-10' : 'gap-2.5'}`}>
      {brand ? (
        <div className="mb-2">
          <BrandMark size={compact ? 56 : 84} />
        </div>
      ) : (
        <div className="mb-1 grid h-12 w-12 place-items-center rounded-[14px] bg-[var(--rail-bg)] text-fg3">
          <Icon name={icon} size={22} />
        </div>
      )}
      <h2 className="text-md font-semibold text-fg">{title}</h2>
      {subtitle && (
        <p className="max-w-[360px] truncate text-center text-2xs leading-[16px] text-fg2">{subtitle}</p>
      )}
      {children && <div className="mt-2 flex items-center gap-2">{children}</div>}
      {hint && <p className="text-2xs text-fg3">{hint}</p>}
    </div>
  );
}
