import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { defaultInternalSettings } from '../src/lib/adapt';
import {
  isProxyDraftComplete,
  parseProxyPort,
  proxyDraftFromSettings,
  proxyDraftIssues,
  proxyDraftPatch,
  proxyModeChangePatch,
  testProxyDraft,
  type ProxyDraft,
} from '../src/settings/proxy';
import {
  ManualProxyFields,
  showsManualProxyFields,
} from '../src/settings/pages/NetworkPage';

const manualDraft = (overrides: Partial<ProxyDraft> = {}): ProxyDraft => ({
  mode: 'manual',
  host: '127.0.0.1',
  portText: '7890',
  user: '',
  pass: '',
  ...overrides,
});



describe('手动代理草稿（design D4 / tasks 4.6）', () => {
  it('首次切换到手动代理时输入项立即可见，地址为空也不隐藏、不提交', () => {
    const saved = defaultInternalSettings();
    saved.network = { proxyMode: 'direct', proxyHost: '', proxyPort: 7890, proxyUser: '', proxyPass: '' };
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
