import { t } from '../i18n/zh-CN';
import { Icon } from './Icon';
import { Button } from './Button';

/** 本地核心初始化失败：窗口仍可见时的可读错误与重试入口（tasks 5.1/5.3）。 */
export function CoreErrorPanel({
  message,
  retrying = false,
  onRetry,
}: {
  message: string;
  retrying?: boolean;
  onRetry: () => void;
}) {
  return (
    <div role="alert" className="flex max-w-[400px] flex-col items-center gap-3 text-center">
      <Icon name="warning" size={28} className="text-danger" />
      <h1 className="text-sm font-semibold text-fg">{t('startup.core.title')}</h1>
      <p className="break-words text-xs leading-[18px] text-fg2">{message}</p>
      <p className="text-2xs leading-[16px] text-fg3">{t('startup.core.hint')}</p>
      <Button variant="primary" size="sm" icon="refresh" loading={retrying} onClick={onRetry}>
        {t('startup.core.retry')}
      </Button>
    </div>
  );
}
