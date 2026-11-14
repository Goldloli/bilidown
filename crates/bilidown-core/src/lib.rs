//! bilidown-core：无 UI 依赖的下载引擎核心。
//!
//! 架构（OpenSpec change `rebuild-desktop-client` design D1–D7）：
//!
//! ```text
//! api/      B 站接口：client / error / types / wbi / parse / video / audio
//!           login / account / pcdn / subtitle
//!           source（UGC 合集/歌单共用的轻量列表模型）
//! download  流式 Range 下载（超时、续传、取消）
//! media     ffmpeg 集成、清晰度选择、文件名/落盘规则
//! storage   AppPaths / settings.json / credentials.json / SQLite（WAL + 迁移）
//! task      TaskManager（actor）/ 状态机 / 下载管线 / watch 快照 + broadcast 事件
//! logging   tracing 初始化
//! ```
//!
//! 该 crate **不允许**依赖任何 UI 框架（egui/eframe/tauri）；UI 文案、图标与
//! 本地化字符串一律由前端负责，core 只输出枚举与结构化数据。

pub mod api;
pub mod download;
pub mod logging;
pub mod media;
pub mod storage;
pub mod task;

// 常用类型便捷导出（壳层接线用）
pub use api::audio::{AudioMenu, AudioMenuPage};
pub use api::client::{BiliClient, DeviceCredentials, StoredCookies};
pub use api::error::{BiliError, BiliResult, ErrorKind};
pub use api::source::{SourceCollection, SourceItem, SourceKind};
pub use api::types::{
    AudioFormat, AudioQuality, BiliId, Codec, Content, ContentKind, Entry, MediaStream, PlayUrl,
    Progress, Quality, TaskSpec, UserInfo,
};
pub use api::video::{UgcEpisode, UgcSeason, UgcSection};
pub use storage::{
    AppPaths, Credentials, Db, HistoryQuery, HistoryRecord, MigrationReport, MotionPreference,
    Settings, Storage, TaskRecord, ThemePreference, UpdateProxyMode,
};
pub use task::{
    ActualParams, ArtifactKind, DownloadArtifact, DownloadPipeline, MAX_CONCURRENT, QueueSnapshot,
    TaskEvent, TaskExtras, TaskId, TaskManager, TaskManagerConfig, TaskMeta, TaskOutcome,
    TaskPhase, TaskResume, TaskRunner, TaskView, artifact_file_valid, completed_entry_indices,
};
