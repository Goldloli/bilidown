/**
 * 关闭设置窗口前的保存协调（design D4 / tasks 4.5）。
 *
 * 所有关闭入口（窗口按钮、原生关闭、快捷键）都先经过 Tauri 的 CloseRequested，
 * 统一在这里：刷新并提交有效草稿 → 等待在途保存 → 成功才放行关闭。
 * 正常保存成功不新增确认；失败/无效草稿保留窗口，由界面提供重试或明确放弃。
 */
import type { SettingsValidationIssue } from '../lib/types';

export interface CloseFlowDeps {
  hasDirtyDraft: () => boolean;
  issues: () => SettingsValidationIssue[];
  /** 提交草稿相对最后有效设置的差异；无差异时应返回 true */
  saveDirtyDraft: () => Promise<boolean>;
  /** 等待在途/排队的保存结束 */
  waitForSaves: () => Promise<void>;
  saveError: () => string | null;
}

export type CloseDecision = 'close' | 'invalid' | 'failed';

export async function decideSettingsClose(deps: CloseFlowDeps): Promise<CloseDecision> {
  await deps.waitForSaves();
  if (deps.issues().length > 0) return 'invalid';
  if (deps.hasDirtyDraft()) {
    const saved = await deps.saveDirtyDraft();
    await deps.waitForSaves();
    if (!saved) return 'failed';
  }
  if (deps.saveError()) return 'failed';
  return 'close';
}

/** Tauri WebviewWindow 中关闭流程用到的最小接口（便于单测注入）。 */
export interface ClosablePreferencesWindow {
  onCloseRequested(
    handler: (event: { preventDefault: () => void }) => void | Promise<void>,
  ): Promise<() => void> | (() => void);
  destroy(): Promise<void> | void;
}

export interface PreferencesCloseHandler {
  readonly allowed: boolean;
  dispose: () => void;
}

/**
 * 安装关闭拦截：第一次关闭请求先 preventDefault 并交给 `requestClose` 决定；
 * 允许关闭时再 destroy（实现方传入 Rust 命令，避免前端 window.destroy capability 限制）。
 */
export async function installPreferencesCloseHandler(
  window: ClosablePreferencesWindow,
  requestClose: () => Promise<boolean>,
): Promise<PreferencesCloseHandler> {
  let allowed = false;
  let closing: Promise<boolean> | null = null;
  const handler = async (event: { preventDefault: () => void }): Promise<void> => {
    if (allowed) return;
    event.preventDefault();
    // 连续关闭请求共享同一次提交/等待，避免重复保存或重复销毁。
    const inFlight = (closing ??= requestClose());
    let canClose = false;
    try {
      canClose = await inFlight;
    } finally {
      if (closing === inFlight) closing = null;
    }
    if (!canClose || allowed) return;
    allowed = true;
    try {
      await window.destroy();
    } catch (error) {
      // Tauri 的 destroy 受 capability 权限约束：失败时不能静默假装已关闭。
      console.error('[settings] 关闭设置窗口失败', error);
      throw error;
    }
  };
  const unlisten = await window.onCloseRequested(handler);
  return {
    get allowed() {
      return allowed;
    },
    dispose: () => unlisten(),
  };
}
