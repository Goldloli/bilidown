import { useRef, useState } from 'react';
import { api } from '../lib/ipc';
import { useAccountStore } from '../stores/useAccountStore';
import { useUiStore } from '../stores/useUiStore';
import { Icon, type IconName } from '../components/Icon';
import { Avatar } from '../components/Avatar';
import { Popover } from '../components/Popover';
import { StatusBadge } from '../components/StatusBadge';
import { t } from '../i18n/zh-CN';

function qualityOf(vip: 'none' | 'monthly' | 'annual', signedIn: boolean): string {
  if (vip !== 'none') return '4K';
  return signedIn ? '1080P' : '480P';
}

/** 账户入口：未登录显示登录按钮；已登录显示头像 + 昵称 + 账户菜单 */
export function AccountButton() {
  const account = useAccountStore((s) => s.account);
  const openPanel = useUiStore((s) => s.openPanel);
  const pushToast = useUiStore((s) => s.pushToast);
  const confirm = useUiStore((s) => s.confirm);
  const [menuOpen, setMenuOpen] = useState(false);
  const anchorRef = useRef<HTMLButtonElement>(null);

  if (!account) {
    return <div className="h-7 w-14 animate-pulse rounded-full bg-[var(--rail-bg)]" />;
  }

  if (!account.signedIn) {
    return (
      <button
        type="button"
        onClick={() => openPanel({ type: 'login' })}
        className={`inline-flex h-7 items-center gap-1.5 rounded-full px-2.5 text-xs transition-colors duration-[var(--dur-fast)] ${
          account.expired
            ? 'bg-danger-weak text-danger hover:brightness-[0.98]'
            : 'bg-hover text-fg2 hover:bg-pressed hover:text-fg'
        }`}
      >
        <Icon name={account.expired ? 'warning' : 'user'} size={14} />
        <span className="hidden min-[820px]:inline">
          {account.expired ? t('account.expiredHint') : t('account.loginHint')}
        </span>
        <span className="min-[820px]:hidden">登录</span>
      </button>
    );
  }

  const vip = account.vip;
  const menuItem = (
    icon: IconName,
    label: string,
    onSelect: () => void,
    danger?: boolean,
  ) => (
    <button
      type="button"
      role="menuitem"
      onClick={() => {
        setMenuOpen(false);
        onSelect();
      }}
      className={`flex w-full items-center gap-2 rounded-[6px] px-2 py-[5px] text-left text-xs transition-colors duration-[var(--dur-fast)] ${
        danger ? 'text-danger hover:bg-danger-weak' : 'text-fg hover:bg-hover'
      }`}
    >
      <Icon name={icon} size={14} className={danger ? '' : 'text-fg2'} />
      <span className="flex-1">{label}</span>
    </button>
  );

  return (
    <>
      <button
        ref={anchorRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onClick={() => setMenuOpen((v) => !v)}
        className="inline-flex h-7 max-w-[180px] items-center gap-1.5 rounded-full bg-hover py-0 pl-1 pr-2 transition-colors duration-[var(--dur-fast)] hover:bg-pressed"
      >
        <Avatar src={account.avatarUrl} className="h-[22px] w-[22px]" iconSize={13} />
        <span className="hidden max-w-[120px] truncate text-xs text-fg min-[820px]:inline">
          {account.name}
        </span>
        {vip !== 'none' && (
          <Icon name="diamond" size={11} className="hidden shrink-0 text-accent min-[820px]:block" />
        )}
        <Icon name="chevron-down" size={10} className="hidden shrink-0 text-fg3 min-[820px]:block" />
      </button>
      <Popover
        open={menuOpen}
        anchorRef={anchorRef}
        onClose={() => setMenuOpen(false)}
        align="end"
        width={252}
      >
        <div className="flex items-center gap-2.5 px-2 pb-2 pt-1.5">
          <Avatar src={account.avatarUrl} className="h-9 w-9" iconSize={16} />
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <span className="truncate text-sm font-semibold text-fg">{account.name}</span>
              {vip !== 'none' && (
                <StatusBadge tone="vip" icon="diamond">
                  {vip === 'annual' ? t('login.vip.annual') : t('login.vip.monthly')}
                </StatusBadge>
              )}
            </div>
            <p className="mt-0.5 text-2xs text-fg2">
              可下载至 {qualityOf(vip, account.signedIn)}
            </p>
          </div>
        </div>
        <div className="my-1 h-px bg-line" />
        {menuItem('refresh', t('account.menu.refresh'), () => {
          void useAccountStore
            .getState()
            .refreshAccount()
            .then(() => pushToast({ kind: 'success', message: '登录态已刷新' }))
            .catch(() => pushToast({ kind: 'error', message: '刷新登录失败' }));
        })}
        {menuItem('settings', t('account.menu.settings'), () => {
          void api.openPreferencesWindow();
        })}
        {menuItem(
          'user',
          t('account.menu.logout'),
          () => {
            void confirm({
              title: t('account.logoutConfirm.title'),
              body: t('account.logoutConfirm.body'),
              confirmLabel: t('account.logoutConfirm.ok'),
              danger: true,
            }).then((ok) => {
              if (!ok) return;
              void useAccountStore.getState().logout().then(() => {
                pushToast({ kind: 'info', message: '已退出登录' });
              });
            });
          },
          true,
        )}
      </Popover>
    </>
  );
}
