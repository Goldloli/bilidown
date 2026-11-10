import { t } from '../../i18n/zh-CN';
import { FieldRow } from '../../components/FieldRow';
import { NumberField } from '../../components/NumberField';
import { PathField } from '../../components/PathField';
import { Switch } from '../../components/Switch';
import { SettingsGroup, useSettingsForm } from '../form';

export function DownloadPage() {
  const { settings, save, issueFor } = useSettingsForm();
  const download = settings.download;
  // SettingsDTO 已接线：adapt 双向映射 downloadSubtitle/downloadCover/checkUpdatesOnStartup
  const subtitle = download.downloadSubtitle ?? false;
  const cover = download.downloadCover ?? false;
  const checkUpdates = download.checkUpdatesOnStartup ?? true;

  const toggle = (
    key: 'downloadSubtitle' | 'downloadCover' | 'checkUpdatesOnStartup',
    checked: boolean,
  ): void => {
    void save({ download: { [key]: checked } });
  };

  return (
    <div>
      <SettingsGroup title="保存位置">
        <FieldRow label={t('settings.download.dir')} description={t('settings.download.dir.desc')}>
          <PathField
            value={download.outputDir}
            width={300}
            onChange={(value) => save({ download: { outputDir: value } })}
            invalid={Boolean(issueFor('download.outputDir'))}
          />
        </FieldRow>
        {issueFor('download.outputDir') && (
          <p className="pb-2 text-2xs text-danger">{issueFor('download.outputDir')}</p>
        )}
      </SettingsGroup>

      <SettingsGroup title={t('settings.download.content')}>
        <FieldRow label={t('settings.download.subtitle')} description={t('settings.download.subtitle.desc')}>
          <Switch
            checked={subtitle}
            onChange={(checked) => toggle('downloadSubtitle', checked)}
            label={t('settings.download.subtitle')}
          />
        </FieldRow>
        <FieldRow label={t('settings.download.cover')} description={t('settings.download.cover.desc')}>
          <Switch
            checked={cover}
            onChange={(checked) => toggle('downloadCover', checked)}
            label={t('settings.download.cover')}
          />
        </FieldRow>
      </SettingsGroup>

      <SettingsGroup title={t('settings.download.updates')}>
        <FieldRow
          label={t('settings.download.checkUpdates')}
          description={t('settings.download.checkUpdates.desc')}
        >
          <Switch
            checked={checkUpdates}
            onChange={(checked) => toggle('checkUpdatesOnStartup', checked)}
            label={t('settings.download.checkUpdates')}
          />
        </FieldRow>
      </SettingsGroup>

      <SettingsGroup title="并发与调度">
        <FieldRow
          label={t('settings.download.concurrent')}
          description={t('settings.download.concurrent.desc')}
          error={issueFor('download.maxConcurrent')}
        >
          <NumberField
            value={download.maxConcurrent}
            min={1}
            max={8}
            invalid={Boolean(issueFor('download.maxConcurrent'))}
            onChange={(value) => save({ download: { maxConcurrent: value } })}
            ariaLabel={t('settings.download.concurrent')}
          />
        </FieldRow>
        <FieldRow label={t('settings.download.clipboard')} description={t('settings.download.clipboard.desc')}>
          <Switch
            checked={download.clipboardWatch}
            onChange={(checked) => save({ download: { clipboardWatch: checked } })}
            label={t('settings.download.clipboard')}
          />
        </FieldRow>
      </SettingsGroup>
    </div>
  );
}
