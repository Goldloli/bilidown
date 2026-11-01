import { create } from 'zustand';
import { api } from '../lib/ipc';
import type { Settings, SettingsPatch, SettingsValidationIssue } from '../lib/types';
import { applyTheme, resolveTheme } from '../lib/theme';
import { applyAccent, normalizeAccent, readAccentOverride, readAccentPref, writeAccentPref } from '../lib/accent';
import { applyMotion, normalizeMotion, readMotionOverride, readMotionPref, writeMotionPref } from '../lib/motion';
import { applyGlass, writeGlassPref } from '../lib/glass';

interface SettingsState {
  settings: Settings | null;
  issues: SettingsValidationIssue[];
  saving: boolean;
  lastSavedAt: number | null;
  setSettings: (settings: Settings) => void;
  setIssues: (issues: SettingsValidationIssue[]) => void;
  update: (patch: SettingsPatch) => Promise<SettingsValidationIssue[]>;
  reload: () => Promise<void>;
}

function applyAppearance(settings: Settings): void {
  applyTheme(resolveTheme(settings.interface.appearance));
  applyGlass(settings.interface.glassBackground);
  // accentColor/motion/glass 已由 DTO 持久化；localStorage 仅作旧版本兜底
  applyAccent(normalizeAccent(readAccentOverride() ?? settings.interface.accentColor ?? readAccentPref()));
  applyMotion(
    normalizeMotion(
      readMotionOverride() ??
        settings.interface.motion ??
        (settings.interface.reduceMotion ? 'reduce' : readMotionPref()),
    ),
  );
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  settings: null,
  issues: [],
  saving: false,
  lastSavedAt: null,

  setSettings: (settings) => {
    applyAppearance(settings);
    writeGlassPref(settings.interface.glassBackground);
    writeAccentPref(settings.interface.accentColor ?? readAccentPref());
    writeMotionPref(settings.interface.motion ?? readMotionPref());
    set({ settings });
  },

  setIssues: (issues) => set({ issues }),

  update: async (patch) => {
    set({ saving: true });
    try {
      const result = await api.updateSettings(patch);
      applyAppearance(result.settings);
      set({
        settings: result.settings,
        issues: result.issues,
        saving: false,
        lastSavedAt: Date.now(),
      });
      return result.issues;
    } catch (error) {
      set({ saving: false });
      throw error;
    }
  },

  reload: async () => {
    const settings = await api.getSettings();
    get().setSettings(settings);
  },
}));
