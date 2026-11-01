//! Windows 系统托盘：显示主窗口 / 暂停全部 / 退出。
//!
//! 关闭主窗口时若托盘可用则隐藏到托盘（见 `window.rs`）；托盘创建失败时关闭即退出，
//! 避免出现「窗口隐藏后无法恢复」的状态。

use crate::window;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;
pub const TRAY_ID: &str = "main-tray";
pub const TRAY_SHOW: &str = "tray-show";
pub const TRAY_PAUSE_ALL: &str = "tray-pause-all";
pub const TRAY_QUIT: &str = "tray-quit";

/// 创建系统托盘图标与菜单；返回是否创建成功。
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id(TRAY_SHOW, "显示主窗口").build(app)?;
    let pause = MenuItemBuilder::with_id(TRAY_PAUSE_ALL, "暂停全部下载").build(app)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT, "退出 bilidown").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &pause, &quit]).build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("bilidown")
        .on_menu_event(|app, event| match event.id.as_ref() {
            TRAY_SHOW => window::show_main(app),
            TRAY_PAUSE_ALL => crate::menu::dispatch(app, "pause-all"),
            TRAY_QUIT => request_quit(app),
            // 托盘菜单事件与主菜单事件共用全局分发通道，未知 id 属于主菜单，直接忽略。
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    tracing::info!(target: "shell::tray", "系统托盘已创建");
    Ok(())
}

/// 托盘退出：有未完成任务时交给前端确认（`window::request_quit_check` 会 emit
/// `shell://quit-requested`），否则直接退出。
fn request_quit(app: &AppHandle) {
    if window::request_quit_check(app) {
        tracing::info!(target: "shell::tray", "托盘退出：有活跃任务，等待前端确认");
        return;
    }
    window::quit_now(app);
}
