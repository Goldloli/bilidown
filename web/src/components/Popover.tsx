import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { motion } from 'motion/react';

interface PopoverProps {
  open: boolean;
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  children: ReactNode;
  align?: 'start' | 'end' | 'center';
  side?: 'bottom' | 'top';
  width?: number;
  className?: string;
  role?: 'menu' | 'listbox' | 'dialog';
  ariaLabel?: string;
}

/** 锚定浮层：portal + 视口内定位 + 外点/Esc 关闭（无箭头，shadow-pop） */
export function Popover({
  open,
  anchorRef,
  onClose,
  children,
  align = 'start',
  side = 'bottom',
  width,
  className,
  role = 'menu',
  ariaLabel,
}: PopoverProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const update = () => {
      const anchor = anchorRef.current;
      const panel = panelRef.current;
      if (!anchor || !panel) return;
      const rect = anchor.getBoundingClientRect();
      const pw = panel.offsetWidth;
      const ph = panel.offsetHeight;
      let left =
        align === 'end' ? rect.right - pw : align === 'center' ? rect.left + rect.width / 2 - pw / 2 : rect.left;
      left = Math.max(8, Math.min(window.innerWidth - pw - 8, left));
      let top = side === 'top' ? rect.top - ph - 6 : rect.bottom + 6;
      if (top + ph > window.innerHeight - 8) top = Math.max(8, rect.top - ph - 6);
      if (top < 8) top = 8;
      setPos({ top, left });
    };
    update();
    const raf = requestAnimationFrame(update);
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [open, anchorRef, align, side]);

  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (panelRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
      }
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open, onClose, anchorRef]);

  if (!open) return null;

  return createPortal(
    <motion.div
      ref={panelRef}
      role={role}
      aria-label={ariaLabel}
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.12, ease: [0.22, 1, 0.36, 1] }}
      style={{
        position: 'fixed',
        top: pos?.top ?? -9999,
        left: pos?.left ?? -9999,
        width,
        transformOrigin: side === 'top' ? 'bottom' : 'top',
      }}
      className={`glass-surface z-[70] rounded-pop border border-line bg-panel p-1 text-fg shadow-[var(--shadow-pop)] ${className ?? ''}`}
    >
      {children}
    </motion.div>,
    document.body,
  );
}
