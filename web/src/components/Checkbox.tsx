import { Icon } from './Icon';

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  disabled?: boolean;
  indeterminate?: boolean;
}

/** 自绘复选框（16×16，粉色选中，无浏览器默认外观） */
export function Checkbox({ checked, onChange, label, disabled, indeterminate }: CheckboxProps) {
  const active = checked || indeterminate;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={indeterminate ? 'mixed' : checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`inline-grid h-4 w-4 shrink-0 place-items-center rounded-[4px] border transition-colors duration-[var(--dur-fast)] disabled:opacity-35 ${
        active ? 'border-accent bg-accent text-on-accent' : 'border-line-strong bg-field text-transparent hover:border-fg3'
      }`}
    >
      <Icon name={indeterminate ? 'minus' : 'check'} size={11} strokeWidth={2.2} />
    </button>
  );
}
