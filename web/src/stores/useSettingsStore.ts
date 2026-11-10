import { create } from 'zustand';
import { api } from '../lib/ipc';
import {
  settingsRevisionOf,
  validateSettings,
  type AdaptedSettings,
} from '../lib/adapt';
import type { Settings, SettingsPatch, SettingsValidationIssue } from '../lib/types';
import { applyTheme, resolveTheme } from '../lib/theme';
import {
  applyAccent,
  normalizeAccent,
  readAccentOverride,
  readAccentPref,
  writeAccentPref,
} from '../lib/accent';
import {
  applyMotion,
  normalizeMotion,
  readMotionOverride,
  readMotionPref,
  writeMotionPref,
} from '../lib/motion';
import { applyGlass, writeGlassPref } from '../lib/glass';
import { errorMessageOf } from '../lib/errors';
import {
  changedPatchFields,
  cloneSettings,
  diffSettings,
  hasDirtyDraft,
  pathsOfPatch,
  reconcileDraftWithSaved,
} from '../settings/draft';
import { SettingsSubmitQueue } from '../settings/submission';

interface SettingsState {
  /** 最后一次由后端接受的有效设置（含 revision），所有窗口共享的持久化真相 */
  settings: AdaptedSettings | null;
  /** 可编辑草稿：文本输入、代理输入、未提交的开关选择都在这里 */
  draft: Settings | null;
  /** 草稿的字段级问题（前端校验；无效草稿不会被提交） */
  issues: SettingsValidationIssue[];
  /** 有 patch 在提交或排队中 */
  saving: boolean;
  /** 最近一次保存失败的中文原因（失败时保留草稿与有效设置） */
  saveError: string | null;
  /** 最近一次保存成功的时间戳；一次性提示由界面按到期计时消费 */
  lastSavedAt: number | null;

  /** 接收后端响应/事件/启动读取；按 revision 丢弃过期值并调和草稿 */
  setSettings: (next: AdaptedSettings) => void;
  /** 只改草稿（输入中），不提交 */
  edit: (patch: SettingsPatch) => void;
  /** 校验并提交补丁（开关/分段即时提交、文本失焦/Enter 提交） */
  save: (patch: SettingsPatch) => Promise<boolean>;
  /** 兼容旧调用方：等价 save */
  update: (patch: SettingsPatch) => Promise<boolean>;
  /** 重试提交当前草稿相对最后有效设置的差异 */
  retrySave: () => Promise<boolean>;
  /** 明确放弃本次修改：草稿回到最后有效设置 */
  discardDraft: () => void;
  /** 等待在途/排队中的保存结束（关闭设置窗口前） */
  waitForSaves: () => Promise<void>;
  hasPendingSaves: () => boolean;
  hasDirtyDraft: () => boolean;
  reload: () => Promise<void>;
  /** 重新建立后端会话/测试时重置本地设置状态 */
  reset: () => void;
}

function applyAppearance(settings: Settings): void {
  if (typeof document === 'undefined' || typeof window === 'undefined') return;
  applyTheme(resolveTheme(settings.interface.appearance));
  applyGlass(settings.interface.glassBackground);
  // 后端有效设置优先于启动兜底缓存；URL 的 QA 覆盖参数仍最高优先级。
  applyAccent(
    readAccentOverride() ?? normalizeAccent(settings.interface.accentColor ?? readAccentPref()),
  );
  applyMotion(
    normalizeMotion(
      readMotionOverride() ??
        settings.interface.motion ??
        (settings.interface.reduceMotion ? 'reduce' : readMotionPref()),
    ),
  );
}

/** 后端有效设置到达后统一覆盖显示缓存（localStorage 仅用于启动前兜底）。 */
function writeDisplayCache(settings: Settings): void {
  writeGlassPref(settings.interface.glassBackground);
  if (settings.interface.accentColor) writeAccentPref(settings.interface.accentColor);
  if (settings.interface.motion) writeMotionPref(settings.interface.motion);
}

export const useSettingsStore = create<SettingsState>()((set, get) => {
  const queue = new SettingsSubmitQueue(
    async (patch) => {
      try {
        const saved = await api.patchSettings(patch);
        const state = get();
        const before = state.settings ?? saved;
        const draft = state.draft ?? cloneSettings(saved);
        const nextDraft = reconcileDraftWithSaved(draft, before, saved, patch);
        applyAppearance(saved);
        writeDisplayCache(saved);
        set({
          settings: saved,
          draft: nextDraft,
          issues: validateSettings(nextDraft),
          saveError: null,
          lastSavedAt: Date.now(),
        });
        return true;
      } catch (error) {
        set({ saveError: errorMessageOf(error) });
        return false;
      }
    },
    (saving) => set({ saving }),
  );

  const commit = async (patch: SettingsPatch): Promise<boolean> => {
    const state = get();
    if (!state.settings || !state.draft) return false;
    const merged = cloneSettings(state.draft);
    for (const { group, key } of pathsOfPatch(patch)) {
      const groupPatch = patch[group] as unknown as Record<string, unknown> | undefined;
      (merged[group] as unknown as Record<string, unknown>)[key] = groupPatch?.[key];
    }
    const issues = validateSettings(merged);
    set({ draft: merged, issues });
    // 不完整/非法草稿不提交，最后有效设置与运行态保持原样（design D3/D4）。
    if (issues.length > 0) {
      // 全局可读错误：即使非法字段不在当前页，也能看到为什么没有保存。
      set({ saveError: issues[0].message });
      return false;
    }
    if (pathsOfPatch(patch).length === 0) return true;
    // 值未变化的重复操作不产生无意义的保存请求；只提交本次补丁中真正变化的字段。
    const effective = changedPatchFields(patch, merged, state.settings);
    if (pathsOfPatch(effective).length === 0) {
      set({ saveError: null });
      return true;
    }
    return queue.enqueue(effective);
  };

  return {
    settings: null,
    draft: null,
    issues: [],
    saving: false,
    saveError: null,
    lastSavedAt: null,

    setSettings: (next) => {
      const state = get();
      const incomingRevision = settingsRevisionOf(next);
      const currentRevision = settingsRevisionOf(state.settings);
      if (
        incomingRevision !== undefined &&
        currentRevision !== undefined &&
        incomingRevision < currentRevision
      ) {
        // 迟到的响应/事件不覆盖较新的已提交设置（design D3）。
        return;
      }
      // Store 已判定接收：同步 ApiClient.lastSettings 与修订状态。
      api.applySettingsSnapshot(next);
      const nextDraft =
        state.settings && state.draft
          ? reconcileDraftWithSaved(state.draft, state.settings, next)
          : cloneSettings(next);
      applyAppearance(next);
      writeDisplayCache(next);
      set({
        settings: next,
        draft: nextDraft,
        issues: validateSettings(nextDraft),
      });
    },

    edit: (patch) => {
      const state = get();
      if (!state.settings) return;
      const draft = state.draft ?? cloneSettings(state.settings);
      const nextDraft = cloneSettings(draft);
      for (const { group, key } of pathsOfPatch(patch)) {
        const groupPatch = patch[group] as unknown as Record<string, unknown> | undefined;
        (nextDraft[group] as unknown as Record<string, unknown>)[key] = groupPatch?.[key];
      }
      set({ draft: nextDraft, issues: validateSettings(nextDraft) });
    },

    save: (patch) => commit(patch),
    update: (patch) => commit(patch),

    retrySave: async () => {
      const state = get();
      if (!state.settings || !state.draft) return false;
      const patch = diffSettings(state.draft, state.settings);
      if (pathsOfPatch(patch).length === 0) {
        set({ saveError: null });
        return true;
      }
      return commit(patch);
    },

    discardDraft: () => {
      const state = get();
      if (!state.settings) return;
      applyAppearance(state.settings);
      writeDisplayCache(state.settings);
      set({
        draft: cloneSettings(state.settings),
        issues: [],
        saveError: null,
      });
    },

    waitForSaves: () => queue.idle(),
    hasPendingSaves: () => queue.busy,
    hasDirtyDraft: () => hasDirtyDraft(get().draft, get().settings),

    reload: async () => {
      const next = await api.getSettings();
      get().setSettings(next);
    },

    reset: () => {
      queue.clear();
      set({
        settings: null,
        draft: null,
        issues: [],
        saving: false,
        saveError: null,
        lastSavedAt: null,
      });
    },
  };
});
