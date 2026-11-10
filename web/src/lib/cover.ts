import { api } from './ipc';

/**
 * B 站远程图片（头像/封面）dataUrl 缓存。
 *
 * design D11 / tasks 7.6：
 * · 已完成图片限制为 128 项且估算 64 MiB（data URL 按字符串长度保守估算）；
 * · LRU 优先淘汰离屏且无组件订阅的条目，被挂载引用的条目最后淘汰；
 * · 同一 URL 的在途请求合并；图片网络请求最多并行 8 个；
 * · 失败不进入成功缓存（下次可重试）；磁盘缓存命中由 shell 负责，失效后仍可再次取回；
 * · 收到 `covers://cleared` 后失效内存条目（清理时同步失效，见 bootstrap）。
 */
export const REMOTE_IMAGE_MAX_ENTRIES = 128;
export const REMOTE_IMAGE_MAX_BYTES = 64 * 1024 * 1024;
export const REMOTE_IMAGE_MAX_CONCURRENT = 8;

interface CacheEntry {
  data: string;
  /** 保守估算占用的字节数（data URL 字符串长度） */
  bytes: number;
}

const entries = new Map<string, CacheEntry>();
/** Map 插入顺序即 LRU 顺序（最旧在前）；命中时重新插入到末尾 */
const inflight = new Map<string, Promise<string | null>>();
/** 当前被组件持有（挂载中）的 URL 计数：优先淘汰未订阅条目 */
const retainCounts = new Map<string, number>();
const slotWaiters: Array<() => void> = [];
let activeRequests = 0;
let cachedBytes = 0;
let cacheEpoch = 0;

export interface CoverCacheStats {
  entries: number;
  bytes: number;
  /** 在途请求数 */
  pending: number;
  /** 正在执行的图片网络请求数（≤ REMOTE_IMAGE_MAX_CONCURRENT） */
  activeRequests: number;
}

/** data URL 按字符串长度保守估算（不按解码后像素大小，避免低估内存）。 */
function estimateBytes(data: string): number {
  return data.length;
}

function evictOne(): boolean {
  // 优先淘汰离屏（未挂载）且无订阅的条目，按 LRU 顺序。
  for (const url of entries.keys()) {
    if ((retainCounts.get(url) ?? 0) === 0) {
      removeEntry(url);
      return true;
    }
  }
  // 全部被组件引用时仍要守住预算；组件自身仍持有 data URL，不谎称内存已释放。
  const oldest = entries.keys().next().value;
  if (oldest === undefined) return false;
  removeEntry(oldest);
  return true;
}

function removeEntry(url: string): void {
  const entry = entries.get(url);
  if (!entry) return;
  entries.delete(url);
  cachedBytes = Math.max(0, cachedBytes - entry.bytes);
}

function overBudget(): boolean {
  return entries.size > REMOTE_IMAGE_MAX_ENTRIES || cachedBytes > REMOTE_IMAGE_MAX_BYTES;
}

function storeEntry(url: string, data: string): void {
  removeEntry(url);
  entries.set(url, { data, bytes: estimateBytes(data) });
  cachedBytes += estimateBytes(data);
  while (overBudget() && evictOne()) {
    /* 逐条淘汰直到回到预算内 */
  }
}

function touch(url: string): string | null {
  const entry = entries.get(url);
  if (!entry) return null;
  entries.delete(url);
  entries.set(url, entry);
  return entry.data;
}

function acquireSlot(): Promise<void> {
  if (activeRequests < REMOTE_IMAGE_MAX_CONCURRENT) {
    activeRequests += 1;
    return Promise.resolve();
  }
  return new Promise<void>((resolve) => {
    slotWaiters.push(() => {
      activeRequests += 1;
      resolve();
    });
  });
}

function releaseSlot(): void {
  activeRequests = Math.max(0, activeRequests - 1);
  const next = slotWaiters.shift();
  if (next) next();
}

function load(normalized: string): Promise<string | null> {
  let pending!: Promise<string | null>;
  pending = (async () => {
    await acquireSlot();
    const epoch = cacheEpoch;
    try {
      const data = await api.fetchCover(normalized);
      const value = typeof data === 'string' && data.length > 0 ? data : null;
      // 缓存清理之后才返回的旧请求不得写回已失效的内存缓存。
      if (value && epoch === cacheEpoch) storeEntry(normalized, value);
      return value;
    } catch {
      // 网络/CDN 失败不进入成功缓存，下次挂载或刷新时会重试。
      return null;
    } finally {
      if (inflight.get(normalized) === pending) inflight.delete(normalized);
      releaseSlot();
    }
  })();
  inflight.set(normalized, pending);
  return pending;
}

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

/**
 * 解析远程图片为 dataUrl：命中内存缓存直接返回，同 URL 在途请求合并，
 * 失败返回 null（不缓存失败）。
 */
export function resolveRemoteImage(url: string): Promise<string | null> {
  const normalized = normalizeRemoteImageUrl(url);
  if (!normalized) return Promise.resolve(null);
  const cached = touch(normalized);
  if (cached !== null) return Promise.resolve(cached);
  const pending = inflight.get(normalized);
  if (pending) return pending;
  return load(normalized);
}

/** 兼容现有封面调用；语义与远程图片解析完全一致。 */
export function resolveCover(url: string): Promise<string | null> {
  return resolveRemoteImage(url);
}

/**
 * 组件挂载期间保留该 URL 的缓存条目（淘汰时避让）；返回幂等释放函数。
 * 只对已在缓存中的条目计数：卸载后条目可被优先淘汰。
 */
export function retainRemoteImage(url: string): () => void {
  const normalized = normalizeRemoteImageUrl(url);
  if (!normalized || !entries.has(normalized)) return () => {};
  retainCounts.set(normalized, (retainCounts.get(normalized) ?? 0) + 1);
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const count = (retainCounts.get(normalized) ?? 0) - 1;
    if (count > 0) retainCounts.set(normalized, count);
    else retainCounts.delete(normalized);
  };
}

/** 清理内存缓存（`covers://cleared`）：在途旧结果不会再写回。 */
export function clearCoverCache(): void {
  entries.clear();
  cachedBytes = 0;
  cacheEpoch += 1;
}

/** 调试/测试用：当前缓存规模与在途请求数。 */
export function coverCacheStats(): CoverCacheStats {
  return {
    entries: entries.size,
    bytes: cachedBytes,
    pending: inflight.size,
    activeRequests,
  };
}

/** 测试用：清空缓存、在途与订阅计数（不改动正在返回的 Promise 结果）。 */
export function resetCoverCacheForTests(): void {
  clearCoverCache();
  retainCounts.clear();
  inflight.clear();
  slotWaiters.length = 0;
  activeRequests = 0;
}
