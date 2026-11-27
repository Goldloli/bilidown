import { motion } from 'motion/react';
import type { CSSProperties, ReactNode, RefObject } from 'react';

interface PopoverMotionPanelProps {
  panelRef: RefObject<HTMLDivElement | null>;
  role: 'menu' | 'listbox' | 'dialog';
  ariaLabel?: string;
  className: string;
  style: CSSProperties;
  children: ReactNode;
}

export function PopoverMotionPanel({
  panelRef,
  role,
  ariaLabel,
  className,
  style,
  children,
}: PopoverMotionPanelProps) {
  return (
    <motion.div
      ref={panelRef}
      role={role}
      aria-label={ariaLabel}
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.12, ease: [0.22, 1, 0.36, 1] }}
      style={style}
      className={className}
    >
      {children}
    </motion.div>
  );
}
