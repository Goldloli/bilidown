import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import {
  applySettingsPatchDTO,
  defaultInternalSettings,
  settingsPatchToDTO,
} from '../src/lib/adapt';
import {
  isProxyDraftComplete,
  isUpdateProxyDraftComplete,
  parseProxyPort,
  proxyDraftFromSettings,
  proxyDraftIssues,
  proxyDraftPatch,
  proxyModeChangePatch,
  testProxyDraft,
  testUpdateProxyDraft,
  updateProxyDraftFromSettings,
  updateProxyDraftIssues,
  updateProxyDraftPatch,
  updateProxyModeChangePatch,
  type ProxyDraft,
  type UpdateProxyDraft,
} from '../src/settings/proxy';
import {
  ManualProxyFields,
  showsManualProxyFields,
  showsUpdateManualProxyFields,
  UpdateProxySettings,
} from '../src/settings/pages/NetworkPage';

const manualDraft = (overrides: Partial<ProxyDraft> = {}): ProxyDraft => ({
  mode: 'manual',
  host: '127.0.0.1',
  portText: '7890',
  user: '',
  pass: '',
  ...overrides,
});

const updateDraft = (
  overrides: Partial<UpdateProxyDraft> = {},
): UpdateProxyDraft => ({
  mode: 'manual',
  host: '127.0.0.1',
  portText: '7897',
  user: '',
  pass: '',
  ...overrides,
});

const neutralUpdateProxy = {
  updateProxyMode: 'system' as const,
  updateProxyHost: '',
  updateProxyPort: 7890,
  updateProxyUser: '',
  updateProxyPass: '',
};

describe('手动代理草稿（design D4 / tasks 4.6）', () => {
  it('首次切换到手动代理时输入项立即可见，地址为空也不隐藏、不提交', () => {
    const saved = defaultInternalSettings();
    saved.network = {
      proxyMode: 'direct',
      proxyHost: '',
      proxyPort: 7890,
      proxyUser: '',
      proxyPass: '',
      ...neutralUpdateProxy,
    };
    const manual: typeof saved.network = { ...saved.network, proxyMode: 'manual' };

    // 可见性只看草稿模式：地址尚未填写同样展示输入项。
    expect(showsManualProxyFields(manual)).toBe(true);
    expect(showsManualProxyFields(saved.network)).toBe(false);

    const html = renderToStaticMarkup(
      <ManualProxyFields
        host=""
        portText="7890"
        user=""
        pass=""
        onHostChange={() => {}}
        onPortChange={() => {}}
        onUserChange={() => {}}
        onPassChange={() => {}}
        onHostCommit={() => {}}
        onPortCommit={() => {}}
        onCommit={() => {}}
      />,
    );
    expect(html).toContain('placeholder="127.0.0.1"');
    expect(html).toContain('aria-label="地址"');
    expect(html).toContain('aria-label="端口"');
    expect(html).toContain('aria-label="用户名（可选）"');
    expect(html).toContain('aria-label="密码（可选）"');
    expect(html).not.toContain('请输入代理地址');
  });

  it('尝试提交无效地址时显示行内提示', () => {
    const html = renderToStaticMarkup(
      <ManualProxyFields
        host=""
        portText="70000"
        user=""
        pass=""
        hostError="请输入代理地址"
        portError="端口需在 1–65535 之间"
        onHostChange={() => {}}
        onPortChange={() => {}}
        onUserChange={() => {}}
        onPassChange={() => {}}
        onHostCommit={() => {}}
        onPortCommit={() => {}}
        onCommit={() => {}}
      />,
    );
    expect(html).toContain('请输入代理地址');
    expect(html).toContain('端口需在 1–65535 之间');
    expect(html).toContain('aria-invalid="true"');
  });

  it('地址为空或端口非法时草稿不完整，不生成提交补丁', () => {
    expect(proxyDraftIssues(manualDraft({ host: '' })).host).toBe('请输入代理地址');
    expect(isProxyDraftComplete(manualDraft({ host: '' }))).toBe(false);
    expect(proxyDraftPatch(manualDraft({ host: '' }))).toBeNull();

    expect(parseProxyPort('70000')).toBeNull();
    expect(parseProxyPort('abc')).toBeNull();
    expect(parseProxyPort('')).toBeNull();
    expect(proxyDraftIssues(manualDraft({ portText: '70000' })).port).toBe(
      '端口需在 1–65535 之间',
    );
    expect(proxyDraftPatch(manualDraft({ portText: '70000' }))).toBeNull();
  });

  it('完整草稿（含可选认证为空）生成完整代理补丁', () => {
    expect(proxyDraftPatch(manualDraft())).toEqual({
      network: {
        proxyMode: 'manual',
        proxyHost: '127.0.0.1',
        proxyPort: 7890,
        proxyUser: '',
        proxyPass: '',
      },
    });
    expect(proxyDraftPatch(manualDraft({ user: 'u', pass: 'p' }))).toEqual({
      network: {
        proxyMode: 'manual',
        proxyHost: '127.0.0.1',
        proxyPort: 7890,
        proxyUser: 'u',
        proxyPass: 'p',
      },
    });
  });

  it('首次切手动且地址为空时不提交；草稿完整后切换才提交补丁', () => {
    // 从直连切换：地址为空 → 只展开草稿，不产生设置请求。
    expect(proxyModeChangePatch({ ...manualDraft({ host: '' }), mode: 'direct' }, 'manual')).toBeNull();
    expect(proxyModeChangePatch({ ...manualDraft({ portText: '70000' }), mode: 'direct' }, 'manual')).toBeNull();
    expect(proxyModeChangePatch({ ...manualDraft({ host: '10.0.0.1', portText: '1080' }), mode: 'direct' }, 'manual')).toEqual({
      network: {
        proxyMode: 'manual',
        proxyHost: '10.0.0.1',
        proxyPort: 1080,
        proxyUser: '',
        proxyPass: '',
      },
    });
    // 恢复直连始终立即提交。
    expect(proxyModeChangePatch(manualDraft(), 'direct')).toEqual({
      network: { proxyMode: 'direct' },
    });
  });

  it('恢复直连时只提交直连模式', () => {
    expect(proxyDraftPatch({ ...manualDraft(), mode: 'direct' })).toEqual({
      network: { proxyMode: 'direct' },
    });
    const draft = proxyDraftFromSettings({
      proxyMode: 'manual',
      proxyHost: '10.0.0.1',
      proxyPort: 1080,
      proxyUser: 'u',
      proxyPass: 'p',
      ...neutralUpdateProxy,
    });
    expect(draft).toEqual({
      mode: 'manual',
      host: '10.0.0.1',
      portText: '1080',
      user: 'u',
      pass: 'p',
    });
  });

  it('连接测试使用当前完整草稿，草稿不完整时不发起测试', async () => {
    const test = vi.fn(async () => ({ ok: true, message: '连接成功' }));

    const success = await testProxyDraft(
      manualDraft({ host: '10.0.0.1', portText: '1080', user: 'u', pass: 'p' }),
      test,
    );
    expect(success).toEqual({ ok: true, message: '连接成功' });
    expect(test).toHaveBeenCalledWith({
      proxyMode: 'manual',
      proxyHost: '10.0.0.1',
      proxyPort: 1080,
      proxyUser: 'u',
      proxyPass: 'p',
      ...neutralUpdateProxy,
    });

    test.mockClear();
    const incomplete = await testProxyDraft(manualDraft({ host: '' }), test);
    expect(incomplete.ok).toBe(false);
    expect(incomplete.message).toBe('请输入代理地址');
    expect(test).not.toHaveBeenCalled();
  });

  it('连接测试失败返回可读反馈，且不会提交设置', async () => {
    const test = vi.fn(async () => {
      throw { kind: 'network', message: '代理不可用', actions: ['retry'] };
    });
    const outgoing = await testProxyDraft(manualDraft(), test);
    expect(outgoing.ok).toBe(false);
    expect(outgoing.message).toContain('代理不可用');
  });
});

describe('独立更新代理设置', () => {
  it('网络页同时呈现媒体代理与更新代理，默认更新跟随系统', () => {
    const network = defaultInternalSettings().network;
    const html = renderToStaticMarkup(
      <UpdateProxySettings
        network={network}
        portText="7890"
        result={null}
        testing={false}
        onChangeMode={() => {}}
        onHostChange={() => {}}
        onPortChange={() => {}}
        onUserChange={() => {}}
        onPassChange={() => {}}
        onHostCommit={() => {}}
        onPortCommit={() => {}}
        onCommit={() => {}}
        onTest={() => {}}
      />,
    );
    expect(html).toContain('软件更新下载代理');
    expect(html).toContain('跟随系统代理');
    expect(html).toContain('不改变视频下载网络');
    expect(html).toContain('aria-label="更新代理模式"');
    expect(showsManualProxyFields(network)).toBe(false);

    const manual = renderToStaticMarkup(
      <UpdateProxySettings
        network={{ ...network, updateProxyMode: 'manual', updateProxyHost: '' }}
        portText="7897"
        result={null}
        testing={false}
        onChangeMode={() => {}}
        onHostChange={() => {}}
        onPortChange={() => {}}
        onUserChange={() => {}}
        onPassChange={() => {}}
        onHostCommit={() => {}}
        onPortCommit={() => {}}
        onCommit={() => {}}
        onTest={() => {}}
      />,
    );
    expect(manual).toContain('aria-label="更新代理地址"');
    expect(manual).toContain('aria-label="更新代理端口"');
  });

  it('默认跟随系统，媒体与更新模式互不影响', () => {
    const defaults = defaultInternalSettings();
    expect(defaults.network.proxyMode).toBe('direct');
    expect(defaults.network.updateProxyMode).toBe('system');
    expect(showsUpdateManualProxyFields(defaults.network)).toBe(false);
    expect(
      showsUpdateManualProxyFields({
        ...defaults.network,
        updateProxyMode: 'manual',
      }),
    ).toBe(true);
  });

  it('不完整更新手动草稿不覆盖已保存值，完整草稿只修改更新字段', () => {
    expect(updateProxyDraftIssues(updateDraft({ host: '' })).host).toContain('更新代理');
    expect(isUpdateProxyDraftComplete(updateDraft({ host: '' }))).toBe(false);
    expect(updateProxyDraftPatch(updateDraft({ host: '' }))).toBeNull();
    expect(updateProxyDraftPatch(updateDraft())).toEqual({
      network: {
        updateProxyMode: 'manual',
        updateProxyHost: '127.0.0.1',
        updateProxyPort: 7897,
        updateProxyUser: '',
        updateProxyPass: '',
      },
    });
    expect(Object.keys(updateProxyDraftPatch(updateDraft())?.network ?? {})).not.toContain(
      'proxyMode',
    );
  });

  it('模式切换保留更新手动配置并立即生成 system/direct 补丁', () => {
    for (const mode of ['system', 'direct'] as const) {
      expect(updateProxyModeChangePatch(updateDraft(), mode)).toEqual({
        network: { updateProxyMode: mode },
      });
    }
    expect(
      updateProxyDraftFromSettings({
        ...defaultInternalSettings().network,
        updateProxyMode: 'direct',
        updateProxyHost: '127.0.0.1',
        updateProxyPort: 7897,
      }),
    ).toMatchObject({ mode: 'direct', host: '127.0.0.1', portText: '7897' });

    const base = defaultInternalSettings();
    base.network.proxyMode = 'manual';
    base.network.proxyHost = 'media.proxy';
    base.network.updateProxyMode = 'manual';
    base.network.updateProxyHost = '127.0.0.1';
    base.network.updateProxyPort = 7897;
    const dto = settingsPatchToDTO(
      { network: { updateProxyMode: 'system' } },
      base,
    );
    expect(dto.updateProxy).toMatchObject({
      mode: 'system',
      host: '127.0.0.1',
      port: 7897,
    });
    expect(dto.proxy).toBeUndefined();

    const applied = applySettingsPatchDTO(base, dto);
    expect(applied.network.updateProxyMode).toBe('system');
    expect(applied.network.updateProxyHost).toBe('127.0.0.1');
    expect(applied.network.proxyHost).toBe('media.proxy');
  });

  it('测试更新连接只使用更新草稿，不触碰媒体代理并报告模式', async () => {
    const test = vi.fn(async () => ({ ok: true, message: '更新连接成功' }));
    const result = await testUpdateProxyDraft(
      updateDraft({ user: 'u', pass: 'p' }),
      test,
    );
    expect(result).toEqual({ ok: true, message: '更新连接成功' });
    expect(test).toHaveBeenCalledWith({
      proxyMode: 'direct',
      proxyHost: '',
      proxyPort: 7890,
      proxyUser: '',
      proxyPass: '',
      updateProxyMode: 'manual',
      updateProxyHost: '127.0.0.1',
      updateProxyPort: 7897,
      updateProxyUser: 'u',
      updateProxyPass: 'p',
    });
  });
});
