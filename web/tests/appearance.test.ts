import { describe, expect, it } from 'vitest';
import {
  DEFAULT_ACCENT,
  accentForeground,
  isHexColor,
  normalizeAccent,
  relativeLuminance,
} from '../src/lib/accent';
import { isMotionPreference, normalizeMotion } from '../src/lib/motion';
import { LOW_SPACE_THRESHOLD, isLowSpace } from '../src/lib/storage';

describe('accent', () => {
  it('normalizes valid hex and falls back to default', () => {
    expect(normalizeAccent('#3b82f6')).toBe('#3B82F6');
    expect(normalizeAccent('#FB7299')).toBe('#FB7299');
    expect(normalizeAccent('nope')).toBe(DEFAULT_ACCENT);
    expect(normalizeAccent(undefined)).toBe(DEFAULT_ACCENT);
    expect(isHexColor('#123abc')).toBe(true);
    expect(isHexColor('#123ab')).toBe(false);
  });

  it('computes luminance and picks readable foreground', () => {
    expect(relativeLuminance('#FFFFFF')).toBeGreaterThan(0.99);
    expect(relativeLuminance('#000000')).toBeLessThan(0.01);
    expect(accentForeground('#FDE047')).toBe('#1d1d1f'); // 亮黄 → 深字
    expect(accentForeground('#3B82F6')).toBe('#ffffff'); // 蓝 → 白字
  });
});

describe('motion preference', () => {
  it('accepts system/reduce/normal and defaults to system', () => {
    expect(normalizeMotion('reduce')).toBe('reduce');
    expect(normalizeMotion('normal')).toBe('normal');
    expect(normalizeMotion('system')).toBe('system');
    expect(normalizeMotion('bogus')).toBe('system');
    expect(isMotionPreference('reduce')).toBe(true);
    expect(isMotionPreference('reduced')).toBe(false);
  });
});

describe('storage low space', () => {
  it('warns below 2GB only', () => {
    const base = {
      dataDir: '/tmp/data',
      downloadDir: '/tmp/dl',
      dataBytes: 1,
      coverBytes: 1,
      tempBytes: 1,
      logBytes: 1,
    };
    expect(isLowSpace({ ...base, downloadFreeBytes: LOW_SPACE_THRESHOLD - 1 })).toBe(true);
    expect(isLowSpace({ ...base, downloadFreeBytes: LOW_SPACE_THRESHOLD })).toBe(false);
    expect(isLowSpace({ ...base, downloadFreeBytes: null })).toBe(false);
    expect(isLowSpace(null)).toBe(false);
  });
});
