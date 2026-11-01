import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { Icon, type IconName } from './Icon';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'quiet';
type Size = 'sm' | 'md' | 'lg';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: IconName;
  trailingIcon?: IconName;
  loading?: boolean;
  children?: ReactNode;
}

const VARIANTS: Record<Variant, string> = {
  primary:
    'bg-accent text-on-accent hover:bg-accent-hover active:bg-accent-pressed disabled:bg-accent/40',
  secondary:
    'bg-field text-fg border border-line-strong hover:bg-hover active:bg-pressed',
  ghost: 'text-fg2 hover:bg-hover active:bg-pressed',
  danger: 'bg-danger text-on-accent hover:opacity-90 active:opacity-100',
  quiet: 'text-fg hover:bg-hover active:bg-pressed',
};

const SIZES: Record<Size, string> = {
  sm: 'h-6 px-2 text-2xs gap-1 rounded-[6px]',
  md: 'h-7 px-3 text-xs gap-1.5 rounded-ctrl',
  lg: 'h-8 px-4 text-sm gap-2 rounded-ctrl',
};

const ICON_SIZE: Record<Size, number> = { sm: 13, md: 14, lg: 15 };

export function Button({
  variant = 'secondary',
  size = 'md',
  icon,
  trailingIcon,
  loading,
  children,
  className,
  disabled,
  ...rest
}: ButtonProps) {
  return (
    <button
      type="button"
      disabled={disabled || loading}
      className={`inline-flex select-none items-center justify-center font-medium transition-colors duration-[var(--dur-fast)] disabled:cursor-default disabled:opacity-40 ${VARIANTS[variant]} ${SIZES[size]} ${className ?? ''}`}
      {...rest}
    >
      {loading ? (
        <Icon name="loader" size={ICON_SIZE[size]} className="spinner" />
      ) : (
        icon && <Icon name={icon} size={ICON_SIZE[size]} />
      )}
      {children}
      {trailingIcon && <Icon name={trailingIcon} size={ICON_SIZE[size]} />}
    </button>
  );
}
