import type { ReactNode, Ref } from 'react';

interface TextFieldProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  invalid?: boolean;
  disabled?: boolean;
  width?: number | string;
  type?: 'text' | 'password';
  ariaLabel?: string;
  trailing?: ReactNode;
  mono?: boolean;
  inputRef?: Ref<HTMLInputElement>;
}

export function TextField({
  value,
  onChange,
  placeholder,
  invalid,
  disabled,
  width = 190,
  type = 'text',
  ariaLabel,
  trailing,
  mono,
  inputRef,
}: TextFieldProps) {
  return (
    <span
      className={`inline-flex h-7 items-center gap-1.5 rounded-ctrl border px-2 ${
        invalid ? 'border-danger' : 'border-field-border focus-within:border-accent'
      } ${disabled ? 'opacity-40' : ''}`}
      style={{ width, background: 'var(--field-bg)' }}
    >
      <input
        ref={inputRef}
        type={type}
        value={value}
        aria-label={ariaLabel}
        aria-invalid={invalid}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        className={`min-w-0 flex-1 bg-transparent text-xs text-fg outline-none placeholder:text-fg3 ${
          mono ? 'font-mono text-2xs' : ''
        }`}
      />
      {trailing}
    </span>
  );
}
