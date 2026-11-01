import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';

interface TooltipProps {
  label: string;
  children: ReactNode;
  side?: 'top' | 'bottom';
  delay?: number;
  disabled?: boolean;
}

/** 轻量 tooltip：200ms 延迟、`--tooltip-bg` 反色底、无箭头 */
export function Tooltip({ label, children, side = 'top', delay = 200, disabled }: TooltipProps) {
  const [open, setOpen] = useState(false);
  const timer = useRef<number | null>(null);
  const show = useCallback(() => {
    if (disabled) return;
    timer.current = window.setTimeout(() => setOpen(true), delay);
  }, [delay, disabled]);
  const hide = useCallback(() => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = null;
    setOpen(false);
  }, []);
  useEffect(() => () => hide(), [hide]);
  return (
    <span
      className="relative inline-flex"
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
    >
      {children}
      {open && (
        <span
          role="tooltip"
          className={`pointer-events-none absolute left-1/2 z-[80] -translate-x-1/2 whitespace-nowrap rounded-md px-2 py-1 text-2xs leading-[15px] shadow-[var(--shadow-pop)] ${
            side === 'top' ? 'bottom-full mb-1.5' : 'top-full mt-1.5'
          }`}
          style={{ background: 'var(--tooltip-bg)', color: 'var(--tooltip-fg)' }}
        >
          {label}
        </span>
      )}
    </span>
  );
}
