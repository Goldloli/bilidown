import type { SVGProps } from 'react';

/** 自绘线性图标集（1.5px stroke，24 viewBox；不用第三方图标字体/资产） */
export type IconName =
  | 'plus'
  | 'minus'
  | 'close'
  | 'check'
  | 'check-circle'
  | 'chevron-down'
  | 'chevron-right'
  | 'chevron-left'
  | 'search'
  | 'clipboard'
  | 'star'
  | 'clock'
  | 'history'
  | 'help'
  | 'settings'
  | 'gear'
  | 'pause'
  | 'play'
  | 'download'
  | 'arrow-down'
  | 'arrow-left'
  | 'trash'
  | 'folder'
  | 'copy'
  | 'more'
  | 'external'
  | 'refresh'
  | 'user'
  | 'reveal'
  | 'warning'
  | 'info'
  | 'link'
  | 'bell'
  | 'monitor'
  | 'moon'
  | 'sun'
  | 'volume'
  | 'wifi'
  | 'lock'
  | 'file'
  | 'calendar'
  | 'edit'
  | 'loader'
  | 'film'
  | 'music'
  | 'book'
  | 'grid'
  | 'bangumi'
  | 'list'
  | 'filter'
  | 'qr'
  | 'diamond'
  | 'image'
  | 'inbox'
  | 'reset'
  | 'dots';

interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'name'> {
  name: IconName;
  size?: number;
  strokeWidth?: number;
}

const PATHS: Record<IconName, React.ReactNode> = {
  plus: <path d="M12 5v14M5 12h14" />,
  minus: <path d="M5 12h14" />,
  close: <path d="M6 6l12 12M18 6L6 18" />,
  check: <path d="M4.5 12.5l5 5 10-11" />,
  'check-circle': (
    <>
      <circle cx="12" cy="12" r="8.25" />
      <path d="M8.4 12.3l2.6 2.6 4.8-5.4" />
    </>
  ),
  'chevron-down': <path d="M6.5 9.5l5.5 5.5 5.5-5.5" />,
  'chevron-right': <path d="M9.5 6.5l5.5 5.5-5.5 5.5" />,
  'chevron-left': <path d="M14.5 6.5L9 12l5.5 5.5" />,
  search: (
    <>
      <circle cx="10.8" cy="10.8" r="6.3" />
      <path d="M15.5 15.5L20 20" />
    </>
  ),
  clipboard: (
    <>
      <rect x="6" y="4.8" width="12" height="15.7" rx="2" />
      <rect x="9" y="3.2" width="6" height="3.4" rx="1.2" />
    </>
  ),
  star: (
    <path d="M12 3.8l2.5 5.1 5.6.8-4 3.9 1 5.6-5.1-2.7-5.1 2.7 1-5.6-4-3.9 5.6-.8z" />
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.4" />
      <path d="M12 7.2v5.1l3.5 2" />
    </>
  ),
  history: (
    <>
      <path d="M4.2 12a7.8 7.8 0 1 0 2.3-5.5" />
      <path d="M4 4.5V9h4.5" />
      <path d="M12 8v4.5l3 1.8" />
    </>
  ),
  help: (
    <>
      <circle cx="12" cy="12" r="8.4" />
      <path d="M9.6 9.2a2.5 2.5 0 1 1 4.2 1.8c-.9.7-1.8 1.2-1.8 2.6" />
      <path d="M12 17.2h.01" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3.1" />
      <path d="M12 3.4v2.2M12 18.4v2.2M3.4 12h2.2M18.4 12h2.2M5.9 5.9l1.6 1.6M16.5 16.5l1.6 1.6M18.1 5.9l-1.6 1.6M7.5 16.5l-1.6 1.6" />
    </>
  ),
  gear: (
    <>
      <path d="M9.95 5.31l.61-2.4h2.88l.61 2.4 1.24.51 2.12-1.26 2.03 2.03-1.26 2.12.51 1.24 2.4.61v2.88l-2.4.61-.51 1.24 1.26 2.12-2.03 2.03-2.12-1.26-1.24.51-.61 2.4h-2.88l-.61-2.4-1.24-.51-2.12 1.26-2.03-2.03 1.26-2.12-.51-1.24-2.4-.61v-2.88l2.4-.61.51-1.24-1.26-2.12 2.03-2.03 2.12 1.26z" />
      <circle cx="12" cy="12" r="3.1" />
    </>
  ),
  pause: <path d="M8.6 5.4v13.2M15.4 5.4v13.2" />,
  play: <path d="M8.2 5.5L18.4 12 8.2 18.5z" />,
  download: (
    <>
      <path d="M12 4v10.6" />
      <path d="M7.6 10.6L12 15l4.4-4.4" />
      <path d="M4.8 19.4h14.4" />
    </>
  ),
  'arrow-down': <path d="M12 4v15M6.2 13.2L12 19l5.8-5.8" />,
  'arrow-left': <path d="M19 12H5M10.8 6.2L5 12l5.8 5.8" />,
  trash: (
    <>
      <path d="M4.5 7h15M9.2 7V4.8h5.6V7" />
      <path d="M6.6 7l1 12.7h8.8l1-12.7" />
      <path d="M10.2 10.8v5.6M13.8 10.8v5.6" />
    </>
  ),
  folder: (
    <path d="M3.5 7.6a2 2 0 0 1 2-2h3.9l1.9 2.4h6.2a2 2 0 0 1 2 2v7.4a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2z" />
  ),
  copy: (
    <>
      <rect x="8.8" y="8.8" width="11" height="11" rx="2" />
      <path d="M5.6 15.2H4.7a1.2 1.2 0 0 1-1.2-1.2V4.7a1.2 1.2 0 0 1 1.2-1.2h9.3a1.2 1.2 0 0 1 1.2 1.2v.9" />
    </>
  ),
  more: (
    <>
      <circle cx="12" cy="5.6" r="1.35" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.35" fill="currentColor" stroke="none" />
      <circle cx="12" cy="18.4" r="1.35" fill="currentColor" stroke="none" />
    </>
  ),
  external: (
    <>
      <path d="M13.6 4.4h6v6" />
      <path d="M19.6 4.4l-8.4 8.4" />
      <path d="M18 13.6v4.6a1.8 1.8 0 0 1-1.8 1.8H5.8A1.8 1.8 0 0 1 4 18.2V7.8A1.8 1.8 0 0 1 5.8 6h4.6" />
    </>
  ),
  refresh: (
    <>
      <path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3" />
      <path d="M19.8 4.3v5h-5" />
    </>
  ),
  user: (
    <>
      <circle cx="12" cy="8.3" r="3.6" />
      <path d="M4.8 20.2c1.1-3.7 3.9-5.6 7.2-5.6s6.1 1.9 7.2 5.6" />
    </>
  ),
  reveal: (
    <>
      <circle cx="10.6" cy="10.6" r="6.1" />
      <path d="M15.2 15.2L20 20" />
      <path d="M8.2 10.6h4.8M10.6 8.2v4.8" />
    </>
  ),
  warning: (
    <>
      <path d="M12 3.9L21 20H3z" />
      <path d="M12 9.6v4.7" />
      <path d="M12 17.2h.01" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.4" />
      <path d="M12 11v5.2M12 7.9h.01" />
    </>
  ),
  link: (
    <>
      <path d="M9.6 14.4l4.8-4.8" />
      <path d="M11 7.2l1.7-1.7a4 4 0 0 1 5.7 5.7L16.7 13" />
      <path d="M13 16.8l-1.7 1.7a4 4 0 0 1-5.7-5.7L7.3 11" />
    </>
  ),
  bell: (
    <>
      <path d="M6.3 9.4a5.7 5.7 0 0 1 11.4 0c0 3.9 1.4 5.3 1.4 5.3H4.9s1.4-1.4 1.4-5.3z" />
      <path d="M10.1 18.6a2 2 0 0 0 3.8 0" />
    </>
  ),
  monitor: (
    <>
      <rect x="3.2" y="4.4" width="17.6" height="12.2" rx="2" />
      <path d="M9 20h6M12 16.6V20" />
    </>
  ),
  moon: <path d="M20 14.6A8.6 8.6 0 1 1 9.4 4a7 7 0 0 0 10.6 10.6z" />,
  sun: (
    <>
      <circle cx="12" cy="12" r="3.9" />
      <path d="M12 3.4v1.8M12 18.8v1.8M3.4 12h1.8M18.8 12h1.8M6 6l1.3 1.3M16.7 16.7L18 18M18 6l-1.3 1.3M7.3 16.7L6 18" />
    </>
  ),
  volume: (
    <>
      <path d="M4.4 9.4h3.4l4.4-3.5v12.2l-4.4-3.5H4.4z" />
      <path d="M15.8 9.2a3.9 3.9 0 0 1 0 5.6M18.2 7a7.2 7.2 0 0 1 0 10" />
    </>
  ),
  wifi: (
    <>
      <path d="M4.4 9.6a11.4 11.4 0 0 1 15.2 0" />
      <path d="M7.2 12.7a7.4 7.4 0 0 1 9.6 0" />
      <path d="M10 15.7a3.4 3.4 0 0 1 4 0" />
      <path d="M12 19h.01" />
    </>
  ),
  lock: (
    <>
      <rect x="5" y="10.4" width="14" height="9.8" rx="2" />
      <path d="M8.4 10.4V7.9a3.6 3.6 0 0 1 7.2 0v2.5" />
      <path d="M12 14v2.8" />
    </>
  ),
  file: (
    <>
      <path d="M6 3.6h8l4 4v12.8H6z" />
      <path d="M14 3.6v4h4M9 12h6M9 15.2h6" />
    </>
  ),
  calendar: (
    <>
      <rect x="3.6" y="5" width="16.8" height="15.2" rx="2" />
      <path d="M3.6 9.4h16.8M8.2 3v4M15.8 3v4" />
    </>
  ),
  edit: (
    <>
      <path d="M4 20h4.2L19.4 8.8l-4.2-4.2L4 15.8z" />
      <path d="M13.6 6.2l4.2 4.2" />
    </>
  ),
  loader: <path d="M12 3.6a8.4 8.4 0 1 1-8.4 8.4" />,
  film: (
    <>
      <rect x="3.2" y="4.6" width="17.6" height="14.8" rx="2" />
      <path d="M8 4.6v14.8M16 4.6v14.8" />
      <path d="M10.8 9.5l3.6 2.5-3.6 2.5z" />
    </>
  ),
  music: (
    <>
      <path d="M9.2 18V6.2l9.2-1.8v11.4" />
      <circle cx="6.6" cy="18" r="2.5" />
      <circle cx="15.8" cy="15.8" r="2.5" />
    </>
  ),
  book: (
    <>
      <path d="M4.2 6.1A2.9 2.9 0 0 1 7.1 3.2H20v17.6H7.1a2.9 2.9 0 0 1-2.9-2.9z" />
      <path d="M8 3.2v17.6" />
    </>
  ),
  grid: (
    <>
      <rect x="4" y="4" width="6.4" height="6.4" rx="1.4" />
      <rect x="13.6" y="4" width="6.4" height="6.4" rx="1.4" />
      <rect x="4" y="13.6" width="6.4" height="6.4" rx="1.4" />
      <rect x="13.6" y="13.6" width="6.4" height="6.4" rx="1.4" />
    </>
  ),
  bangumi: (
    <>
      <rect x="3.2" y="6.4" width="17.6" height="12" rx="2" />
      <path d="M8.2 3l3.8 3.4L15.8 3" />
      <path d="M10.8 10.2l3.6 2.3-3.6 2.3z" />
    </>
  ),
  list: <path d="M4.6 6.6h14.8M4.6 12h14.8M4.6 17.4h14.8" />,
  filter: <path d="M4 5.2h16l-6.2 7.2v5.9L10.2 20v-7.6z" />,
  qr: (
    <>
      <rect x="3.6" y="3.6" width="7" height="7" rx="1" />
      <rect x="13.4" y="3.6" width="7" height="7" rx="1" />
      <rect x="3.6" y="13.4" width="7" height="7" rx="1" />
      <path d="M13.4 13.4h3v3h-3zM18.4 17h2M17 20.4h3" />
    </>
  ),
  diamond: <path d="M12 3.4l4.6 6.1L12 20.6 7.4 9.5z" />,
  image: (
    <>
      <rect x="3.6" y="4.6" width="16.8" height="14.8" rx="2" />
      <circle cx="8.8" cy="9.4" r="1.5" />
      <path d="M4.2 17.2l4.6-4.6 3.4 3.2 3.2-3.6 4.8 5.4" />
    </>
  ),
  inbox: (
    <>
      <path d="M4 5.2h16l1.4 8.8v5.2H2.6V14z" />
      <path d="M2.6 14h5.2l1.2 2.4h6l1.2-2.4h5.2" />
    </>
  ),
  reset: (
    <>
      <path d="M4.5 12a7.5 7.5 0 1 0 7.5-7.5c-2.5 0-4.6 1.2-5.9 3.1" />
      <path d="M5.6 3.8v4.4h4.4" />
    </>
  ),
  dots: (
    <>
      <circle cx="5.5" cy="12" r="1.35" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.35" fill="currentColor" stroke="none" />
      <circle cx="18.5" cy="12" r="1.35" fill="currentColor" stroke="none" />
    </>
  ),
};

export function Icon({ name, size = 18, strokeWidth = 1.5, className, ...rest }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
      {...rest}
    >
      {PATHS[name]}
    </svg>
  );
}
