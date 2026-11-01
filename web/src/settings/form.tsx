import type { ReactNode } from 'react';
import type { Settings, SettingsPatch } from '../lib/types';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useState } from 'react';

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

export function useSettingsForm() {
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  const issues = useSettingsStore((s) => s.issues);
  const [saving, setSaving] = useState(false);

  const save = (patch: SettingsPatch) => {
    setSaving(true);
    void update(patch).finally(() => setSaving(false));
  };

  const issueFor = (path: string): string | undefined =>
    issues.find((issue) => issue.path === path)?.message;

  return { settings: settings as Settings, save, issueFor, saving, issues };
}
