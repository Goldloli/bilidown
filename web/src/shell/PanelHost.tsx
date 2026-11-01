import { useUiStore } from '../stores/useUiStore';
import { AddLinkPanel } from '../panels/AddLinkPanel';
import { AddPanel } from '../panels/AddPanel';
import { LoginPanel } from '../panels/LoginPanel';
import { TaskDetailPanel } from '../panels/TaskDetailPanel';

/** 主窗口覆盖层面板宿主（同一时刻只允许一个面板） */
export function PanelHost() {
  const panel = useUiStore((s) => s.panel);
  const closePanel = useUiStore((s) => s.closePanel);

  switch (panel?.type) {
    case 'add-link':
      return (
        <AddLinkPanel
          open
          initialText={panel.initialText}
          onClose={closePanel}
        />
      );
    case 'add':
      return <AddPanel parse={panel.parse} onClose={closePanel} />;
    case 'detail':
      return <TaskDetailPanel taskId={panel.taskId} onClose={closePanel} />;
    case 'login':
      return <LoginPanel onClose={closePanel} />;
    default:
      return null;
  }
}
