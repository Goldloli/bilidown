import { useEffect, useState } from 'react';
import { t } from '../../i18n/zh-CN';
import {
  glassUnavailableReason,
  isGlassSupported,
  shouldShowGlassSettings,
} from '../../lib/platform';
import type { Appearance } from '../../lib/types';
import {
  ACCENT_PRESETS,
  DEFAULT_ACCENT,
  accentForeground,
  applyAccent,
  normalizeAccent,
  readAccentOverride,
  readAccentPref,
  writeAccentPref,
} from '../../lib/accent';
import {
  MOTION_OPTIONS,
  applyMotion,
  normalizeMotion,
  readMotionOverride,
  readMotionPref,
  writeMotionPref,
  type MotionPreference,
} from '../../lib/motion';
import { FieldRow } from '../../components/FieldRow';
import { Icon } from '../../components/Icon';
import { SegmentedControl } from '../../components/SegmentedControl';
import { Switch } from '../../components/Switch';
import { SettingsGroup, useSettingsForm } from '../form';

const MOTION_KEYS: Record<MotionPreference, string> = {
  system: 'settings.interface.motion.system',
  reduce: 'settings.interface.motion.reduce',
  normal: 'settings.interface.motion.normal',
};

const PRESET_KEYS: Record<string, string> = {
  pink: 'settings.interface.accent.pink',
  blue: 'settings.interface.accent.blue',
  purple: 'settings.interface.accent.purple',
  green: 'settings.interface.accent.green',
  orange: 'settings.interface.accent.orange',
  red: 'settings.interface.accent.red',
  cyan: 'settings.interface.accent.cyan',
  gray: 'settings.interface.accent.gray',
};

export function InterfacePage() {
  const { settings, save } = useSettingsForm();
  const ui = settings.interface;
  const currentAccent = normalizeAccent(readAccentOverride() ?? ui.accentColor ?? readAccentPref());
  const currentMotion = normalizeMotion(readMotionOverride() ?? ui.motion ?? readMotionPref());
  const [hexDraft, setHexDraft] = useState(currentAccent);
  const [hexError, setHexError] = useState(false);

  useEffect(() => {
    setHexDraft(currentAccent);
    setHexError(false);
  }, [currentAccent]);

  const chooseAccent = (hex: string) => {
    const value = normalizeAccent(hex);
    applyAccent(value);
    writeAccentPref(value);
    setHexDraft(value);
    setHexError(false);
    save({ interface: { accentColor: value } });
  };

  const commitHex = () => {
    const bare = hexDraft.trim().replace(/^#/, '').toUpperCase();
    if (!/^[0-9A-F]{6}$/.test(bare)) {
      setHexError(true);
      return;
    }
    chooseAccent(`#${bare}`);
  };

  const chooseMotion = (value: MotionPreference) => {
    writeMotionPref(value);
    applyMotion(value);
    save({ interface: { motion: value } });
  };

  return (
    <div>
      <SettingsGroup title={t('settings.interface.appearance')} description={t('settings.interface.appearance.desc')}>
        <FieldRow label={t('settings.interface.appearance')}>
          <SegmentedControl<Appearance>
            value={ui.appearance}
            onChange={(value) => save({ interface: { appearance: value } })}
            ariaLabel={t('settings.interface.appearance')}
            options={[
              { value: 'system', label: t('settings.interface.system') },
              { value: 'light', label: t('settings.interface.light'), icon: 'sun' },
              { value: 'dark', label: t('settings.interface.dark'), icon: 'moon' },
            ]}
          />
        </FieldRow>
        {shouldShowGlassSettings() && (
          <FieldRow
            label="磨砂玻璃背景"
            description={
              isGlassSupported()
                ? '半透明窗口与浮层让系统材质透出；关闭后回退不透明。'
                : glassUnavailableReason()
            }
          >
            <Switch
              checked={isGlassSupported() && ui.glassBackground}
              onChange={(checked) => save({ interface: { glassBackground: checked } })}
              disabled={!isGlassSupported()}
              label="磨砂玻璃背景"
            />
          </FieldRow>
        )}
      </SettingsGroup>

      <SettingsGroup title={t('settings.interface.accent')} description={t('settings.interface.accent.desc')}>
        <div className="flex flex-wrap items-center gap-2 py-3">
          {ACCENT_PRESETS.map((preset) => {
            const selected = normalizeAccent(preset.color) === currentAccent;
            return (
              <button
                key={preset.id}
                type="button"
                aria-label={`${t((PRESET_KEYS[preset.id] ?? 'settings.interface.accent') as 'settings.interface.accent.pink')} ${preset.color}`}
                title={t((PRESET_KEYS[preset.id] ?? 'settings.interface.accent') as 'settings.interface.accent.pink')}
                onClick={() => chooseAccent(preset.color)}
                className={`relative grid h-7 w-7 place-items-center rounded-full border border-black/10 transition-transform duration-[var(--dur-fast)] hover:scale-105 ${
                  selected ? 'outline outline-2 outline-offset-2 outline-[var(--accent)]' : ''
                }`}
                style={{ background: preset.color }}
              >
                {selected && (
                  <span style={{ color: accentForeground(preset.color) }}>
                    <Icon name="check" size={13} strokeWidth={2.4} />
                  </span>
                )}
              </button>
            );
          })}
        </div>
        <FieldRow label={t('settings.interface.accent.custom')} description={t('settings.interface.accent.custom.desc')}>
          <div className="flex items-center gap-1.5">
            <input
              type="color"
              aria-label={t('settings.interface.accent.custom')}
              value={currentAccent.toLowerCase()}
              onChange={(event) => chooseAccent(event.target.value)}
              className="h-6 w-8 cursor-pointer rounded border border-line bg-transparent p-0"
            />
            <input
              type="text"
              aria-label={t('settings.interface.accent.hex')}
              value={hexDraft}
              spellCheck={false}
              onChange={(event) => {
                setHexDraft(event.target.value);
                setHexError(false);
              }}
              onBlur={commitHex}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  commitHex();
                }
              }}
              className={`num h-6 w-[86px] rounded-ctrl border bg-field px-2 text-2xs text-fg outline-none ${
                hexError ? 'border-danger' : 'border-field-border focus:border-accent'
              }`}
            />
            <button
              type="button"
              onClick={() => chooseAccent(DEFAULT_ACCENT)}
              className="inline-flex h-6 items-center gap-1 rounded-ctrl px-2 text-2xs text-fg2 hover:bg-hover hover:text-fg"
            >
              <Icon name="reset" size={11} />
              {t('settings.interface.accent.reset')}
            </button>
          </div>
        </FieldRow>
        {hexError && (
          <p className="pb-2 text-2xs text-danger">{t('settings.interface.accent.invalid')}</p>
        )}
      </SettingsGroup>

      <SettingsGroup title={t('settings.interface.motion')} description={t('settings.interface.motion.desc')}>
        <FieldRow label={t('settings.interface.motion')}>
          <SegmentedControl<MotionPreference>
            value={currentMotion}
            onChange={chooseMotion}
            ariaLabel={t('settings.interface.motion')}
            options={MOTION_OPTIONS.map((option) => ({
              value: option.value,
              label: t(MOTION_KEYS[option.value] as 'settings.interface.motion.system'),
            }))}
          />
        </FieldRow>
      </SettingsGroup>

      <p className="px-1 text-2xs leading-[15px] text-fg3">
        Dock 徽标与底栏状态展示由系统行为控制；成功/危险/警告等语义色不随强调色变化。
      </p>
    </div>
  );
}
