import type { ReactNode } from 'react';
import type { Settings } from '../lib/types';
import { useSettingsStore } from '../stores/useSettingsStore';
import { hasDirtyDraft } from './draft';

export function SettingsGroup({
  title,
  description,
  children,
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="mb-6">
      <h2 className="mb-1 px-1 text-sm font-semibold text-fg">{title}</h2>
      {description && <p className="mb-2 px-1 text-2xs leading-[15px] text-fg2">{description}</p>}
      <div
        className="rounded-card border border-line px-4 py-1 [&>*+*]:border-t [&>*+*]:border-line"
        style={{ background: 'var(--card)' }}
      >
        {children}
      </div>
    </section>
  );
}

/**
 * 设置表单（design D4）：区分「草稿」「最后有效设置」「提交中」「保存失败」。
 *
 * · `settings` 是当前草稿（界面显示值），`saved` 是最后一次后端有效设置；
 * · `edit` 只改草稿（输入中），`save` 校验后按序提交（开关/分段即时、文本失焦/Enter）；
 * · 保存失败保留草稿并暴露统一中文错误与重试/放弃动作。
 */
export function useSettingsForm() {
  const saved = useSettingsStore((s) => s.settings);
  const draft = useSettingsStore((s) => s.draft);
  const issues = useSettingsStore((s) => s.issues);
  const saving = useSettingsStore((s) => s.saving);
  const saveError = useSettingsStore((s) => s.saveError);
  const edit = useSettingsStore((s) => s.edit);
  const save = useSettingsStore((s) => s.save);
  const retrySave = useSettingsStore((s) => s.retrySave);
  const discardDraft = useSettingsStore((s) => s.discardDraft);

  const issueFor = (path: string): string | undefined =>
    issues.find((issue) => issue.path === path)?.message;

  return {
    /** 草稿（编辑中的显示值） */
    settings: (draft ?? saved) as Settings,
    /** 最后一次后端有效设置 */
    saved,
    issues,
    issueFor,
    saving,
    saveError,
    dirty: hasDirtyDraft(draft, saved),
    edit,
    save,
    retrySave,
    discardDraft,
  };
}
