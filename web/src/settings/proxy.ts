/**
 * 手动代理草稿（design D4 / tasks 4.6）。
 *
 * 代理输入在本地以字符串草稿维护：地址/端口未填完时不提交，原有效网络配置继续工作；
 * 只有完整有效的草稿才生成设置补丁；连接测试只读取草稿，不产生设置提交。
 */
import type { ProxyDTO } from '../lib/contract';
import type { NetworkSettings, SettingsPatch } from '../lib/types';
import { errorMessageOf } from '../lib/errors';

export interface ProxyDraft {
  mode: 'direct' | 'manual';
  host: string;
  portText: string;
  user: string;
  pass: string;
}

export interface ProxyDraftIssues {
  host?: string;
  port?: string;
}

export function proxyDraftFromSettings(network: NetworkSettings): ProxyDraft {
  return {
    mode: network.proxyMode,
    host: network.proxyHost,
    portText: String(network.proxyPort),
    user: network.proxyUser,
    pass: network.proxyPass,
  };
}

/** 端口草稿 → 合法端口（1–65535 的整数）；非法返回 null。 */
export function parseProxyPort(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const port = Number.parseInt(trimmed, 10);
  return port >= 1 && port <= 65535 ? port : null;
}

export function proxyDraftIssues(draft: ProxyDraft): ProxyDraftIssues {
  if (draft.mode !== 'manual') return {};
  const issues: ProxyDraftIssues = {};
  if (!draft.host.trim()) issues.host = '请输入代理地址';
  if (parseProxyPort(draft.portText) === null) issues.port = '端口需在 1–65535 之间';
  return issues;
}

export function isProxyDraftComplete(draft: ProxyDraft): boolean {
  const issues = proxyDraftIssues(draft);
  return !issues.host && !issues.port;
}

/**
 * 模式切换时是否立即提交（design D4）：
 * · 切回直连是完整有效状态，立即提交；
 * · 首次切手动代理若草稿尚不完整（地址为空/端口非法），只展开输入项、不提交。
 */
export function proxyModeChangePatch(draft: ProxyDraft, mode: 'direct' | 'manual'): SettingsPatch | null {
  if (mode === 'direct') return { network: { proxyMode: 'direct' } };
  const manual: ProxyDraft = { ...draft, mode: 'manual' };
  return isProxyDraftComplete(manual) ? proxyDraftPatch(manual) : null;
}

/** 完整有效的代理草稿 → 设置补丁（直连时只提交 mode，沿用契约语义）。 */
export function proxyDraftPatch(draft: ProxyDraft): SettingsPatch | null {
  if (draft.mode === 'direct') {
    return { network: { proxyMode: 'direct' } };
  }
  if (!isProxyDraftComplete(draft)) return null;
  return {
    network: {
      proxyMode: 'manual',
      proxyHost: draft.host.trim(),
      proxyPort: parseProxyPort(draft.portText) ?? 0,
      proxyUser: draft.user,
      proxyPass: draft.pass,
    },
  };
}

/** 连接测试使用当前完整草稿（含可选认证），不读取也不写入已保存设置。 */
export function proxyDraftDTO(draft: ProxyDraft): ProxyDTO {
  if (draft.mode === 'direct') return { mode: 'direct' };
  return {
    mode: 'manual',
    host: draft.host.trim(),
    port: parseProxyPort(draft.portText) ?? undefined,
    user: draft.user,
    pass: draft.pass,
  };
}

/** 草稿 → 内部 NetworkSettings（连接测试复用 api.testProxy 的参数形状）。 */
export function proxyDraftToNetwork(draft: ProxyDraft): NetworkSettings {
  return {
    proxyMode: draft.mode,
    proxyHost: draft.host.trim(),
    proxyPort: parseProxyPort(draft.portText) ?? 7890,
    proxyUser: draft.user,
    proxyPass: draft.pass,
  };
}

/**
 * 用当前完整草稿执行连接测试：不提交设置、不写已保存配置（tasks 4.6）。
 * 草稿不完整时直接给出可读反馈，不调用网络测试。
 */
export async function testProxyDraft(
  draft: ProxyDraft,
  test: (network: NetworkSettings) => Promise<{ ok: boolean; message: string }>,
): Promise<{ ok: boolean; message: string }> {
  const issues = proxyDraftIssues(draft);
  if (draft.mode === 'manual' && (issues.host || issues.port)) {
    return { ok: false, message: issues.host ?? issues.port ?? '无法连接代理' };
  }
  try {
    return await test(proxyDraftToNetwork(draft));
  } catch (error) {
    return { ok: false, message: `无法连接代理：${errorMessageOf(error)}` };
  }
}
