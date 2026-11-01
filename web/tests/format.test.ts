import { describe, expect, it } from 'vitest';
import {
  formatBytes,
  formatDateGroup,
  formatDateTime,
  formatDuration,
  formatEta,
  formatPercent,
  formatSpeed,
} from '../src/lib/format';

describe('format（统一中文数字/时间格式化）', () => {
  it('字节格式化：空值、KB/MB/GB 与一位小数', () => {
    expect(formatBytes(null)).toBe('—');
    expect(formatBytes(0)).toBe('—');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1024)).toBe('1 KB');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(1024 ** 2 * 27.8)).toBe('27.8 MB');
    expect(formatBytes(1024 ** 3 * 1.24)).toBe('1.2 GB');
  });

  it('速度格式化', () => {
    expect(formatSpeed(0)).toBe('—');
    expect(formatSpeed(1024 * 1024 * 6.2)).toBe('6.2 MB/s');
  });

  it('时长与 ETA', () => {
    expect(formatDuration(42)).toBe('42 秒');
    expect(formatDuration(192)).toBe('3 分 12 秒');
    expect(formatDuration(3720)).toBe('1 小时 2 分');
    expect(formatEta(192)).toBe('剩余 3 分 12 秒');
    expect(formatEta(0.4)).toBe('即将完成');
    expect(formatEta(null)).toBe('—');
  });

  it('百分比钳制', () => {
    expect(formatPercent(0.452, 1)).toBe('45.2%');
    expect(formatPercent(1.4)).toBe('100%');
    expect(formatPercent(-1)).toBe('0%');
  });

  it('日期分组与时间文案（今天/昨天/同年/跨年）', () => {
    const now = new Date(2026, 9, 29, 15, 0, 0).getTime();
    const today = new Date(2026, 9, 29, 14, 5, 0).getTime();
    const yesterday = new Date(2026, 9, 28, 9, 20, 0).getTime();
    const earlier = new Date(2026, 9, 26, 8, 0, 0).getTime();
    const lastYear = new Date(2025, 11, 1, 8, 0, 0).getTime();

    expect(formatDateGroup(today, now)).toBe('今天');
    expect(formatDateGroup(yesterday, now)).toBe('昨天');
    expect(formatDateGroup(earlier, now)).toBe('2026年10月26日');
    expect(formatDateTime(today, now)).toBe('今天 14:05');
    expect(formatDateTime(yesterday, now)).toBe('昨天 09:20');
    expect(formatDateTime(earlier, now)).toBe('10月26日 08:00');
    expect(formatDateTime(lastYear, now)).toBe('2025年12月1日');
  });
});
