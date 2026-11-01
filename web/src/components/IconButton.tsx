import type { ButtonHTMLAttributes } from 'react';
import { Icon, type IconName } from './Icon';
import { Tooltip } from './Tooltip';

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  label: string;
  size?: number;
  active?: boolean;
  tone?: 'default' | 'danger';
  tooltipSide?: 'top' | 'bottom';
}

/** 底栏/标题栏图标按钮：28×28，hover 仅换底色（无位移） */
export function IconButton({
  icon,
  label,
  size = 18,
  active,
  tone = 'default',
  className,
  tooltipSide = 'top',
  ...rest
}: IconButtonProps) {
  return (
    <Tooltip label={label} side={tooltipSide}>
      <button
        type="button"
        aria-label={label}
        aria-pressed={active}
        className={`grid h-7 w-7 place-items-center rounded-tile transition-colors duration-[var(--dur-fast)] ${
          active
            ? 'bg-accent-weak text-accent'
            : tone === 'danger'
              ? 'text-fg2 hover:bg-danger-weak hover:text-danger'
              : 'text-fg2 hover:bg-hover hover:text-fg active:bg-pressed'
        } ${className ?? ''}`}
        {...rest}
      >
        <Icon name={icon} size={size} />
      </button>
    </Tooltip>
  );
}
