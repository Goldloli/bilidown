import '../../i18n/zh-CN.dynamic';
import { useEffect, useState } from 'react';
import { t } from '../../i18n/zh-CN';
import { api } from '../../lib/ipc';
import type { NetworkSettings, SettingsPatch } from '../../lib/types';
import { Button } from '../../components/Button';
import { FieldRow } from '../../components/FieldRow';
import { StatusBadge } from '../../components/StatusBadge';
import { SegmentedControl } from '../../components/SegmentedControl';
import { TextField } from '../../components/TextField';
import { SettingsGroup, useSettingsForm } from '../form';
import {
  isProxyDraftComplete,
  parseProxyPort,
  proxyDraftIssues,
  proxyDraftPatch,
  proxyModeChangePatch,
  testProxyDraft,
  type ProxyDraft,
} from '../proxy';

/** 手动代理输入是否可见：只看草稿模式，地址为空也不隐藏（design D4 / tasks 4.6）。 */
export function showsManualProxyFields(network: NetworkSettings): boolean {
  return network.proxyMode === 'manual';
}

export interface ManualProxyFieldsProps {
  host: string;
  portText: string;
  user: string;
  pass: string;
  hostError?: string;
  portError?: string;
  onHostChange: (value: string) => void;
  onPortChange: (value: string) => void;
  onUserChange: (value: string) => void;
  onPassChange: (value: string) => void;
  onHostCommit: () => void;
  onPortCommit: () => void;
  onCommit: () => void;
}

/** 手动代理草稿输入组（地址/端口/可选认证）；不完整草稿也始终可见。 */
export function ManualProxyFields({
  host,
  portText,
  user,
  pass,
  hostError,
  portError,
  onHostChange,
  onPortChange,
  onUserChange,
  onPassChange,
  onHostCommit,
  onPortCommit,
  onCommit,
}: ManualProxyFieldsProps) {
  return (
    <>
      <FieldRow label={t('settings.network.host')} error={hostError}>
        <span
          onBlur={onHostCommit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onHostCommit();
            }
          }}
        >
          <TextField
            value={host}
            onChange={onHostChange}
            placeholder="127.0.0.1"
            width={200}
            invalid={Boolean(hostError)}
            ariaLabel={t('settings.network.host')}
          />
        </span>
      </FieldRow>
      <FieldRow label={t('settings.network.port')} error={portError}>
        <span
          onBlur={onPortCommit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onPortCommit();
            }
          }}
        >
          <TextField
            value={portText}
            onChange={onPortChange}
            width={96}
            invalid={Boolean(portError)}
            ariaLabel={t('settings.network.port')}
          />
        </span>
      </FieldRow>
      <FieldRow label={t('settings.network.user')}>
        <span
          onBlur={onCommit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onCommit();
            }
          }}
        >
          <TextField
            value={user}
            onChange={onUserChange}
            width={200}
            ariaLabel={t('settings.network.user')}
          />
        </span>
      </FieldRow>
      <FieldRow label={t('settings.network.pass')}>
        <span
          onBlur={onCommit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onCommit();
            }
          }}
        >
          <TextField
            type="password"
            value={pass}
            onChange={onPassChange}
            width={200}
            ariaLabel={t('settings.network.pass')}
          />
        </span>
      </FieldRow>
    </>
  );
}

export function NetworkPage() {
  const { settings, edit, save } = useSettingsForm();
  const network = settings.network;
  const [portText, setPortText] = useState(() => String(network.proxyPort));
  const [touched, setTouched] = useState<{ host: boolean; port: boolean }>({
    host: false,
    port: false,
  });
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null);

  // 跨窗口/后端修正端口时同步本地文本；本地非法文本不会被覆盖成别的值。
  useEffect(() => {
    if (parseProxyPort(portText) === network.proxyPort) return;
    setPortText(String(network.proxyPort));
    // 仅在端口有效值变化时同步；本地编辑中的非法文本保留。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [network.proxyPort]);

  const buildDraft = (): ProxyDraft => ({
    mode: network.proxyMode,
    host: network.proxyHost,
    portText,
    user: network.proxyUser,
    pass: network.proxyPass,
  });

  const submitDraft = (draft: ProxyDraft): boolean => {
    const patch = proxyDraftPatch(draft);
    if (!patch) return false;
    void save(patch);
    return true;
  };

  const changeMode = (mode: 'direct' | 'manual') => {
    setResult(null);
    setTouched({ host: false, port: false });
    edit({ network: { proxyMode: mode } });
    // 首次切手动：只展开草稿输入，不完整草稿不提交，原有效网络配置继续工作。
    const patch = proxyModeChangePatch(buildDraft(), mode);
    if (patch) void save(patch);
  };

  const commitProxyDraft = (): void => {
    if (network.proxyMode !== 'manual') return;
    if (!isProxyDraftComplete(buildDraft())) return;
    submitDraft(buildDraft());
  };

  const changeHost = (value: string) => {
    edit({ network: { proxyHost: value } });
    setResult(null);
  };

  const changePort = (value: string) => {
    setPortText(value);
    const port = parseProxyPort(value);
    if (port !== null) edit({ network: { proxyPort: port } });
    setResult(null);
  };

  const changeCredentials = (patch: SettingsPatch): void => {
    edit(patch);
    setResult(null);
  };

  const finishHost = (): void => {
    setTouched((current) => ({ ...current, host: true }));
    commitProxyDraft();
  };

  const finishPort = (): void => {
    setTouched((current) => ({ ...current, port: true }));
    commitProxyDraft();
  };

  /** 可选认证字段：失焦/Enter 时若草稿完整则提交，否则保持草稿。 */
  const finishCredentials = (): void => {
    commitProxyDraft();
  };

  const test = async (): Promise<void> => {
    setTesting(true);
    setResult(null);
    try {
      // 使用当前完整草稿（含可选认证）测试，只读草稿、不产生设置提交。
      const outcome = await testProxyDraft(buildDraft(), (network) => api.testProxy(network));
      setResult(outcome);
    } finally {
      setTesting(false);
    }
  };

  const draftIssues = proxyDraftIssues(buildDraft());
  const hostError = touched.host ? draftIssues.host : undefined;
  const portError = touched.port ? draftIssues.port : undefined;

  return (
    <div>
      <SettingsGroup title={t('settings.network.mode')} description={t('settings.network.mode.desc')}>
        <FieldRow label={t('settings.network.mode')}>
          <SegmentedControl<'direct' | 'manual'>
            value={network.proxyMode}
            onChange={changeMode}
            ariaLabel={t('settings.network.mode')}
            options={[
              { value: 'direct', label: t('settings.network.direct') },
              { value: 'manual', label: t('settings.network.manual') },
            ]}
          />
        </FieldRow>

        {showsManualProxyFields(network) && (
          <ManualProxyFields
            host={network.proxyHost}
            portText={portText}
            user={network.proxyUser}
            pass={network.proxyPass}
            hostError={hostError}
            portError={portError}
            onHostChange={changeHost}
            onPortChange={changePort}
            onUserChange={(value) => changeCredentials({ network: { proxyUser: value } })}
            onPassChange={(value) => changeCredentials({ network: { proxyPass: value } })}
            onHostCommit={finishHost}
            onPortCommit={finishPort}
            onCommit={finishCredentials}
          />
        )}
        <FieldRow label="连接测试">
          <div className="flex items-center gap-2">
            {result && (
              <StatusBadge tone={result.ok ? 'success' : 'danger'} dot>
                {result.message}
              </StatusBadge>
            )}
            <Button size="sm" variant="secondary" loading={testing} onClick={() => void test()}>
              {testing ? t('settings.network.testing') : t('settings.network.test')}
            </Button>
          </div>
        </FieldRow>
      </SettingsGroup>

      <p className="px-1 text-2xs leading-[15px] text-fg3">
        默认直连：bilidown 不读取系统代理设置，避免与系统网络工具冲突；如需代理请在此显式配置。
      </p>
    </div>
  );
}
