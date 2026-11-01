//! 原生菜单栏 + 快捷键 + 菜单可用性（tasks 4.3）。
//!
//! - macOS 应用菜单/文件/编辑/控制/账户/窗口/帮助，accelerator 在菜单项上声明；
//! - 菜单点击不直接操作 core，而是 emit `shell://menu`，由前端执行与按钮相同的 store action
//!   （design D8：菜单事件 emit 给前端执行）；
//! - 可用性随队列统计与登录态变化（`refresh`），快捷键随禁用态失效（macOS 原生行为）。

use crate::dto::{MenuActionDTO, MenuStateDTO};
use crate::events;
use crate::state::{MenuCounts, ShellState};
use std::collections::BTreeMap;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder, MenuItemKind, SubmenuBuilder};
#[cfg(target_os = "macos")]
use tauri::menu::AboutMetadata;
use tauri::{AppHandle, Manager, Wry};

/// 单项动作 id（快捷键在文件菜单上声明；控制菜单复用同一动作但不带快捷键）。
pub const ACTION_IDS: &[&str] = &[
    "open-link",
    "clipboard-add",
    "start-all",
    "pause-all",
    "retry-all",
    "clear-finished",
    "clear-inactive",
    "preferences",
    "login",
    "logout",
    "refresh-account",
    "show-logs",
    "copy-diagnostics",
    "quit",
];

// Windows 的 Win32 ACCEL 无法表达 Cmd（META）修饰键，muda 会拒绝该加速键并让菜单构建
// 报错（进而使 setup panic，见 add-windows-support）。因此：
// · 通用组合统一用 CmdOrCtrl，在非 macOS 上自然解析为 Ctrl；
// · macOS 的 ⌃⌘ 组合在 Windows 上用 Ctrl+Alt 对应，保持与 macOS 版一一对应。
#[cfg(target_os = "macos")]
const ACCEL_CLIPBOARD_ADD: &str = "Cmd+Ctrl+V";
#[cfg(not(target_os = "macos"))]
const ACCEL_CLIPBOARD_ADD: &str = "Ctrl+Alt+V";

#[cfg(target_os = "macos")]
const ACCEL_PAUSE_ALL: &str = "Cmd+Ctrl+S";
#[cfg(not(target_os = "macos"))]
const ACCEL_PAUSE_ALL: &str = "Ctrl+Alt+S";

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    // 关于元数据仅 macOS 应用菜单使用（Windows 见视图/帮助菜单）。
    #[cfg(target_os = "macos")]
    let about = {
        let version = app.package_info().version.to_string();
        AboutMetadata {
            name: Some("bilidown".into()),
            version: Some(version),
            comments: Some("小巧但有用的哔哩哔哩下载器".into()),
            ..Default::default()
        }
    };

    let prefs = MenuItemBuilder::with_id("preferences", "设置…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;

    // 应用菜单（仅 macOS：Windows 无「应用菜单」概念，退出放文件菜单）
    // 注意：不使用系统 Quit role —— macOS 原生 Cmd+Q 走 NSApp.terminate:，
    // 不经过 Tauri RunEvent::ExitRequested，无法实现退出保护（4.3 实测坑）。
    // 改为自定义菜单项 + accelerator，emit shell://menu 由前端确认后调 quit_app。
    let quit = MenuItemBuilder::with_id("quit", "退出 bilidown")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;
    #[cfg(target_os = "macos")]
    let app_menu = SubmenuBuilder::new(app, "bilidown")
        .about(Some(about))
        .separator()
        .item(&prefs)
        .separator()
        .services()
        .separator()
        .hide_with_text("隐藏 bilidown")
        .hide_others_with_text("隐藏其他")
        .show_all_with_text("显示全部")
        .separator()
        .item(&quit)
        .build()?;

    // 文件菜单（spec：至少包含这些项；⌘W 用系统 CloseWindow role）
    let open_link = MenuItemBuilder::with_id("open-link", "打开链接…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let clipboard_add = MenuItemBuilder::with_id("clipboard-add", "从剪贴板添加…")
        .accelerator(ACCEL_CLIPBOARD_ADD)
        .build(app)?;
    let start_all = MenuItemBuilder::with_id("start-all", "开始全部下载")
        .accelerator("CmdOrCtrl+S")
        .build(app)?;
    let pause_all = MenuItemBuilder::with_id("pause-all", "暂停全部下载")
        .accelerator(ACCEL_PAUSE_ALL)
        .build(app)?;
    let retry_all = MenuItemBuilder::with_id("retry-all", "重试全部下载")
        .accelerator("CmdOrCtrl+Shift+R")
        .build(app)?;
    let clear_finished = MenuItemBuilder::with_id("clear-finished", "清空已完成的下载")
        .accelerator("CmdOrCtrl+K")
        .build(app)?;
    let clear_inactive = MenuItemBuilder::with_id("clear-inactive", "清空未激活的下载")
        .accelerator("CmdOrCtrl+Shift+K")
        .build(app)?;
    // 文件菜单（macOS：⌘W 用系统 CloseWindow role；Windows：退出放文件菜单）
    #[cfg(target_os = "macos")]
    let file_menu = SubmenuBuilder::new(app, "文件")
        .item(&open_link)
        .item(&clipboard_add)
        .separator()
        .item(&start_all)
        .item(&pause_all)
        .item(&retry_all)
        .separator()
        .item(&clear_finished)
        .item(&clear_inactive)
        .separator()
        .close_window_with_text("关闭窗口")
        .build()?;
    #[cfg(not(target_os = "macos"))]
    let file_menu = SubmenuBuilder::new(app, "文件")
        .item(&open_link)
        .item(&clipboard_add)
        .separator()
        .item(&start_all)
        .item(&pause_all)
        .item(&retry_all)
        .separator()
        .item(&clear_finished)
        .item(&clear_inactive)
        .separator()
        .item(&quit)
        .build()?;

    // 编辑菜单
    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .undo_with_text("撤销")
        .redo_with_text("重做")
        .separator()
        .cut_with_text("剪切")
        .copy_with_text("复制")
        .paste_with_text("粘贴")
        .select_all_with_text("全选")
        .build()?;

    // 控制 / 账户 / 窗口菜单：仅 macOS（Windows 由界面按钮、账户入口与标题栏接管）
    #[cfg(target_os = "macos")]
    let (control_menu, account_menu, window_menu) = {
        // 控制菜单：与文件菜单同一套动作，但不占用快捷键（避免重复 accelerator）
        let ctl_start = MenuItemBuilder::with_id("ctl-start-all", "开始全部下载").build(app)?;
        let ctl_pause = MenuItemBuilder::with_id("ctl-pause-all", "暂停全部下载").build(app)?;
        let ctl_retry = MenuItemBuilder::with_id("ctl-retry-all", "重试全部下载").build(app)?;
        let ctl_clear_finished =
            MenuItemBuilder::with_id("ctl-clear-finished", "清空已完成的下载").build(app)?;
        let ctl_clear_inactive =
            MenuItemBuilder::with_id("ctl-clear-inactive", "清空未激活的下载").build(app)?;
        let control_menu = SubmenuBuilder::new(app, "控制")
            .item(&ctl_start)
            .item(&ctl_pause)
            .item(&ctl_retry)
            .separator()
            .item(&ctl_clear_finished)
            .item(&ctl_clear_inactive)
            .build()?;

        // 账户菜单
        let login = MenuItemBuilder::with_id("login", "扫码登录…").build(app)?;
        let logout = MenuItemBuilder::with_id("logout", "退出登录").build(app)?;
        let refresh_account =
            MenuItemBuilder::with_id("refresh-account", "刷新账号状态").build(app)?;
        let account_menu = SubmenuBuilder::new(app, "账户")
            .item(&login)
            .item(&logout)
            .separator()
            .item(&refresh_account)
            .build()?;

        // 窗口菜单
        let window_menu = SubmenuBuilder::new(app, "窗口")
            .minimize_with_text("最小化")
            .maximize_with_text("缩放")
            .fullscreen_with_text("进入全屏幕")
            .separator()
            .bring_all_to_front_with_text("前置全部窗口")
            .build()?;

        (control_menu, account_menu, window_menu)
    };

    // 帮助菜单
    let show_logs = MenuItemBuilder::with_id("show-logs", "显示日志").build(app)?;
    let copy_diagnostics =
        MenuItemBuilder::with_id("copy-diagnostics", "复制诊断信息").build(app)?;
    let help_menu = SubmenuBuilder::new(app, "帮助")
        .item(&show_logs)
        .item(&copy_diagnostics)
        .build()?;

    #[cfg(target_os = "macos")]
    let menu = MenuBuilder::new(app)
        .items(&[
            &app_menu,
            &file_menu,
            &edit_menu,
            &control_menu,
            &account_menu,
            &window_menu,
            &help_menu,
        ])
        .build()?;
    // Windows：文件 / 编辑 / 视图 / 帮助（无应用菜单、无窗口菜单）
    #[cfg(not(target_os = "macos"))]
    let menu = {
        let view_menu = SubmenuBuilder::new(app, "视图").item(&prefs).build()?;
        MenuBuilder::new(app)
            .items(&[&file_menu, &edit_menu, &view_menu, &help_menu])
            .build()?
    };
    app.set_menu(menu)?;

    refresh(app);
    #[cfg(target_os = "macos")]
    tracing::info!(target: "shell::menu", "原生菜单已安装（7 个顶层菜单，含应用菜单）");
    #[cfg(not(target_os = "macos"))]
    tracing::info!(target: "shell::menu", "原生菜单已安装（4 个顶层菜单：文件/编辑/视图/帮助）");
    Ok(())
}

/// 菜单点击：emit `shell://menu` 给前端；`ctl-` 前缀归一为动作 id。
pub fn dispatch(app: &AppHandle, raw_id: &str) {
    let action = raw_id.strip_prefix("ctl-").unwrap_or(raw_id);
    if ACTION_IDS.contains(&action) {
        let state = app.state::<ShellState>();
        let (counts, logged_in) = match state.try_core() {
            Some(core) => (core.menu_counts(), core.account().logged_in),
            None => (MenuCounts::default(), false),
        };
        // 隐藏菜单保留了 ACCEL 表，不能依赖 Win32 对灰色菜单项的拦截。
        if !action_enabled(action, &counts, logged_in) {
            return;
        }
        tracing::info!(target: "shell::menu", action, raw = raw_id, "菜单动作 → 前端");
        let payload = MenuActionDTO { id: action.to_string(), label: None };
        #[cfg(target_os = "windows")]
        {
            if matches!(action, "open-link" | "clipboard-add" | "clear-finished" | "clear-inactive" | "login" | "logout") {
                crate::window::show_main(app);
            }
            // 所有窗口都会订阅 shell://menu，只向主窗口发送，避免设置窗口重复执行。
            use tauri::Emitter;
            if let Err(err) = app.emit_to("main", events::SHELL_MENU, payload) {
                tracing::warn!(target: "shell::menu", %err, "菜单动作发送失败");
            }
        }
        #[cfg(not(target_os = "windows"))]
        events::emit(
            app,
            events::SHELL_MENU,
            payload,
        );
    } else {
        tracing::warn!(target: "shell::menu", raw = raw_id, "未映射的菜单 id");
    }
}

pub fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    dispatch(app, event.id().as_ref());
}

/// 递归查找菜单项：`Menu::get` 只查顶层，子菜单里的项必须自己遍历（4.3 踩坑）。
fn find_item(items: Vec<MenuItemKind<Wry>>, id: &str) -> Option<MenuItem<Wry>> {
    for item in items {
        match item {
            MenuItemKind::MenuItem(item) => {
                if item.id().as_ref() == id {
                    return Some(item);
                }
            }
            MenuItemKind::Submenu(submenu) => {
                if let Ok(children) = submenu.items()
                    && let Some(found) = find_item(children, id)
                {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn find(menu: &tauri::menu::Menu<Wry>, id: &str) -> Option<MenuItem<Wry>> {
    find_item(menu.items().ok()?, id)
}

fn set_enabled(menu: &tauri::menu::Menu<Wry>, id: &str, enabled: bool) {
    // PredefinedMenuItem 不支持 set_enabled（系统 role），find 只返回自定义 MenuItem。
    if let Some(item) = find(menu, id) {
        let current = item.is_enabled().unwrap_or(false);
        if current != enabled
            && let Err(err) = item.set_enabled(enabled)
        {
            tracing::warn!(target: "shell::menu", id, %err, "设置菜单项可用性失败");
        }
    }
}

fn set_pair(menu: &tauri::menu::Menu<Wry>, id: &str, enabled: bool) {
    set_enabled(menu, id, enabled);
    set_enabled(menu, &format!("ctl-{id}"), enabled);
}

/// 根据 core 队列快照 / 登录态刷新菜单可用性（spec「菜单可用性」）。
pub fn refresh(app: &AppHandle) {
    let Some(menu) = app.menu() else {
        return;
    };
    let state = app.state::<ShellState>();
    let (counts, logged_in) = match state.try_core() {
        Some(core) => (core.menu_counts(), core.account().logged_in),
        None => (MenuCounts::default(), false),
    };
    apply_rules(&menu, &counts, logged_in);
}

fn apply_rules(menu: &tauri::menu::Menu<Wry>, counts: &MenuCounts, logged_in: bool) {
    for action in ACTION_IDS {
        set_pair(menu, action, action_enabled(action, counts, logged_in));
    }
}

/// 展示和执行共用同一套规则，隐藏菜单后禁用项也不会绕过检查。
fn action_enabled(action: &str, counts: &MenuCounts, logged_in: bool) -> bool {
    match action {
        "start-all" => counts.can_start_all(),
        "pause-all" => counts.can_pause_all(),
        "retry-all" => counts.can_retry_all(),
        "clear-finished" => counts.can_clear_finished(),
        "clear-inactive" => counts.can_clear_inactive(),
        "login" => !logged_in,
        "logout" | "refresh-account" => logged_in,
        _ => ACTION_IDS.contains(&action),
    }
}

/// dev 用：读取菜单项实际 enabled 状态，验证「无任务置灰」。
pub fn enabled_map(app: &AppHandle) -> MenuStateDTO {
    let mut enabled = BTreeMap::new();
    let Some(menu) = app.menu() else {
        return MenuStateDTO { enabled };
    };
    let ids = ACTION_IDS.iter().map(|id| id.to_string()).chain(
        [
            "start-all",
            "pause-all",
            "retry-all",
            "clear-finished",
            "clear-inactive",
        ]
        .iter()
        .map(|id| format!("ctl-{id}")),
    );
    for id in ids {
        if let Some(item) = find(&menu, &id) {
            enabled.insert(id, item.is_enabled().unwrap_or(false));
        }
    }
    MenuStateDTO { enabled }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_queue_blocks_hidden_menu_accelerators() {
        let counts = MenuCounts::default();
        for action in ["start-all", "pause-all", "retry-all", "clear-finished", "clear-inactive"] {
            assert!(!action_enabled(action, &counts, false), "{action}");
        }
        for action in ["open-link", "clipboard-add", "preferences", "quit"] {
            assert!(action_enabled(action, &counts, false), "{action}");
        }
    }

    #[test]
    fn menu_actions_follow_current_queue_and_account_state() {
        let counts = MenuCounts { paused: 1, failed: 1, ..Default::default() };
        assert!(action_enabled("start-all", &counts, false));
        assert!(action_enabled("retry-all", &counts, false));
        assert!(action_enabled("clear-finished", &counts, false));
        assert!(action_enabled("clear-inactive", &counts, false));
        assert!(!action_enabled("pause-all", &counts, false));
        assert!(action_enabled("pause-all", &MenuCounts { active: 1, ..Default::default() }, false));
        assert!(action_enabled("login", &counts, false));
        assert!(!action_enabled("logout", &counts, false));
        assert!(!action_enabled("login", &counts, true));
        assert!(action_enabled("logout", &counts, true));
        assert!(!action_enabled("unknown", &counts, true));
    }
}
