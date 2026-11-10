import { devParams } from '../dev/scenario';
import { BrandGlyph } from '../components/BrandMark';
import { Skeleton } from '../components/Skeleton';
import { useAsyncComponent } from '../lib/asyncComponent';
import { TrafficLights } from './TrafficLights';
import { WindowControls } from './WindowControls';

const loadAccountButton = () =>
  import('./AccountButton').then((module) => module.AccountButton);

/**
 * 标题栏：macOS 使用系统 overlay 交通灯（CSS 左侧留白 80px）；
 * Windows 由 `<WindowControls>` 自绘最小化/最大化/关闭三键（CSS 切换到右侧）。
 * Tauri 下由系统/自绘按钮负责交互，mock 环境仅绘制占位用于截图评审。
 */
export function TitleBar() {
  const mock = devParams().mock;
  const accountButton = useAsyncComponent(loadAccountButton, true);

  return (
    <header
      data-tauri-drag-region
      className="drag-region titlebar relative z-30 flex h-[var(--titlebar-h)] shrink-0 items-center gap-2"
    >
      {mock && devParams().os !== 'windows' && <TrafficLights />}
      <div className="titlebar-brand flex items-center gap-[7px]">
        <BrandGlyph size={17} />
        <span className="text-[13px] font-semibold leading-none tracking-[-0.01em] text-fg">
          bilidown
        </span>
      </div>
      <div className="no-drag ml-auto flex items-center gap-1.5">
        {accountButton.Page ? (
          <accountButton.Page />
        ) : (
          <Skeleton className="h-6 w-6" rounded="rounded-full" />
        )}
      </div>
      <WindowControls />
    </header>
  );
}
