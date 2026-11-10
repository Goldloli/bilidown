import '../i18n/zh-CN.dynamic';
import { useState } from 'react';
import { t } from '../i18n/zh-CN';
import { handleParseText } from '../lib/actions';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import { Sheet } from '../components/Sheet';

/** 添加链接面板：多行输入、粘贴即识别 */
export function AddLinkPanel({
  open,
  initialText,
  onClose,
}: {
  open: boolean;
  initialText?: string;
  onClose: () => void;
}) {
  const [text, setText] = useState(initialText ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!text.trim()) {
      setError('请粘贴至少一个 B 站链接');
      return;
    }
    setBusy(true);
    setError(null);
    const result = await handleParseText(text);
    setBusy(false);
    // 需要确认时 handleParseText 已切换到添加确认面板；此时不能再关闭（否则会把新面板一起关掉）
    if (result && !result.requiresConfirmation) {
      setText('');
      onClose();
    }
  };

  return (
    <Sheet open={open} onClose={onClose} width={560} ariaLabel={t('bottombar.add')}>
      <div className="flex items-start justify-between px-5 pb-2 pt-4">
        <div>
          <h2 className="text-md font-semibold text-fg">添加 B 站链接</h2>
          <p className="mt-1 text-2xs leading-[15px] text-fg2">
            多条链接以空格或回车分隔；支持视频 / 番剧 / 课程 / 音频
          </p>
        </div>
        <button
          type="button"
          aria-label={t('common.close')}
          onClick={onClose}
          className="grid h-7 w-7 place-items-center rounded-tile text-fg3 hover:bg-hover hover:text-fg"
        >
          <Icon name="close" size={15} />
        </button>
      </div>
      <div className="px-5 pb-3">
        <textarea
          autoFocus
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              void submit();
            }
          }}
          placeholder={'粘贴 B 站链接，每行一条；例如：\nhttps://www.bilibili.com/video/BV1…'}
          className={`h-[136px] w-full resize-none rounded-ctrl border bg-field p-2.5 text-xs leading-[18px] text-fg outline-none placeholder:text-fg3 ${
            error ? 'border-danger' : 'border-field-border focus:border-accent'
          }`}
        />
        {error && (
          <p className="mt-1.5 flex items-center gap-1 text-2xs text-danger">
            <Icon name="warning" size={11} />
            {error}
          </p>
        )}
        <p className="mt-2 flex items-center gap-1 text-2xs text-fg3">
          <Icon name="info" size={11} />
          解析后若包含多个条目，会打开确认面板；单条内容按默认清晰度直接入队
        </p>
      </div>
      <div className="flex items-center justify-between border-t border-line px-5 py-3">
        <span className="text-2xs text-fg3">⌘O 打开 · Esc 关闭</span>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={onClose}>
            {t('add.cancel')}
          </Button>
          <Button variant="primary" loading={busy} onClick={() => void submit()}>
            解析链接
          </Button>
        </div>
      </div>
    </Sheet>
  );
}
