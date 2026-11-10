import { useEffect, useState } from 'react';
import { Icon } from './Icon';
import { isRemoteImageUrl, resolveRemoteImage, retainRemoteImage } from '../lib/cover';

type AvatarStatus = 'loading' | 'ready' | 'error';

interface AvatarProps {
  src: string | null | undefined;
  alt?: string;
  className?: string;
  iconSize?: number;
}

/** 登录头像：远程图片经 shell 代理，加载中或失败时回退到用户图标。 */
export function Avatar({ src, alt = '', className = '', iconSize = 18 }: AvatarProps) {
  const remote = isRemoteImageUrl(src);
  const [resolved, setResolved] = useState<string | null>(remote ? null : (src ?? null));
  const [status, setStatus] = useState<AvatarStatus>(src ? 'loading' : 'error');

  useEffect(() => {
    let cancelled = false;
    if (!src) {
      setResolved(null);
      setStatus('error');
      return;
    }
    if (!isRemoteImageUrl(src)) {
      setResolved(src);
      setStatus('loading');
      return () => {
        cancelled = true;
      };
    }

    setResolved(null);
    setStatus('loading');
    void resolveRemoteImage(src).then((data) => {
      if (cancelled) return;
      if (data) {
        setResolved(data);
        setStatus('loading');
      } else {
        setStatus('error');
      }
    });
    return () => {
      cancelled = true;
    };
  }, [src]);

  // 已挂载头像保留缓存条目（tasks 7.6）。
  useEffect(() => {
    if (!remote || !resolved || status === 'error') return;
    return retainRemoteImage(src ?? '');
  }, [remote, resolved, status, src]);

  const showImage = Boolean(resolved) && status !== 'error';

  return (
    <span
      className={`grid shrink-0 place-items-center overflow-hidden rounded-full bg-accent-weak text-accent ${className}`}
      data-avatar-state={status}
    >
      {showImage ? (
        <img
          src={resolved ?? undefined}
          alt={alt}
          aria-hidden={alt ? undefined : true}
          draggable={false}
          onLoad={() => setStatus('ready')}
          onError={() => {
            setResolved(null);
            setStatus('error');
          }}
          className="h-full w-full object-cover"
        />
      ) : (
        <Icon name="user" size={iconSize} />
      )}
    </span>
  );
}
