import { t, type StringKey } from '../i18n/zh-CN';
import type {
  CodecId,
  ContentKind,
  ErrorAction,
  ErrorKind,
  TaskPhase,
  TaskStatus,
} from './types';

export const KIND_ICON: Record<ContentKind, string> = {
  video: 'play',
  bangumi: 'bangumi',
  course: 'course',
  audio: 'audio',
};

export function kindLabel(kind: ContentKind): string {
  return t(`kind.${kind}` as StringKey);
}

export function kindIcon(kind: ContentKind): string {
  return KIND_ICON[kind];
}

export function statusLabel(status: TaskStatus): string {
  return t(`status.${status}` as StringKey);
}

export type Tone = 'accent' | 'success' | 'danger' | 'neutral' | 'warning';

export function statusTone(status: TaskStatus): Tone {
  switch (status) {
    case 'downloading':
    case 'resolving':
    case 'merging':
      return 'accent';
    case 'completed':
      return 'success';
    case 'failed':
      return 'danger';
    case 'paused':
      return 'neutral';
    case 'cancelled':
      return 'neutral';
    case 'queued':
      return 'neutral';
    default:
      return 'neutral';
  }
}

export function phaseLabel(phase: TaskPhase): string {
  return t(`phase.${phase}` as StringKey);
}

export function errorKindLabel(kind: ErrorKind): string {
  return t(`error.${kind}` as StringKey);
}

export function errorActionLabel(action: ErrorAction): string {
  return t(`errorAction.${action}` as StringKey);
}

export function codecLabel(codec: CodecId): string {
  switch (codec) {
    case 'avc':
      return 'AVC';
    case 'hevc':
      return 'HEVC';
    case 'av1':
      return 'AV1';
  }
}

export function audioLabel(audio: 'original' | 'mp3'): string {
  return audio === 'mp3' ? 'MP3' : '原格式';
}

/** 清晰度短标签：480P · 60fps */
export function qualityShort(label: string, fps?: number): string {
  return fps && fps > 30 ? `${label} · ${fps}fps` : label;
}
