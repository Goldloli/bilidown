import { api } from './ipc';

/**
 * B 站远程图片 dataUrl 缓存（头像和封面带防盗链，必须由 shell 用 Referer 代理获取）。
 * 同一 URL 的并发请求合并为一次 invoke；失败返回 null，由图片组件显示占位。
 */
const remoteImageCache = new Map<string, Promise<string | null>>();

export function normalizeRemoteImageUrl(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith('//')) return `https:${trimmed}`;
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  return null;
}

export function isRemoteImageUrl(url: string | null | undefined): url is string {
  return Boolean(url && normalizeRemoteImageUrl(url));
}

export function resolveRemoteImage(url: string): Promise<string | null> {
  const normalized = normalizeRemoteImageUrl(url);
  if (!normalized) return Promise.resolve(null);
  const cached = remoteImageCache.get(normalized);
  if (cached) return cached;
  const pending = api
    .fetchCover(normalized)
    .then((data) => (typeof data === 'string' && data.length > 0 ? data : null))
    .catch(() => null)
    .then((result) => {
      // 网络故障不应永久缓存；下次用户刷新或重新挂载组件时可以重试。
      if (!result) remoteImageCache.delete(normalized);
      return result;
    });
  remoteImageCache.set(normalized, pending);
  return pending;
}

/** 兼容现有封面调用；语义与远程图片解析完全一致。 */
export function resolveCover(url: string): Promise<string | null> {
  return resolveRemoteImage(url);
}

export function clearCoverCache(): void {
  remoteImageCache.clear();
}
