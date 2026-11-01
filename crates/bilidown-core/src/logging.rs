//! tracing 初始化（desktop-shell spec「可诊断性」）：滚动日志写入应用数据目录。
//!
//! 壳在启动时调用 [`init`]；core 内部只发 `tracing` 事件，不打印 cookie 值。

use std::path::Path;
use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;

use crate::api::error::BiliResult;

static GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// 初始化 tracing：按天滚动写 `<log_dir>/bilidown.log`，同时输出到 stderr。
///
/// 可重复调用（已初始化时返回 Ok，不 panic）。
pub fn init(log_dir: &Path, default_level: &str) -> BiliResult<()> {
    std::fs::create_dir_all(log_dir)?;
    let appender = tracing_appender::rolling::daily(log_dir, "bilidown.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let _ = GUARD.set(guard);
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let result = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .try_init();
    match result {
        Ok(()) => {
            tracing::info!(log_dir = %log_dir.display(), "tracing initialized");
            Ok(())
        }
        Err(_) => Ok(()), // 已被其他初始化器占用（测试/嵌入）视为成功
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), "info").unwrap();
        init(dir.path(), "info").unwrap();
    }
}
