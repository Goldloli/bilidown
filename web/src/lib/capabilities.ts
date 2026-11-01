import { devParams } from '../dev/scenario';

/**
 * 「看起来有功能但其实没接线」的防护：
 * · mock 模式全部可用（用于视觉验收/演示）
 * · 真实 Tauri 模式（shell 尚未接线）先禁用并提示「待接线」
 * shell 落地对应命令后，把 pendingFeatures 里对应项移除即可。
 */
export type PendingFeature =
  | 'redownload'
  | 'reveal'
  | 'open-file'
  | 'copy-link'
  | 'copy-diagnostics'
  | 'reset';

/**
 * 4.5 后上述命令均已由 shell 实现（见 src-tauri/COMMANDS.md §2 EXTENSION），
 * 因此真实 adapter 下不再禁用。仍有未实现能力时把对应项加回此数组即可：
 * · 通知点击「定位到任务」（7.3，尚未实现，目前 UI 没有入口）
 */
const PENDING_IN_REAL: PendingFeature[] = [];

export function isMockMode(): boolean {
  return devParams().mock;
}

export function isPending(feature: PendingFeature): boolean {
  return !isMockMode() && PENDING_IN_REAL.includes(feature);
}

/** 禁用态 tooltip：`label · 待接线`；未接线时返回 null 表示不展示 */
export function pendingLabel(feature: PendingFeature, label: string): string {
  return isPending(feature) ? `${label} · 待接线` : label;
}
