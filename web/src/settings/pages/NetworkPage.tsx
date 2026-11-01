import { useState } from 'react';
import { t } from '../../i18n/zh-CN';
import { api } from '../../lib/ipc';
import { Button } from '../../components/Button';
import { FieldRow } from '../../components/FieldRow';
import { NumberField } from '../../components/NumberField';
import { StatusBadge } from '../../components/StatusBadge';
import { SegmentedControl } from '../../components/SegmentedControl';
import { TextField } from '../../components/TextField';
import { SettingsGroup, useSettingsForm } from '../form';

export function NetworkPage() {
  const { settings, save, issueFor } = useSettingsForm();
  const network = settings.network;
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null);

  const test = () => {
    setTesting(true);
    setResult(null);
    void api
      .testProxy(network)
      .then(setResult)
      .finally(() => setTesting(false));
  };

  return (
    <div>
      <SettingsGroup title={t('settings.network.mode')} description={t('settings.network.mode.desc')}>
        <FieldRow label={t('settings.network.mode')}>
          <SegmentedControl<'direct' | 'manual'>
            value={network.proxyMode}
            onChange={(value) => {
              save({ network: { proxyMode: value } });
              setResult(null);
            }}
            ariaLabel={t('settings.network.mode')}
            options={[
              { value: 'direct', label: t('settings.network.direct') },
              { value: 'manual', label: t('settings.network.manual') },
            ]}
          />
        </FieldRow>

        {network.proxyMode === 'manual' && (
          <>
            <FieldRow label={t('settings.network.host')} error={issueFor('network.proxyHost')}>
              <TextField
                value={network.proxyHost}
                onChange={(value) => {
                  save({ network: { proxyHost: value } });
                  setResult(null);
                }}
                placeholder="127.0.0.1"
                width={200}
                invalid={Boolean(issueFor('network.proxyHost'))}
                ariaLabel={t('settings.network.host')}
              />
            </FieldRow>
            <FieldRow label={t('settings.network.port')} error={issueFor('network.proxyPort')}>
              <NumberField
                value={network.proxyPort}
                min={1}
                max={65535}
                step={1}
                invalid={Boolean(issueFor('network.proxyPort'))}
                onChange={(value) => {
                  save({ network: { proxyPort: value } });
                  setResult(null);
                }}
                ariaLabel={t('settings.network.port')}
              />
            </FieldRow>
            <FieldRow label={t('settings.network.user')}>
              <TextField
                value={network.proxyUser}
                onChange={(value) => save({ network: { proxyUser: value } })}
                width={200}
                ariaLabel={t('settings.network.user')}
              />
            </FieldRow>
            <FieldRow label={t('settings.network.pass')}>
              <TextField
                type="password"
                value={network.proxyPass}
                onChange={(value) => save({ network: { proxyPass: value } })}
                width={200}
                ariaLabel={t('settings.network.pass')}
              />
            </FieldRow>
          </>
        )}
        <FieldRow label="连接测试">
          <div className="flex items-center gap-2">
            {result && (
              <StatusBadge tone={result.ok ? 'success' : 'danger'} dot>
                {result.message}
              </StatusBadge>
            )}
            <Button size="sm" variant="secondary" loading={testing} onClick={test}>
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

