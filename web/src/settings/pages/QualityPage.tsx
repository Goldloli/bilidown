import { t } from '../../i18n/zh-CN';
import { api } from '../../lib/ipc';
import { QUALITY_TIERS } from '../../lib/constants';
import type { AudioFormatPreference, CodecPreference, QualityStrategy } from '../../lib/types';
import { useUiStore } from '../../stores/useUiStore';
import { Button } from '../../components/Button';
import { FieldRow } from '../../components/FieldRow';
import { StatusBadge } from '../../components/StatusBadge';
import { SegmentedControl } from '../../components/SegmentedControl';
import { SelectMenu } from '../../components/SelectMenu';
import { Switch } from '../../components/Switch';
import { TextField } from '../../components/TextField';
import { SettingsGroup, useSettingsForm } from '../form';

export function QualityPage() {
  const { settings, save, issueFor } = useSettingsForm();
  const pushToast = useUiStore((s) => s.pushToast);
  const quality = settings.quality;

  const detect = () => {
    void api.detectFfmpeg().then((result) => {
      save({
        quality: {
          ffmpegDetected: result.detected,
          ffmpegVersion: result.version,
          ffmpegPath: result.path ?? quality.ffmpegPath,
        },
      });
      pushToast({
        kind: result.detected ? 'success' : 'warning',
        message: result.detected ? `已检测到 ffmpeg ${result.version ?? ''}` : '未找到 ffmpeg',
      });
    });
  };

  return (
    <div>
      <SettingsGroup title="清晰度策略" description={t('settings.quality.strategy.desc')}>
        <FieldRow label={t('settings.quality.strategy')}>
          <SegmentedControl<QualityStrategy>
            value={quality.strategy}
            onChange={(value) => save({ quality: { strategy: value } })}
            ariaLabel={t('settings.quality.strategy')}
            options={[
              { value: 'best', label: t('settings.quality.best') },
              { value: 'fixed', label: t('settings.quality.fixed') },
            ]}
          />
        </FieldRow>
        {quality.strategy === 'fixed' && (
          <>
            <FieldRow label={t('settings.quality.fixedQuality')} error={issueFor('quality.fixedQuality')}>
              <SelectMenu
                value={String(quality.fixedQuality)}
                width={170}
                onChange={(value) => save({ quality: { fixedQuality: Number(value) } })}
                options={QUALITY_TIERS.map((tier) => ({
                  value: String(tier.id),
                  label: tier.label,
                  hint: tier.vip ? '需大会员' : undefined,
                }))}
                ariaLabel={t('settings.quality.fixedQuality')}
              />
            </FieldRow>
            <FieldRow
              label="自动回退"
              description="关闭后，指定档位不可用时任务会直接失败；开启则回退到实际最高可用档并在任务信息中标注"
            >
              <Switch
                checked={quality.fallback}
                onChange={(checked) => save({ quality: { fallback: checked } })}
                label="自动回退"
              />
            </FieldRow>
          </>
        )}
      </SettingsGroup>

      <SettingsGroup title="编码与音频">
        <FieldRow label={t('settings.quality.codec')} description={t('settings.quality.codec.desc')}>
          <SegmentedControl<CodecPreference>
            value={quality.codecPreference}
            onChange={(value) => save({ quality: { codecPreference: value } })}
            ariaLabel={t('settings.quality.codec')}
            options={[
              { value: 'auto', label: t('settings.quality.auto') },
              { value: 'avc', label: 'AVC' },
              { value: 'hevc', label: 'HEVC' },
              { value: 'av1', label: 'AV1' },
            ]}
          />
        </FieldRow>
        <FieldRow label={t('settings.quality.audio')} description={t('settings.quality.audio.desc')}>
          <SegmentedControl<AudioFormatPreference>
            value={quality.audioFormat}
            onChange={(value) => save({ quality: { audioFormat: value } })}
            ariaLabel={t('settings.quality.audio')}
            options={[
              { value: 'original', label: '原格式' },
              { value: 'mp3', label: 'MP3' },
            ]}
          />
        </FieldRow>
        <FieldRow label={t('settings.quality.metadata')} description={t('settings.quality.metadata.desc')}>
          <Switch
            checked={quality.writeMetadata}
            onChange={(checked) => save({ quality: { writeMetadata: checked } })}
            label={t('settings.quality.metadata')}
          />
        </FieldRow>
      </SettingsGroup>

      <SettingsGroup title="ffmpeg" description="用于 DASH 流合并与音频转码">
        <FieldRow label="状态">
          <div className="flex items-center gap-2">
            <StatusBadge tone={quality.ffmpegDetected ? 'success' : 'danger'} dot>
              {quality.ffmpegDetected
                ? t('settings.quality.ffmpeg.detected', { version: quality.ffmpegVersion ?? '' })
                : t('settings.quality.ffmpeg.missing')}
            </StatusBadge>
            <Button size="sm" variant="secondary" icon="refresh" onClick={detect}>
              {t('settings.quality.ffmpeg.recheck')}
            </Button>
          </div>
        </FieldRow>
        <FieldRow label="可执行路径">
          <TextField
            value={quality.ffmpegPath}
            onChange={(value) => save({ quality: { ffmpegPath: value } })}
            width={300}
            mono
            ariaLabel="ffmpeg 路径"
          />
        </FieldRow>
      </SettingsGroup>
    </div>
  );
}
