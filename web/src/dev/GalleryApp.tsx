import '../i18n/zh-CN.dynamic';
import { useEffect, useState } from 'react';
import { devParams } from './scenario';
import { ERR_AUTH, ERR_FFMPEG, statesTasks } from './mockData';
import { useUiStore } from '../stores/useUiStore';
import { BrandMark } from '../components/BrandMark';
import { Button } from '../components/Button';
import { Checkbox } from '../components/Checkbox';
import { ConfirmDialogHost } from '../components/ConfirmDialog';
import { EmptyState } from '../components/EmptyState';
import { ErrorBanner } from '../components/ErrorBanner';
import { Icon, type IconName } from '../components/Icon';
import { NumberField } from '../components/NumberField';
import { PathField } from '../components/PathField';
import { ProgressBar } from '../components/ProgressBar';
import { SegmentedControl } from '../components/SegmentedControl';
import { SelectMenu } from '../components/SelectMenu';
import { Skeleton } from '../components/Skeleton';
import { StatusBadge } from '../components/StatusBadge';
import { Switch } from '../components/Switch';
import { TextField } from '../components/TextField';
import { ToastViewport } from '../components/Toast';
import { QueueRow } from '../views/QueueRow';
import { TrafficLights } from '../shell/TrafficLights';
import { WindowControls } from '../shell/WindowControls';

const ICONS: IconName[] = [
  'plus', 'minus', 'close', 'check', 'check-circle', 'chevron-down', 'chevron-right',
  'chevron-left', 'search', 'clipboard', 'star', 'clock', 'history', 'help', 'settings',
  'pause', 'play', 'download', 'arrow-down', 'arrow-left', 'trash', 'folder', 'copy',
  'more', 'external', 'refresh', 'user', 'reveal', 'warning', 'info', 'link', 'bell',
  'monitor', 'moon', 'sun', 'volume', 'wifi', 'lock', 'file', 'calendar', 'edit', 'loader',
  'film', 'music', 'book', 'grid', 'bangumi', 'list', 'filter', 'qr', 'diamond', 'image',
  'inbox', 'reset', 'dots',
];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-6">
      <h2 className="mb-2 text-2xs font-semibold uppercase tracking-wide text-fg3">{title}</h2>
      <div className="rounded-card border border-line p-4" style={{ background: 'var(--card)' }}>
        {children}
      </div>
    </section>
  );
}

/** 组件与状态矩阵页（#/gallery），用于 5.4 组件验收与截图自检 */
export function GalleryApp() {
  const [switch1, setSwitch1] = useState(true);
  const [switch2, setSwitch2] = useState(false);
  const [segment, setSegment] = useState<'a' | 'b' | 'c'>('a');
  const [number, setNumber] = useState(3);
  const [text, setText] = useState('');
  const [path, setPath] = useState('~/Downloads/bilidown');
  const [select, setSelect] = useState('best');
  const [checked, setChecked] = useState(true);
  const [checked2, setChecked2] = useState(false);
  const tasks = statesTasks();
  const pushToast = useUiStore((s) => s.pushToast);
  const confirm = useUiStore((s) => s.confirm);

  useEffect(() => {
    const raf = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        document.documentElement.dataset.ready = '1';
      }),
    );
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="flex h-full flex-col overflow-hidden" style={{ background: 'var(--bg-window)' }}>
      <header className="drag-region titlebar relative flex h-[var(--titlebar-h)] shrink-0 items-center gap-2">
        {devParams().mock && devParams().os !== 'windows' && <TrafficLights />}
        {devParams().mock && devParams().os === 'windows' && <WindowControls />}
        <span className="text-sm font-semibold text-fg">组件矩阵</span>
        <span className="text-2xs text-fg3">设计系统 5.4 自检</span>
        <a
          href="#/"
          className="ml-auto rounded-ctrl px-2 py-1 text-2xs text-fg2 hover:bg-hover hover:text-fg"
        >
          返回主窗口
        </a>
      </header>
      <main className="scroll-area min-h-0 flex-1 overflow-y-auto px-4 pb-6" style={{ background: 'var(--bg-content)' }}>
        <div className="mx-auto max-w-[1080px] pt-4">
          <Section title="品牌 / Logo / 空态">
            <div className="flex flex-wrap items-start gap-8">
              <BrandMark size={72} />
              <div className="min-w-[280px] flex-1">
                <EmptyState
                  brand
                  title="拖入或粘贴 B 站链接"
                  subtitle="支持视频 · 番剧 · 课程 · 音频，粘贴后自动解析"
                  hint="也可以按 ⌘O 打开链接面板"
                >
                  <Button variant="primary" icon="plus">
                    添加链接
                  </Button>
                </EmptyState>
              </div>
            </div>
          </Section>

          <Section title="图标（1.5px stroke，18/20px）">
            <div className="grid grid-cols-10 gap-y-3 sm:grid-cols-12 md:grid-cols-14">
              {ICONS.map((name) => (
                <div key={name} className="flex flex-col items-center gap-1 py-1 text-fg2">
                  <Icon name={name} size={18} />
                  <span className="text-[9px] leading-[11px] text-fg3">{name}</span>
                </div>
              ))}
            </div>
          </Section>

          <Section title="按钮">
            <div className="flex flex-wrap items-center gap-2">
              <Button variant="primary">主按钮</Button>
              <Button variant="primary" icon="download">主按钮 + 图标</Button>
              <Button variant="secondary">次按钮</Button>
              <Button variant="secondary" icon="folder">选择目录</Button>
              <Button variant="ghost" icon="settings">白描边</Button>
              <Button variant="danger" icon="trash">危险</Button>
              <Button variant="quiet">安静</Button>
            </div>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <Button size="sm" variant="primary">小按钮</Button>
              <Button size="sm" variant="secondary">小按钮</Button>
              <Button size="lg" variant="primary">大按钮</Button>
              <Button variant="primary" disabled>禁用</Button>
              <Button variant="primary" loading>处理中</Button>
            </div>
          </Section>

          <Section title="进度条 / 状态标签">
            <div className="grid max-w-[720px] gap-3">
              <ProgressBar value={0.42} tone="accent" height={8} />
              <ProgressBar value={1} tone="success" height={8} />
              <ProgressBar value={0.7} tone="danger" height={8} />
              <ProgressBar value={0.55} tone="neutral" height={8} />
              <ProgressBar value={0.2} tone="warning" height={8} />
              <ProgressBar value={0.4} tone="accent" height={8} indeterminate />
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              <StatusBadge tone="accent" dot>下载中</StatusBadge>
              <StatusBadge tone="success" icon="check-circle">已完成</StatusBadge>
              <StatusBadge tone="danger" icon="warning">失败</StatusBadge>
              <StatusBadge tone="neutral">已暂停</StatusBadge>
              <StatusBadge tone="warning">需大会员</StatusBadge>
              <StatusBadge tone="vip" icon="diamond">年度大会员</StatusBadge>
            </div>
          </Section>

          <Section title="开关 / 复选 / 分段">
            <div className="flex flex-wrap items-center gap-8">
              <div className="flex items-center gap-3">
                <Switch checked={switch1} onChange={setSwitch1} label="switch-on" />
                <Switch checked={switch2} onChange={setSwitch2} label="switch-off" />
                <Switch checked disabled onChange={() => {}} label="switch-disabled" />
              </div>
              <div className="flex items-center gap-3">
                <Checkbox checked={checked} onChange={setChecked} label="checked" />
                <Checkbox checked={checked2} onChange={setChecked2} label="unchecked" />
                <Checkbox checked={false} indeterminate onChange={() => {}} label="indeterminate" />
                <Checkbox checked disabled onChange={() => {}} label="disabled" />
              </div>
              <SegmentedControl
                value={segment}
                onChange={setSegment}
                options={[
                  { value: 'a', label: '跟随系统' },
                  { value: 'b', label: '浅色', icon: 'sun' },
                  { value: 'c', label: '深色', icon: 'moon' },
                ]}
              />
              <SegmentedControl
                size="sm"
                value={segment}
                onChange={setSegment}
                options={[
                  { value: 'a', label: '全部' },
                  { value: 'b', label: '已完成' },
                  { value: 'c', label: '失败' },
                ]}
              />
            </div>
          </Section>

          <Section title="输入控件">
            <div className="flex flex-wrap items-center gap-6">
              <div className="flex items-center gap-2">
                <span className="text-2xs text-fg2">NumberField</span>
                <NumberField value={number} onChange={setNumber} min={1} max={8} />
              </div>
              <div className="flex items-center gap-2">
                <span className="text-2xs text-fg2">TextField</span>
                <TextField value={text} onChange={setText} placeholder="搜索" width={180} />
              </div>
              <div className="flex items-center gap-2">
                <span className="text-2xs text-fg2">PathField</span>
                <PathField value={path} onChange={setPath} width={260} />
              </div>
              <div className="flex items-center gap-2">
                <span className="text-2xs text-fg2">SelectMenu</span>
                <SelectMenu
                  value={select}
                  onChange={setSelect}
                  options={[
                    { value: 'best', label: '自动选择最佳画质' },
                    { value: '1080', label: '1080P', hint: '推荐' },
                    { value: '720', label: '720P' },
                  ]}
                />
              </div>
              <div className="flex items-center gap-2">
                <span className="text-2xs text-fg2">invalid</span>
                <NumberField value={0} onChange={() => {}} min={1} max={8} invalid />
              </div>
            </div>
          </Section>

          <Section title="骨架 / Toast / 确认框">
            <div className="flex flex-wrap items-center gap-6">
              <div className="w-[320px] space-y-2">
                <Skeleton className="h-3 w-2/3" />
                <Skeleton className="h-2.5 w-1/3" />
                <Skeleton className="h-6 w-full" rounded="rounded-full" />
              </div>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  onClick={() => pushToast({ kind: 'success', message: '已添加 3 个任务' })}
                >
                  成功 Toast
                </Button>
                <Button
                  size="sm"
                  onClick={() => pushToast({ kind: 'error', message: '登录已失效，请重新扫码' })}
                >
                  错误 Toast
                </Button>
                <Button
                  size="sm"
                  onClick={() =>
                    void confirm({
                      title: '清空全部历史？',
                      body: '只删除历史记录，不会删除磁盘上的下载文件。',
                      confirmLabel: '清空',
                      danger: true,
                    })
                  }
                >
                  确认框
                </Button>
              </div>
            </div>
            <div className="mt-4 grid gap-3 md:grid-cols-2">
              <ErrorBanner error={ERR_AUTH} compact />
              <ErrorBanner error={ERR_FFMPEG} compact />
            </div>
          </Section>

          <Section title="队列行九态（104px 行卡 + 胶囊进度）">
            <div className="space-y-2">
              {tasks.map((task) => (
                <QueueRow key={task.id} task={task} />
              ))}
            </div>
          </Section>
        </div>
      </main>
      <ToastViewport />
      <ConfirmDialogHost />
    </div>
  );
}
