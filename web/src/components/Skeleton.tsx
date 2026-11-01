interface SkeletonProps {
  className?: string;
  rounded?: string;
}

/** 骨架条（不做扫光，避免 Web 审美残留；仅低对比脉动） */
export function Skeleton({ className, rounded = 'rounded-md' }: SkeletonProps) {
  return (
    <div
      aria-hidden="true"
      className={`animate-pulse ${rounded} ${className ?? ''}`}
      style={{ background: 'var(--rail-bg)' }}
    />
  );
}

export function QueueRowSkeleton() {
  return (
    <div
      className="flex items-center gap-3 rounded-card p-2"
      style={{ background: 'var(--card)', boxShadow: '0 0 0 1px var(--hairline)' }}
    >
      <Skeleton className="h-[81px] w-[144px]" rounded="rounded-tile" />
      <div className="flex-1 space-y-2">
        <Skeleton className="h-3.5 w-2/3" />
        <Skeleton className="h-2.5 w-1/3" />
        <Skeleton className="h-6 w-full" rounded="rounded-full" />
      </div>
    </div>
  );
}
