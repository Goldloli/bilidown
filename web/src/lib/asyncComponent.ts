import { useEffect, useState, type ComponentType } from 'react';

/**
 * 页面/面板代码的异步加载（tasks 8.1/8.2）：失败可重试，且不复用被拒绝的
 * React.lazy 缓存；未启用时保持空闲。
 */
export function useAsyncComponent<P extends object>(
  loader: () => Promise<ComponentType<P>>,
  enabled = true,
) {
  const [state, setState] = useState<{
    Page: ComponentType<P> | null;
    error: string | null;
  }>({ Page: null, error: null });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    setState((current) => ({ ...current, error: null }));
    loader().then(
      (Page) => {
        if (alive) setState({ Page, error: null });
      },
      (error: unknown) => {
        if (!alive) return;
        setState({
          Page: null,
          error: error instanceof Error ? error.message : String(error),
        });
      },
    );
    return () => {
      alive = false;
    };
    // loader 为模块级稳定函数；enabled/attempt 变化时重新加载。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, attempt]);

  return {
    Page: state.Page,
    error: state.error,
    retry: () => setAttempt((value) => value + 1),
  };
}
