import { describe, expect, it, vi } from 'vitest';
import { createApiClient, type Backend } from '../src/lib/ipc';
import type { ClearOutcomeDTO } from '../src/lib/contract';

function backendWith(overrides: Partial<Backend>): Backend {
  return {
    clearFinished: async (): Promise<ClearOutcomeDTO> => ({ removed: 0, failed: [] }),
    clearInactive: async (): Promise<ClearOutcomeDTO> => ({ removed: 0, failed: [] }),
    retryPersistence: async () => {},
    forceQuitApp: async () => {},
    quitApp: async () => {},
    ...overrides,
  } as unknown as Backend;
}

function loggedClient(backend: Backend) {
  const client = createApiClient();
  client.setBackend(backend);
  return client;
}

describe('批量清理契约（clear_finished / clear_inactive）', () => {
  it('返回真实移除数与失败 id，不把失败当成功', async () => {
    const clearFinished = vi.fn(async () => ({ removed: 2, failed: ['task-x'] }));
    const client = loggedClient(backendWith({ clearFinished }));
    const outcome = await client.clearCompleted();
    expect(outcome).toEqual({ removed: 2, failed: ['task-x'] });
  });

  it('clear_inactive 使用同一结构化返回并清理未激活任务', async () => {
    const clearInactive = vi.fn(async () => ({ removed: 3, failed: [] }));
    const client = loggedClient(backendWith({ clearInactive }));

    const outcome = await client.clearInactive();

    expect(outcome).toEqual({ removed: 3, failed: [] });
    expect(clearInactive).toHaveBeenCalledTimes(1);
  });

  it('缺失 failed 字段时按空数组容错', async () => {
    const client = loggedClient(
      backendWith({ clearInactive: async () => ({ removed: 1 }) as ClearOutcomeDTO }),
    );
    await expect(client.clearInactive()).resolves.toEqual({ removed: 1, failed: [] });
  });
});

describe('持久化重试与强制退出命令', () => {
  it('retryPersistence 只调用 retry_persistence 命令，不触发重新下载', async () => {
    const retryPersistence = vi.fn(async () => {});
    const retryTask = vi.fn(async () => {});
    const client = loggedClient(backendWith({ retryPersistence, retryTask } as Partial<Backend>));

    await client.retryPersistence();

    expect(retryPersistence).toHaveBeenCalledTimes(1);
    expect(retryTask).not.toHaveBeenCalled();
  });

  it('forceQuitApp 调用 force_quit_app', async () => {
    const forceQuitApp = vi.fn(async () => {});
    const client = loggedClient(backendWith({ forceQuitApp }));
    await client.forceQuitApp();
    expect(forceQuitApp).toHaveBeenCalledTimes(1);
  });
});
