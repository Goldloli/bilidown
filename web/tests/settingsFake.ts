/**
 * 设置测试替身：按 patch_settings 契约（省略=保持、null=清空、返回递增 revision）模拟后端。
 * 仅用于 vitest；不进入生产构建。
 */
import type { Backend } from '../src/lib/ipc';
import {
  applySettingsPatchDTO,
  defaultInternalSettings,
  settingsFromDTO,
  settingsToDTO,
} from '../src/lib/adapt';
import type { SettingsDTO, SettingsPatchDTO } from '../src/lib/contract';
import type { Settings } from '../src/lib/types';
import type { AdaptedSettings } from '../src/lib/adapt';

export interface Deferred<T = void> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
}

export function deferred<T = void>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

export interface SettingsFake {
  backend: Backend;
  patchCalls: SettingsPatchDTO[];
  saved: () => Settings;
  revision: () => number;
  patchError: () => string | null;
  setPatchError: (message: string | null) => void;
  /** 模拟 settings://changed 事件的适配结果（带 revision） */
  event: (settings: Settings, revision: number, prev?: Settings | null) => AdaptedSettings;
}

export interface SettingsFakeOptions {
  saved?: Settings;
  revision?: number;
  /** 在每次 patch 被后端接受前执行（用于制造在途/乱序） */
  beforePatch?: (patch: SettingsPatchDTO, index: number) => Promise<void>;
  patchError?: string | null;
}

export function createSettingsFake(options: SettingsFakeOptions = {}): SettingsFake {
  let saved = options.saved ?? defaultInternalSettings();
  let revision = options.revision ?? 0;
  let patchError = options.patchError ?? null;
  const patchCalls: SettingsPatchDTO[] = [];

  const dto = (settings: Settings, rev: number): SettingsDTO => settingsToDTO(settings, rev);

  const backend = {
    getSettings: async (): Promise<SettingsDTO> => dto(saved, revision),
    setSettings: async (settings: SettingsDTO): Promise<SettingsDTO> => {
      saved = settingsFromDTO(settings, saved);
      revision += 1;
      return dto(saved, revision);
    },
    patchSettings: async (patch: SettingsPatchDTO): Promise<SettingsDTO> => {
      const index = patchCalls.length;
      patchCalls.push(patch);
      await options.beforePatch?.(patch, index);
      if (patchError) {
        throw { kind: 'unknown', message: patchError, actions: [] };
      }
      saved = applySettingsPatchDTO(saved, patch);
      revision += 1;
      return dto(saved, revision);
    },
    testProxy: async () => ({ ok: true, message: '连接成功' }),
    testUpdateProxy: async () => ({ ok: true, message: '更新连接成功' }),
    on: () => () => {},
  } as unknown as Backend;

  return {
    backend,
    patchCalls,
    saved: () => saved,
    revision: () => revision,
    patchError: () => patchError,
    setPatchError: (message) => {
      patchError = message;
    },
    event: (settings, rev, prev) => settingsFromDTO(settingsToDTO(settings, rev), prev),
  };
}
