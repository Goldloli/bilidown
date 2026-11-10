//! 事件协议：Rust → 前端的事件名与发送助手（design D8 + web/src/lib/contract.ts）。
//!
//! payload 一律为 `crate::dto` 中的 camelCase DTO。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 队列全量快照（≤10Hz，core watch 通道驱动）
pub const QUEUE_SNAPSHOT: &str = "queue://snapshot";
/// 任务重要事件（core broadcast 通道驱动）
pub const QUEUE_EVENTS: &str = "queue://events";
/// 登录态变化（payload: AccountInfoDTO | null）
pub const ACCOUNT_CHANGED: &str = "account://changed";
/// 设置变化（payload: SettingsDTO）
pub const SETTINGS_CHANGED: &str = "settings://changed";
/// 历史变化（payload: HistoryEntryDTO[]）
pub const HISTORY_CHANGED: &str = "history://changed";
/// 通用提示（payload: NoticeDTO）
pub const APP_NOTICE: &str = "app://notice";
/// 原生菜单动作（payload: MenuActionDTO）
pub const SHELL_MENU: &str = "shell://menu";
/// 拖拽进入/离开（payload: DragPayloadDTO）
pub const SHELL_DRAG: &str = "shell://drag";
/// 拖拽落点 / 单实例唤起链接（payload: DropPayloadDTO）
pub const SHELL_DROP: &str = "shell://drop";
/// 有活跃任务时请求退出（payload: QueueStatsDTO）
pub const SHELL_QUIT_REQUESTED: &str = "shell://quit-requested";
/// core 启动阶段（payload: CoreStatusDTO，design D5/5.2/5.4）
pub const CORE_STATUS: &str = "shell://core-status";

pub fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    if let Err(err) = app.emit(event, payload) {
        tracing::warn!(target: "shell::events", event, %err, "事件发送失败");
    }
}

/// 发送通用提示（info/success/warning/error）。
pub fn notice(app: &AppHandle, level: &str, message: impl Into<String>) {
    emit(app, APP_NOTICE, crate::dto::NoticeDTO::new(level, message));
}

/// 带业务 code 的提示（前端可据此显示可操作 toast，如剪贴板检测）。
pub fn notice_coded(app: &AppHandle, level: &str, code: &str, message: impl Into<String>) {
    let mut notice = crate::dto::NoticeDTO::new(level, message);
    notice.code = Some(code.to_string());
    emit(app, APP_NOTICE, notice);
}
