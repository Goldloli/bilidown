//! 任务模型与状态机（design D2/D3）。
//!
//! 状态：`Queued → Resolving → Downloading → Merging → Done`，
//! 旁路 `Paused` / `Failed(reason)` / `Cancelled`。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::api::error::BiliError;
use crate::api::types::{AudioQuality, Codec, ContentKind, Progress, Quality, TaskSpec};

/// 任务 id（UUID 字符串；所有操作以 id 寻址，避免 v1 下标错位）。
pub type TaskId = String;

/// 批量清理结果：真实移除数量 + 删除失败、仍保留在队列中的任务 id（design D2）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearOutcome {
    pub removed: usize,
    pub failed: Vec<TaskId>,
}

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Queued,
    Resolving,
    Downloading,
    Merging,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl TaskPhase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskPhase::Done | TaskPhase::Failed | TaskPhase::Cancelled
        )
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            TaskPhase::Resolving | TaskPhase::Downloading | TaskPhase::Merging
        )
    }

    pub fn is_queued(self) -> bool {
        matches!(self, TaskPhase::Queued)
    }

    pub fn can_pause(self) -> bool {
        self.is_queued() || self.is_active()
    }

    pub fn can_resume(self) -> bool {
        matches!(self, TaskPhase::Paused)
    }

    pub fn can_cancel(self) -> bool {
        !self.is_terminal() || self == TaskPhase::Cancelled
    }

    pub fn can_retry(self) -> bool {
        matches!(self, TaskPhase::Failed | TaskPhase::Cancelled)
    }

    pub fn can_remove(self) -> bool {
        true
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TaskPhase::Queued => "queued",
            TaskPhase::Resolving => "resolving",
            TaskPhase::Downloading => "downloading",
            TaskPhase::Merging => "merging",
            TaskPhase::Paused => "paused",
            TaskPhase::Done => "done",
            TaskPhase::Failed => "failed",
            TaskPhase::Cancelled => "cancelled",
        }
    }

    pub fn from_status(s: &str) -> TaskPhase {
        match s {
            "queued" => TaskPhase::Queued,
            "resolving" => TaskPhase::Resolving,
            "downloading" => TaskPhase::Downloading,
            "merging" => TaskPhase::Merging,
            "paused" => TaskPhase::Paused,
            "done" => TaskPhase::Done,
            "failed" => TaskPhase::Failed,
            "cancelled" => TaskPhase::Cancelled,
            _ => TaskPhase::Queued,
        }
    }
}

/// 一次下载任务的可序列化视图（watch 快照 + SQLite 持久化共用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskView {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub phase: TaskPhase,
    pub title: String,
    pub upper: String,
    pub cover_url: Option<String>,
    pub page_url: String,
    pub content_kind: ContentKind,
    /// 全部条目标题（resolve 后填充）。
    #[serde(default)]
    pub entries: Vec<String>,
    /// 实际生效的条目下标。
    #[serde(default)]
    pub selected_entries: Vec<usize>,
    pub progress: Progress,
    pub speed_bps: f64,
    pub output_path: Option<String>,
    pub file_size: Option<u64>,
    /// 本次任务成功发布的主产物；旧持久化记录可为空。
    #[serde(default)]
    pub artifacts: Vec<DownloadArtifact>,
    /// 由旧版单路径字段补出的兼容产物。
    #[serde(default)]
    pub artifacts_legacy: bool,
    pub error: Option<BiliError>,
    /// 是否由上次退出恢复（UI 提示「已从上次退出恢复」）。
    pub restored: bool,
    /// 回退后实际采用的清晰度/编码/音质。
    pub actual_quality: Option<Quality>,
    pub actual_codec: Option<Codec>,
    pub actual_audio: Option<AudioQuality>,
    /// 可选后处理产物（字幕/封面；失败不影响主任务）。旧数据反序列化时为空。
    #[serde(default)]
    pub extras: TaskExtras,
    /// 记录保存失败的可见状态（design D8/3.8）；成功后清空。旧数据反序列化为 None。
    #[serde(default)]
    pub persist_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 任务附加产物信息（design D2：字幕/封面可选后处理）。
///
/// 所有字段 `serde(default)`，旧快照/持久化数据反序列化不受影响；
/// 后处理失败只记录 `*_error`（任务本身仍为 Done）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskExtras {
    /// 已下载字幕文件（与视频同目录）。
    #[serde(default)]
    pub subtitles: Vec<String>,
    /// 已下载封面文件。
    #[serde(default)]
    pub cover: Option<String>,
    /// 字幕后处理失败原因（非致命）。
    #[serde(default)]
    pub subtitle_error: Option<String>,
    /// 封面后处理失败原因（非致命）。
    #[serde(default)]
    pub cover_error: Option<String>,
}

impl TaskExtras {
    pub fn is_empty(&self) -> bool {
        self.subtitles.is_empty()
            && self.cover.is_none()
            && self.subtitle_error.is_none()
            && self.cover_error.is_none()
    }

    /// 是否成功产出了附加文件。
    pub fn has_output(&self) -> bool {
        !self.subtitles.is_empty() || self.cover.is_some()
    }

    /// 合并另一个条目的附加信息（批量任务的多个产物）。
    pub fn merge(&mut self, other: TaskExtras) {
        self.subtitles.extend(other.subtitles);
        if self.cover.is_none() {
            self.cover = other.cover;
        }
        self.subtitle_error = merge_errors(self.subtitle_error.take(), other.subtitle_error);
        self.cover_error = merge_errors(self.cover_error.take(), other.cover_error);
    }
}

/// 持久化任务元信息（design D7）：只保存无法从请求参数或产物重算的展示事实。
///
/// 旧数据库记录没有该列内容（`{}`）；损坏或未知版本时只丢失元信息，任务本体仍可恢复。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskMetadata {
    /// 元信息结构版本；0 = 旧记录/未写入。
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub upper: String,
    #[serde(default)]
    pub entries: Vec<String>,
    #[serde(default)]
    pub selected_entries: Vec<usize>,
    #[serde(default)]
    pub actual_quality: Option<Quality>,
    #[serde(default)]
    pub actual_codec: Option<Codec>,
    #[serde(default)]
    pub actual_audio: Option<AudioQuality>,
    #[serde(default)]
    pub extras: TaskExtras,
}

/// 当前任务元信息结构版本。
pub const TASK_METADATA_VERSION: u32 = 1;

fn merge_errors(current: Option<String>, incoming: Option<String>) -> Option<String> {
    match (current, incoming) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

impl TaskView {
    /// 可持久化的元信息快照（design D7）。
    pub fn metadata(&self) -> TaskMetadata {
        TaskMetadata {
            version: TASK_METADATA_VERSION,
            upper: self.upper.clone(),
            entries: self.entries.clone(),
            selected_entries: self.selected_entries.clone(),
            actual_quality: self.actual_quality,
            actual_codec: self.actual_codec,
            actual_audio: self.actual_audio,
            extras: self.extras.clone(),
        }
    }

    /// 应用恢复出的元信息；只覆盖元信息字段，不伪造缺失值（design D7）。
    pub fn apply_metadata(&mut self, metadata: TaskMetadata) {
        self.upper = metadata.upper;
        self.entries = metadata.entries;
        self.selected_entries = metadata.selected_entries;
        self.actual_quality = metadata.actual_quality;
        self.actual_codec = metadata.actual_codec;
        self.actual_audio = metadata.actual_audio;
        self.extras = metadata.extras;
    }

    pub fn new(spec: TaskSpec) -> Self {
        let now = now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            page_url: spec.id.page_url(),
            content_kind: spec.id.kind(),
            title: spec.id.to_string(),
            upper: String::new(),
            cover_url: None,
            entries: Vec::new(),
            selected_entries: Vec::new(),
            phase: TaskPhase::Queued,
            progress: Progress::default(),
            speed_bps: 0.0,
            output_path: None,
            file_size: None,
            artifacts: Vec::new(),
            artifacts_legacy: false,
            error: None,
            restored: false,
            actual_quality: None,
            actual_codec: None,
            actual_audio: None,
            extras: TaskExtras::default(),
            persist_error: None,
            spec,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn upsert_artifact(&mut self, artifact: DownloadArtifact) {
        if let Some(existing) = self.artifacts.iter_mut().find(|current| {
            current.entry_index == artifact.entry_index && current.kind == artifact.kind
        }) {
            *existing = artifact;
        } else {
            self.artifacts.push(artifact);
        }
        self.refresh_artifact_projection();
    }

    pub fn merge_artifacts(&mut self, artifacts: impl IntoIterator<Item = DownloadArtifact>) {
        for artifact in artifacts {
            if let Some(existing) = self.artifacts.iter_mut().find(|current| {
                current.entry_index == artifact.entry_index && current.kind == artifact.kind
            }) {
                *existing = artifact;
            } else {
                self.artifacts.push(artifact);
            }
        }
        self.refresh_artifact_projection();
    }

    pub fn refresh_artifact_projection(&mut self) {
        if let Some(first) = self.artifacts.first() {
            self.output_path = Some(first.path.clone());
            self.file_size = Some(self.artifacts.iter().map(|artifact| artifact.size).sum());
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
    }

    pub fn is_active(&self) -> bool {
        self.phase.is_active()
    }

    /// 进度百分比（0.0–1.0）。
    pub fn fraction(&self) -> f32 {
        self.progress.fraction()
    }

    pub fn touch(&mut self) {
        self.updated_at = now_ms();
    }
}

/// 全局队列快照（watch 通道，≤10Hz 推送全量）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueSnapshot {
    /// 单调递增序号（前端可丢弃乱序旧快照）。
    pub seq: u64,
    pub tasks: Vec<TaskView>,
    pub active_count: u32,
    pub queued_count: u32,
    pub paused_count: u32,
    pub done_count: u32,
    pub failed_count: u32,
    pub total_speed_bps: f64,
}

impl QueueSnapshot {
    pub fn from_tasks(seq: u64, tasks: Vec<TaskView>) -> Self {
        let mut snap = QueueSnapshot {
            seq,
            tasks,
            ..Default::default()
        };
        for t in &snap.tasks {
            match t.phase {
                TaskPhase::Resolving | TaskPhase::Downloading | TaskPhase::Merging => {
                    snap.active_count += 1;
                    snap.total_speed_bps += t.speed_bps;
                }
                TaskPhase::Queued => snap.queued_count += 1,
                TaskPhase::Paused => snap.paused_count += 1,
                TaskPhase::Done => snap.done_count += 1,
                TaskPhase::Failed | TaskPhase::Cancelled => snap.failed_count += 1,
            }
        }
        snap
    }

    pub fn find(&self, id: &str) -> Option<&TaskView> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn has_unfinished(&self) -> bool {
        self.tasks.iter().any(|t| !t.phase.is_terminal())
    }
}

/// 当前 Unix 毫秒时间戳。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_helpers() {
        assert!(TaskPhase::Done.is_terminal());
        assert!(!TaskPhase::Paused.is_terminal());
        assert!(TaskPhase::Downloading.is_active());
        assert!(TaskPhase::Queued.can_pause());
        assert!(TaskPhase::Downloading.can_pause());
        assert!(TaskPhase::Paused.can_resume());
        assert!(!TaskPhase::Queued.can_resume());
        assert!(TaskPhase::Failed.can_retry());
        assert!(!TaskPhase::Done.can_retry());
        assert_eq!(TaskPhase::from_status("paused"), TaskPhase::Paused);
        assert_eq!(TaskPhase::from_status("unknown"), TaskPhase::Queued);
    }

    #[test]
    fn extras_serde_defaults_and_merge() {
        // 旧 JSON（没有 extras 字段）反序列化不受影响
        let legacy = serde_json::json!({
            "output_path": null,
            "file_size": 0
        });
        let outcome: TaskOutcome = serde_json::from_value(legacy).unwrap();
        assert!(outcome.extras.is_empty());
        assert!(outcome.artifacts.is_empty());
        assert!(!outcome.extras.has_output());
        // 合并
        let mut a = TaskExtras {
            subtitles: vec!["a.zh-CN.srt".into()],
            cover: None,
            subtitle_error: None,
            cover_error: None,
        };
        let b = TaskExtras {
            subtitles: vec!["b.en-US.srt".into()],
            cover: Some("b.jpg".into()),
            subtitle_error: None,
            cover_error: Some("cover timeout".into()),
        };
        a.merge(b);
        assert_eq!(a.subtitles.len(), 2);
        assert_eq!(a.cover.as_deref(), Some("b.jpg"));
        assert_eq!(a.cover_error.as_deref(), Some("cover timeout"));
        assert!(a.has_output());
        // 错误合并去重
        a.merge(TaskExtras {
            subtitle_error: Some("x".into()),
            ..Default::default()
        });
        a.merge(TaskExtras {
            subtitle_error: Some("y".into()),
            ..Default::default()
        });
        assert_eq!(a.subtitle_error.as_deref(), Some("x; y"));
        // 空 extras 序列化稳定
        let json = serde_json::to_value(TaskExtras::default()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"subtitles": [], "cover": null, "subtitle_error": null, "cover_error": null})
        );
    }

    #[test]
    fn task_view_legacy_json_without_extras() {
        let mut view = TaskView::new(TaskSpec::new(crate::api::types::BiliId::Video {
            bvid: "BV1".into(),
        }));
        view.extras.subtitles.push("x.srt".into());
        let value = serde_json::to_value(&view).unwrap();
        let mut legacy = value.clone();
        legacy.as_object_mut().unwrap().remove("extras");
        let back: TaskView = serde_json::from_value(legacy).unwrap();
        assert!(back.extras.is_empty(), "旧数据无 extras 时应回退默认值");
        let round: TaskView = serde_json::from_value(value).unwrap();
        assert_eq!(round.extras.subtitles, vec!["x.srt".to_string()]);
        assert!(round.artifacts.is_empty());
    }

    #[test]
    fn degraded_stream_pair_keeps_both_files_for_one_entry() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("entry.video.m4s");
        let audio = dir.path().join("entry.audio.m4s");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(&audio, b"audio").unwrap();
        let mut view = TaskView::new(TaskSpec::new(crate::api::types::BiliId::Video {
            bvid: "BV-streams".into(),
        }));
        view.selected_entries = vec![2];
        view.upsert_artifact(DownloadArtifact::stream(
            video,
            5,
            2,
            Some("P3".into()),
            ArtifactKind::VideoStream,
        ));
        view.upsert_artifact(DownloadArtifact::stream(
            audio,
            5,
            2,
            Some("P3".into()),
            ArtifactKind::AudioStream,
        ));

        assert_eq!(view.artifacts.len(), 2, "同一条目的视频/音频必须同时保留");
        assert!(completed_entry_indices(&view).contains(&2));
    }

    #[test]
    fn missing_or_wrong_size_artifact_is_not_completed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("entry.mp4");
        std::fs::write(&path, b"abc").unwrap();
        let mut view = TaskView::new(TaskSpec::new(crate::api::types::BiliId::Video {
            bvid: "BV-invalid".into(),
        }));
        view.selected_entries = vec![0];
        view.artifacts.push(DownloadArtifact::media(
            path,
            99,
            Some(0),
            Some("P1".into()),
        ));
        assert!(completed_entry_indices(&view).is_empty());
    }

    #[test]
    fn non_contiguous_valid_artifacts_do_not_skip_missing_middle_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entry2 = dir.path().join("p3.mp4");
        let entry12 = dir.path().join("p13.mp4");
        std::fs::write(&entry2, b"p3").unwrap();
        std::fs::write(&entry12, b"p13").unwrap();
        let mut view = TaskView::new(TaskSpec::new(crate::api::types::BiliId::Video {
            bvid: "BV-gaps".into(),
        }));
        view.selected_entries = vec![2, 7, 12];
        view.artifacts = vec![
            DownloadArtifact::media(entry2, 2, Some(2), Some("P3".into())),
            DownloadArtifact::media(
                dir.path().join("missing-p8.mp4"),
                3,
                Some(7),
                Some("P8".into()),
            ),
            DownloadArtifact::media(entry12, 3, Some(12), Some("P13".into())),
        ];

        let completed = completed_entry_indices(&view);
        assert!(completed.contains(&2));
        assert!(!completed.contains(&7));
        assert!(completed.contains(&12));
    }

    #[test]
    fn re_download_replaces_invalid_artifact_for_the_same_entry() {
        let dir = tempfile::tempdir().unwrap();
        let replacement = dir.path().join("p8-new.mp4");
        std::fs::write(&replacement, b"new").unwrap();
        let mut view = TaskView::new(TaskSpec::new(crate::api::types::BiliId::Video {
            bvid: "BV-replace".into(),
        }));
        view.selected_entries = vec![7];
        view.artifacts.push(DownloadArtifact::media(
            dir.path().join("p8-missing.mp4"),
            0,
            Some(7),
            Some("P8".into()),
        ));
        assert!(completed_entry_indices(&view).is_empty());

        view.upsert_artifact(DownloadArtifact::media(
            replacement,
            3,
            Some(7),
            Some("P8".into()),
        ));

        assert_eq!(view.artifacts.len(), 1);
        assert!(completed_entry_indices(&view).contains(&7));
    }

    #[test]
    fn snapshot_counts() {
        let mut views = Vec::new();
        for (i, phase) in [
            TaskPhase::Queued,
            TaskPhase::Downloading,
            TaskPhase::Merging,
            TaskPhase::Paused,
            TaskPhase::Done,
            TaskPhase::Failed,
            TaskPhase::Cancelled,
        ]
        .into_iter()
        .enumerate()
        {
            let mut v = TaskView::new(TaskSpec::new(crate::api::types::BiliId::Video {
                bvid: format!("BV{i}"),
            }));
            v.phase = phase;
            v.speed_bps = 100.0;
            views.push(v);
        }
        let snap = QueueSnapshot::from_tasks(1, views);
        assert_eq!(snap.active_count, 2);
        assert_eq!(snap.queued_count, 1);
        assert_eq!(snap.paused_count, 1);
        assert_eq!(snap.done_count, 1);
        assert_eq!(snap.failed_count, 2);
        assert_eq!(snap.total_speed_bps, 200.0);
        assert!(snap.has_unfinished());
    }
}

/// resolve 后的内容元信息（worker → manager 上报）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMeta {
    pub title: String,
    pub upper: String,
    pub cover_url: Option<String>,
    pub page_url: String,
    pub content_kind: ContentKind,
    pub entries: Vec<String>,
    pub selected_entries: Vec<usize>,
    pub duration_secs: Option<u64>,
}

/// 回退后实际采用的参数（回写任务信息）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActualParams {
    pub quality: Option<Quality>,
    pub codec: Option<Codec>,
    pub audio: Option<AudioQuality>,
}

/// 任务运行结果。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub output_path: Option<std::path::PathBuf>,
    pub file_size: u64,
    #[serde(default)]
    pub artifacts: Vec<DownloadArtifact>,
    /// 可选后处理产物（字幕/封面），随 Done 事件进入任务视图。
    #[serde(default)]
    pub extras: TaskExtras,
}

/// 恢复运行时需要的进度提示（暂存于任务视图 progress JSON）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskResume {
    pub completed_entries: HashSet<usize>,
    pub stage: crate::api::types::DownloadStage,
}

/// 一个成功发布的主产物。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadArtifact {
    pub path: String,
    pub size: u64,
    #[serde(default)]
    pub entry_index: Option<usize>,
    #[serde(default)]
    pub entry_title: Option<String>,
    pub kind: ArtifactKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Media,
    VideoStream,
    AudioStream,
}

impl DownloadArtifact {
    pub fn media(
        path: PathBuf,
        size: u64,
        entry_index: Option<usize>,
        entry_title: Option<String>,
    ) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            size,
            entry_index,
            entry_title,
            kind: ArtifactKind::Media,
        }
    }

    pub fn stream(
        path: PathBuf,
        size: u64,
        entry_index: usize,
        entry_title: Option<String>,
        kind: ArtifactKind,
    ) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            size,
            entry_index: Some(entry_index),
            entry_title,
            kind,
        }
    }
}

pub fn artifact_file_valid(artifact: &DownloadArtifact) -> bool {
    fs::metadata(Path::new(&artifact.path))
        .map(|metadata| metadata.is_file() && metadata.len() == artifact.size)
        .unwrap_or(false)
}

pub fn completed_entry_indices(view: &TaskView) -> HashSet<usize> {
    let selected: HashSet<usize> = if !view.selected_entries.is_empty() {
        view.selected_entries.iter().copied().collect()
    } else {
        view.spec
            .selected_entries
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect()
    };
    let mut counts: HashMap<(usize, ArtifactKind), usize> = HashMap::new();
    for artifact in &view.artifacts {
        let Some(entry_index) = artifact.entry_index else {
            continue;
        };
        if (!selected.is_empty() && !selected.contains(&entry_index))
            || !artifact_file_valid(artifact)
        {
            continue;
        }
        *counts.entry((entry_index, artifact.kind)).or_default() += 1;
    }
    let mut completed = HashSet::new();
    for (&(entry, kind), &count) in &counts {
        if count != 1 {
            continue;
        }
        if kind == ArtifactKind::Media
            || (counts.get(&(entry, ArtifactKind::VideoStream)) == Some(&1)
                && counts.get(&(entry, ArtifactKind::AudioStream)) == Some(&1))
        {
            completed.insert(entry);
        }
    }
    completed
}

/// worker 事件（进程内通道，由 manager 消费；不直接暴露给前端）。
#[derive(Debug)]
pub enum WorkerEvent {
    Meta {
        id: TaskId,
        meta: TaskMeta,
    },
    Phase {
        id: TaskId,
        phase: TaskPhase,
    },
    Progress {
        id: TaskId,
        progress: Progress,
        speed_bps: f64,
    },
    Actual {
        id: TaskId,
        actual: ActualParams,
    },
    Output {
        id: TaskId,
        artifact: DownloadArtifact,
        ack: tokio::sync::oneshot::Sender<crate::api::error::BiliResult<()>>,
    },
    Finished {
        id: TaskId,
        result: crate::api::error::BiliResult<TaskOutcome>,
        paused: bool,
    },
    /// 取消/移除后等待 worker 收敛的上限已到（design D2）：actor 收到后终止并回收 worker。
    Converge {
        id: TaskId,
    },
}
