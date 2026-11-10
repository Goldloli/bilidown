import '../../i18n/zh-CN.dynamic';
import { useEffect, useState } from 'react';
import { t, type StringKey } from '../../i18n/zh-CN';
import { APP_BUILD, APP_VERSION } from '../../lib/constants';
import { api } from '../../lib/ipc';
import { formatBytes, formatSpeed } from '../../lib/format';
import { isPending, pendingLabel } from '../../lib/capabilities';
import {
  cancelAppUpdateDownload,
  checkAppUpdate,
  getAppUpdateState,
  installAppUpdate,
  openAboutLink,
  openDownloadedUpdate,
  openUpdateFileLocation,
  startAppUpdateDownload,
  subscribeAppUpdate,
  type AboutLink,
  type AppUpdateState,
} from '../../lib/updater';
import type { AppInfoDTO } from '../../lib/contract';
import { useUiStore } from '../../stores/useUiStore';
import { BrandMark } from '../../components/BrandMark';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { ProgressBar } from '../../components/ProgressBar';
import { SettingsGroup } from '../form';

const LINKS: AboutLink[] = ['repository', 'releases', 'issues', 'license', 'author', 'acknowledgements'];

const LINK_KEYS: Record<AboutLink, StringKey> = {
  repository: 'settings.about.repository',
  releases: 'settings.about.releases',
  issues: 'settings.about.issues',
  license: 'settings.about.license',
  author: 'settings.about.author',
  acknowledgements: 'settings.about.acknowledgements',
};

const IDLE_STATE: AppUpdateState = { status: 'idle', downloadedBytes: 0 };

/** Tauri 命令错误是对象，避免界面出现 [object Object] */
function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object') {
    const record = error as { message?: unknown; error?: unknown };
    const value = record.message ?? record.error;
    if (typeof value === 'string') return value;
    try {
      return JSON.stringify(error);
    } catch {
      return '未知错误';
    }
  }
  return String(error);
}

export function AboutPage() {
  const pushToast = useUiStore((s) => s.pushToast);
  const [info, setInfo] = useState<AppInfoDTO | null>(null);
  const [state, setState] = useState<AppUpdateState>(IDLE_STATE);
  const [busy, setBusy] = useState(false);

  const version = info?.version ?? APP_VERSION;
  const osLabel = info ? (info.os === 'windows' ? 'Windows' : info.os === 'macos' ? 'macOS' : info.os) : null;
  const archLabel = info ? (info.arch === 'x86_64' ? 'x64' : info.arch) : null;
  const build = info ? `Tauri ${info.tauriVersion} · ${osLabel} · ${archLabel}` : APP_BUILD;
  const platform = info ? `${osLabel} · ${archLabel}` : APP_BUILD;

  useEffect(() => {
    let mounted = true;
    api
      .getAppInfo()
      .then((result) => mounted && setInfo(result))
      .catch(() => undefined);
    getAppUpdateState()
      .then((result) => mounted && setState(result))
      .catch((error: unknown) => {
        if (!mounted) return;
        setState({ status: 'error', downloadedBytes: 0, error: errorText(error) });
      });
    const unsubscribe = subscribeAppUpdate((next) => setState(next));
    return () => {
      mounted = false;
      unsubscribe();
    };
  }, []);

  const toastError = (error: unknown) =>
    pushToast({
      kind: 'error',
      message: t('settings.about.openFailed', { message: errorText(error) }),
    });

  const check = async () => {
    setBusy(true);
    setState((prev) => ({ ...prev, status: 'checking', error: undefined }));
    try {
      const result = await checkAppUpdate();
      setState(
        result.updateAvailable
          ? {
              status: 'available',
              version: result.release?.version,
              downloadedBytes: 0,
              totalBytes: result.release?.assetSize,
            }
          : { status: 'up-to-date', version: result.currentVersion, downloadedBytes: 0, progress: 1 },
      );
    } catch (error) {
      setState({ status: 'error', downloadedBytes: 0, error: errorText(error) });
    } finally {
      setBusy(false);
    }
  };

  const download = async () => {
    if (!state.version) return;
    setBusy(true);
    try {
      setState(await startAppUpdateDownload(state.version));
      pushToast({ kind: 'info', message: t('settings.about.downloadStarted') });
    } catch (error) {
      setState({ status: 'error', downloadedBytes: 0, error: errorText(error) });
    } finally {
      setBusy(false);
    }
  };

  const cancelDownload = async () => {
    try {
      await cancelAppUpdateDownload();
      setState({ status: 'available', version: state.version, downloadedBytes: 0 });
      pushToast({ kind: 'info', message: t('settings.about.downloadCancelled') });
    } catch (error) {
      toastError(error);
    }
  };

  const openInstaller = async (reveal: boolean) => {
    try {
      if (reveal) await openUpdateFileLocation();
      else await openDownloadedUpdate();
    } catch (error) {
      toastError(error);
    }
  };

  /** Windows 一键更新：保存任务 → 退出 → 静默安装 → 自动重启（macOS 会返回错误，走打开安装包） */
  const installUpdate = async () => {
    try {
      await installAppUpdate();
    } catch (error) {
      toastError(error);
    }
  };

  const openLink = async (link: AboutLink) => {
    try {
      await openAboutLink(link);
    } catch (error) {
      toastError(error);
    }
  };

  const progress =
    state.progress ?? (state.totalBytes && state.totalBytes > 0 ? state.downloadedBytes / state.totalBytes : 0);

  return (
    <div>
      <div className="mb-6 flex flex-col items-center gap-2 py-2">
        <BrandMark size={72} />
        <div className="text-center">
          <h2 className="text-md font-semibold text-fg">bilidown</h2>
          <p className="mt-0.5 text-2xs text-fg2">B 站视频下载器 · 本地自用构建</p>
        </div>
      </div>

      <SettingsGroup title="版本与构建">
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">{t('settings.about.version')}</span>
          <span className="num text-2xs text-fg2">{version}</span>
        </div>
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">{t('settings.about.build')}</span>
          <span className="text-2xs text-fg2">{build}</span>
        </div>
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">{t('settings.about.platform')}</span>
          <span className="text-2xs text-fg2">{platform}</span>
        </div>
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">{t('settings.about.license')}</span>
          <span className="text-2xs text-fg2">{t('settings.about.licenseValue')}</span>
        </div>
      </SettingsGroup>

      <SettingsGroup title={t('settings.about.updates')}>
        <div className="py-2.5">
          {state.status === 'idle' && (
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-sm text-fg">{t('settings.about.idle')}</p>
                <p className="mt-0.5 text-2xs text-fg2">{t('settings.about.idleDesc')}</p>
              </div>
              <Button size="sm" variant="secondary" icon="refresh" onClick={() => void check()} disabled={busy}>
                {t('settings.about.check')}
              </Button>
            </div>
          )}

          {state.status === 'checking' && (
            <div className="flex items-center gap-2 text-sm text-fg2">
              <Icon name="loader" size={14} className="spinner text-accent" />
              {t('settings.about.checking')}
            </div>
          )}

          {state.status === 'up-to-date' && (
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-2">
                <Icon name="check-circle" size={15} className="shrink-0 text-success" />
                <div className="min-w-0">
                  <p className="text-sm text-fg">{t('settings.about.upToDate')}</p>
                  <p className="mt-0.5 text-2xs text-fg2">
                    {t('settings.about.upToDateDesc', { version: state.version ?? version })}
                  </p>
                </div>
              </div>
              <Button size="sm" variant="secondary" icon="refresh" onClick={() => void check()} disabled={busy}>
                {t('settings.about.checkAgain')}
              </Button>
            </div>
          )}

          {(state.status === 'available' || state.status === 'downloading' || state.status === 'downloaded') && (
            <div>
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-sm font-medium text-fg">
                    {state.status === 'downloaded'
                      ? t('settings.about.downloaded')
                      : t('settings.about.newVersion', {
                          version: state.version ?? '',
                        })}
                  </p>
                  <p className="mt-0.5 text-2xs text-fg2">
                    {state.status === 'downloaded'
                      ? t('settings.about.downloadedDesc', { filename: state.filename ?? '' })
                      : t('settings.about.newVersionDesc')}
                  </p>
                </div>
                <button
                  type="button"
                  className="inline-flex shrink-0 items-center gap-1 rounded-[5px] px-1.5 py-0.5 text-2xs text-accent hover:bg-accent-weak"
                  onClick={() => void openLink('releases')}
                >
                  <Icon name="external" size={11} />
                  {t('settings.about.viewRelease')}
                </button>
              </div>

              {state.status === 'downloading' && (
                <div className="mt-2.5">
                  <ProgressBar value={progress} ariaLabel={t('settings.about.downloading')} height={6} />
                  <div className="num mt-1.5 flex items-center justify-between text-2xs text-fg2">
                    <span>{t('settings.about.downloading')}</span>
                    <span>
                      {t('settings.about.downloadProgress', {
                        done: formatBytes(state.downloadedBytes),
                        total: formatBytes(state.totalBytes ?? 0),
                      })}
                      {state.bytesPerSecond ? ` · ${formatSpeed(state.bytesPerSecond)}` : ''}
                    </span>
                  </div>
                </div>
              )}

              <div className="mt-2.5 flex flex-wrap gap-1.5">
                {state.status === 'available' && (
                  <Button size="sm" variant="primary" icon="download" onClick={() => void download()} loading={busy}>
                    {t('settings.about.downloadUpdate')}
                  </Button>
                )}
                {state.status === 'downloading' && (
                  <Button size="sm" variant="secondary" icon="close" onClick={() => void cancelDownload()}>
                    {t('settings.about.cancelDownload')}
                  </Button>
                )}
                {state.status === 'downloaded' && (
                  <>
                    <Button size="sm" variant="primary" icon="download" onClick={() => void installUpdate()}>
                      {t('settings.about.installUpdate')}
                    </Button>
                    <Button size="sm" variant="secondary" icon="external" onClick={() => void openInstaller(false)}>
                      {t('settings.about.openInstaller')}
                    </Button>
                    <Button size="sm" variant="secondary" icon="reveal" onClick={() => void openInstaller(true)}>
                      {t('settings.about.revealInstaller')}
                    </Button>
                  </>
                )}
              </div>
            </div>
          )}

          {state.status === 'error' && (
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-2">
                <Icon name="warning" size={15} className="shrink-0 text-danger" />
                <div className="min-w-0">
                  <p className="text-sm text-fg">{t('settings.about.checkFailed')}</p>
                  <p className="mt-0.5 break-all text-2xs text-fg2">{state.error ?? ''}</p>
                </div>
              </div>
              <Button size="sm" variant="secondary" icon="refresh" onClick={() => void check()} disabled={busy}>
                {t('settings.about.retry')}
              </Button>
            </div>
          )}
        </div>
      </SettingsGroup>

      <SettingsGroup title={t('settings.about.links')}>
        {LINKS.map((link) => (
          <button
            key={link}
            type="button"
            onClick={() => void openLink(link)}
            className="flex w-full items-center justify-between py-2 text-left hover:text-accent"
          >
            <span className="text-sm text-fg">{t(LINK_KEYS[link])}</span>
            <Icon name="external" size={12} className="text-fg3" />
          </button>
        ))}
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">{t('settings.about.copyright')}</span>
          <span className="text-2xs text-fg2">{t('settings.about.copyrightValue')}</span>
        </div>
        <p className="pb-2.5 pt-1 text-2xs leading-[16px] text-fg2">{t('settings.about.ackDesc')}</p>
      </SettingsGroup>

      <SettingsGroup title={t('settings.about.diagnostics')}>
        <div className="flex flex-wrap items-center gap-2 py-2.5">
          <Button
            size="sm"
            variant="secondary"
            icon="file"
            onClick={() => {
              void api.revealLogs();
              pushToast({ kind: 'info', message: '已打开日志目录' });
            }}
          >
            {t('settings.about.logs')}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            icon="copy"
            disabled={isPending('copy-diagnostics')}
            title={pendingLabel('copy-diagnostics', t('settings.about.copyDiagnostics'))}
            onClick={() => {
              if (isPending('copy-diagnostics')) return;
              void api.copyDiagnostics().then(() =>
                pushToast({ kind: 'success', message: t('settings.about.copied') }),
              );
            }}
          >
            {t('settings.about.copyDiagnostics')}
          </Button>
        </div>
      </SettingsGroup>
    </div>
  );
}
