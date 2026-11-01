import { useEffect, useState } from 'react';
import { Icon } from './Icon';

interface NumberFieldProps {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  invalid?: boolean;
  disabled?: boolean;
  width?: number;
  suffix?: string;
  ariaLabel?: string;
}

/** 数字输入：白底 1px 边框 + 右侧步进器（自绘，替换浏览器默认外观） */
export function NumberField({
  value,
  onChange,
  min = -Infinity,
  max = Infinity,
  step = 1,
  invalid,
  disabled,
  width = 72,
  suffix,
  ariaLabel,
}: NumberFieldProps) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => {
    setDraft(String(value));
  }, [value]);

  const commit = (raw: string) => {
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) {
      setDraft(String(value));
      return;
    }
    const clamped = Math.min(max, Math.max(min, parsed));
    setDraft(String(clamped));
    if (clamped !== value) onChange(clamped);
  };

  const bump = (delta: number) => {
    const next = Math.min(max, Math.max(min, value + delta));
    setDraft(String(next));
    onChange(next);
  };

  return (
    <span
      className={`inline-flex h-7 items-stretch overflow-hidden rounded-ctrl border bg-field ${
        invalid ? 'border-danger' : 'border-field-border'
      } ${disabled ? 'opacity-40' : ''}`}
      style={{ width }}
    >
      <input
        type="text"
        inputMode="numeric"
        aria-label={ariaLabel}
        aria-invalid={invalid}
        disabled={disabled}
        value={draft}
        onChange={(event) => setDraft(event.target.value.replace(/[^\d-]/g, ''))}
        onBlur={() => commit(draft)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.currentTarget.blur();
          }
        }}
        className="num min-w-0 flex-1 bg-transparent px-2 text-right text-xs text-fg outline-none"
      />
      {suffix && <span className="flex items-center pr-1 text-2xs text-fg3">{suffix}</span>}
      <span className="flex w-[18px] flex-col border-l border-line">
        <button
          type="button"
          tabIndex={-1}
          aria-label="增加"
          disabled={disabled || value >= max}
          onClick={() => bump(step)}
          className="grid flex-1 place-items-center text-fg3 hover:bg-hover hover:text-fg disabled:opacity-30"
        >
          <Icon name="chevron-down" size={9} style={{ transform: 'rotate(180deg)' }} />
        </button>
        <button
          type="button"
          tabIndex={-1}
          aria-label="减少"
          disabled={disabled || value <= min}
          onClick={() => bump(-step)}
          className="grid flex-1 place-items-center border-t border-line text-fg3 hover:bg-hover hover:text-fg disabled:opacity-30"
        >
          <Icon name="chevron-down" size={9} />
        </button>
      </span>
    </span>
  );
}
