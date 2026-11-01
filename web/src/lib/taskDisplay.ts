import { formatBytes, formatPercent, formatSpeed } from './format';
import { kindLabel } from './media';
import type { QueueStats, QueueTask } from './types';
import { t } from '../i18n/zh-CN';

/** 行卡副信息：UP 主 · 类型 · 清晰度 · 编码 · 条目数 */
export function taskSubline(task: QueueTask): string {
  const parts = [task.uploader, kindLabel(task.kind), task.params.qualityLabel];
  if (task.kind !== 'audio') parts.push(task.params.codec.toUpperCase());
  if (task.params.entryCount > 1) parts.push(`${task.params.entryCount} 项`);
  return parts.join(' · ');
}

/** 底栏全局状态文案 */
export function statusSummary(stats: QueueStats, showSpeed: boolean, showTotal = false): string {
  const parts: string[] = [];
  if (stats.downloading > 0) {
    const speed = showSpeed && stats.totalSpeedBps > 0 ? ` · ${formatSpeed(stats.totalSpeedBps)}` : '';
    parts.push(`${t('bottombar.status.downloading', { n: stats.downloading })}${speed}`);
  }
  if (stats.paused > 0) parts.push(t('bottombar.status.paused', { n: stats.paused }));
  if (stats.failed > 0) parts.push(t('bottombar.status.failed', { n: stats.failed }));
  if (parts.length === 0) {
    if (stats.completed > 0) parts.push(t('bottombar.status.completed', { n: stats.completed }));
    else if (stats.queued > 0) parts.push(t('bottombar.status.queued', { n: stats.queued }));
    else return t('bottombar.status.empty');
  }
  // 仅当真实任务数超过当前可视/已渲染行数时补总数，避免「统计数字 > 可见行数」被误读
  const prefix = showTotal ? `共 ${stats.total} 项 · ` : '';
  return prefix + parts.join(' · ');
}

export type CapsuleTone = 'accent' | 'success' | 'danger' | 'neutral' | 'warning' | 'restored';

export interface CapsuleView {
  tone: CapsuleTone;
  icon: 'clock' | 'loader' | 'download' | 'pause' | 'check-circle' | 'warning' | 'close' | 'reset' | 'merge';
  text: string;
  right?: string;
  indeterminate?: boolean;
  /** 进度填充比例（0–1） */
  fill: number;
  /** 进度条是否显示填充层 */
  showFill: boolean;
}

export function capsuleView(task: QueueTask): CapsuleView {
  const { progress, status } = task;
  const percent = formatPercent(progress.percent);
  const done = formatBytes(progress.doneBytes);
  const total = formatBytes(progress.totalBytes);

  switch (status) {
    case 'queued':
      return { tone: 'neutral', icon: 'clock', text: t('status.queued'), fill: 0, showFill: false };
    case 'resolving':
      return {
        tone: 'accent',
        icon: 'loader',
        text: t('status.resolving'),
        fill: 0.04,
        showFill: true,
        indeterminate: true,
      };
    case 'downloading':
      return {
        tone: 'accent',
        icon: 'download',
        text: `${percent} · ${done} / ${total}`,
        // 速度/剩余时间放标题行右侧（见 QueueRow），胶囊内不重复
        right: undefined,
        fill: progress.percent,
        showFill: true,
      };
    case 'merging':
      return {
        tone: 'accent',
        icon: 'merge',
        text: t('status.merging'),
        fill: 0.98,
        showFill: true,
        indeterminate: true,
      };
    case 'paused':
      if (task.restored) {
        return {
          tone: 'restored',
          icon: 'reset',
          text: t('queue.restoredHint'),
          fill: progress.percent,
          showFill: false,
        };
      }
      return {
        tone: 'neutral',
        icon: 'pause',
        text: `${t('status.paused')} · ${percent}`,
        fill: progress.percent,
        showFill: false,
      };
    case 'completed':
      return {
        tone: 'success',
        icon: 'check-circle',
        text: t('status.completed'),
        fill: 1,
        showFill: false,
      };
    case 'failed':
      return {
        tone: 'danger',
        icon: 'warning',
        text: task.error ? task.error.message : t('status.failed'),
        fill: 1,
        showFill: false,
      };
    case 'cancelled':
      return {
        tone: 'neutral',
        icon: 'close',
        text: t('status.cancelled'),
        fill: progress.percent,
        showFill: false,
      };
    default:
      return { tone: 'neutral', icon: 'clock', text: '', fill: 0, showFill: false };
  }
}

export const CAPSULE_TONE_STYLE: Record<
  CapsuleTone,
  { background: string; color: string; fill?: string }
> = {
  accent: { background: 'var(--capsule-track)', color: 'var(--capsule-text)', fill: 'var(--capsule-fill)' },
  success: { background: 'var(--capsule-success)', color: '#ffffff' },
  danger: { background: 'var(--capsule-danger)', color: '#ffffff' },
  neutral: { background: 'var(--capsule-neutral)', color: 'var(--capsule-neutral-text)' },
  warning: { background: 'var(--capsule-warning)', color: 'var(--capsule-warning-text)' },
  restored: { background: 'var(--capsule-warning)', color: 'var(--capsule-warning-text)' },
};
