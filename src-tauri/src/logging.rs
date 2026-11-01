//! 壳层日志接线（tasks 4.1 / 4.5）。
//!
//! 4.5 起**落盘由 core 负责**：`bilidown_core::logging::init()` 初始化 tracing
//! （写 `<data>/logs/bilidown.log`，按天滚动）。壳不再自建 tracing subscriber，
//! 只保留 `tauri-plugin-log` 的 stdout target 用来捕获 tauri/wry 的 `log` 记录。
//!
//! （4.1 阶段曾用 tauri-plugin-log 直接写文件 + tracing→log 桥；接入 core 后为避免
//! 两份日志写同一文件而收敛，见 src-tauri/SHELL_NOTES.md。）

use bilidown_core::AppPaths;
use std::path::PathBuf;
use tauri::Wry;
use tauri::plugin::TauriPlugin;

/// 标准应用数据目录（与 core 一致；core 未初始化时用于兜底显示）。
pub fn paths() -> AppPaths {
    AppPaths::standard().unwrap_or_else(|_| {
        let base = dirs::data_dir().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join("Library/Application Support")
        });
        AppPaths::new(base.join(bilidown_core::storage::paths::APP_DIR_NAME))
    })
}

pub fn app_data_dir() -> PathBuf {
    paths().root().to_path_buf()
}

pub fn logs_dir() -> PathBuf {
    paths().logs_dir()
}

pub fn plugin() -> TauriPlugin<Wry> {
    use tauri_plugin_log::{Target, TargetKind};

    tauri_plugin_log::Builder::new()
        .clear_targets()
        // 仅 stdout：文件日志由 core 的 tracing subscriber 负责（避免双写同一文件）
        .target(Target::new(TargetKind::Stdout))
        .level(log::LevelFilter::Info)
        .level_for("tauri", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .level_for("tao", log::LevelFilter::Warn)
        .build()
}

/// 初始化 core tracing 日志（写 `<data>/logs/`）。
pub fn init_tracing(paths: &AppPaths) -> Result<(), String> {
    bilidown_core::logging::init(&paths.logs_dir(), "info")
        .map_err(|e| format!("初始化日志失败: {}", e.describe()))
}
