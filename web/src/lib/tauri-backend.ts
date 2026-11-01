import { CONTRACT_EVENTS, type ContractEventName } from './contract';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  AccountInfoDTO,
  AddOptionsDTO,
  AddTasksResultDTO,
  AppInfoDTO,
  HistoryEntryDTO,
  HistoryFilterDTO,
  LoginPollDTO,
  LoginStartDTO,
  MenuStateDTO,
  ParseResultDTO,
  QueueSnapshotDTO,
  SettingsDTO,
} from './contract';
import type { Backend } from './ipc';

// 共享数据事件各窗均接收；菜单、外部添加和退出确认只由主窗口执行。
const MAIN_WINDOW_EVENTS = new Set<ContractEventName>([
  CONTRACT_EVENTS.shellMenu,
  CONTRACT_EVENTS.shellDrop,
  CONTRACT_EVENTS.shellQuitRequested,
]);

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: call } = await import('@tauri-apps/api/core');
  return call<T>(cmd, args);
}

/** Tauri 2 后端适配层：命令名/参数名严格对齐 src-tauri/COMMANDS.md（camelCase） */
export function createTauriBackend(): Backend {
  return {
    getAppInfo: () => invoke<AppInfoDTO>('get_app_info'),
    openPreferences: () => invoke<void>('open_preferences'),
    quitApp: () => invoke<void>('quit_app'),
    getDiagnostics: () => invoke<string>('get_diagnostics'),

    parseInput: (input) => invoke<ParseResultDTO>('parse_input', { input }),
    addTasks: (input: string, options: AddOptionsDTO) =>
      invoke<AddTasksResultDTO>('add_tasks', { input, options }),
    listQueue: () => invoke<QueueSnapshotDTO>('list_queue'),
    pauseTask: (taskId) => invoke<void>('pause_task', { taskId }),
    resumeTask: (taskId) => invoke<void>('resume_task', { taskId }),
    cancelTask: (taskId) => invoke<void>('cancel_task', { taskId }),
    retryTask: (taskId) => invoke<void>('retry_task', { taskId }),
    removeTask: (taskId) => invoke<void>('remove_task', { taskId }),
    startAll: () => invoke<number>('start_all'),
    pauseAll: () => invoke<number>('pause_all'),
    retryAll: () => invoke<number>('retry_all'),
    clearFinished: () => invoke<number>('clear_finished'),
    clearInactive: () => invoke<number>('clear_inactive'),

    getAccount: () => invoke<AccountInfoDTO | null>('get_account'),
    loginStart: () => invoke<LoginStartDTO>('login_start'),
    loginPoll: () => invoke<LoginPollDTO>('login_poll'),
    loginLogout: () => invoke<void>('login_logout'),
    refreshAccount: () => invoke<AccountInfoDTO | null>('refresh_account'),

    getSettings: () => invoke<SettingsDTO>('get_settings'),
    setSettings: (settings) => invoke<SettingsDTO>('set_settings', { settings }),

    listHistory: (filter?: HistoryFilterDTO) =>
      invoke<HistoryEntryDTO[]>('list_history', { filter }),
    removeHistory: (id) => invoke<void>('remove_history', { id }),
    clearHistory: () => invoke<number>('clear_history'),

    pickDirectory: () => invoke<string | null>('pick_directory'),
    revealOutput: (taskId) => invoke<void>('reveal_output', { taskId }),
    revealPath: (path) => invoke<void>('reveal_path', { path }),
    openFile: (path) => invoke<void>('open_file', { path }),
    showLogs: () => invoke<void>('show_logs'),
    getMenuState: () => invoke<MenuStateDTO>('get_menu_state'),
    requestQuitCheck: () => invoke<boolean>('request_quit_check'),

    // EXTENSION（契约未列，已在交付说明中同步）
    copyText: (text) => invoke<void>('copy_text', { text }),
    readClipboardText: () => invoke<string>('read_clipboard_text'),
    openExternal: (url) => invoke<void>('open_external', { url }),
    detectFfmpeg: () =>
      invoke<{ detected: boolean; version: string | null; path: string | null }>('detect_ffmpeg'),
    testProxy: (proxy) => invoke<{ ok: boolean; message: string }>('test_proxy', { proxy }),
    fetchCover: (url) => invoke<string>('fetch_cover', { url }),

    on<T>(event: ContractEventName, handler: (payload: T) => void) {
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      void import('@tauri-apps/api/event')
        .then(({ listen }) => {
          const label = getCurrentWindow().label;
          if (cancelled || (label !== 'main' && MAIN_WINDOW_EVENTS.has(event))) return () => {};
          // 默认 Any listener 会接收 emit_to 的事件，须显式绑定本窗口目标。
          return listen<T>(event, (e) => handler(e.payload), { target: label });
        })
        .then((un) => {
          if (cancelled) un();
          else unlisten = un;
        })
        .catch(() => {
          /* 非 Tauri 环境静默失败 */
        });
      return () => {
        cancelled = true;
        unlisten?.();
        unlisten = null;
      };
    },
  };
}

export { CONTRACT_EVENTS };
