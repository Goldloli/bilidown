import type { Appearance } from './types';

const THEME_OVERRIDE_KEY = 'bilidown.themeOverride';

export type ResolvedTheme = 'light' | 'dark';

export function readThemeOverride(): ResolvedTheme | null {
  try {
    const params = new URLSearchParams(window.location.search);
    const t = params.get('theme');
    if (t === 'light' || t === 'dark') {
      sessionStorage.setItem(THEME_OVERRIDE_KEY, t);
      return t;
    }
    const stored = sessionStorage.getItem(THEME_OVERRIDE_KEY);
    if (stored === 'light' || stored === 'dark') return stored;
  } catch {
    /* 忽略 */
  }
  return null;
}

export function systemTheme(): ResolvedTheme {
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function resolveTheme(appearance: Appearance): ResolvedTheme {
  const override = readThemeOverride();
  if (override) return override;
  if (appearance === 'light' || appearance === 'dark') return appearance;
  return systemTheme();
}

export function applyTheme(theme: ResolvedTheme): void {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

export function applyReducedMotion(reduced: boolean): void {
  if (reduced) document.documentElement.dataset.motion = 'reduced';
  else delete document.documentElement.dataset.motion;
}

export function watchSystemTheme(onChange: (theme: ResolvedTheme) => void): () => void {
  const mq = window.matchMedia?.('(prefers-color-scheme: dark)');
  if (!mq) return () => {};
  const handler = (e: MediaQueryListEvent) => onChange(e.matches ? 'dark' : 'light');
  mq.addEventListener('change', handler);
  return () => mq.removeEventListener('change', handler);
}
