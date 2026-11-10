import { useRef, useState } from 'react';
import { api } from '../lib/ipc';
import { retryStartupNetwork, retryStartupSection } from '../lib/bootstrap';
import { useAccountStore } from '../stores/useAccountStore';
import { useStartupStore } from '../stores/useStartupStore';
import { useUiStore } from '../stores/useUiStore';
import { Icon, type IconName } from '../components/Icon';
import { Avatar } from '../components/Avatar';
import { Popover } from '../components/Popover';
import { StatusBadge } from '../components/StatusBadge';
import { t } from '../i18n/zh-CN';
import type { AccountState, AccountValidation } from '../lib/types';

/** 账号入口展示状态：校验中/降级不显示成「未登录」（tasks 5.4）。 */
export type AccountEntryView = 'loading' | 'validating' | 'degraded' | 'login' | 'signed-in';

export function accountEntryView(
  account: AccountState | null,
  validation: AccountValidation,
  accountLoadFailed = false,
): AccountEntryView {
  const signedIn = Boolean(account?.signedIn);
  if (!signedIn && validation === 'pending') return 'validating';
  if (!signedIn && validation === 'degraded') return 'degraded';
  // 账号分区独立加载失败：提供重试入口，而不是显示成「未登录」。
  if (!signedIn && accountLoadFailed) return 'degraded';
  if (!account) return 'loading';
  if (!account.signedIn) return 'login';
  return 'signed-in';
}

function qualityOf(vip: 'none' | 'monthly' | 'annual', signedIn: boolean): string {
  if (vip !== 'none') return '4K';
  return signedIn ? '1080P' : '480P';
}

/** 账户入口：未登录显示登录按钮；已登录显示头像 + 昵称 + 账户菜单 */
export function ValidatingPill() {
  return (
    <button
      type="button"
      disabled
      aria-label={t('account.validating')}
      title={t('account.validationPendingTitle')}
      className="inline-flex h-7 cursor-default items-center gap-1.5 rounded-full bg-hover px-2.5 text-xs text-fg3"
    >
      <Icon name="loader" size={13} className="animate-spin" />
      <span className="hidden min-[820px]:inline">{t('account.validating')}</span>
      <span className="min-[820px]:hidden">校验中</span>
    </button>
  );
}

export function ValidationRetryPill({
  retrying = false,
  onRetry,
}: {
  retrying?: boolean;
  onRetry?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={() => (onRetry ? onRetry() : void retryStartupNetwork())}
      disabled={retrying}
      aria-label={t('account.validationDegraded')}
      title={t('account.validationDegraded')}
      className="inline-flex h-7 items-center gap-1.5 rounded-full bg-danger-weak px-2.5 text-xs text-danger transition-colors duration-[var(--dur-fast)] hover:brightness-[0.98] disabled:opacity-60"
    >
      <Icon
        name={retrying ? 'loader' : 'warning'}
        size={14}
        className={retrying ? 'animate-spin' : ''}
      />
      <span className="hidden min-[820px]:inline">{t('account.validationDegraded')}</span>
      <span className="min-[820px]:hidden">校验未完成</span>
    </button>
  );
}

export function AccountButton() {
  const account = useAccountStore((s) => s.account);
  const validation = useAccountStore((s) => s.validation);
  const retryingNetwork = useStartupStore((s) => s.retryingNetwork);
  const accountLoadFailed = useStartupStore((s) => s.sections.account.status === 'error');
  const openPanel = useUiStore((s) => s.openPanel);
  const pushToast = useUiStore((s) => s.pushToast);
  const confirm = useUiStore((s) => s.confirm);
  const [menuOpen, setMenuOpen] = useState(false);
  const anchorRef = useRef<HTMLButtonElement>(null);

  const view = accountEntryView(account, validation, accountLoadFailed);
  const retryAccount = () => {
    if (accountLoadFailed) void retryStartupSection('account');
    else void retryStartupNetwork();
  };
  // 账号校验未完成时不虚报「未登录」（tasks 5.4）；网络降级/加载失败时提供重试入口。
  if (view === 'validating') return <ValidatingPill />;
  if (view === 'degraded') {
    return <ValidationRetryPill retrying={retryingNetwork} onRetry={retryAccount} />;
  }
  if (!account || view === 'loading') {
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
        {validation === 'pending' && (
          <Icon
            name="loader"
            size={10}
            className="hidden shrink-0 animate-spin text-fg3 min-[820px]:block"
          />
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
