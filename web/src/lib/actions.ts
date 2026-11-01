import { api } from './ipc';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useUiStore } from '../stores/useUiStore';
import type { ParseResult } from './types';
import { t } from '../i18n/zh-CN';
import { resolveUgcSeason, seasonToParseResult } from './collection';
import { buildDefaultSelection, buildDefaultSelectionPlan } from './addSelection';

export { buildDefaultSelection };

/** 解析文本：普通解析；UGC 合集二次确认后进入确认面板或快速入队 */
export async function handleParseText(text: string): Promise<ParseResult | null> {
  const ui = useUiStore.getState();
  try {
    const result = await api.parseLinks(text);
    // 2) UGC 合集：解析成功后再查合集，>1 集时二次确认
    if (result.items.length === 1) {
      const bvid = text.match(/BV[0-9A-Za-z]+/)?.[0] ?? result.items[0].id.match(/BV[0-9A-Za-z]+/)?.[0];
      if (bvid) {
        const season = await resolveUgcSeason(bvid);
        if (season && season.episodes.length > 1) {
          const whole = await ui.confirm({
            title: t('collection.seasonTitle'),
            body: t('collection.seasonBody', { title: season.title, n: season.episodes.length }),
            confirmLabel: t('collection.seasonConfirm'),
            cancelLabel: t('collection.seasonCurrent'),
          });
          if (whole) {
            ui.openPanel({ type: 'add', parse: seasonToParseResult(season) });
            return null;
          }
        }
      }
    }
    if (result.items.length === 0) {
      const reason = result.failures[0]?.reason ?? t('toast.clipboardEmpty');
      ui.pushToast({ kind: 'warning', message: reason });
      return null;
    }
    if (result.requiresConfirmation) {
      ui.openPanel({ type: 'add', parse: result });
      return result;
    }
    const settings = useSettingsStore.getState().settings;
    if (!settings) return result;
    const plan = buildDefaultSelectionPlan(result, settings);
    const firstIssue = Object.values(plan.issuesByItem)[0];
    if (firstIssue) {
      ui.pushToast({ kind: 'warning', message: firstIssue.message });
      return null;
    }
    const selectedCount = Object.values(plan.selection.entries).reduce(
      (count, entries) => count + entries.length,
      0,
    );
    if (selectedCount === 0) {
      ui.pushToast({ kind: 'warning', message: '没有可下载的条目' });
      return null;
    }
    const ids = await api.enqueue(result, plan.selection);
    ui.pushToast({
      kind: 'success',
      message: t('toast.tasksAdded', { n: ids.length }),
    });
    return result;
  } catch (error) {
    ui.pushToast({
      kind: 'error',
      message: error instanceof Error ? error.message : String(error),
    });
    return null;
  }
}

/** 从剪贴板读取并进入添加流程（不自动下载） */
export async function addFromClipboard(): Promise<void> {
  const ui = useUiStore.getState();
  try {
    const text = await api.readClipboardText();
    if (!text.trim()) {
      ui.pushToast({ kind: 'warning', message: t('toast.clipboardEmpty') });
      return;
    }
    await handleParseText(text);
  } catch {
    ui.pushToast({ kind: 'warning', message: t('toast.clipboardEmpty') });
  }
}

export async function copyLink(url: string): Promise<void> {
  const ui = useUiStore.getState();
  try {
    await api.copyText(url);
    ui.pushToast({ kind: 'success', message: t('toast.copied') });
  } catch {
    ui.pushToast({ kind: 'error', message: '复制失败' });
  }
}
