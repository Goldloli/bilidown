import '../i18n/zh-CN.dynamic';
import { useEffect, useMemo, useRef, useState } from 'react';
import { t } from '../i18n/zh-CN';
import { api } from '../lib/ipc';
import { errorMessageOf } from '../lib/errors';
import type { EnqueueSelection } from '../lib/ipc';
import {
  buildDefaultSelectionPlan,
  type DefaultSelectionIssue,
} from '../lib/addSelection';
import { formatBytes, formatDateTime, formatDuration } from '../lib/format';
import { kindIcon, kindLabel } from '../lib/media';
import type {
  AudioFormatId,
  CodecId,
  EnqueueFailure,
  ParseItem,
  ParseResult,
  QualityOption,
} from '../lib/types';
import { useSettingsStore } from '../stores/useSettingsStore';
import { useUiStore } from '../stores/useUiStore';
import { Button } from '../components/Button';
import { Checkbox } from '../components/Checkbox';
import { Icon, type IconName } from '../components/Icon';
import { PathField } from '../components/PathField';
import { SegmentedControl } from '../components/SegmentedControl';
import { Sheet } from '../components/Sheet';
import { Thumbnail } from '../components/Thumbnail';

function fallbackEntries(items: ParseItem[]): Record<string, string[]> {
  return Object.fromEntries(
    items.map((item) => [
      item.id,
      item.entries.filter((entry) => entry.selectedByDefault).map((entry) => entry.id),
    ]),
  );
}

function QualityPill({
  option,
  selected,
  onSelect,
}: {
  option: QualityOption;
  selected: boolean;
  onSelect: () => void;
}) {
  const hint = option.unavailableReason;
  return (
    <button
      type="button"
      disabled={!option.available}
      title={hint ?? `${option.resolution}${option.fps > 30 ? ` · ${option.fps}fps` : ''}`}
      onClick={onSelect}
      className={`inline-flex h-6 items-center gap-1 rounded-tag border px-2 text-2xs transition-colors duration-[var(--dur-fast)] disabled:cursor-default ${
        selected
          ? 'border-accent bg-accent font-medium text-white'
          : option.available
            ? 'border-line-strong text-fg hover:bg-hover'
            : 'border-line text-fg3 opacity-55'
      }`}
    >
      {option.label}
      {!option.available && <Icon name="lock" size={9} />}
    </button>
  );
}

export function AddPanel({
  parse,
  onClose,
}: {
  parse: ParseResult;
  onClose: () => void;
}) {
  const items = parse.items;
  const settings = useSettingsStore((s) => s.settings);
  const pushToast = useUiStore((s) => s.pushToast);
  const setView = useUiStore((s) => s.setView);
  const confirm = useUiStore((s) => s.confirm);

  const initialPlan = useMemo(
    () => (settings ? buildDefaultSelectionPlan(parse, settings) : null),
    [parse, settings],
  );
  const defaultsApplied = useRef(initialPlan !== null);
  const [activeId, setActiveId] = useState(items[0]?.id ?? '');
  const [selected, setSelected] = useState<Record<string, string[]>>(
    () => initialPlan?.selection.entries ?? fallbackEntries(items),
  );
  const [quality, setQuality] = useState<Record<string, number>>(
    () => initialPlan?.selection.quality ?? {},
  );
  const [qualityIssues, setQualityIssues] = useState<Record<string, DefaultSelectionIssue>>(
    () => initialPlan?.issuesByItem ?? {},
  );
  const [codec, setCodec] = useState<Record<string, CodecId>>(
    () => initialPlan?.selection.codec ?? {},
  );
  const [audio, setAudio] = useState<Record<string, AudioFormatId>>(
    () => initialPlan?.selection.audio ?? {},
  );
  const [audioQn, setAudioQn] = useState<Record<string, number>>(
    () => initialPlan?.selection.audioQn ?? {},
  );
  const [outputDir, setOutputDir] = useState(initialPlan?.selection.outputDir ?? '');
  const [submitting, setSubmitting] = useState(false);
  // 按内容记录入队结果：成功的条目不重复提交，失败的保留并只重试失败内容（design D8/6.4）
  const [failedItems, setFailedItems] = useState<EnqueueFailure[]>([]);
  const [doneItemIds, setDoneItemIds] = useState<string[]>([]);

  useEffect(() => {
    if (!settings || !initialPlan || defaultsApplied.current) return;
    setSelected(initialPlan.selection.entries);
    setQuality(initialPlan.selection.quality);
    setQualityIssues(initialPlan.issuesByItem);
    setCodec(initialPlan.selection.codec);
    setAudio(initialPlan.selection.audio);
    setAudioQn(initialPlan.selection.audioQn ?? {});
    setOutputDir(initialPlan.selection.outputDir);
    defaultsApplied.current = true;
  }, [initialPlan, settings]);

  const active = items.find((item) => item.id === activeId) ?? items[0];
  const activeSelected = selected[active?.id ?? ''] ?? [];
  const allSelected = active ? activeSelected.length === active.entries.length : false;
  const someSelected = activeSelected.length > 0 && !allSelected;

  const pendingItems = useMemo(
    () =>
      items.filter(
        (item) => (selected[item.id]?.length ?? 0) > 0 && !doneItemIds.includes(item.id),
      ),
    [items, selected, doneItemIds],
  );
  const pendingCount = useMemo(
    () => pendingItems.reduce((sum, item) => sum + (selected[item.id]?.length ?? 0), 0),
    [pendingItems, selected],
  );
  const blockingQualityIssues = useMemo(
    () =>
      items.flatMap((item) => {
        if (doneItemIds.includes(item.id)) return [];
        const issue = qualityIssues[item.id];
        return (selected[item.id]?.length ?? 0) > 0 && issue ? [issue] : [];
      }),
    [items, qualityIssues, selected, doneItemIds],
  );
  const activeQualityIssue = active ? qualityIssues[active.id] : undefined;
  const activeBytes = useMemo(
    () =>
      active?.entries
        .filter((entry) => activeSelected.includes(entry.id))
        .reduce((sum, entry) => sum + (entry.sizeBytes ?? 0), 0) ?? 0,
    [active, activeSelected],
  );

  const toggleEntry = (entryId: string) => {
    if (!active) return;
    setSelected((prev) => {
      const current = prev[active.id] ?? [];
      return {
        ...prev,
        [active.id]: current.includes(entryId)
          ? current.filter((id) => id !== entryId)
          : [...current, entryId],
      };
    });
  };

  const toggleAll = () => {
    if (!active) return;
    setSelected((prev) => ({
      ...prev,
      [active.id]: allSelected ? [] : active.entries.map((entry) => entry.id),
    }));
  };

  const submit = async () => {
    if (!settings || pendingCount === 0 || blockingQualityIssues.length > 0) return;
    if (pendingCount > 30) {
      const ok = await confirm({
        title: '下载数量较多',
        body: `本次将下载 ${pendingCount} 个文件，短时间大量请求可能触发 B 站风控，建议分批下载。`,
        confirmLabel: '继续下载',
      });
      if (!ok) return;
    }
    const itemIds = pendingItems.map((item) => item.id);
    const selection: EnqueueSelection = {
      itemIds,
      entries: selected,
      quality,
      codec,
      audio,
      audioQn,
      outputDir,
    };
    setSubmitting(true);
    setFailedItems([]);
    try {
      const outcome = await api.enqueue(parse, selection);
      if (outcome.succeededItemIds.length > 0) {
        setDoneItemIds((prev) => [...new Set([...prev, ...outcome.succeededItemIds])]);
      }
      if (outcome.failures.length > 0) {
        // 部分失败：保留已成功内容，只对失败内容提供重试
        setFailedItems(outcome.failures);
        pushToast({
          kind: outcome.taskIds.length > 0 ? 'warning' : 'error',
          message:
            outcome.taskIds.length > 0
              ? t('add.partial.summary', {
                  added: outcome.taskIds.length,
                  failed: outcome.failures.length,
                })
              : outcome.failures[0].message,
        });
        return;
      }
      pushToast({ kind: 'success', message: t('toast.tasksAdded', { n: outcome.taskIds.length }) });
      setView('queue');
      onClose();
    } catch (error) {
      pushToast({
        kind: 'error',
        message: t('add.submitError', {
          message: errorMessageOf(error),
        }),
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Sheet open onClose={onClose} width={660} ariaLabel={t('add.title')}>
      <div className="flex shrink-0 items-start gap-3 px-5 pb-2 pt-4">
        <Thumbnail
          src={active?.coverUrl}
          alt={active?.title ?? ''}
          className="h-[90px] w-[160px] shrink-0"
          iconSize={22}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-2xs text-fg2">
            <Icon name={(active ? kindIcon(active.kind) : 'film') as IconName} size={12} />
            <span>{active ? kindLabel(active.kind) : ''}</span>
            <span className="text-fg3">·</span>
            <span className="truncate">{active?.uploader}</span>
          </div>
          <h2 className="mt-1 line-clamp-2 text-md font-semibold leading-[20px] text-fg">
            {active?.title}
          </h2>
          <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-2xs text-fg3">
            {active?.durationSec != null && <span>时长 {formatDuration(active.durationSec)}</span>}
            {active?.publishedAt != null && (
              <span>发布于 {formatDateTime(active.publishedAt)}</span>
            )}
            {active && active.entries.length > 1 && (
              <span className="num text-fg2">{activeSelected.length} / {active.entries.length} 项已选</span>
            )}
          </div>
        </div>
        <button
          type="button"
          aria-label={t('common.close')}
          onClick={onClose}
          className="grid h-7 w-7 shrink-0 place-items-center rounded-tile text-fg3 hover:bg-hover hover:text-fg"
        >
          <Icon name="close" size={15} />
        </button>
      </div>

      {items.length > 1 && (
        <div className="scroll-area flex shrink-0 gap-1.5 overflow-x-auto px-5 pb-2 pt-1">
          {items.map((item) => {
            const isActive = item.id === active?.id;
            const count = selected[item.id]?.length ?? 0;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setActiveId(item.id)}
                className={`inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-2xs transition-colors duration-[var(--dur-fast)] ${
                  isActive
                    ? 'border-accent bg-accent-weak text-accent'
                    : 'border-line text-fg2 hover:bg-hover'
                }`}
              >
                <Icon name={kindIcon(item.kind) as IconName} size={12} />
                <span className="max-w-[200px] truncate">{item.title}</span>
                <span className={isActive ? 'text-accent/80' : 'text-fg3'}>
                  {count}/{item.entries.length}
                </span>
              </button>
            );
          })}
        </div>
      )}

      <div className="scroll-area min-h-0 flex-1 px-5 pb-1">
        {active && active.entries.length > 0 && (
          <div className="overflow-hidden rounded-card border border-line" style={{ background: 'var(--card-inset)' }}>
            <div className="flex h-9 items-center gap-2 border-b border-line px-3">
              <Checkbox
                checked={allSelected}
                indeterminate={someSelected}
                onChange={toggleAll}
                label={t('add.selectAll')}
              />
              <span className="text-xs font-medium text-fg">{t('add.selectAll')}</span>
              <span className="num ml-auto text-2xs text-fg2">
                已选 {activeSelected.length} / {active.entries.length}
              </span>
            </div>
            <div className="scroll-area max-h-[212px] overflow-y-auto py-1">
              {active.entries.map((entry) => {
                const checked = activeSelected.includes(entry.id);
                return (
                  <label
                    key={entry.id}
                    className="flex h-[30px] cursor-default items-center gap-2 rounded-[6px] px-3 hover:bg-hover"
                  >
                    <Checkbox checked={checked} onChange={() => toggleEntry(entry.id)} />
                    <span className="min-w-0 flex-1 truncate text-xs text-fg">{entry.title}</span>
                    {entry.durationSec != null && (
                      <span className="num shrink-0 text-2xs text-fg3">{formatDuration(entry.durationSec)}</span>
                    )}
                    {entry.sizeBytes != null && (
                      <span className="num w-14 shrink-0 text-right text-2xs text-fg3">
                        {formatBytes(entry.sizeBytes)}
                      </span>
                    )}
                  </label>
                );
              })}
            </div>
          </div>
        )}

        <div className="mt-4 space-y-2.5">
          {active && active.qualities.length > 0 && (
            <div className="flex items-start gap-3">
              <span className="w-[64px] shrink-0 pt-1 text-xs text-fg2">{t('add.quality')}</span>
              <div className="min-w-0">
                <div className="flex flex-wrap gap-1">
                  {active.qualities.map((option) => (
                    <QualityPill
                      key={option.id}
                      option={option}
                      selected={quality[active.id] === option.id}
                      onSelect={() => {
                        setQuality((prev) => ({ ...prev, [active.id]: option.id }));
                        setQualityIssues((prev) => {
                          if (!prev[active.id]) return prev;
                          const next = { ...prev };
                          delete next[active.id];
                          return next;
                        });
                      }}
                    />
                  ))}
                </div>
                {activeQualityIssue && (
                  <p className="mt-1 flex items-center gap-1 text-2xs text-danger">
                    <Icon name="warning" size={11} />
                    {activeQualityIssue.message}
                  </p>
                )}
                <p className="mt-1 text-2xs text-fg3">
                  锁定档位需大会员；未登录时最高仅 480P
                </p>
              </div>
            </div>
          )}

          {active && active.codecs.length > 0 && (
            <div className="flex items-start gap-3">
              <span className="w-[64px] shrink-0 pt-1 text-xs text-fg2">{t('add.codec')}</span>
              <div>
                <SegmentedControl
                  size="sm"
                  ariaLabel={t('add.codec')}
                  value={codec[active.id] ?? 'avc'}
                  onChange={(value) => setCodec((prev) => ({ ...prev, [active.id]: value }))}
                  options={active.codecs.map((option) => ({
                    value: option.id,
                    label: option.label,
                    disabled: !option.available,
                  }))}
                />
                <p className="mt-1 text-2xs text-fg3">AVC 兼容性最佳；接口缺失的编码自动置灰</p>
              </div>
            </div>
          )}

          {active && active.kind === 'audio' && active.audioFormats.length > 0 && (
            <div className="flex items-start gap-3">
              <span className="w-[64px] shrink-0 pt-1 text-xs text-fg2">{t('add.audio')}</span>
              <div>
                <SegmentedControl
                  size="sm"
                  ariaLabel={t('add.audio')}
                  value={audio[active.id] ?? 'original'}
                  onChange={(value) => setAudio((prev) => ({ ...prev, [active.id]: value }))}
                  options={active.audioFormats.map((option) => ({
                    value: option.id,
                    label: option.label,
                    disabled: !option.available,
                  }))}
                />
                <p className="mt-1 text-2xs text-fg3">{t('add.audio.hint')}</p>
              </div>
            </div>
          )}

          {active && active.audioQualities.length > 0 && (
            <div className="flex items-start gap-3">
              <span className="w-[64px] shrink-0 pt-1 text-xs text-fg2">
                {t('add.audioQuality')}
              </span>
              <div className="flex flex-wrap gap-1.5">
                <button
                  type="button"
                  onClick={() => setAudioQn((prev) => ({ ...prev, [active.id]: 0 }))}
                  className={`inline-flex h-6 items-center rounded-tag border px-2 text-2xs transition-colors duration-[var(--dur-fast)] ${
                    (audioQn[active.id] ?? 0) === 0
                      ? 'border-accent bg-accent font-medium text-white'
                      : 'border-line-strong text-fg hover:bg-hover'
                  }`}
                >
                  {t('add.audioQuality.auto')}
                </button>
                {active.audioQualities.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    onClick={() => setAudioQn((prev) => ({ ...prev, [active.id]: option.id }))}
                    className={`inline-flex h-6 items-center rounded-tag border px-2 text-2xs transition-colors duration-[var(--dur-fast)] ${
                      audioQn[active.id] === option.id
                        ? 'border-accent bg-accent font-medium text-white'
                        : 'border-line-strong text-fg hover:bg-hover'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="flex items-center gap-3">
            <span className="w-[64px] shrink-0 text-xs text-fg2">{t('add.directory')}</span>
            <PathField value={outputDir} onChange={setOutputDir} width={300} />
          </div>
        </div>

        {parse.failures.length > 0 && (
          <div
            className="mt-4 rounded-card border border-line p-3"
            style={{ background: 'var(--danger-weak)' }}
          >
            <div className="flex items-center gap-1.5 text-2xs font-medium text-danger">
              <Icon name="warning" size={12} />
              {t('add.failure.title')}
              <span className="text-fg3">· {parse.failures.length} 行</span>
            </div>
            {parse.failures.map((failure) => (
              <p key={failure.raw} className="mt-1 flex gap-1 truncate text-2xs text-fg2">
                <span className="truncate text-fg3">{failure.raw}</span>
                <span className="shrink-0 text-fg3">—</span>
                <span className="shrink-0">{failure.reason}</span>
              </p>
            ))}
          </div>
        )}
        {failedItems.length > 0 && (
          <div
            className="mt-4 rounded-card border border-line p-3"
            style={{ background: 'var(--danger-weak)' }}
            role="alert"
          >
            <div className="flex items-center gap-1.5 text-2xs font-medium text-danger">
              <Icon name="warning" size={12} />
              {t('add.partial.title')}
              <span className="text-fg3">· {failedItems.length} 项</span>
            </div>
            {failedItems.map((failure) => (
              <p key={failure.itemId} className="mt-1 flex gap-1 truncate text-2xs text-fg2">
                <span className="truncate text-fg3">{failure.title}</span>
                <span className="shrink-0 text-fg3">—</span>
                <span className="shrink-0">{failure.message}</span>
              </p>
            ))}
            <div className="mt-2">
              <Button
                size="sm"
                variant="primary"
                icon="refresh"
                loading={submitting}
                onClick={() => void submit()}
              >
                {t('add.partial.retry', { n: failedItems.length })}
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="flex shrink-0 items-center justify-between border-t border-line px-5 py-3">
        <div className="min-w-0">
          <span className="num block text-2xs text-fg2">
            {doneItemIds.length > 0 ? `已成功 ${doneItemIds.length} 个内容 · ` : ''}
            共 {pendingCount} 项，将下载 {pendingCount} 个文件
            {activeBytes > 0 ? ` · 当前条目约 ${formatBytes(activeBytes)}` : ''}
          </span>
          {blockingQualityIssues[0] && (
            <span className="mt-0.5 block truncate text-2xs text-danger">
              {blockingQualityIssues[0].message}
            </span>
          )}
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={onClose}>
            {t('add.cancel')}
          </Button>
          <Button
            variant="primary"
            loading={submitting}
            disabled={pendingCount === 0 || blockingQualityIssues.length > 0}
            onClick={() => void submit()}
          >
            {t('add.confirm', { n: pendingCount })}
          </Button>
        </div>
      </div>
    </Sheet>
  );
}
