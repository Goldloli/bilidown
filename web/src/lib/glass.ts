/**
 * 磨砂玻璃背景（原生 vibrancy 透出）——纯前端 UI 偏好：
 * · 持久化在 core 设置（`windowGlass`）；localStorage 仅作为启动时/旧版本兜底
 * · 仅在真实 Tauri 窗口生效；浏览器 mock/截图环境自动回退不透明，避免误导视觉验收
 */

import { isGlassSupported } from './platform';

const GLASS_KEY = 'bilidown.glassBackground';

export function isTauriEnv(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

export function readGlassPref(): boolean {
  try {
    const raw = localStorage.getItem(GLASS_KEY);
    return raw === null ? true : raw === '1';
  } catch {
    return false;
  }
}

export function writeGlassPref(enabled: boolean): void {
  try {
    localStorage.setItem(GLASS_KEY, enabled ? '1' : '0');
  } catch {
    /* 隐私模式/无 localStorage 时忽略 */
  }
}

/** 应用/回退材质：Tauri 且系统支持原生材质时 data-glass=on；否则始终 off */
export function applyGlass(enabled: boolean): void {
  const active = enabled && isTauriEnv() && isGlassSupported();
  document.documentElement.dataset.glass = active ? 'on' : 'off';
}
