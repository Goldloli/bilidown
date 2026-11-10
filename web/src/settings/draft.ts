/**
 * 设置草稿工具（design D4）：草稿与最后一次有效设置分离。
 *
 * · `edit` 只改草稿，不提交；
 * · `save`/`commit` 提交有效草稿，失败保留草稿；
 * · 收到响应/跨窗口事件时按服务端最新值调和草稿，未提交的本地编辑不被重置。
 */
import { mergeSettings } from '../lib/adapt';
import type { Settings, SettingsPatch } from '../lib/types';

export type SettingsGroupKey = keyof Settings;

const GROUP_KEYS: SettingsGroupKey[] = [
  'general',
  'interface',
  'download',
  'quality',
  'network',
];

type GroupRecord = Record<string, unknown>;

function groupRecord(settings: Settings, group: SettingsGroupKey): GroupRecord {
  return settings[group] as unknown as GroupRecord;
}

/** 深拷贝内部设置（各组都是单层标量字段）。 */
export function cloneSettings(settings: Settings): Settings {
  return mergeSettings(settings, {});
}

export interface PatchPath {
  group: SettingsGroupKey;
  key: string;
}

/** 补丁里显式包含的字段路径。 */
export function pathsOfPatch(patch: SettingsPatch): PatchPath[] {
  const paths: PatchPath[] = [];
  for (const group of GROUP_KEYS) {
    const value = patch[group];
    if (!value) continue;
    for (const key of Object.keys(value)) {
      paths.push({ group, key });
    }
  }
  return paths;
}

/** 草稿相对最后有效设置的差异补丁（只包含变化字段）。 */
export function diffSettings(draft: Settings, saved: Settings): SettingsPatch {
  const patch: SettingsPatch = {};
  for (const group of GROUP_KEYS) {
    const draftGroup = groupRecord(draft, group);
    const savedGroup = groupRecord(saved, group);
    const changed: GroupRecord = {};
    let any = false;
    for (const key of Object.keys(savedGroup)) {
      if (!Object.is(draftGroup[key], savedGroup[key])) {
        changed[key] = draftGroup[key];
        any = true;
      }
    }
    if (any) {
      (patch as Record<string, unknown>)[group] = changed;
    }
  }
  return patch;
}

/** 只保留与最后有效设置不同的补丁字段（避免重复提交未变化的值）。 */
export function changedPatchFields(
  patch: SettingsPatch,
  draft: Settings,
  saved: Settings,
): SettingsPatch {
  const result = {} as Record<string, Record<string, unknown>>;
  for (const { group, key } of pathsOfPatch(patch)) {
    const draftGroup = draft[group] as unknown as Record<string, unknown>;
    const savedGroup = saved[group] as unknown as Record<string, unknown>;
    if (Object.is(draftGroup[key], savedGroup[key])) continue;
    result[group] = { ...result[group], [key]: draftGroup[key] };
  }
  return result as SettingsPatch;
}

/** 是否有未提交的有效/无效草稿。 */
export function hasDirtyDraft(draft: Settings | null, saved: Settings | null): boolean {
  if (!draft || !saved) return false;
  return pathsOfPatch(diffSettings(draft, saved)).length > 0;
}

/**
 * 以服务端最新设置调和草稿（design D4）：
 * · 本次提交过的字段：若草稿仍是提交值，采纳服务端最新值（可能已被其他窗口更新）；
 * · 用户本地编辑且未提交的字段：保留草稿，不被旧事件重置；
 * · 其余字段：跟随服务端最新值（跨窗口修改立即生效）。
 */
export function reconcileDraftWithSaved(
  draft: Settings,
  before: Settings,
  after: Settings,
  submitted?: SettingsPatch | null,
): Settings {
  const next = cloneSettings(draft);
  const submittedRecord = submitted as Record<string, GroupRecord> | undefined;
  for (const group of GROUP_KEYS) {
    const draftGroup = groupRecord(next, group);
    const beforeGroup = groupRecord(before, group);
    const afterGroup = groupRecord(after, group);
    const patchGroup = submittedRecord?.[group];
    for (const key of Object.keys(afterGroup)) {
      const draftValue = draftGroup[key];
      if (patchGroup && Object.prototype.hasOwnProperty.call(patchGroup, key)) {
        if (Object.is(draftValue, patchGroup[key])) draftGroup[key] = afterGroup[key];
        continue;
      }
      if (!Object.is(draftValue, beforeGroup[key])) continue;
      draftGroup[key] = afterGroup[key];
    }
  }
  return next;
}
