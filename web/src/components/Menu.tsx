import { useEffect, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

export interface MenuItem {
  type?: 'item' | 'separator';
  label?: string;
  icon?: ReactNode;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
}

interface MenuPanelProps {
  items: MenuItem[];
  onClose: () => void;
  x?: number;
  y?: number;
  width?: number;
  ariaLabel?: string;
}

/** 右键菜单/下拉菜单内容：28 高行、图标 + 文案 + 快捷键 */
export function MenuPanel({ items, onClose, x, y, width = 224, ariaLabel = '菜单' }: MenuPanelProps) {
  const ref = useRef<HTMLDivElement>(null);
  const position = useRef<{ left: number; top: number } | null>(null);

  useEffect(() => {
    if (x == null || y == null) return;
    const panel = ref.current;
    if (!panel) return;
    const w = panel.offsetWidth;
    const h = panel.offsetHeight;
    position.current = {
      left: Math.min(x, window.innerWidth - w - 10),
      top: Math.min(y, window.innerHeight - h - 10),
    };
    panel.style.left = `${position.current.left}px`;
    panel.style.top = `${position.current.top}px`;
  }, [x, y]);

  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
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
  }, [onClose]);

  const panel = (
    <div
      ref={ref}
      role="menu"
      aria-label={ariaLabel}
      className="glass-surface rounded-pop border border-line bg-panel p-1 shadow-[var(--shadow-pop)]"
      style={
        x == null
          ? { width }
          : { width, position: 'fixed', left: x, top: y, zIndex: 85 }
      }
    >
      {items.map((item, index) =>
        item.type === 'separator' ? (
          <div key={`sep-${index}`} className="my-1 h-px bg-line" role="separator" />
        ) : (
          <button
            key={item.label ?? index}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onClick={() => {
              item.onSelect?.();
              onClose();
            }}
            className={`flex w-full items-center gap-2 rounded-[6px] px-2 py-[5px] text-left text-xs transition-colors duration-[var(--dur-fast)] disabled:opacity-35 ${
              item.danger
                ? 'text-danger hover:bg-danger-weak'
                : 'text-fg hover:bg-hover active:bg-pressed'
            }`}
          >
            <span className="grid w-4 shrink-0 place-items-center text-fg2">{item.icon}</span>
            <span className="flex-1 truncate">{item.label}</span>
            {item.shortcut && <span className="shrink-0 text-2xs text-fg3">{item.shortcut}</span>}
          </button>
        ),
      )}
    </div>
  );

  if (x == null) return panel;
  return createPortal(panel, document.body);
}
