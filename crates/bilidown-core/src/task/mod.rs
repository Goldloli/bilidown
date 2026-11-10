//! 任务域：actor 化的 TaskManager + 状态机 + 下载管线（design D2/D3/D4）。
//!
//! 对外 API：
//! - [`TaskManager::start`] 创建 manager（恢复上次队列）；
//! - 命令方法（enqueue/pause/resume/cancel/retry/remove/clear_finished 等）返回
//!   `BiliResult<()>`，内部通过 mpsc + oneshot 由监督任务串行处理；
//! - [`TaskManager::subscribe_snapshot`] 全量快照（watch，≤10Hz）；
//! - [`TaskManager::subscribe_events`] 结构化事件（broadcast，立即推送）。

pub mod event;
pub mod manager;
pub mod pipeline;
pub mod state;

pub use event::TaskEvent;
pub use manager::{MAX_CONCURRENT, TaskManager, TaskManagerConfig};
pub use pipeline::{DownloadPipeline, TaskReporter, TaskRunCtx, TaskRunner};
pub use state::{
    ActualParams, ArtifactKind, ClearOutcome, DownloadArtifact, QueueSnapshot, TaskExtras, TaskId,
    TaskMeta, TaskOutcome, TaskPhase, TaskResume, TaskView, artifact_file_valid,
    completed_entry_indices,
};
