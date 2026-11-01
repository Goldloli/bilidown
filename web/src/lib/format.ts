/** 统一的中文数字/时间格式化（ui-theme spec「文本与数字格式化」） */

const KB = 1024;

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return '—';
  if (bytes < KB) return `${Math.round(bytes)} B`;
  if (bytes < KB * KB) return `${trim1(bytes / KB)} KB`;
  if (bytes < KB * KB * KB) return `${trim1(bytes / (KB * KB))} MB`;
  return `${trim1(bytes / (KB * KB * KB))} GB`;
}

export function formatSpeed(bps: number | null | undefined): string {
  if (bps == null || !Number.isFinite(bps) || bps <= 0) return '—';
  return `${formatBytes(bps)}/s`;
}

export function formatDuration(sec: number | null | undefined): string {
  if (sec == null || !Number.isFinite(sec) || sec <= 0) return '—';
  const total = Math.round(sec);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h} 小时 ${m} 分`;
  if (m > 0) return s > 0 ? `${m} 分 ${s} 秒` : `${m} 分`;
  return `${s} 秒`;
}

export function formatEta(sec: number | null | undefined): string {
  if (sec == null || !Number.isFinite(sec) || sec < 0) return '—';
  if (sec < 1) return '即将完成';
  const total = Math.round(sec);
  if (total < 60) return `剩余 ${total} 秒`;
  const m = Math.floor(total / 60);
  const s = total % 60;
  if (m < 60) return `剩余 ${m} 分${s > 0 ? ` ${s} 秒` : ''}`;
  const h = Math.floor(m / 60);
  return `剩余 ${h} 小时 ${m % 60} 分`;
}

export function formatPercent(percent: number | null | undefined, digits = 0): string {
  if (percent == null || !Number.isFinite(percent)) return '—';
  return `${(Math.max(0, Math.min(1, percent)) * 100).toFixed(digits)}%`;
}

export function formatCount(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—';
  if (n >= 100000000) return `${trim1(n / 100000000)} 亿`;
  if (n >= 10000) return `${trim1(n / 10000)} 万`;
  return `${n}`;
}

export function formatWordCount(n: number | null | undefined): string {
  if (n == null) return '—';
  return `${n} 字`;
}

function trim1(v: number): string {
  return v >= 100 ? `${Math.round(v)}` : v.toFixed(1).replace(/\.0$/, '');
}

function startOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function dayDiff(ts: number, now = Date.now()): number {
  return Math.round((startOfDay(now) - startOfDay(ts)) / 86400000);
}

/** 「今天 14:05」「昨天 09:20」「2026年10月27日」 */
export function formatDateTime(ts: number, now = Date.now()): string {
  const diff = dayDiff(ts, now);
  const d = new Date(ts);
  const hm = `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
  if (diff === 0) return `今天 ${hm}`;
  if (diff === 1) return `昨天 ${hm}`;
  if (d.getFullYear() === new Date(now).getFullYear()) {
    return `${d.getMonth() + 1}月${d.getDate()}日 ${hm}`;
  }
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
}

/** 历史日期分组标题 */
export function formatDateGroup(ts: number, now = Date.now()): string {
  const diff = dayDiff(ts, now);
  if (diff === 0) return '今天';
  if (diff === 1) return '昨天';
  const d = new Date(ts);
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
}

export function pad2(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}
