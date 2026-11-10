import { beforeEach, describe, expect, it, vi } from 'vitest';

const { fetchCover } = vi.hoisted(() => ({
  fetchCover: vi.fn<(url: string) => Promise<string>>(),
}));

vi.mock('../src/lib/ipc', () => ({
  api: { fetchCover },
}));

import {
  coverCacheStats,
  clearCoverCache,
  REMOTE_IMAGE_MAX_BYTES,
  REMOTE_IMAGE_MAX_CONCURRENT,
  REMOTE_IMAGE_MAX_ENTRIES,
  resolveRemoteImage,
  resetCoverCacheForTests,
  retainRemoteImage,
} from '../src/lib/cover';

const MB = 1024 * 1024;

describe('图片内存缓存预算（design D11 / tasks 7.6）', () => {
  beforeEach(() => {
    resetCoverCacheForTests();
    fetchCover.mockReset();
  });

  it('条数上限 128：继续加载会按 LRU 淘汰最旧条目', async () => {
    fetchCover.mockImplementation(async () => 'x');
    for (let index = 0; index < REMOTE_IMAGE_MAX_ENTRIES + 12; index += 1) {
      await resolveRemoteImage(`https://i.example/cover-${index}.jpg`);
    }
    expect(coverCacheStats().entries).toBe(REMOTE_IMAGE_MAX_ENTRIES);

    // 最旧的条目已被淘汰：再次访问会重新请求（磁盘缓存仍由 shell 命中）。
    await resolveRemoteImage('https://i.example/cover-0.jpg');
    expect(fetchCover).toHaveBeenLastCalledWith('https://i.example/cover-0.jpg');
    expect(fetchCover).toHaveBeenCalledTimes(REMOTE_IMAGE_MAX_ENTRIES + 13);
  });

  it('估算 64 MiB 上限：按 data URL 字符串长度保守计算', async () => {
    const oneMiB = 'x'.repeat(MB);
    fetchCover.mockResolvedValue(oneMiB);
    for (let index = 0; index < 70; index += 1) {
      await resolveRemoteImage(`https://i.example/big-${index}.jpg`);
    }
    const stats = coverCacheStats();
    expect(stats.bytes).toBeLessThanOrEqual(REMOTE_IMAGE_MAX_BYTES);
    expect(stats.entries).toBe(64);
  });

  it('优先淘汰离屏且无订阅的条目；挂载中的图片保留显示', async () => {
    fetchCover.mockImplementation(async (url) => `data:${url}`);
    await resolveRemoteImage('https://i.example/pinned.jpg');
    const release = retainRemoteImage('https://i.example/pinned.jpg');

    for (let index = 0; index < REMOTE_IMAGE_MAX_ENTRIES + 4; index += 1) {
      await resolveRemoteImage(`https://i.example/other-${index}.jpg`);
    }
    const callsAfterOthers = fetchCover.mock.calls.length;

    // 被组件订阅的条目不会被淘汰：直接命中内存缓存
    await expect(resolveRemoteImage('https://i.example/pinned.jpg')).resolves.toBe(
      'data:https://i.example/pinned.jpg',
    );
    expect(fetchCover.mock.calls.length).toBe(callsAfterOthers);

    // 释放订阅后不再是「保留条目」：随着 LRU 推进最终会被淘汰并重新请求
    release();
    for (let index = 0; index < REMOTE_IMAGE_MAX_ENTRIES; index += 1) {
      await resolveRemoteImage(`https://i.example/after-release-${index}.jpg`);
    }
    await resolveRemoteImage('https://i.example/pinned.jpg');
    expect(fetchCover.mock.calls.length).toBe(
      callsAfterOthers + REMOTE_IMAGE_MAX_ENTRIES + 1,
    );
  });

  it('失败不进入成功缓存，后续可重试；磁盘再次命中可用', async () => {
    fetchCover.mockRejectedValueOnce(new Error('cdn blocked'));
    await expect(resolveRemoteImage('https://i.example/retry.jpg')).resolves.toBeNull();
    expect(coverCacheStats().entries).toBe(0);

    fetchCover.mockResolvedValueOnce('data:image/jpeg;base64,AA==');
    await expect(resolveRemoteImage('https://i.example/retry.jpg')).resolves.toBe(
      'data:image/jpeg;base64,AA==',
    );
    expect(coverCacheStats().entries).toBe(1);
    expect(fetchCover).toHaveBeenCalledTimes(2);

    // 命中内存缓存后不再请求
    await resolveRemoteImage('https://i.example/retry.jpg');
    expect(fetchCover).toHaveBeenCalledTimes(2);
  });

  it('清理内存缓存后重新从磁盘/网络取回，且清理前在途结果不回写', async () => {
    fetchCover.mockResolvedValueOnce('data:first');
    await resolveRemoteImage('https://i.example/cleared.jpg');
    expect(coverCacheStats().entries).toBe(1);

    fetchCover.mockResolvedValueOnce('data:second');
    clearCoverCache();
    expect(coverCacheStats()).toMatchObject({ entries: 0, bytes: 0, pending: 0 });

    await expect(resolveRemoteImage('https://i.example/cleared.jpg')).resolves.toBe('data:second');
    expect(fetchCover).toHaveBeenCalledTimes(2);
    expect(coverCacheStats().entries).toBe(1);
  });

  it('同一 URL 在途请求合并，图片网络请求最多并行 8 个', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    fetchCover.mockImplementation(async () => {
      await gate;
      return 'data:x';
    });

    const urls = Array.from({ length: REMOTE_IMAGE_MAX_CONCURRENT + 4 }, (_unused, index) => {
      return `https://i.example/parallel-${index}.jpg`;
    });
    // 同 URL 的并发请求只发起一次网络请求
    const deduped = resolveRemoteImage(urls[0]);
    expect(resolveRemoteImage(urls[0])).toBe(deduped);

    const pending = urls.map((url) => resolveRemoteImage(url));
    await vi.waitFor(() => expect(fetchCover).toHaveBeenCalledTimes(REMOTE_IMAGE_MAX_CONCURRENT));
    expect(coverCacheStats().activeRequests).toBe(REMOTE_IMAGE_MAX_CONCURRENT);

    release();
    await Promise.all(pending);
    expect(fetchCover).toHaveBeenCalledTimes(urls.length);
    expect(coverCacheStats().activeRequests).toBe(0);
  });
});
