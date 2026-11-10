import { describe, expect, it } from 'vitest';
import { defaultInternalSettings } from '../src/lib/adapt';
import {
  diffSettings,
  hasDirtyDraft,
  pathsOfPatch,
  reconcileDraftWithSaved,
} from '../src/settings/draft';
import { mergePatches } from '../src/settings/submission';

describe('草稿差异与顺序补丁合并（design D4）', () => {
  it('合并待提交补丁：不同字段全部保留、同字段以后一次为准', () => {
    const merged = mergePatches(
      { interface: { appearance: 'dark' } },
      { download: { maxConcurrent: 5 } },
    );
    const mergedAgain = mergePatches(merged, {
      interface: { appearance: 'light' },
      download: { maxConcurrent: 4 },
    });
    expect(mergedAgain).toEqual({
      interface: { appearance: 'light' },
      download: { maxConcurrent: 4 },
    });
    expect(pathsOfPatch(mergedAgain)).toHaveLength(2);
  });

  it('差异补丁只包含变化的字段', () => {
    const saved = defaultInternalSettings();
    const draft = {
      ...saved,
      interface: { ...saved.interface, appearance: 'dark' as const },
      download: { ...saved.download, maxConcurrent: 7 },
    };
    expect(diffSettings(draft, saved)).toEqual({
      interface: { appearance: 'dark' },
      download: { maxConcurrent: 7 },
    });
    expect(hasDirtyDraft(draft, saved)).toBe(true);
    expect(hasDirtyDraft(saved, saved)).toBe(false);
    expect(hasDirtyDraft(null, saved)).toBe(false);
  });

  it('服务端事件调和：本地未提交编辑保留，未编辑字段跟随服务端', () => {
    const before = defaultInternalSettings();
    const draft = {
      ...before,
      download: { ...before.download, maxConcurrent: 6 },
    };
    const after = {
      ...before,
      interface: { ...before.interface, appearance: 'dark' as const },
      general: { ...before.general, notifyOnComplete: false },
    };
    const next = reconcileDraftWithSaved(draft, before, after);
    expect(next.download.maxConcurrent).toBe(6);
    expect(next.interface.appearance).toBe('dark');
    expect(next.general.notifyOnComplete).toBe(false);
  });

  it('本次提交字段在服务端被更新时采纳服务端最新值', () => {
    const before = defaultInternalSettings();
    const draft = {
      ...before,
      download: { ...before.download, maxConcurrent: 5 },
    };
    // 本窗口提交 5，但服务端最终值为其他窗口稍后提交的 7。
    const after = {
      ...before,
      download: { ...before.download, maxConcurrent: 7 },
    };
    const next = reconcileDraftWithSaved(
      draft,
      before,
      after,
      { download: { maxConcurrent: 5 } },
    );
    expect(next.download.maxConcurrent).toBe(7);
  });
});
