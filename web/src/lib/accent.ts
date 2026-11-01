/**
 * 强调色（OpenSpec add-appearance-and-maintenance-settings D1）
 * · 8 个预设 + 自定义 #RRGGBB，持久化在设置；默认蓝色 #3B82F6
 * · 按相对亮度自动选择白/深色前景（--text-on-accent）
 * · hover/pressed/weak 由 tokens.css 基于 --accent 的 color-mix 派生
 */

export const DEFAULT_ACCENT = '#3B82F6';
const ACCENT_KEY = 'bilidown.accentColor';

export interface AccentPreset {
  id: string;
  color: string;
  label: string;
}

export const ACCENT_PRESETS: AccentPreset[] = [
  { id: 'pink', color: '#FB7299', label: 'B 站粉' },
  { id: 'blue', color: '#3B82F6', label: '蓝' },
  { id: 'purple', color: '#8B5CF6', label: '紫' },
  { id: 'green', color: '#10B981', label: '绿' },
  { id: 'orange', color: '#F59E0B', label: '橙' },
  { id: 'red', color: '#EF4444', label: '红' },
  { id: 'cyan', color: '#06B6D4', label: '青' },
  { id: 'gray', color: '#6B7280', label: '灰' },
];

/** 黄色等浅强调色用于对比度自测（非 swatch 预设） */
const EXTRA_COLORS: Record<string, string> = {
  yellow: '#FDE047',
  lime: '#A3E635',
  white: '#FFFFFF',
  black: '#111111',
};

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

export function isHexColor(value: unknown): value is string {
  return typeof value === 'string' && HEX_RE.test(value);
}

export function normalizeAccent(value?: string | null): string {
  if (isHexColor(value)) return value.toUpperCase();
  return DEFAULT_ACCENT;
}

/** WCAG 相对亮度 */
export function relativeLuminance(hex: string): number {
  const color = normalizeAccent(hex).slice(1);
  const channels = [0, 2, 4].map((offset) => parseInt(color.slice(offset, offset + 2), 16) / 255);
  const linear = channels.map((channel) =>
    channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

/** 浅色强调色自动用深色文字（阈值按设计 D1：亮度 > 0.55） */
export function accentForeground(hex: string): '#ffffff' | '#1d1d1f' {
  return relativeLuminance(hex) > 0.55 ? '#1d1d1f' : '#ffffff';
}

/** 强调色作为文字时的可读色（浅色过亮时压暗；暗色过暗时提亮） */
export function accentTextColor(hex: string, theme?: string): string {
  const color = normalizeAccent(hex);
  const luminance = relativeLuminance(color);
  const resolvedTheme = theme ?? document.documentElement.dataset.theme ?? 'light';
  if (resolvedTheme === 'dark') {
    return luminance < 0.12
      ? `color-mix(in srgb, ${color} 60%, #ffffff)`
      : color;
  }
  return luminance > 0.55
    ? `color-mix(in srgb, ${color} 55%, #000000)`
    : color;
}

export function applyAccent(hex?: string | null): string {
  const color = normalizeAccent(hex);
  const root = document.documentElement;
  root.style.setProperty('--accent', color);
  const foreground = accentForeground(color);
  root.style.setProperty('--text-on-accent', foreground);
  root.style.setProperty('--accent-text', accentTextColor(color));
  // 供 CSS 兜底：普通强调色白字、浅强调色深字
  root.dataset.accentFg = foreground === '#1d1d1f' ? 'dark' : 'light';
  return color;
}

/** 截图/QA 强制覆盖：?accent=blue | ?accent=%23FDE047（生产环境无该参数） */
export function readAccentOverride(): string | null {
  try {
    const param = new URLSearchParams(window.location.search).get('accent');
    if (!param) return null;
    const preset = ACCENT_PRESETS.find((item) => item.id === param);
    if (preset) return preset.color;
    const extra = EXTRA_COLORS[param];
    if (extra) return extra;
    const hexMatch = param.match(/^#?([0-9a-fA-F]{6})$/);
    if (hexMatch) return `#${hexMatch[1].toUpperCase()}`;
  } catch {
    /* 忽略 */
  }
  return null;
}

export function readAccentPref(): string {
  const override = readAccentOverride();
  if (override) return override;
  try {
    const stored = localStorage.getItem(ACCENT_KEY);
    if (isHexColor(stored)) return stored.toUpperCase();
  } catch {
    /* 隐私模式忽略 */
  }
  return DEFAULT_ACCENT;
}

export function writeAccentPref(hex: string): void {
  try {
    localStorage.setItem(ACCENT_KEY, normalizeAccent(hex));
  } catch {
    /* 忽略 */
  }
}
