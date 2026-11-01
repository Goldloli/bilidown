import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  applyCapabilities,
  applyPlatform,
  glassUnavailableReason,
  isGlassSupported,
  shouldShowGlassSettings,
} from '../src/lib/platform';
import { applyGlass, readGlassPref, writeGlassPref } from '../src/lib/glass';
import { defaultInternalSettings, mergeSettings } from '../src/lib/adapt';

describe('native glass capability and preference', () => {
  let dataset: Record<string, string>;

  beforeEach(() => {
    dataset = {};
    const storage = new Map<string, string>();
    vi.stubGlobal('document', { documentElement: { dataset } });
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
  });

  afterEach(() => vi.unstubAllGlobals());

  it('stays opaque before native capabilities are confirmed', () => {
    expect(readGlassPref()).toBe(true);
    expect(isGlassSupported()).toBe(false);
    applyGlass(true);
    expect(dataset.glass).toBe('off');
  });

  it('falls back without overwriting the saved preference', () => {
    writeGlassPref(true);
    applyCapabilities({ glassSupported: false, glassUnavailableReason: '窗口模式不支持', notificationsSupported: false });
    applyGlass(readGlassPref());
    expect(dataset.glass).toBe('off');
    expect(glassUnavailableReason()).toBe('窗口模式不支持');
    expect(readGlassPref()).toBe(true);
  });

  it('can switch off and back on without losing capability', () => {
    applyCapabilities({ glassSupported: true, notificationsSupported: true });
    applyGlass(true);
    expect(dataset.glass).toBe('on');
    applyGlass(false);
    expect(dataset.glass).toBe('off');
    expect(isGlassSupported()).toBe(true);
    applyGlass(true);
    expect(dataset.glass).toBe('on');
  });

  it('keeps browser previews opaque even with a supported mock capability', () => {
    vi.stubGlobal('window', {});
    applyCapabilities({ glassSupported: true, notificationsSupported: true });
    applyGlass(true);
    expect(dataset.glass).toBe('off');
  });

  it('hides the glass row on Windows and keeps it capability-driven on macOS', () => {
    applyPlatform('windows');
    expect(shouldShowGlassSettings()).toBe(false);

    applyPlatform('macos');
    applyCapabilities({
      glassSupported: false,
      glassUnavailableReason: '当前 macOS 窗口模式不支持',
      notificationsSupported: true,
    });
    expect(shouldShowGlassSettings()).toBe(true);
    expect(isGlassSupported()).toBe(false);
    expect(glassUnavailableReason()).toBe('当前 macOS 窗口模式不支持');
  });

  it('does not overwrite a saved glass preference when another Windows setting changes', () => {
    const settings = defaultInternalSettings();
    settings.interface.glassBackground = true;
    const next = mergeSettings(settings, { interface: { appearance: 'dark' } });
    expect(next.interface.appearance).toBe('dark');
    expect(next.interface.glassBackground).toBe(true);
  });
});
