/** 开发/截图模式的路由与场景解析（仅 mock 模式使用） */

export type ScenarioKey =
  | 'demo'
  | 'empty'
  | 'queue'
  | 'states'
  | 'bulk'
  | 'add-link'
  | 'add-multi-p'
  | 'add-panel'
  | 'add-panel-preferences'
  | 'add-panel-no-fallback'
  | 'task-detail'
  | 'task-detail-failed'
  | 'task-detail-done'
  | 'task-detail-artifacts'
  | 'season-confirm'
  | 'login'
  | 'login-scanned'
  | 'login-expired'
  | 'login-failed'
  | 'login-success'
  | 'history'
  | 'history-artifacts'
  | 'history-legacy'
  | 'history-empty'
  | 'persist-error'
  | 'quit-failure'
  | 'errors';

export type RouteName = 'main' | 'preferences' | 'gallery';

export interface DevParams {
  mock: boolean;
  scenario: ScenarioKey;
  theme: 'light' | 'dark' | null;
  category: string;
  qa: boolean;
  route: RouteName;
  /** 截图/评审用平台覆盖：?os=windows 让 mock 呈现 Windows chrome 与文案 */
  os: 'windows' | 'macos' | null;
}

function collectParams(): URLSearchParams {
  const params = new URLSearchParams(window.location.search);
  const hash = window.location.hash;
  const q = hash.indexOf('?');
  if (q >= 0) {
    new URLSearchParams(hash.slice(q + 1)).forEach((value, key) => params.set(key, value));
  }
  return params;
}

export function readDevParams(): DevParams {
  const params = collectParams();
  const hash = window.location.hash;
  const route: RouteName = hash.startsWith('#/preferences')
    ? 'preferences'
    : hash.startsWith('#/gallery')
      ? 'gallery'
      : 'main';
  const themeParam = params.get('theme');
  const scenario = (params.get('scenario') as ScenarioKey | null) ?? 'queue';
  const isTauri = typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
  const mockParam = params.get('mock');
  const osParam = params.get('os');
  return {
    mock: mockParam === '1' || (!isTauri && mockParam !== '0'),
    scenario,
    theme: themeParam === 'light' || themeParam === 'dark' ? themeParam : null,
    category: params.get('category') ?? 'general',
    qa: params.get('qa') === '1',
    route,
    os: osParam === 'windows' || osParam === 'macos' ? osParam : null,
  };
}

let cached: DevParams | null = null;

export function devParams(): DevParams {
  cached ??= readDevParams();
  return cached;
}
