import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { App } from '../src/App';
import { MainWindow } from '../src/shell/MainWindow';
import {
  ValidationRetryPill,
  ValidatingPill,
  accountEntryView,
} from '../src/shell/AccountButton';
import { CoreErrorPanel } from '../src/components/CoreErrorPanel';
import { ANONYMOUS_ACCOUNT } from '../src/lib/adapt';
import { useAccountStore } from '../src/stores/useAccountStore';
import { useQueueStore } from '../src/stores/useQueueStore';
import { useSettingsStore } from '../src/stores/useSettingsStore';
import { useStartupStore } from '../src/stores/useStartupStore';

function installDomStubs(hash = ''): void {
  vi.stubGlobal('document', {
    documentElement: {
      dataset: { os: 'macos' },
      style: { setProperty: vi.fn(), colorScheme: '' },
    },
  });
  vi.stubGlobal('window', {
    location: { search: '?mock=1', hash, pathname: '/' },
    sessionStorage: { getItem: () => null, setItem: () => {} },
    localStorage: { getItem: () => null, setItem: () => {} },
    matchMedia: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
    requestAnimationFrame: () => 0,
    cancelAnimationFrame: () => {},
    setTimeout: globalThis.setTimeout.bind(globalThis),
    clearTimeout: globalThis.clearTimeout.bind(globalThis),
  });
}

beforeEach(() => {
  installDomStubs();
  useStartupStore.getState().reset();
  useQueueStore.setState({
    snapshot: null,
    loading: true,
    error: null,
    filesVerified: false,
    keyword: '',
    selectedTaskId: null,
  });
  useSettingsStore.getState().reset();
  useAccountStore.setState({
    account: null,
    validation: 'idle',
    validationGeneration: 0,
    validationError: null,
    challenge: null,
    loginState: 'idle',
    loginMessage: '',
    busy: false,
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('首屏先于网络就绪（tasks 5.1）', () => {
  it('主窗口初始渲染标题栏/窗口控制/骨架，不需要任何初始化数据', () => {
    const html = renderToStaticMarkup(<MainWindow />);
    expect(html).toContain('bilidown');
    expect(html).toContain('win-control');
    expect(html).toContain('animate-pulse');
    // 网络/账号未就绪不是错误态
    expect(html).not.toContain('引擎初始化失败');
    expect(html).not.toContain('账号校验未完成');
  });

  it('设置路由先渲染加载骨架，并保留窗口控制', () => {
    installDomStubs('#/preferences');
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain('正在加载偏好设置');
    expect(html).toContain('win-control');
  });

  it('核心初始化失败：可读原因与重试入口，依赖核心的区域不渲染', () => {
    const html = renderToStaticMarkup(
      <CoreErrorPanel message="引擎初始化失败：数据库迁移失败" retrying={false} onRetry={() => {}} />,
    );
    expect(html).toContain('引擎初始化失败');
    expect(html).toContain('数据库迁移失败');
    expect(html).toContain('重试初始化');
  });
});

describe('账号校验状态展示（tasks 5.4）', () => {
  it('校验中显示明确状态，不虚报未登录', () => {
    const html = renderToStaticMarkup(<ValidatingPill />);
    expect(html).toContain('账号校验中');
    expect(html).not.toContain('登录解锁 1080P');
  });

  it('网络降级时保留重试入口，不误判凭据失效', () => {
    const html = renderToStaticMarkup(<ValidationRetryPill retrying={false} />);
    expect(html).toContain('账号校验未完成，点击重试');
    expect(html).not.toContain('登录已失效');
  });

  it('入口状态：校验中/降级不显示未登录；校验结束后才显示登录入口', () => {
    expect(accountEntryView(null, 'idle')).toBe('loading');
    expect(accountEntryView(null, 'pending')).toBe('validating');
    expect(accountEntryView(null, 'degraded')).toBe('degraded');
    expect(accountEntryView({ ...ANONYMOUS_ACCOUNT }, 'pending')).toBe('validating');
    expect(accountEntryView({ ...ANONYMOUS_ACCOUNT }, 'degraded')).toBe('degraded');
    expect(accountEntryView({ ...ANONYMOUS_ACCOUNT }, 'ready')).toBe('login');
    // 账号分区独立加载失败：不显示成「未登录」，而是给重试入口
    expect(accountEntryView(null, 'ready', true)).toBe('degraded');
    expect(accountEntryView({ ...ANONYMOUS_ACCOUNT }, 'ready', true)).toBe('degraded');
    const signedIn = { ...ANONYMOUS_ACCOUNT, signedIn: true, mid: 1, name: '示例用户' };
    // 已登录账号在校验中仍保持登录展示（不误清除、不降级）
    expect(accountEntryView(signedIn, 'pending')).toBe('signed-in');
    expect(accountEntryView(signedIn, 'degraded')).toBe('signed-in');
  });
});
