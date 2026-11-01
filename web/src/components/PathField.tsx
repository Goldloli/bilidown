import { api } from '../lib/ipc';
import { isPending, pendingLabel } from '../lib/capabilities';
import { Icon } from './Icon';

interface PathFieldProps {
  value: string;
  onChange: (value: string) => void;
  onOpen?: () => void;
  invalid?: boolean;
  width?: number | string;
}

/** 目录选择：路径 + 选择… + 打开（自绘，替换原生 file input） */
export function PathField({ value, onChange, onOpen, invalid, width = 260 }: PathFieldProps) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        className={`inline-flex h-7 items-center gap-1.5 rounded-ctrl border px-2 ${
          invalid ? 'border-danger' : 'border-field-border'
        }`}
        style={{ width, background: 'var(--field-bg)' }}
      >
        <Icon name="folder" size={13} className="shrink-0 text-fg3" />
        <span className="truncate text-xs text-fg2" title={value}>
          {value || '未选择'}
        </span>
      </span>
      <button
        type="button"
        onClick={() => {
          void api.chooseDirectory(value).then((next) => {
            if (next) onChange(next);
          });
        }}
        className="h-7 rounded-ctrl border border-line-strong px-2 text-2xs text-fg hover:bg-hover"
      >
        选择…
      </button>
      <button
        type="button"
        aria-label={pendingLabel('reveal', '打开目录')}
        title={pendingLabel('reveal', '打开目录')}
        disabled={isPending('reveal')}
        onClick={() => {
          if (isPending('reveal')) return;
          if (onOpen) onOpen();
          else void api.openInFinder(value);
        }}
        className="grid h-7 w-7 place-items-center rounded-ctrl text-fg2 hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:opacity-35"
      >
        <Icon name="external" size={14} />
      </button>
    </span>
  );
}
