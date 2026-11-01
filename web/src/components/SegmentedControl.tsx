import { Icon, type IconName } from './Icon';

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: IconName;
  disabled?: boolean;
}

interface SegmentedControlProps<T extends string> {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  size?: 'sm' | 'md';
  className?: string;
  ariaLabel?: string;
}

/** macOS 分段控件（自绘：轨道 + 白色选中胶囊，无浏览器默认外观） */
export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
  size = 'md',
  className,
  ariaLabel,
}: SegmentedControlProps<T>) {
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={`inline-flex shrink-0 items-center rounded-ctrl p-[2px] ${className ?? ''}`}
      style={{ background: 'var(--segment-bg)' }}
    >
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={active}
            disabled={option.disabled}
            onClick={() => onChange(option.value)}
            className={`inline-flex items-center gap-1 rounded-[5px] font-medium transition-colors duration-[var(--dur-fast)] disabled:opacity-40 ${
              size === 'sm' ? 'h-6 px-2 text-2xs' : 'h-[24px] px-2.5 text-xs'
            } ${active ? 'text-fg' : 'text-fg2 hover:text-fg'}`}
            style={
              active
                ? {
                    background: 'var(--segment-selected)',
                    boxShadow: '0 1px 2px rgba(0,0,0,0.12), 0 0 0 0.5px rgba(0,0,0,0.08)',
                  }
                : undefined
            }
          >
            {option.icon && <Icon name={option.icon} size={13} />}
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
