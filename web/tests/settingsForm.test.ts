import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '../src/lib/ipc';
import { defaultInternalSettings } from '../src/lib/adapt';
import type { Settings } from '../src/lib/types';
import { useSettingsStore } from '../src/stores/useSettingsStore';
import {
  decideSettingsClose,
  installPreferencesCloseHandler,
  type ClosablePreferencesWindow,
  type CloseFlowDeps,
} from '../src/settings/closeFlow';
import { createSettingsFake, deferred, type SettingsFake } from './settingsFake';

async function setup(fake: SettingsFake): Promise<void> {
  api.setBackend(fake.backend);
  useSettingsStore.getState().reset();
  await useSettingsStore.getState().reload();
}

function closeDeps(): CloseFlowDeps {
  return {
    hasDirtyDraft: () => useSettingsStore.getState().hasDirtyDraft(),
    issues: () => useSettingsStore.getState().issues,
    saveDirtyDraft: () => useSettingsStore.getState().retrySave(),
    waitForSaves: () => useSettingsStore.getState().waitForSaves(),
    saveError: () => useSettingsStore.getState().saveError,
  };
}

afterEach(() => {
  useSettingsStore.getState().reset();
  vi.unstubAllGlobals();
});

describe('草稿与提交分离（design D4 / tasks 4.5）', () => {
  it('编辑只改草稿不提交；显式保存才提交有效补丁', async () => {
    const fake = createSettingsFake();
    await setup(fake);
    const store = useSettingsStore.getState();

    store.edit({ quality: { ffmpegPath: '/tmp/ffmpeg' } });
    expect(fake.patchCalls).toHaveLength(0);
    expect(useSettingsStore.getState().draft?.quality.ffmpegPath).toBe('/tmp/ffmpeg');
    expect(useSettingsStore.getState().settings?.quality.ffmpegPath).not.toBe('/tmp/ffmpeg');

    expect(await useSettingsStore.getState().save({ quality: { ffmpegPath: '/tmp/ffmpeg' } })).toBe(
      true,
    );
    expect(fake.patchCalls).toHaveLength(1);
    expect(fake.patchCalls[0].ffmpegPath).toBe('/tmp/ffmpeg');
    expect(useSettingsStore.getState().settings?.quality.ffmpegPath).toBe('/tmp/ffmpeg');
  });

  it('无效未提交草稿不替换最后有效设置，也不标记为已保存', async () => {
    const fake = createSettingsFake();
    await setup(fake);
    const store = useSettingsStore.getState();

    useSettingsStore.getState().edit({ network: { proxyMode: 'manual', proxyHost: '' } });
    expect(useSettingsStore.getState().issues.some((issue) => issue.path === 'network.proxyHost')).toBe(
      true,
    );

    const ok = await useSettingsStore.getState().save({
      network: { proxyMode: 'manual', proxyHost: '', proxyPort: 7890, proxyUser: '', proxyPass: '' },
    });

    expect(ok).toBe(false);
    expect(fake.patchCalls).toHaveLength(0);
    let state = useSettingsStore.getState();
    expect(state.settings?.network.proxyMode).toBe('direct');
    expect(state.lastSavedAt).toBeNull();
    expect(state.draft?.network.proxyMode).toBe('manual');
    expect(state.saveError).toBe('请输入代理地址');

    // 补全有效草稿后保存成功，全局错误清除。
    state.edit({ network: { proxyHost: '127.0.0.1', proxyPort: 7890 } });
    expect(
      await useSettingsStore.getState().save({
        network: { proxyMode: 'manual', proxyHost: '127.0.0.1', proxyPort: 7890, proxyUser: '', proxyPass: '' },
      }),
    ).toBe(true);
    state = useSettingsStore.getState();
    expect(state.saveError).toBeNull();
    expect(state.settings?.network.proxyMode).toBe('manual');
    expect(state.settings?.network.proxyHost).toBe('127.0.0.1');
    void store;
  });

  it('保存失败保留草稿与统一错误，重试成功后才更新有效设置；放弃修改恢复有效设置', async () => {
    const fake = createSettingsFake({ patchError: '设置写盘失败' });
    await setup(fake);

    useSettingsStore.getState().edit({ interface: { appearance: 'dark' } });
    expect(await useSettingsStore.getState().save({ interface: { appearance: 'dark' } })).toBe(false);

    let state = useSettingsStore.getState();
    expect(state.saveError).toBe('设置写盘失败');
    expect(state.settings?.interface.appearance).toBe('system');
    expect(state.draft?.interface.appearance).toBe('dark');

    fake.setPatchError(null);
    expect(await useSettingsStore.getState().retrySave()).toBe(true);
    state = useSettingsStore.getState();
    expect(state.saveError).toBeNull();
    expect(state.settings?.interface.appearance).toBe('dark');
    expect(state.hasDirtyDraft()).toBe(false);

    useSettingsStore.getState().edit({ interface: { appearance: 'light' } });
    useSettingsStore.getState().discardDraft();
    state = useSettingsStore.getState();
    expect(state.draft?.interface.appearance).toBe('dark');
    expect(state.saveError).toBeNull();
    expect(state.hasDirtyDraft()).toBe(false);
  });

  it('旧 localStorage 只作启动兜底；后端有效设置到达后统一覆盖显示缓存', async () => {
    const legacy = new Map<string, string>([
      ['bilidown.glassBackground', '0'],
      ['bilidown.accentColor', '#FB7299'],
      ['bilidown.motion', 'reduce'],
    ]);
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => legacy.get(key) ?? null,
      setItem: (key: string, value: string) => void legacy.set(key, value),
      removeItem: (key: string) => void legacy.delete(key),
      clear: () => legacy.clear(),
      key: (index: number) => [...legacy.keys()][index] ?? null,
      get length() {
        return legacy.size;
      },
    });
    const saved: Settings = defaultInternalSettings();
    saved.interface.accentColor = '#3B82F6';
    saved.interface.glassBackground = true;
    saved.interface.motion = 'system';
    const fake = createSettingsFake({ saved });
    await setup(fake);

    const state = useSettingsStore.getState();
    expect(state.settings?.interface.accentColor).toBe('#3B82F6');
    expect(state.settings?.interface.glassBackground).toBe(true);
    expect(state.settings?.interface.motion).toBe('system');
    // 后端有效设置到达后覆盖 localStorage 显示缓存。
    expect(legacy.get('bilidown.glassBackground')).toBe('1');
    expect(legacy.get('bilidown.accentColor')).toBe('#3B82F6');
    expect(legacy.get('bilidown.motion')).toBe('system');
  });
});

describe('关闭设置窗口前提交与等待保存（design D4 / tasks 4.5）', () => {
  it('无草稿且无在途保存时直接放行关闭，不增加确认', async () => {
    const fake = createSettingsFake();
    await setup(fake);
    await expect(decideSettingsClose(closeDeps())).resolves.toBe('close');
    expect(fake.patchCalls).toHaveLength(0);
  });

  it('关闭前先提交有效草稿并等待在途保存完成', async () => {
    const gate = deferred<void>();
    const fake = createSettingsFake({
      beforePatch: async (_patch, index) => {
        if (index === 0) await gate.promise;
      },
    });
    await setup(fake);

    const saving = useSettingsStore.getState().save({ interface: { appearance: 'dark' } });
    await vi.waitFor(() => expect(fake.patchCalls).toHaveLength(1));

    let settled = false;
    const decision = decideSettingsClose(closeDeps()).finally(() => {
      settled = true;
    });
    await Promise.resolve();
    expect(settled).toBe(false);

    gate.resolve();
    await saving;
    await expect(decision).resolves.toBe('close');
    // 关闭流程不会重复提交已保存的草稿。
    expect(fake.patchCalls).toHaveLength(1);
    expect(useSettingsStore.getState().settings?.interface.appearance).toBe('dark');
  });

  it('保存失败时保留窗口；重试成功后关闭', async () => {
    const fake = createSettingsFake({ patchError: '设置写盘失败' });
    await setup(fake);
    useSettingsStore.getState().edit({ download: { maxConcurrent: 5 } });

    await expect(decideSettingsClose(closeDeps())).resolves.toBe('failed');
    expect(useSettingsStore.getState().saveError).toBe('设置写盘失败');

    fake.setPatchError(null);
    await expect(decideSettingsClose(closeDeps())).resolves.toBe('close');
    expect(useSettingsStore.getState().settings?.download.maxConcurrent).toBe(5);
  });

  it('无效草稿不会在关闭时被静默提交或丢弃，交由用户明确放弃', async () => {
    const fake = createSettingsFake();
    await setup(fake);
    useSettingsStore.getState().edit({ network: { proxyMode: 'manual', proxyHost: '' } });

    await expect(decideSettingsClose(closeDeps())).resolves.toBe('invalid');
    expect(fake.patchCalls).toHaveLength(0);
    expect(useSettingsStore.getState().settings?.network.proxyMode).toBe('direct');
    expect(useSettingsStore.getState().draft?.network.proxyMode).toBe('manual');
  });

  it('连续关闭请求只执行一次提交/等待与一次销毁', async () => {
    let closeCalls = 0;
    let release!: (ok: boolean) => void;
    const pending = new Promise<boolean>((resolve) => {
      release = resolve;
    });
    let destroyed = 0;
    const handlers: Array<(event: { preventDefault: () => void }) => void | Promise<void>> = [];
    const installed = await installPreferencesCloseHandler(
      {
        onCloseRequested: (handler) => {
          handlers.push(handler);
          return () => {};
        },
        destroy: () => {
          destroyed += 1;
        },
      },
      () => {
        closeCalls += 1;
        return pending;
      },
    );

    const first = handlers[0]({ preventDefault: () => {} });
    const second = handlers[0]({ preventDefault: () => {} });
    await Promise.resolve();
    expect(closeCalls).toBe(1);
    release(true);
    await Promise.all([first, second]);
    expect(destroyed).toBe(1);
    expect(installed.allowed).toBe(true);
  });

  it('窗口按钮/原生关闭/快捷键共用 CloseRequested：先 preventDefault，保存通过后才 destroy', async () => {
    const handlers: Array<(event: { preventDefault: () => void }) => void | Promise<void>> = [];
    let destroyed = 0;
    const window: ClosablePreferencesWindow = {
      onCloseRequested: (handler) => {
        handlers.push(handler);
        return () => {};
      },
      destroy: () => {
        destroyed += 1;
      },
    };
    let allow = false;
    const installed = await installPreferencesCloseHandler(window, async () => allow);

    const firstClose = { preventDefault: vi.fn() };
    await handlers[0](firstClose);
    expect(firstClose.preventDefault).toHaveBeenCalledTimes(1);
    expect(destroyed).toBe(0);
    expect(installed.allowed).toBe(false);

    allow = true;
    await handlers[0]({ preventDefault: vi.fn() });
    expect(destroyed).toBe(1);
    expect(installed.allowed).toBe(true);

    // 关闭放行后不再重复拦截/重复提示。
    expect(handlers).toHaveLength(1);
  });
});
