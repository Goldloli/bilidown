import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '../src/lib/ipc';
import {
  applySettingsPatchDTO,
  defaultInternalSettings,
  settingsFromDTO,
  settingsPatchToDTO,
  settingsToDTO,
} from '../src/lib/adapt';
import { useSettingsStore } from '../src/stores/useSettingsStore';
import { createSettingsFake, deferred, type SettingsFake } from './settingsFake';

async function setup(fake: SettingsFake): Promise<void> {
  api.setBackend(fake.backend);
  useSettingsStore.getState().reset();
  await useSettingsStore.getState().reload();
}

afterEach(() => {
  useSettingsStore.getState().reset();
});

describe('设置按序提交与修订号（design D3 / tasks 4.4）', () => {
  it('并发修改不同字段时全部保留，后一次提交不回退前一次修改', async () => {
    const gate = deferred<void>();
    const fake = createSettingsFake({
      beforePatch: async (_patch, index) => {
        if (index === 0) await gate.promise;
      },
    });
    await setup(fake);
    const store = useSettingsStore.getState();

    const first = store.save({ interface: { appearance: 'dark' } });
    await vi.waitFor(() => expect(fake.patchCalls).toHaveLength(1));
    const second = store.save({ download: { maxConcurrent: 5 } });
    gate.resolve();

    await expect(Promise.all([first, second])).resolves.toEqual([true, true]);
    expect(fake.patchCalls).toEqual([{ theme: 'dark' }, { maxConcurrent: 5 }]);
    const state = useSettingsStore.getState();
    expect(state.settings?.interface.appearance).toBe('dark');
    expect(state.settings?.download.maxConcurrent).toBe(5);
    expect(state.draft?.interface.appearance).toBe('dark');
    expect(state.draft?.download.maxConcurrent).toBe(5);
  });

  it('同字段连续两次选择按操作顺序提交，最终保留最后一次有效选择', async () => {
    const gate = deferred<void>();
    const fake = createSettingsFake({
      beforePatch: async (_patch, index) => {
        if (index === 0) await gate.promise;
      },
    });
    await setup(fake);
    const store = useSettingsStore.getState();

    const first = store.save({ interface: { appearance: 'dark' } });
    await vi.waitFor(() => expect(fake.patchCalls).toHaveLength(1));
    const second = store.save({ interface: { appearance: 'light' } });
    gate.resolve();
    await Promise.all([first, second]);

    expect(fake.patchCalls.map((call) => call.theme)).toEqual(['dark', 'light']);
    expect(useSettingsStore.getState().settings?.interface.appearance).toBe('light');
  });

  it('空闲时的连续操作合并为同一批，不同字段全部保留且同字段以后一次为准', async () => {
    const gate = deferred<void>();
    const fake = createSettingsFake({
      beforePatch: async (_patch, index) => {
        if (index === 0) await gate.promise;
      },
    });
    await setup(fake);
    const store = useSettingsStore.getState();

    const first = store.save({ interface: { appearance: 'dark' } });
    await vi.waitFor(() => expect(fake.patchCalls).toHaveLength(1));
    // 第一次在途时，后续两次操作进入同一待提交批次。
    const second = store.save({ download: { maxConcurrent: 6 } });
    const third = store.save({ download: { maxConcurrent: 4 }, general: { notifyOnComplete: false } });
    gate.resolve();
    await Promise.all([first, second, third]);

    expect(fake.patchCalls).toHaveLength(2);
    expect(fake.patchCalls[1]).toEqual({ maxConcurrent: 4, notifications: false });
    const state = useSettingsStore.getState();
    expect(state.settings?.download.maxConcurrent).toBe(4);
    expect(state.settings?.general.notifyOnComplete).toBe(false);
  });

  it('双窗口交错：自己的成功提交不会被迟到的旧响应/旧事件覆盖', async () => {
    const gate = deferred<void>();
    const fake = createSettingsFake({
      beforePatch: async (_patch, index) => {
        if (index === 0) await gate.promise;
      },
    });
    await setup(fake);
    const store = useSettingsStore.getState();

    const saving = store.save({ interface: { appearance: 'dark' } });
    await vi.waitFor(() => expect(fake.patchCalls).toHaveLength(1));

    // 另一个窗口在本窗口保存期间修改并发数（服务端 revision 2），事件先到达。
    const otherWindow = applySettingsPatchDTO(
      applySettingsPatchDTO(defaultInternalSettings(), { theme: 'dark' }),
      { maxConcurrent: 7 },
    );
    store.setSettings(fake.event(otherWindow, 2, store.settings));
    // 未提交的本地草稿不被跨窗口事件重置；其他字段立即跟随服务端。
    expect(useSettingsStore.getState().draft?.interface.appearance).toBe('dark');
    expect(useSettingsStore.getState().draft?.download.maxConcurrent).toBe(7);

    gate.resolve();
    await saving;

    const state = useSettingsStore.getState();
    // 迟到的 patch 响应（revision 1）不得回退事件（revision 2）。
    expect(state.settings?.download.maxConcurrent).toBe(7);
    expect(state.settings?.interface.appearance).toBe('dark');
    expect(state.settings?.revision).toBe(2);
  });

  it('过期事件不覆盖较新的已提交设置', async () => {
    const fake = createSettingsFake();
    await setup(fake);
    const store = useSettingsStore.getState();
    expect(await store.save({ download: { maxConcurrent: 5 } })).toBe(true);
    expect(useSettingsStore.getState().settings?.revision).toBe(1);

    const stale = fake.event(defaultInternalSettings(), 0, useSettingsStore.getState().settings);
    useSettingsStore.getState().setSettings(stale);

    const state = useSettingsStore.getState();
    expect(state.settings?.download.maxConcurrent).toBe(5);
    expect(state.settings?.revision).toBe(1);
  });

  it('事件接受后 ApiClient.lastSettings 同步，可用于补丁中省略字段的映射', async () => {
    const fake = createSettingsFake();
    await setup(fake);
    const store = useSettingsStore.getState();

    // 另一窗口配置了手动代理（revision 1）。后端 DTO 会完整携带代理字段。
    const otherWindow = defaultInternalSettings();
    otherWindow.network = {
      proxyMode: 'manual',
      proxyHost: '10.0.0.1',
      proxyPort: 1080,
      proxyUser: 'u',
      proxyPass: 'p',
    };
    store.setSettings(fake.event(otherWindow, 1, store.settings));

    // 只改端口的补丁需要 ApiClient 缓存的当前代理其余字段来补全 DTO。
    expect(await useSettingsStore.getState().save({ network: { proxyPort: 2080 } })).toBe(true);
    expect(fake.patchCalls[0].proxy).toEqual({
      mode: 'manual',
      host: '10.0.0.1',
      port: 2080,
      user: 'u',
      pass: 'p',
    });
  });

  it('重新建立后端会话后修订状态重置，旧修订号不再参与比较', async () => {
    const first = createSettingsFake({ revision: 5 });
    await setup(first);
    expect(useSettingsStore.getState().settings?.revision).toBe(5);

    const second = createSettingsFake({ revision: 0 });
    await setup(second);
    expect(useSettingsStore.getState().settings?.revision).toBe(0);
    expect(await useSettingsStore.getState().save({ download: { maxConcurrent: 2 } })).toBe(true);
    expect(useSettingsStore.getState().settings?.revision).toBe(1);
  });

  it('保存成功才标记已保存；失败不改最后有效设置', async () => {
    const fake = createSettingsFake({ patchError: 'database is locked' });
    await setup(fake);
    const store = useSettingsStore.getState();

    const ok = await store.save({ interface: { appearance: 'dark' } });
    expect(ok).toBe(false);
    const failed = useSettingsStore.getState();
    expect(failed.saveError).toBe('database is locked');
    expect(failed.lastSavedAt).toBeNull();
    expect(failed.settings?.interface.appearance).toBe('system');
    expect(failed.draft?.interface.appearance).toBe('dark');

    fake.setPatchError(null);
    expect(await useSettingsStore.getState().retrySave()).toBe(true);
    const saved = useSettingsStore.getState();
    expect(saved.saveError).toBeNull();
    expect(saved.lastSavedAt).not.toBeNull();
    expect(saved.settings?.interface.appearance).toBe('dark');
  });
});

describe('设置补丁映射（design D3 契约）', () => {
  it('内部补丁 → SettingsPatchDTO 只包含显式字段，运行态字段不提交', () => {
    const base = defaultInternalSettings();

    expect(settingsPatchMapping({}, base)).toEqual({});
    expect(settingsPatchMapping({ quality: { ffmpegDetected: false } }, base)).toEqual({});
    expect(
      settingsPatchMapping(
        { download: { outputDir: '  ' }, quality: { ffmpegPath: '' } },
        base,
      ),
    ).toEqual({ downloadDir: null, ffmpegPath: null });
    expect(settingsPatchMapping({ interface: { appearance: 'dark' } }, base)).toEqual({
      theme: 'dark',
    });
    expect(
      settingsPatchMapping(
        { network: { proxyMode: 'direct' } },
        { ...base, network: { ...base.network, proxyMode: 'manual', proxyHost: '127.0.0.1' } },
      ),
    ).toEqual({ proxy: { mode: 'direct' } });
  });

  it('代理补丁按当前已保存值补全完整描述，可选认证可为空', () => {
    const base = defaultInternalSettings();
    const manualBase = {
      ...base,
      network: {
        proxyMode: 'manual' as const,
        proxyHost: '127.0.0.1',
        proxyPort: 7890,
        proxyUser: 'u',
        proxyPass: 'p',
      },
    };
    expect(settingsPatchMapping({ network: { proxyPort: 1080 } }, manualBase)).toEqual({
      proxy: { mode: 'manual', host: '127.0.0.1', port: 1080, user: 'u', pass: 'p' },
    });
  });

  it('Mock 后端补丁合并语义与 Rust 一致（省略保持 / null 清空）', () => {
    const base = defaultInternalSettings();
    const merged = applySettingsPatchDTO(base, {
      maxConcurrent: 6,
      downloadDir: null,
      proxy: { mode: 'manual', host: '127.0.0.1', port: 1080, user: '', pass: '' },
    });
    expect(merged.download.maxConcurrent).toBe(6);
    expect(merged.download.outputDir).toBe('');
    expect(merged.network.proxyMode).toBe('manual');
    expect(merged.network.proxyHost).toBe('127.0.0.1');
    expect(merged.network.proxyPort).toBe(1080);

    const backToDirect = applySettingsPatchDTO(merged, { proxy: { mode: 'direct' } });
    expect(backToDirect.network.proxyMode).toBe('direct');
    expect(backToDirect.network.proxyHost).toBe('');
    // 省略字段保持不变
    expect(backToDirect.download.maxConcurrent).toBe(6);
  });
});


function settingsPatchMapping(
  patch: Parameters<typeof settingsPatchToDTO>[0],
  base: Parameters<typeof settingsPatchToDTO>[1],
) {
  return settingsPatchToDTO(patch, base);
}
