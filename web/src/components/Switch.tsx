interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
}

/** macOS 风格开关（38×22，自绘） */
export function Switch({ checked, onChange, disabled, label }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full transition-colors duration-[var(--dur-base)] disabled:opacity-40 ${
        checked ? 'bg-accent' : 'bg-[var(--segment-bg)] ring-1 ring-inset ring-line-strong'
      }`}
    >
      <span
        className="absolute h-[18px] w-[18px] rounded-full bg-white shadow-[0_1px_2px_rgba(0,0,0,0.28)] transition-transform duration-[var(--dur-base)]"
        style={{ transform: checked ? 'translateX(18px)' : 'translateX(2px)' }}
      />
    </button>
  );
}
