import { useRef, useState } from 'react';
import { Icon, type IconName } from './Icon';
import { Popover } from './Popover';

export interface MenuOption<T extends string = string> {
  value: T;
  label: string;
  hint?: string;
  icon?: IconName;
  disabled?: boolean;
  danger?: boolean;
}

interface SelectMenuProps<T extends string> {
  value: T | null;
  options: MenuOption<T>[];
  onChange: (value: T) => void;
  placeholder?: string;
  width?: number;
  disabled?: boolean;
  className?: string;
  ariaLabel?: string;
}

/** 自绘下拉：灰底圆角 + 双箭头（对齐 macOS 偏好设置，非浏览器 select） */
export function SelectMenu<T extends string>({
  value,
  options,
  onChange,
  placeholder = '请选择',
  width = 190,
  disabled,
  className,
  ariaLabel,
}: SelectMenuProps<T>) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLButtonElement>(null);
  const selected = options.find((o) => o.value === value);
  return (
    <>
      <button
        ref={anchorRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className={`inline-flex h-7 items-center justify-between gap-2 rounded-ctrl border border-transparent bg-[var(--segment-bg)] px-2.5 text-xs text-fg transition-colors duration-[var(--dur-fast)] hover:brightness-[0.98] disabled:opacity-40 ${className ?? ''}`}
        style={{ minWidth: width }}
      >
        <span className={`truncate ${selected ? '' : 'text-fg3'}`}>{selected?.label ?? placeholder}</span>
        <Icon name="chevron-down" size={12} className="shrink-0 text-fg3" />
      </button>
      <Popover
        open={open}
        anchorRef={anchorRef}
        onClose={() => setOpen(false)}
        align="end"
        width={width}
        role="listbox"
        ariaLabel={ariaLabel}
      >
        {options.map((option) => {
          const active = option.value === value;
          return (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={active}
              disabled={option.disabled}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
              className={`flex w-full items-center gap-2 rounded-[6px] px-2 py-[5px] text-left text-xs transition-colors duration-[var(--dur-fast)] disabled:opacity-40 ${
                option.danger ? 'text-danger hover:bg-danger-weak' : 'text-fg hover:bg-hover'
              }`}
            >
              {option.icon && <Icon name={option.icon} size={14} className="text-fg2" />}
              <span className="flex-1 truncate">{option.label}</span>
              {option.hint && <span className="text-2xs text-fg3">{option.hint}</span>}
              {active && <Icon name="check" size={13} className="text-accent" />}
            </button>
          );
        })}
      </Popover>
    </>
  );
}
