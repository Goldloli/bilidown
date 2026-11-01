/** 动态效果偏好（OpenSpec appearance D2）：system 默认 / reduce / normal */

export type MotionPreference = 'system' | 'reduce' | 'normal';

const MOTION_KEY = 'bilidown.motion';

export const MOTION_OPTIONS: Array<{ value: MotionPreference; label: string }> = [
  { value: 'system', label: '跟随系统' },
  { value: 'reduce', label: '减弱' },
  { value: 'normal', label: '正常' },
];

export function isMotionPreference(value: unknown): value is MotionPreference {
  return value === 'system' || value === 'reduce' || value === 'normal';
}

export function normalizeMotion(value?: string | null): MotionPreference {
  return isMotionPreference(value) ? value : 'system';
}

export function systemPrefersReducedMotion(): boolean {
  return Boolean(window.matchMedia?.('(prefers-reduced-motion: reduce)').matches);
}

export function motionIsReduced(preference: MotionPreference): boolean {
  if (preference === 'reduce') return true;
  if (preference === 'normal') return false;
  return systemPrefersReducedMotion();
}

/** data-motion 存实际生效值（reduce/normal），供 motion.css 命中 */
export function applyMotion(preference: MotionPreference): void {
  document.documentElement.dataset.motion = motionIsReduced(preference) ? 'reduce' : 'normal';
}

/** 截图/QA 强制覆盖：?motion=reduce|normal|system */
export function readMotionOverride(): MotionPreference | null {
  try {
    const param = new URLSearchParams(window.location.search).get('motion');
    if (isMotionPreference(param)) return param;
  } catch {
    /* 忽略 */
  }
  return null;
}

export function readMotionPref(): MotionPreference {
  const override = readMotionOverride();
  if (override) return override;
  try {
    const stored = localStorage.getItem(MOTION_KEY);
    if (isMotionPreference(stored)) return stored;
  } catch {
    /* 忽略 */
  }
  return 'system';
}

export function writeMotionPref(preference: MotionPreference): void {
  try {
    localStorage.setItem(MOTION_KEY, preference);
  } catch {
    /* 忽略 */
  }
}

/** system 档跟随系统切换（主窗口启动时订阅一次） */
export function watchSystemMotion(preference: MotionPreference, onChange: () => void): () => void {
  const mq = window.matchMedia?.('(prefers-reduced-motion: reduce)');
  if (!mq) return () => {};
  const handler = () => {
    if (preference === 'system') onChange();
  };
  mq.addEventListener('change', handler);
  return () => mq.removeEventListener('change', handler);
}
