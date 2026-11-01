import { beforeEach, describe, expect, it, vi } from 'vitest';

const { fetchCover } = vi.hoisted(() => ({
  fetchCover: vi.fn<(url: string) => Promise<string>>(),
}));

vi.mock('../src/lib/ipc', () => ({
  api: { fetchCover },
}));

import {
  clearCoverCache,
  isRemoteImageUrl,
  normalizeRemoteImageUrl,
  resolveCover,
  resolveRemoteImage,
} from '../src/lib/cover';

describe('remote image loading', () => {
  beforeEach(() => {
    clearCoverCache();
    fetchCover.mockReset();
  });

  it('recognizes and normalizes remote image URLs without accepting local or unsafe schemes', () => {
    expect(normalizeRemoteImageUrl('//i0.hdslb.com/bfs/face/a.jpg')).toBe(
      'https://i0.hdslb.com/bfs/face/a.jpg',
    );
    expect(normalizeRemoteImageUrl(' https://i0.hdslb.com/a.jpg ')).toBe(
      'https://i0.hdslb.com/a.jpg',
    );
    expect(normalizeRemoteImageUrl('http://i0.hdslb.com/a.jpg')).toBe(
      'http://i0.hdslb.com/a.jpg',
    );
    expect(normalizeRemoteImageUrl('/avatars/mock.svg')).toBeNull();
    expect(normalizeRemoteImageUrl('data:image/png;base64,AA')).toBeNull();
    expect(isRemoteImageUrl('/avatars/mock.svg')).toBe(false);
    expect(isRemoteImageUrl('javascript:alert(1)')).toBe(false);
  });

  it('deduplicates concurrent requests for the same normalized URL', async () => {
    fetchCover.mockResolvedValue('data:image/jpeg;base64,AA==');
    const first = resolveRemoteImage('//i0.hdslb.com/bfs/face/a.jpg');
    const second = resolveCover('https://i0.hdslb.com/bfs/face/a.jpg');

    await expect(first).resolves.toBe('data:image/jpeg;base64,AA==');
    await expect(second).resolves.toBe('data:image/jpeg;base64,AA==');
    expect(fetchCover).toHaveBeenCalledTimes(1);
    expect(fetchCover).toHaveBeenCalledWith('https://i0.hdslb.com/bfs/face/a.jpg');
  });

  it('shares a concurrent failure but allows a later retry', async () => {
    fetchCover.mockRejectedValue(new Error('cdn blocked'));

    const first = resolveRemoteImage('https://i0.hdslb.com/a.jpg');
    const concurrent = resolveRemoteImage('https://i0.hdslb.com/a.jpg');
    await expect(first).resolves.toBeNull();
    await expect(concurrent).resolves.toBeNull();
    expect(fetchCover).toHaveBeenCalledTimes(1);

    await expect(resolveRemoteImage('https://i0.hdslb.com/a.jpg')).resolves.toBeNull();
    expect(fetchCover).toHaveBeenCalledTimes(2);
  });
});
