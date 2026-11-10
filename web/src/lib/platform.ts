/**
 * 平台信息：由 bootstrap 从 Rust `get_app_info` 取一次写入 `<html data-os>`，
 * CSS 与少量组件据此做平台分支（不依赖 UA 嗅探）。
 */

export type Platform = 'windows' | 'macos' | 'linux' | 'other';

export function applyPlatform(os: string): void {
  const platform: Platform =
    os === 'windows' ? 'windows' : os === 'macos' ? 'macos' : os === 'linux' ? 'linux' : 'other';
  document.documentElement.dataset.os = platform;
}

export function currentPlatform(): Platform {
  // 非浏览器上下文（SSR/单测/构建脚本）没有平台信息，按 other 处理
  if (typeof document === 'undefined') return 'other';
  const value = document.documentElement.dataset.os;
  if (value === 'windows' || value === 'macos' || value === 'linux') return value;
  return 'other';
}

export function isWindows(): boolean {
  return currentPlatform() === 'windows';
}

export function isMacOS(): boolean {
  return currentPlatform() === 'macos';
}

/** macOS 与 Windows 共用底栏的常驻偏好设置入口。 */
export function showsSettingsShortcut(): boolean {
  return isMacOS() || isWindows();
}

/** Windows 不展示不可用的磨砂选项；其他平台继续由现有能力判断控制。 */
export function shouldShowGlassSettings(platform: Platform = currentPlatform()): boolean {
  return platform !== 'windows';
}

/** 由 bootstrap 写入的平台能力（来自 Rust `get_app_info`）。 */
export interface PlatformCapabilities {
  glassSupported: boolean;
  glassUnavailableReason?: string | null;
  notificationsSupported: boolean;
}

export function applyCapabilities(caps: PlatformCapabilities): void {
  const root = document.documentElement;
  root.dataset.glassSupported = caps.glassSupported ? '1' : '0';
  if (caps.glassUnavailableReason) root.dataset.glassUnavailableReason = caps.glassUnavailableReason;
  else delete root.dataset.glassUnavailableReason;
  root.dataset.notificationsSupported = caps.notificationsSupported ? '1' : '0';
}

/** 能力确认前保持不透明，避免首帧透出未经模糊的桌面内容。 */
export function isGlassSupported(): boolean {
  return document.documentElement.dataset.glassSupported === '1';
}

export function glassUnavailableReason(): string {
  return document.documentElement.dataset.glassUnavailableReason
    ?? '当前窗口模式不支持原生背景模糊，已使用不透明背景。';
}

export function isNotificationsSupported(): boolean {
  return document.documentElement.dataset.notificationsSupported !== '0';
}
