import type { AudioFormatOption, CodecOption, QualityOption } from './types';

/** 清晰度档位（B 站 playurl 常见 qn；实际可用档位由接口返回决定，不虚构） */
export const QUALITY_TIERS: Array<Omit<QualityOption, 'available' | 'unavailableReason'>> = [
  { id: 127, label: '8K', resolution: '7680×4320', fps: 30, vip: true },
  { id: 126, label: '杜比视界', resolution: '3840×2160', fps: 60, vip: true },
  { id: 125, label: 'HDR', resolution: '3840×2160', fps: 60, vip: true },
  { id: 120, label: '4K', resolution: '3840×2160', fps: 60, vip: true },
  { id: 116, label: '1080P60', resolution: '1920×1080', fps: 60, vip: true },
  { id: 112, label: '1080P+', resolution: '1920×1080', fps: 60, vip: true },
  { id: 80, label: '1080P', resolution: '1920×1080', fps: 30, vip: false },
  { id: 74, label: '720P60', resolution: '1280×720', fps: 60, vip: false },
  { id: 64, label: '720P', resolution: '1280×720', fps: 30, vip: false },
  { id: 32, label: '480P', resolution: '854×480', fps: 30, vip: false },
  { id: 16, label: '360P', resolution: '640×360', fps: 30, vip: false },
];

/** 音质档位（DASH audio id；实际可用档位仍由接口返回决定） */
export const AUDIO_QUALITY_TIERS: Array<{ id: number; label: string }> = [
  { id: 30252, label: 'Hi-Res' },
  { id: 30251, label: 'FLAC' },
  { id: 30250, label: '杜比全景声' },
  { id: 30280, label: '192K' },
  { id: 30232, label: '132K' },
  { id: 30228, label: '128K' },
  { id: 30216, label: '64K' },
];

export const APP_VERSION = '2.4.1';
export const APP_BUILD = 'Tauri 2 · React 19';

export const VIDEO_CODECS: CodecOption[] = [
  { id: 'avc', label: 'AVC', available: true },
  { id: 'hevc', label: 'HEVC', available: true },
  { id: 'av1', label: 'AV1', available: false },
];

export const AUDIO_FORMATS: AudioFormatOption[] = [
  { id: 'original', label: '原格式', available: true, hint: 'm4a / flac' },
  { id: 'mp3', label: 'MP3', available: true, hint: '需 ffmpeg' },
];

/** 默认能力标记（capability 字符串 → qn 数值用） */
export const CAPABILITY_LABELS = { anonymousMax: '480P', signedInMax: '1080P', vipMax: '4K' } as const;
