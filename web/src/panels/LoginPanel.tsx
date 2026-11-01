import { useEffect, useRef } from 'react';
import { t } from '../i18n/zh-CN';
import { useAccountStore } from '../stores/useAccountStore';
import { useUiStore } from '../stores/useUiStore';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import { Avatar } from '../components/Avatar';
import { Sheet } from '../components/Sheet';
import { StatusBadge } from '../components/StatusBadge';

function QrCanvas({ modules }: { modules: boolean[][] }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || modules.length === 0) return;
    const dpr = window.devicePixelRatio || 1;
    const cssSize = 200;
    canvas.width = cssSize * dpr;
    canvas.height = cssSize * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    const dark = document.documentElement.dataset.theme === 'dark';
    const quiet = 4;
    const total = modules.length + quiet * 2;
    const cell = cssSize / total;
    ctx.fillStyle = dark ? '#1c1c1e' : '#ffffff';
    ctx.fillRect(0, 0, cssSize, cssSize);
    ctx.fillStyle = dark ? '#f5f5f7' : '#1d1d1f';
    for (let row = 0; row < modules.length; row++) {
      for (let col = 0; col < modules[row].length; col++) {
        if (!modules[row][col]) continue;
        const x = (col + quiet) * cell;
        const y = (row + quiet) * cell;
        ctx.fillRect(x, y, Math.ceil(cell), Math.ceil(cell));
      }
    }
  }, [modules]);

  return (
    <canvas
      ref={ref}
      width={200}
      height={200}
      role="img"
      aria-label="B 站登录二维码"
      className="h-[200px] w-[200px] rounded-[10px]"
      style={{ imageRendering: 'pixelated' }}
    />
  );
}

/** 扫码登录面板：二维码 canvas（深色主题反色）+ 五种状态 + 过期刷新 */
export function LoginPanel({ onClose }: { onClose: () => void }) {
  const challenge = useAccountStore((s) => s.challenge);
  const loginState = useAccountStore((s) => s.loginState);
  const loginMessage = useAccountStore((s) => s.loginMessage);
  const account = useAccountStore((s) => s.account);
  const startLogin = useAccountStore((s) => s.startLogin);
  const pollLogin = useAccountStore((s) => s.pollLogin);
  const cancelLogin = useAccountStore((s) => s.cancelLogin);
  const pushToast = useUiStore((s) => s.pushToast);

  useEffect(() => {
    void startLogin();
    return () => {
      void cancelLogin();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (loginState !== 'waiting' && loginState !== 'scanned') return;
    const timer = window.setInterval(() => {
      void pollLogin();
    }, 1500);
    return () => window.clearInterval(timer);
  }, [loginState, pollLogin]);

  useEffect(() => {
    if (loginState === 'success') {
      pushToast({ kind: 'success', message: t('toast.loginSuccess') });
    }
  }, [loginState, pushToast]);

  const close = () => {
    void cancelLogin();
    onClose();
  };

  const stateText: Record<string, string> = {
    idle: '正在获取二维码…',
    waiting: t('login.waiting'),
    scanned: t('login.scanned'),
    success: t('login.success'),
    expired: t('login.expired'),
    failed: t('login.failed'),
  };

  return (
    <Sheet open onClose={close} width={400} ariaLabel={t('login.title')}>
      <div className="flex items-start justify-between px-5 pb-1 pt-4">
        <div>
          <h2 className="text-md font-semibold text-fg">{t('login.title')}</h2>
          <p className="mt-1 max-w-[300px] text-2xs leading-[15px] text-fg2">{t('login.subtitle')}</p>
        </div>
        <button
          type="button"
          aria-label={t('common.close')}
          onClick={close}
          className="grid h-7 w-7 place-items-center rounded-tile text-fg3 hover:bg-hover hover:text-fg"
        >
          <Icon name="close" size={15} />
        </button>
      </div>

      <div className="flex flex-col items-center px-5 pb-2 pt-3">
        {loginState === 'success' ? (
          <div className="flex w-full flex-col items-center gap-3 py-3">
            <Avatar src={account?.avatarUrl} className="h-16 w-16" iconSize={30} />
            <div className="text-center">
              <div className="flex items-center justify-center gap-1.5">
                <span className="text-md font-semibold text-fg">{account?.name ?? '已登录'}</span>
                {account?.vip !== 'none' && (
                  <StatusBadge tone="vip" icon="diamond">
                    {account?.vip === 'annual' ? t('login.vip.annual') : t('login.vip.monthly')}
                  </StatusBadge>
                )}
              </div>
              <p className="mt-1 text-2xs text-fg2">
                {t('login.capability', { quality: account?.vip !== 'none' ? '4K' : '1080P' })}
              </p>
            </div>
            <Button variant="primary" onClick={close} className="mt-1">
              完成
            </Button>
          </div>
        ) : (
          <div className="relative">
            <div
              className={`rounded-[12px] p-2 ${
                loginState === 'expired' || loginState === 'failed' ? 'opacity-30' : ''
              } ${loginState === 'scanned' ? 'opacity-70' : ''}`}
              style={{ background: 'var(--card)' }}
            >
              {challenge ? (
                <QrCanvas modules={challenge.modules} />
              ) : (
                <div className="grid h-[200px] w-[200px] place-items-center bg-[var(--card-inset)]">
                  <Icon name="loader" size={24} className="spinner text-fg3" />
                </div>
              )}
            </div>
            {(loginState === 'expired' || loginState === 'failed') && (
              <div className="absolute inset-0 grid place-items-center">
                <Button
                  variant="primary"
                  size="sm"
                  icon="refresh"
                  onClick={() => void startLogin()}
                >
                  {t('login.refresh')}
                </Button>
              </div>
            )}
          </div>
        )}

        <div className="mt-3 flex items-center gap-1.5 text-center">
          {loginState === 'waiting' && <Icon name="qr" size={13} className="text-fg3" />}
          {loginState === 'scanned' && <Icon name="check-circle" size={13} className="text-success" />}
          {loginState === 'expired' && <Icon name="clock" size={13} className="text-warning" />}
          {loginState === 'failed' && <Icon name="warning" size={13} className="text-danger" />}
          <span className="text-xs text-fg2">{stateText[loginState] ?? loginMessage}</span>
        </div>
        {(loginState === 'waiting' || loginState === 'scanned') && challenge && (
          <p className="mt-1 text-2xs text-fg3">二维码 {challenge.expiresInSec} 秒后过期</p>
        )}
      </div>

      <div className="flex items-center justify-between border-t border-line px-5 py-3">
        <span className="flex items-center gap-1 text-2xs text-fg3">
          <Icon name="lock" size={11} />
          凭据仅保存在本机
        </span>
        <div className="flex gap-2">
          {loginState !== 'success' && (
            <Button variant="ghost" size="md" onClick={() => void startLogin()}>
              刷新二维码
            </Button>
          )}
          <Button variant="secondary" onClick={close}>
            {t('common.close')}
          </Button>
        </div>
      </div>
    </Sheet>
  );
}
