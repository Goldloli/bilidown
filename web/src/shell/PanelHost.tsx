import { useAsyncComponent } from '../lib/asyncComponent';
import { Skeleton } from '../components/Skeleton';
import { useUiStore } from '../stores/useUiStore';

/** 面板加载态：轻量覆盖层，避免打开面板时闪现空白（tasks 8.1/8.2）。 */
function PanelFallback() {
  return (
    <div className="absolute inset-0 z-40 grid place-items-center" style={{ background: 'var(--overlay)' }}>
      <div
        className="w-[min(560px,86vw)] rounded-sheet border p-4"
        style={{ background: 'var(--bg-elevated)', borderColor: 'var(--border-subtle)' }}
      >
        <Skeleton className="mb-3 h-4 w-40" />
        <Skeleton className="mb-2 h-8 w-full" />
        <Skeleton className="h-8 w-full" />
      </div>
    </div>
  );
}

function PanelError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className="absolute inset-0 z-40 grid place-items-center" style={{ background: 'var(--overlay)' }}>
      <div
        className="flex w-[min(420px,86vw)] flex-col items-center gap-3 rounded-sheet border p-5 text-center"
        style={{ background: 'var(--bg-elevated)', borderColor: 'var(--border-subtle)' }}
      >
        <p className="text-[13px]">面板加载失败：{message}</p>
        <button
          type="button"
          className="rounded-md px-3 py-1 text-[12px]"
          style={{ background: 'var(--accent)', color: 'var(--accent-fg)' }}
          onClick={onRetry}
        >
          重新加载
        </button>
      </div>
    </div>
  );
}

const loadAddLink = () => import('../panels/AddLinkPanel').then((module) => module.AddLinkPanel);
const loadAdd = () => import('../panels/AddPanel').then((module) => module.AddPanel);
const loadLogin = () => import('../panels/LoginPanel').then((module) => module.LoginPanel);
const loadDetail = () =>
  import('../panels/TaskDetailPanel').then((module) => module.TaskDetailPanel);

/** 主窗口覆盖层面板宿主（同一时刻只允许一个面板）；面板代码按需加载。 */
export function PanelHost() {
  const panel = useUiStore((s) => s.panel);
  const closePanel = useUiStore((s) => s.closePanel);

  const addLink = useAsyncComponent(loadAddLink, panel?.type === 'add-link');
  const add = useAsyncComponent(loadAdd, panel?.type === 'add');
  const login = useAsyncComponent(loadLogin, panel?.type === 'login');
  const detail = useAsyncComponent(loadDetail, panel?.type === 'detail');

  if (!panel) return null;

  switch (panel.type) {
    case 'add-link': {
      const Page = addLink.Page;
      if (Page) return <Page open initialText={panel.initialText} onClose={closePanel} />;
      return addLink.error ? (
        <PanelError message={addLink.error} onRetry={addLink.retry} />
      ) : (
        <PanelFallback />
      );
    }
    case 'add': {
      const Page = add.Page;
      if (Page) return <Page parse={panel.parse} onClose={closePanel} />;
      return add.error ? (
        <PanelError message={add.error} onRetry={add.retry} />
      ) : (
        <PanelFallback />
      );
    }
    case 'detail': {
      const Page = detail.Page;
      if (Page) return <Page taskId={panel.taskId} onClose={closePanel} />;
      return detail.error ? (
        <PanelError message={detail.error} onRetry={detail.retry} />
      ) : (
        <PanelFallback />
      );
    }
    case 'login': {
      const Page = login.Page;
      if (Page) return <Page onClose={closePanel} />;
      return login.error ? (
        <PanelError message={login.error} onRetry={login.retry} />
      ) : (
        <PanelFallback />
      );
    }
    default:
      return null;
  }
}
