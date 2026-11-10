/** 启动初始化失败：可读错误 + 重试（tasks 5.1/5.3，替换旧的 innerHTML 拼接路径）。 */
export function BootError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-3 px-10 text-center"
      style={{ background: 'var(--bg-window)', color: 'var(--text-primary)' }}
    >
      <p className="text-[13px]">引擎初始化失败：{message}</p>
      <p className="text-[12px]" style={{ color: 'var(--text-secondary)' }}>
        可先重试；若持续失败，请检查网络或数据目录权限后重启应用。
      </p>
      <button
        type="button"
        className="rounded-md px-3 py-1 text-[12px]"
        style={{ background: 'var(--accent)', color: 'var(--accent-fg)' }}
        onClick={onRetry}
      >
        重新连接
      </button>
    </div>
  );
}
