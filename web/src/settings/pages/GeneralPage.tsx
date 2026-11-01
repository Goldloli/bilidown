import { useCallback, useEffect, useState } from 'react';
import { t } from '../../i18n/zh-CN';
import { formatBytes } from '../../lib/format';
import {
  clearAppCache,
  getStorageStats,
  isLowSpace,
  openDataDir,
  type ClearTarget,
  type StorageStats,
} from '../../lib/storage';
import { useUiStore } from '../../stores/useUiStore';
import { FieldRow } from '../../components/FieldRow';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { SettingsGroup, useSettingsForm } from '../form';
import { Switch } from '../../components/Switch';
import { isNotificationsSupported } from '../../lib/platform';

function InfoRow({
  label,
  value,
  danger,
  mono = true,
}: {
  label: string;
  value: string;
  danger?: boolean;
  mono?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2">
      <span className="shrink-0 text-sm text-fg">{label}</span>
      <span className={`min-w-0 break-all text-right text-2xs ${danger ? 'text-danger' : 'text-fg2'} ${mono ? 'num' : ''}`}>
        {value}
      </span>
    </div>
  );
}

export function GeneralPage() {
  const { settings, save } = useSettingsForm();
  const general = settings.general;
  const confirm = useUiStore((s) => s.confirm);
  const pushToast = useUiStore((s) => s.pushToast);
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [clearing, setClearing] = useState<ClearTarget | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setStats(await getStorageStats());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const lowSpace = isLowSpace(stats);
  const freeText = stats
    ? stats.downloadFreeBytes != null && stats.downloadTotalBytes != null
      ? `${formatBytes(stats.downloadFreeBytes)} / ${formatBytes(stats.downloadTotalBytes)}`
      : t('settings.general.storage.unknown')
    : t('settings.general.storage.unknown');

  const clear = async (targets: ClearTarget[], title: string, body: string, confirmLabel: string) => {
    const ok = await confirm({ title, body, confirmLabel, danger: true });
    if (!ok) return;
    setClearing(targets[0]);
    try {
      let freedBytes = 0;
      let removedFiles = 0;
      for (const target of targets) {
        const result = await clearAppCache(target);
        freedBytes += result.freedBytes;
        removedFiles += result.removedFiles;
      }
      pushToast({
        kind: 'success',
        message: t('settings.general.storage.cleared', {
          size: formatBytes(freedBytes),
          n: removedFiles,
        }),
      });
      await load();
    } catch (err) {
      pushToast({
        kind: 'error',
        message: t('settings.general.storage.clearFailed', {
          message: err instanceof Error ? err.message : String(err),
        }),
      });
    } finally {
      setClearing(null);
    }
  };

  const clearCache = () =>
    clear(
      ['covers', 'temp'],
      t('settings.general.storage.clearCacheConfirm'),
      t('settings.general.storage.clearCacheBody'),
      t('settings.general.storage.clearCache'),
    );

  const clearLogs = () =>
    clear(
      ['logs'],
      t('settings.general.storage.clearLogsConfirm'),
      t('settings.general.storage.clearLogsBody'),
      t('settings.general.storage.clearLogs'),
    );

  const revealData = async () => {
    try {
      await openDataDir();
    } catch (err) {
      pushToast({
        kind: 'error',
        message: t('settings.general.storage.openFailed', {
          message: err instanceof Error ? err.message : String(err),
        }),
      });
    }
  };

  return (
    <div>
      <SettingsGroup title={t('settings.general.queue')}>
        <FieldRow label={t('settings.general.keepQueue')} description={t('settings.general.keepQueue.desc')}>
          <span className="text-2xs text-fg2">已启用（固有行为）</span>
        </FieldRow>
      </SettingsGroup>

      <SettingsGroup title={t('settings.general.afterDownload')}>
        <FieldRow
          label={t('settings.general.notify')}
          description={
            isNotificationsSupported()
              ? undefined
              : '当前为开发/免安装运行，系统通知不可靠；任务完成或失败会改用应用内提示。'
          }
        >
          <Switch
            checked={general.notifyOnComplete}
            onChange={(checked) => save({ general: { notifyOnComplete: checked } })}
            disabled={!isNotificationsSupported()}
            label={t('settings.general.notify')}
          />
        </FieldRow>
      </SettingsGroup>

      <SettingsGroup
        title={t('settings.general.storage')}
        description={t('settings.general.storage.desc')}
      >
        {loading ? (
          <p className="py-3 text-2xs text-fg3">{t('settings.general.storage.loading')}</p>
        ) : error ? (
          <div className="flex items-center justify-between gap-3 py-2.5">
            <span className="flex min-w-0 items-center gap-1.5 text-2xs text-danger">
              <Icon name="warning" size={12} />
              <span className="break-all">
                {t('settings.general.storage.loadFailed')}
                {': '}
                {error}
              </span>
            </span>
            <Button size="sm" variant="secondary" icon="refresh" onClick={() => void load()}>
              {t('settings.general.storage.retry')}
            </Button>
          </div>
        ) : stats ? (
          <>
            <InfoRow label={t('settings.general.storage.downloadDir')} value={stats.downloadDir} />
            <InfoRow
              label={t('settings.general.storage.freeSpace')}
              value={freeText}
              danger={lowSpace}
            />
            {lowSpace && (
              <p className="flex items-center gap-1 pb-2 text-2xs text-danger">
                <Icon name="warning" size={11} />
                {t('settings.general.storage.lowSpace')}
              </p>
            )}
            <InfoRow label={t('settings.general.storage.dataDir')} value={stats.dataDir} />
            <InfoRow
              label={t('settings.general.storage.total')}
              value={formatBytes(stats.dataBytes)}
            />
            <InfoRow label={t('settings.general.storage.covers')} value={formatBytes(stats.coverBytes)} />
            <InfoRow label={t('settings.general.storage.temp')} value={formatBytes(stats.tempBytes)} />
            <InfoRow label={t('settings.general.storage.logs')} value={formatBytes(stats.logBytes)} />
            <div className="flex flex-wrap items-center gap-1.5 py-2.5">
              <Button size="sm" variant="secondary" icon="refresh" onClick={() => void load()} disabled={Boolean(clearing)}>
                {t('settings.general.storage.refresh')}
              </Button>
              <Button size="sm" variant="secondary" icon="trash" loading={clearing === 'covers'} onClick={() => void clearCache()} disabled={Boolean(clearing)}>
                {t('settings.general.storage.clearCache')}
              </Button>
              <Button size="sm" variant="secondary" icon="file" loading={clearing === 'logs'} onClick={() => void clearLogs()} disabled={Boolean(clearing)}>
                {t('settings.general.storage.clearLogs')}
              </Button>
              <Button size="sm" variant="secondary" icon="folder" onClick={() => void revealData()}>
                {t('settings.general.storage.openDataDir')}
              </Button>
            </div>
          </>
        ) : null}
      </SettingsGroup>
    </div>
  );
}
