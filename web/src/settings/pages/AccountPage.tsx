import '../../i18n/zh-CN.dynamic';
import { t } from '../../i18n/zh-CN';
import { useAccountStore } from '../../stores/useAccountStore';
import { useUiStore } from '../../stores/useUiStore';
import { Button } from '../../components/Button';
import { Avatar } from '../../components/Avatar';
import { StatusBadge } from '../../components/StatusBadge';
import { SettingsGroup } from '../form';

export function AccountPage({ onLogin }: { onLogin: () => void }) {
  const account = useAccountStore((s) => s.account);
  const refreshAccount = useAccountStore((s) => s.refreshAccount);
  const logout = useAccountStore((s) => s.logout);
  const confirm = useUiStore((s) => s.confirm);
  const pushToast = useUiStore((s) => s.pushToast);

  if (!account) {
    return <div className="text-xs text-fg2">正在加载账号信息…</div>;
  }

  const vipLabel =
    account.vip === 'annual'
      ? t('login.vip.annual')
      : account.vip === 'monthly'
        ? t('login.vip.monthly')
        : null;

  return (
    <div>
      <SettingsGroup title={t('settings.account.status')}>
        <div className="flex items-center gap-3 py-3">
          <Avatar src={account.avatarUrl} className="h-12 w-12" iconSize={22} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <span className="text-sm font-semibold text-fg">
                {account.signedIn ? account.name : account.expired ? t('settings.account.expired') : t('settings.account.anonymous')}
              </span>
              {vipLabel && (
                <StatusBadge tone="vip" icon="diamond">
                  {vipLabel}
                </StatusBadge>
              )}
              {account.expired && <StatusBadge tone="danger">{t('login.expiredBadge')}</StatusBadge>}
            </div>
            <p className="mt-0.5 text-2xs text-fg2">
              {account.signedIn
                ? `UID ${account.mid ?? '—'} · ${t('login.capability', { quality: vipLabel ? '4K' : '1080P' })}`
                : '登录后可解锁 1080P 及以上清晰度'}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap gap-2 py-2.5">
          {account.signedIn ? (
            <>
              <Button
                size="sm"
                variant="secondary"
                icon="refresh"
                onClick={() => {
                  void refreshAccount().then(() =>
                    pushToast({ kind: 'success', message: '登录态已刷新' }),
                  );
                }}
              >
                {t('settings.account.refresh')}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                icon="user"
                className="text-danger hover:bg-danger-weak"
                onClick={() => {
                  void confirm({
                    title: t('account.logoutConfirm.title'),
                    body: t('account.logoutConfirm.body'),
                    confirmLabel: t('account.logoutConfirm.ok'),
                    danger: true,
                  }).then((ok) => {
                    if (ok)
                      void logout().then(() =>
                        pushToast({ kind: 'info', message: '已退出登录' }),
                      );
                  });
                }}
              >
                {t('settings.account.logout')}
              </Button>
            </>
          ) : (
            <Button size="sm" variant="primary" icon="qr" onClick={onLogin}>
              {t('settings.account.login')}
            </Button>
          )}
        </div>
      </SettingsGroup>

      <SettingsGroup title={t('settings.account.capability')} description={t('settings.account.capability.desc', {
        anon: '480P',
        signed: '1080P',
        vip: '4K',
      })}>
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">未登录</span>
          <div className="flex items-center gap-2">
            {!account.signedIn && <StatusBadge tone="neutral">当前</StatusBadge>}
            <span className="num text-2xs text-fg2">最高 480P</span>
          </div>
        </div>
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">登录账号</span>
          <div className="flex items-center gap-2">
            {account.signedIn && account.vip === 'none' && <StatusBadge tone="neutral">当前</StatusBadge>}
            <span className="num text-2xs text-fg2">最高 1080P</span>
          </div>
        </div>
        <div className="flex items-center justify-between py-2">
          <span className="text-sm text-fg">大会员</span>
          <div className="flex items-center gap-2">
            {account.vip !== 'none' && <StatusBadge tone="vip">当前</StatusBadge>}
            <span className="num text-2xs text-fg2">最高 4K</span>
          </div>
        </div>
      </SettingsGroup>
      <p className="px-1 text-2xs leading-[15px] text-fg3">
        登录状态仅保存在本机（0600 权限），不会上传 cookie；失效时会在主窗口提示重新扫码。
      </p>
    </div>
  );
}
