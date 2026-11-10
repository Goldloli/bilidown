import { useEffect, useState } from 'react';
import { Icon } from './Icon';
import { isMockMode } from '../lib/capabilities';
import { resolveCover, retainRemoteImage } from '../lib/cover';

interface ThumbnailProps {
  src: string | null | undefined;
  alt: string;
  className?: string;
  iconSize?: number;
}

/**
 * 16:9 封面：
 * · mock / 本地资源直接 <img>
 * · 真实 B 站封面（http/https）经 shell `fetch_cover` 取 dataUrl（带 Referer 与缓存）
 * · 加载中骨架、失败显示自有占位（不出现破图）
 */
export function Thumbnail({ src, alt, className, iconSize = 18 }: ThumbnailProps) {
  const remote = Boolean(src && /^https?:\/\//i.test(src));
  const needsFetch = remote && !isMockMode();
  const [resolved, setResolved] = useState<string | null>(needsFetch ? null : (src ?? null));
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>(src ? 'loading' : 'error');

  useEffect(() => {
    let cancelled = false;
    if (!src) {
      setResolved(null);
      setStatus('error');
      return;
    }
    if (!needsFetch) {
      setResolved(src);
      setStatus('loading');
      return () => {
        cancelled = true;
      };
    }
    setResolved(null);
    setStatus('loading');
    void resolveCover(src).then((data) => {
      if (cancelled) return;
      if (data) setResolved(data);
      else setStatus('error');
    });
    return () => {
      cancelled = true;
    };
  }, [src, needsFetch]);

  // 已挂载图片保留内存缓存条目，淘汰时优先选择离屏且无订阅的图片（tasks 7.6）。
  useEffect(() => {
    if (!needsFetch || !resolved || status === 'error') return;
    return retainRemoteImage(src ?? '');
  }, [needsFetch, resolved, status, src]);

  return (
    <div
      className={`relative overflow-hidden rounded-tile ${className ?? ''}`}
      style={{ background: 'var(--card-inset)' }}
    >
      {resolved && status !== 'error' && (
        <img
          src={resolved}
          alt={alt}
          draggable={false}
          loading="lazy"
          onLoad={() => setStatus('ready')}
          onError={() => setStatus('error')}
          className={`h-full w-full object-cover transition-opacity duration-[var(--dur-base)] ${
            status === 'ready' ? 'opacity-100' : 'opacity-0'
          }`}
        />
      )}
      {(status === 'error' || status === 'loading') && (
        <div className="absolute inset-0 grid place-items-center text-fg3">
          <Icon name={status === 'error' ? 'image' : 'film'} size={iconSize} />
        </div>
      )}
    </div>
  );
}
