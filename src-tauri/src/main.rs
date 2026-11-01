//! bilidown Tauri 2 壳（tasks 4.1–4.5）。
//!
//! 分层：`commands`（IPC 入口）/ `dto`（core→前端契约映射）/ `state`（core 接线 + 事件桥）
//! / `menu`（原生菜单）/ `window`（窗口行为与退出保护）/ `dock`（Dock 徽标）
//! / `covers`（封面抓取缓存）/ `logging`（日志插件与目录）。
//!
//! 注意：`dev_*` 调试命令仅在 debug 构建注册；release 包（`pnpm tauri build`）不含 dev 命令，
//! 也不打包 `src-tauri/dev/`（frontendDist 指向 `../web/dist`）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod covers;
mod dock;
mod dto;
mod energy;
mod events;
mod logging;
mod maintenance;
mod menu;
mod platform;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod preferences_window;
mod state;
#[cfg(target_os = "windows")]
mod tray;
mod update;
mod window;
#[cfg(target_os = "windows")]
mod windows_chrome;
#[cfg(target_os = "windows")]
mod windows_shadow;

use tauri::Manager;

fn main() {
    let builder = tauri::Builder::default()
        // 单实例必须最先注册（design D8）
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            window::show_main(app);
            let links = link_args(&argv);
            if links.is_empty() {
                events::notice(app, "info", "bilidown 已在运行，已切换到主窗口");
            } else {
                tracing::info!(target: "shell::app", links = links.len(), "单实例唤起携带链接");
                events::emit(
                    app,
                    events::SHELL_DROP,
                    dto::DropPayloadDTO {
                        paths: Vec::new(),
                        text: links.join("\n"),
                    },
                );
            }
        }))
        .plugin(logging::plugin())
        .plugin({
            let window_state = tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                );
            // macOS / Windows 设置每次相对主窗口定位；主窗口仍恢复原有状态。
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let window_state = window_state.skip_initial_state("preferences");
            window_state.build()
        })
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init());

    // 正式命令（debug/release 都有）
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        // 应用
        commands::get_app_info,
        commands::open_preferences,
        commands::quit_app,
        commands::get_diagnostics,
        maintenance::get_storage_stats,
        maintenance::clear_app_cache,
        maintenance::open_data_dir,
        // 解析 / 队列
        commands::parse_input,
        commands::add_tasks,
        commands::list_queue,
        commands::pause_task,
        commands::resume_task,
        commands::cancel_task,
        commands::retry_task,
        commands::remove_task,
        commands::start_all,
        commands::pause_all,
        commands::retry_all,
        commands::clear_finished,
        commands::clear_inactive,
        // 账户 / 登录
        commands::get_account,
        commands::login_start,
        commands::login_poll,
        commands::login_logout,
        commands::refresh_account,
        // 设置
        commands::get_settings,
        commands::set_settings,
        // 历史
        commands::list_history,
        commands::remove_history,
        commands::clear_history,
        // 系统集成
        commands::pick_directory,
        commands::reveal_output,
        commands::reveal_path,
        commands::open_file,
        commands::show_logs,
        commands::get_menu_state,
        commands::request_quit_check,
        // EXTENSION（web/src/lib/contract.ts）
        commands::copy_text,
        commands::read_clipboard_text,
        commands::open_external,
        commands::detect_ffmpeg,
        commands::test_proxy,
        commands::fetch_cover,
        // UGC 合集
        commands::resolve_ugc_season,
        // 应用内更新（add-download-features 4.1-4.3）
        commands::get_app_update_state,
        commands::get_app_version,
        commands::check_app_update,
        commands::start_app_update_download,
        commands::cancel_app_update_download,
        commands::open_downloaded_update,
        commands::open_update_file_location,
        commands::open_about_link,
        update::install_app_update,
    ]);

    // debug 构建额外注册 dev_*（release 包不包含）
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::get_app_info,
        commands::open_preferences,
        commands::quit_app,
        commands::get_diagnostics,
        maintenance::get_storage_stats,
        maintenance::clear_app_cache,
        maintenance::open_data_dir,
        commands::parse_input,
        commands::add_tasks,
        commands::list_queue,
        commands::pause_task,
        commands::resume_task,
        commands::cancel_task,
        commands::retry_task,
        commands::remove_task,
        commands::start_all,
        commands::pause_all,
        commands::retry_all,
        commands::clear_finished,
        commands::clear_inactive,
        commands::get_account,
        commands::login_start,
        commands::login_poll,
        commands::login_logout,
        commands::refresh_account,
        commands::get_settings,
        commands::set_settings,
        commands::list_history,
        commands::remove_history,
        commands::clear_history,
        commands::pick_directory,
        commands::reveal_output,
        commands::reveal_path,
        commands::open_file,
        commands::show_logs,
        commands::get_menu_state,
        commands::request_quit_check,
        commands::copy_text,
        commands::read_clipboard_text,
        commands::open_external,
        commands::detect_ffmpeg,
        commands::test_proxy,
        commands::fetch_cover,
        // UGC 合集
        commands::resolve_ugc_season,
        // 应用内更新（add-download-features 4.1-4.3）
        commands::get_app_update_state,
        commands::get_app_version,
        commands::check_app_update,
        commands::start_app_update_download,
        commands::cancel_app_update_download,
        commands::open_downloaded_update,
        commands::open_update_file_location,
        commands::open_about_link,
        update::install_app_update,
        commands::dev_report,
        commands::dev_menu_state,
        commands::dev_menu_dispatch,
        commands::dev_notify,
        commands::dev_set_badge,
        commands::dev_get_badge,
        commands::dev_emit_notice,
        commands::dev_sample_output,
        commands::dev_stat_file,
        commands::dev_close_window,
        commands::dev_show_window,
        commands::dev_window_visible,
        commands::dev_request_quit,
    ]);

    let app = builder
        .setup(|app| {
            // 配置里的 windowEffects 在部分 macOS/Tauri 组合下不会生效；
            // 显式给主窗口再设一次，保证启动后磨砂玻璃可复现。
            #[cfg(target_os = "macos")]
            if let Some(win) = app.get_webview_window("main") {
                let effects = tauri::window::EffectsBuilder::new()
                    .effect(tauri::window::Effect::LiquidGlassRegular)
                    .effect(tauri::window::Effect::UnderWindowBackground)
                    .state(tauri::window::EffectState::Active)
                    .build();
                if let Err(err) = win.set_effects(effects) {
                    tracing::warn!(target: "shell::window", error = %err, "主窗口设置磨砂玻璃失败");
                }
            }
            app.manage(state::ShellState::new());
            app.manage(update::AppUpdateState::default());
            // 更新完成后的自动重启（--updated）不重复检查更新
            if !std::env::args().any(|arg| arg == "--updated") {
                update::startup_check_interface();
            }
            menu::build(app.handle())?;
            #[cfg(target_os = "windows")]
            if let Some(win) = app.get_webview_window("main") {
                // 保留菜单 accelerator，隐藏会与自绘标题栏重叠的原生横条。
                win.hide_menu()?;
                windows_chrome::install(&win)?;
                win.show()?;
            }
            window::install(app.handle());
            // Windows：系统托盘（关闭窗口隐藏到托盘；创建失败则关闭即退出）
            #[cfg(target_os = "windows")]
            match tray::install(app.handle()) {
                Ok(()) => window::set_tray_available(true),
                Err(err) => tracing::warn!(
                    target: "shell::tray",
                    error = %err,
                    "系统托盘创建失败：关闭窗口将退出应用"
                ),
            }
            state::spawn_init(app.handle().clone());
            tracing::info!(
                target: "shell::app",
                version = %app.package_info().version,
                "bilidown shell 启动完成，core 后台初始化中"
            );
            Ok(())
        })
        .on_menu_event(menu::on_menu_event)
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");

    app.run(window::handle_run_event);
}

/// 从单实例唤起参数中提取 B 站链接（macOS 通过 open -a / URL scheme 唤起时 argv 带链接）。
fn link_args(argv: &[String]) -> Vec<String> {
    argv.iter()
        .filter(|arg| {
            let lower = arg.to_lowercase();
            lower.contains("bilibili.com")
                || lower.contains("b23.tv")
                || arg.contains("BV")
                || lower.starts_with("av")
        })
        .cloned()
        .collect()
}
