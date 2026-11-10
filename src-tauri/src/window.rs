//! 主窗口行为：关闭=隐藏、Dock 再次唤起、拖拽落点、退出保护（tasks 4.3 / 4.4）。

use crate::dto::{DragPayloadDTO, DropPayloadDTO};
use crate::events;
use crate::state::ShellState;
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

/// 系统托盘是否创建成功（Windows 关闭语义依赖它；macOS 恒为 false 且不使用）。
#[cfg(target_os = "windows")]
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
pub fn set_tray_available(available: bool) {
    TRAY_AVAILABLE.store(available, Ordering::Relaxed);
}

#[cfg(target_os = "windows")]
pub fn tray_available() -> bool {
    TRAY_AVAILABLE.load(Ordering::Relaxed)
}

/// 立即退出应用：先停止 core（保存队列/进度），再退出主循环。
/// 由托盘「退出」与 Windows「托盘不可用时关闭窗口」复用。
#[cfg(target_os = "windows")]
pub fn quit_now(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(core) = handle.state::<ShellState>().try_core()
            && let Err(err) = core.manager.shutdown().await
        {
            tracing::warn!(target: "shell::window", error = %err.describe(), "退出前 shutdown 失败");
        }
        handle.state::<ShellState>().confirm_quit();
        handle.exit(0);
    });
}

pub fn install(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!(target: "shell::window", "主窗口不存在，跳过窗口事件安装");
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => {
            // 窗口重新激活：失效文件状态展示缓存并通知前端刷新（design D9/7.2）。
            if let Some(core) = handle.state::<ShellState>().try_core() {
                core.file_status.invalidate_all();
            }
            crate::events::emit(&handle, crate::events::FILES_INVALIDATED, ());
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            #[cfg(target_os = "windows")]
            {
                if tray_available() {
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.hide();
                    }
                    tracing::info!(target: "shell::window", "关闭主窗口 → 隐藏到托盘（应用保持运行）");
                } else if !request_quit_check(&handle) {
                    // 托盘不可用：关闭即退出，避免窗口隐藏后无法恢复
                    tracing::info!(target: "shell::window", "托盘不可用且无活跃任务 → 退出应用");
                    quit_now(&handle);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Some(w) = handle.get_webview_window("main") {
                    let _ = w.hide();
                }
                tracing::info!(target: "shell::window", "关闭主窗口 → 隐藏（应用保持运行）");
            }
        }
        WindowEvent::DragDrop(drag) => match drag {
            tauri::DragDropEvent::Enter { .. } => {
                events::emit(&handle, events::SHELL_DRAG, DragPayloadDTO { active: true });
            }
            tauri::DragDropEvent::Over { .. } => {}
            tauri::DragDropEvent::Drop { paths, .. } => {
                events::emit(
                    &handle,
                    events::SHELL_DRAG,
                    DragPayloadDTO { active: false },
                );
                let payload = drop_payload(paths);
                tracing::info!(
                    target: "shell::window",
                    paths = payload.paths.len(),
                    text_bytes = payload.text.len(),
                    "拖拽落点"
                );
                events::emit(&handle, events::SHELL_DROP, payload);
                show_main(&handle);
            }
            tauri::DragDropEvent::Leave => {
                events::emit(
                    &handle,
                    events::SHELL_DRAG,
                    DragPayloadDTO { active: false },
                );
            }
            _ => {}
        },
        _ => {}
    });
}

/// 拖入内容：文件路径 + 可识别文本文件（.txt/.url/.webloc/.md）的内容。
/// 注意：macOS 原生 drag-drop 只携带文件路径；从浏览器拖选中文本/链接不会产生路径，
/// 该场景需前端 HTML5 drop 处理（window dragDropEnabled=false 时），作为已知限制记录。
fn drop_payload(paths: &[PathBuf]) -> DropPayloadDTO {
    let mut texts = Vec::new();
    for path in paths {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "txt" | "url" | "webloc" | "md")
            && let Ok(content) = std::fs::read_to_string(path)
        {
            texts.push(content);
        }
    }
    DropPayloadDTO {
        paths: paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        text: texts.join("\n"),
    }
}

/// 显示并聚焦主窗口（Dock 再次唤起 / 单实例二次启动 / 拖拽落点）。
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// dev 验证：触发与用户点关闭按钮相同的 CloseRequested 流程（应被拦截并隐藏）。
#[cfg(debug_assertions)]
pub fn close_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.close();
    }
}

/// dev 验证：主窗口当前是否可见。
#[cfg(debug_assertions)]
pub fn main_visible(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 退出保护：有活跃任务且未确认时，emit `shell://quit-requested`（payload=QueueSnapshot）
/// 并返回 true（应阻止退出）。
pub fn request_quit_check(app: &AppHandle) -> bool {
    let state = app.state::<ShellState>();
    if state.quit_confirmed() {
        return false;
    }
    let Some(core) = state.try_core() else {
        return false;
    };
    let snapshot = core.snapshot();
    let running = snapshot.active_count + snapshot.queued_count;
    if running == 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    show_main(app);
    events::emit(
        app,
        events::SHELL_QUIT_REQUESTED,
        crate::dto::stats_dto(&snapshot),
    );
    tracing::info!(target: "shell::window", running, "有活跃任务，请求前端确认退出");
    true
}

pub fn handle_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        // Reopen 为 macOS 专有事件（Windows 无 Dock，改由托盘/单实例唤起）。
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            tracing::info!(target: "shell::window", "Dock 再次唤起 → 显示主窗口");
            show_main(app);
        }
        RunEvent::ExitRequested { api, .. } => {
            tracing::info!(target: "shell::window", "收到 ExitRequested，检查退出保护");
            if request_quit_check(app) {
                api.prevent_exit();
                events::notice(
                    app,
                    "warning",
                    "有未完成的任务，退出前请确认（任务会暂停并在下次启动恢复）",
                );
            } else {
                tracing::info!(target: "shell::window", "无活跃任务，允许退出");
            }
        }
        _ => {}
    }
}

/// 更新安装：先优雅停止任务并退出，再启动 NSIS 安装器（`/S /UPDATE /R /ARGS=--updated`）。
/// 安装器必须在应用退出后运行，避免静默安装时强杀进程弄脏队列数据库。
#[cfg(target_os = "windows")]
pub fn quit_and_install(app: &AppHandle, installer: PathBuf) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(core) = handle.state::<ShellState>().try_core()
            && let Err(err) = core.manager.shutdown().await
        {
            tracing::warn!(target: "shell::window", error = %err.describe(), "更新前 shutdown 失败");
        }
        handle.state::<ShellState>().confirm_quit();
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            match std::process::Command::new(&installer)
                .args(["/S", "/UPDATE", "/R", "/ARGS=--updated"])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
            {
                Ok(_) => {
                    tracing::info!(target: "shell::update", path = %installer.display(), "已启动安装器，应用退出")
                }
                Err(err) => tracing::warn!(target: "shell::update", error = %err, "启动安装器失败"),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = installer;
        }
        handle.exit(0);
    });
}
