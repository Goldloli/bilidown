import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { api, type Backend } from '../src/lib/ipc';
import { attemptQuit, cancelQuit, forceQuit, retryQuitSave } from '../src/lib/quit';
import { useUiStore } from '../src/stores/useUiStore';
import { QuitFailureDialog } from '../src/components/QuitFailureDialog';

const SAVE_ERROR = { kind: 'disk', message: '任务状态保存失败：database is locked', actions: ['retry'] };

function setBackend(overrides: Partial<Backend>): {
  quitApp: ReturnType<typeof vi.fn>;
  forceQuitApp: ReturnType<typeof vi.fn>;
} {
  const quitApp = overrides.quitApp ?? vi.fn(async () => {});
  const forceQuitApp = overrides.forceQuitApp ?? vi.fn(async () => {});
  api.setBackend({ quitApp, forceQuitApp } as unknown as Backend);
  return { quitApp, forceQuitApp };
}

beforeEach(() => {
  useUiStore.setState({ quitFailure: null, confirmRequest: null, toasts: [] });
});

afterEach(() => {
  vi.restoreAllMocks();
  useUiStore.setState({ quitFailure: null, confirmRequest: null, toasts: [] });
});

describe('退出保存失败对话框（design D8 / tasks 3.9）', () => {
  it('正常保存成功直接退出，不增加确认或失败对话框', async () => {
    const { quitApp } = setBackend({});

    await expect(attemptQuit()).resolves.toBe(true);

    expect(quitApp).toHaveBeenCalledTimes(1);
    expect(useUiStore.getState().quitFailure).toBeNull();
    expect(useUiStore.getState().confirmRequest).toBeNull();
  });

  it('保存失败时展示可读错误，不宣称退出成功', async () => {
    setBackend({
      quitApp: vi.fn(async () => {
        throw SAVE_ERROR;
      }),
    });

    await expect(attemptQuit()).resolves.toBe(false);

    expect(useUiStore.getState().quitFailure).toBe(SAVE_ERROR.message);
  });

  it('重试保存成功后关闭对话框并退出（再次走 quit_app）', async () => {
    let attempts = 0;
    const { quitApp } = setBackend({
      quitApp: vi.fn(async () => {
        attempts += 1;
        if (attempts === 1) throw SAVE_ERROR;
      }),
    });

    await attemptQuit();
    expect(useUiStore.getState().quitFailure).toBe(SAVE_ERROR.message);

    await expect(retryQuitSave()).resolves.toBe(true);

    expect(quitApp).toHaveBeenCalledTimes(2);
    expect(useUiStore.getState().quitFailure).toBeNull();
  });

  it('「仍然退出」调用 force_quit_app 并关闭对话框', async () => {
    const { forceQuitApp } = setBackend({
      quitApp: vi.fn(async () => {
        throw SAVE_ERROR;
      }),
    });
    await attemptQuit();

    await forceQuit();

    expect(forceQuitApp).toHaveBeenCalledTimes(1);
    expect(useUiStore.getState().quitFailure).toBeNull();
  });

  it('「取消」保留应用且允许稍后重试保存', async () => {
    setBackend({
      quitApp: vi.fn(async () => {
        throw SAVE_ERROR;
      }),
    });
    await attemptQuit();

    cancelQuit();

    expect(useUiStore.getState().quitFailure).toBeNull();
  });

  it('强制退出失败时给出可读错误提示', async () => {
    setBackend({
      forceQuitApp: vi.fn(async () => {
        throw { kind: 'unknown', message: '无法退出', actions: [] };
      }),
    });

    await forceQuit();

    const toasts = useUiStore.getState().toasts;
    expect(toasts.at(-1)?.kind).toBe('error');
    expect(toasts.at(-1)?.message).toContain('无法退出');
  });
});

describe('退出保存失败对话框文案与动作', () => {
  it('展示错误原因与「重试保存 / 仍然退出 / 取消」三个动作', () => {
    const html = renderToStaticMarkup(
      <QuitFailureDialog
        message={SAVE_ERROR.message}
        busy={null}
        onRetry={() => {}}
        onForce={() => {}}
        onCancel={() => {}}
      />,
    );

    expect(html).toContain('退出前保存失败');
    expect(html).toContain(SAVE_ERROR.message);
    expect(html).toContain('重试保存');
    expect(html).toContain('仍然退出');
    expect(html).toContain('取消');
    expect(html).toContain('role="alertdialog"');
  });
});
