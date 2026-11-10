//! IPC DTO 层（tasks 4.5）：把 `bilidown-core` 的 snake_case 类型映射为
//! `web/src/lib/contract.ts` 约定的 camelCase DTO。
//!
//! 前端契约来源：`web/src/lib/contract.ts`（ui-engineer 产出）。
//! - 命令成功返回：本文件 DTO（camelCase）；
//! - 命令失败返回：`AppErrorDTO { kind, message, actions }`（camelCase）；
//! - 事件 payload：本文件 DTO（camelCase）。
//!
//! 命名对照：core `TaskView` → `TaskDTO`、core `QueueSnapshot` → `QueueSnapshotDTO`、
//! core `BiliError` → `AppErrorDTO`、core `Settings` ↔ `SettingsDTO`（双向）。

use bilidown_core::api::account::{AccountState, LoginOutcome};
use bilidown_core::api::login::{LoginState, QrLogin};
use bilidown_core::api::parse::ParsedInput;
use bilidown_core::api::types::{
    AudioFormat, AudioQuality, Codec, Content, ContentKind, PlayUrl, Quality, TierUnavailable,
};
use bilidown_core::media::ffmpeg::Ffmpeg;
use bilidown_core::storage::{HistoryRecord, MotionPreference, Settings, ThemePreference};
use bilidown_core::{
    BiliError, DownloadArtifact, QueueSnapshot, TaskEvent, TaskExtras, TaskPhase, TaskSpec,
    TaskView, completed_entry_indices,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use crate::state::FileStatusCache;

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

/// 前端 `AppErrorDTO`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppErrorDTO {
    pub kind: String,
    pub message: String,
    pub actions: Vec<String>,
}

impl AppErrorDTO {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            actions: vec!["retry".into()],
        }
    }
}

impl From<std::io::Error> for AppErrorDTO {
    fn from(error: std::io::Error) -> Self {
        Self {
            kind: "disk".into(),
            message: error.to_string(),
            actions: vec!["choose-dir".into()],
        }
    }
}

impl From<BiliError> for AppErrorDTO {
    fn from(error: BiliError) -> Self {
        use bilidown_core::ErrorKind;
        let (kind, actions): (&str, Vec<String>) = match error.kind() {
            ErrorKind::RiskControl => ("risk", vec!["retry".into()]),
            ErrorKind::Auth => ("auth", vec!["relogin".into()]),
            ErrorKind::Permission => ("paid", vec!["open-settings".into()]),
            ErrorKind::Region => ("region", vec!["retry".into()]),
            ErrorKind::VipRequired => ("vip", vec!["relogin".into()]),
            ErrorKind::Network => ("network", vec!["retry".into()]),
            ErrorKind::Timeout => ("network", vec!["retry".into()]),
            ErrorKind::Cdn => ("cdn", vec!["retry".into()]),
            ErrorKind::Ffmpeg => (
                "ffmpeg",
                vec!["install-ffmpeg".into(), "open-settings".into()],
            ),
            ErrorKind::Disk => ("disk", vec!["choose-dir".into()]),
            ErrorKind::Parse => ("parse", vec!["retry".into()]),
            ErrorKind::InvalidInput => ("parse", vec!["retry".into()]),
            ErrorKind::Cancelled => ("unknown", vec!["retry".into()]),
            ErrorKind::Api | ErrorKind::Other => ("unknown", vec!["retry".into()]),
        };
        Self {
            kind: kind.to_string(),
            message: error.describe(),
            actions,
        }
    }
}

pub type CommandResult<T> = Result<T, AppErrorDTO>;

// ---------------------------------------------------------------------------
// 应用 / 通用
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDTO {
    pub product_name: String,
    pub version: String,
    pub tauri_version: String,
    pub os: String,
    pub arch: String,
    pub identifier: String,
    pub data_dir: String,
    pub log_dir: String,
    pub ffmpeg_found: bool,
    pub ffmpeg_path: Option<String>,
    /// 当前窗口策略是否支持原生材质，不受用户开关偏好影响。
    pub glass_supported: bool,
    pub glass_unavailable_reason: Option<String>,
    /// 系统通知是否可靠可用（Windows 开发/免安装运行下为 false）。
    pub notifications_supported: bool,
    /// 本次启动是否由更新安装完成后的自动重启触发（`--updated`）。
    pub just_updated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeDTO {
    pub level: String,
    pub code: Option<String>,
    pub message: String,
    pub task_id: Option<String>,
}

impl NoticeDTO {
    pub fn new(level: &str, message: impl Into<String>) -> Self {
        Self {
            level: level.to_string(),
            code: None,
            message: message.into(),
            task_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuActionDTO {
    pub id: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuStateDTO {
    pub enabled: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragPayloadDTO {
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropPayloadDTO {
    pub paths: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegDetectionDTO {
    pub detected: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestProxyResultDTO {
    pub ok: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// 队列 / 任务
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgressDTO {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub stage_done: u64,
    pub stage_total: u64,
    pub percent: f64,
    pub speed_bps: f64,
    pub eta_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskExtrasDTO {
    #[serde(default)]
    pub subtitles: Vec<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub subtitle_error: Option<String>,
    #[serde(default)]
    pub cover_error: Option<String>,
}

impl From<&TaskExtras> for TaskExtrasDTO {
    fn from(extras: &TaskExtras) -> Self {
        Self {
            subtitles: extras.subtitles.clone(),
            cover: extras.cover.clone(),
            subtitle_error: extras.subtitle_error.clone(),
            cover_error: extras.cover_error.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDTO {
    pub id: String,
    pub status: String,
    pub phase: String,
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub page_url: String,
    pub quality: String,
    pub output_path: Option<String>,
    pub artifacts: Vec<TaskArtifactDTO>,
    pub artifact_coverage: String,
    pub error: Option<AppErrorDTO>,
    pub restored: bool,
    pub sort_index: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub progress: TaskProgressDTO,
    pub owner: Option<String>,
    pub entries: Option<Vec<String>>,
    pub entry_count: Option<usize>,
    pub codec: Option<String>,
    pub audio_format: Option<String>,
    pub output_dir: Option<String>,
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub extras: TaskExtrasDTO,
    /// 记录保存失败的可读原因（design D8/3.8）；None = 已可靠保存。
    #[serde(default)]
    pub persist_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactDTO {
    pub path: String,
    pub size: u64,
    pub entry_index: Option<usize>,
    pub entry_title: Option<String>,
    pub kind: String,
    pub exists: bool,
    pub size_matches: bool,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatsDTO {
    pub total: u32,
    pub queued: u32,
    pub active: u32,
    pub paused: u32,
    pub failed: u32,
    pub finished: u32,
    pub cancelled: u32,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueCapabilityDTO {
    pub anonymous_max: String,
    pub signed_in_max: String,
    pub vip_max: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfoDTO {
    pub mid: u64,
    pub uname: String,
    pub avatar_url: Option<String>,
    /// `annual` / `monthly` / `none`
    pub vip: String,
    pub level: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshotDTO {
    pub revision: u64,
    pub tasks: Vec<TaskDTO>,
    pub stats: QueueStatsDTO,
    pub account: Option<AccountInfoDTO>,
    pub capability: QueueCapabilityDTO,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventDTO {
    pub task_id: String,
    pub kind: String,
    pub message: Option<String>,
    pub at: i64,
}

// ---------------------------------------------------------------------------
// 解析 / 入队
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOptionDTO {
    pub id: u32,
    pub label: String,
    pub resolution: String,
    pub fps: u32,
    pub vip: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodecOptionDTO {
    pub id: String,
    pub label: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFormatOptionDTO {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioQualityOptionDTO {
    pub id: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseEntryDTO {
    pub index: usize,
    pub title: String,
    pub duration_secs: Option<u64>,
    pub selected_default: bool,
    pub size_bytes: Option<u64>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResultDTO {
    pub kind: String,
    pub source_id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub page_url: String,
    pub owner: String,
    pub entries: Vec<ParseEntryDTO>,
    pub raw_input: String,
    pub duration_secs: Option<u64>,
    pub published_at: Option<i64>,
    pub qualities: Option<Vec<QualityOptionDTO>>,
    pub codecs: Option<Vec<CodecOptionDTO>>,
    pub audio_formats: Option<Vec<AudioFormatOptionDTO>>,
    pub audio_qualities: Option<Vec<AudioQualityOptionDTO>>,

}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AddOptionsDTO {
    pub quality: String,
    /// 清晰度 qn（唯一真相；0/None = 自动）
    pub quality_qn: Option<u32>,
    pub codec: String,
    pub audio_format: String,
    /// 音质 qn（可选；透传到 TaskSpec.audio_quality）
    pub audio_qn: Option<u32>,
    pub output_dir: String,
    pub entry_indexes: Option<Vec<usize>>,
    pub note: Option<String>,
}

pub fn validate_add_options(options: &AddOptionsDTO) -> Result<(), String> {
    if options.entry_indexes.as_ref().is_some_and(Vec::is_empty) {
        return Err("至少选择一个条目".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedInputDTO {
    pub line: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTasksResultDTO {
    pub task_ids: Vec<String>,
    pub rejected: Vec<RejectedInputDTO>,
}

// ---------------------------------------------------------------------------
// 登录
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartDTO {
    pub key: String,
    pub qr_matrix: Vec<Vec<bool>>,
    pub qr_url: String,
    pub expires_in_secs: u64,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginPollDTO {
    pub status: String,
    pub message: String,
    pub account: Option<AccountInfoDTO>,
}

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDTO {
    /// `direct` / `manual`
    pub mode: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub pass: Option<String>,
}

/// core 启动阶段事件（design D5/5.2/5.4）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreStatusDTO {
    /// `local-ready` / `network-ready` / `network-degraded`
    pub stage: String,
    /// 本次网络准备使用的代次（旧结果不得覆盖新状态）
    pub generation: u64,
    /// 网络准备是否已降级（离线/弱网仍可查看本地内容）
    pub degraded: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDTO {
    pub download_dir: String,
    pub max_concurrent: usize,
    pub quality: String,
    pub codec: String,
    pub audio_format: String,
    pub ffmpeg_path: String,
    pub proxy: ProxyDTO,
    pub theme: String,
    pub notifications: bool,
    pub watch_clipboard: bool,
    pub parse_concurrency: Option<u32>,
    pub history_limit: Option<usize>,
    pub history_clear_policy: Option<String>,
    pub dock_badge: Option<bool>,
    pub reduce_motion: Option<bool>,
    pub show_speed_in_status: Option<bool>,
    pub write_metadata: Option<bool>,
    pub download_subtitle: Option<bool>,
    pub download_cover: Option<bool>,
    pub check_updates_on_startup: Option<bool>,
    pub accent_color: Option<String>,
    pub motion: Option<String>,
    pub quality_fallback: Option<bool>,
    pub glass_background: Option<bool>,
    pub sound_on_complete: Option<bool>,
    pub reveal_in_finder_on_complete: Option<bool>,
    pub keep_queue_on_exit: Option<bool>,
    pub ffmpeg_detected: Option<bool>,
    pub ffmpeg_version: Option<String>,
    /// 设置修订号（design D3）：同一进程内单调递增，供前端丢弃乱序响应/事件。
    #[serde(default)]
    pub revision: Option<u64>,
}

// ---------------------------------------------------------------------------
// 历史
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryDTO {
    pub id: String,
    pub task_id: Option<String>,
    pub title: String,
    pub cover_url: Option<String>,
    pub source_kind: String,
    pub source_id: String,
    pub page_url: String,
    pub quality: String,
    pub output_path: String,
    pub file_size: u64,
    pub status: String,
    pub error: Option<String>,
    pub finished_at: i64,
    pub owner: Option<String>,
    pub file_exists: bool,
    pub artifacts: Vec<TaskArtifactDTO>,
    pub artifact_coverage: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryFilterDTO {
    pub status: Option<String>,
    pub keyword: Option<String>,
}

// ---------------------------------------------------------------------------
// 转换：队列
// ---------------------------------------------------------------------------

/// B 站接口返回的封面可能是 `http://` 或协议相对 `//`；生产 CSP 只允许 https，统一升级。
fn normalize_cover(url: Option<String>) -> Option<String> {
    url.map(|url| {
        if let Some(rest) = url.strip_prefix("http://") {
            format!("https://{rest}")
        } else if let Some(rest) = url.strip_prefix("//") {
            format!("https://{rest}")
        } else {
            url
        }
    })
}

pub fn content_kind(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Video => "video",
        ContentKind::Bangumi => "bangumi",
        ContentKind::Cheese => "course",
        ContentKind::Audio | ContentKind::AudioList => "audio",
    }
}

fn phase_to_contract(
    phase: TaskPhase,
    stage: bilidown_core::api::types::DownloadStage,
) -> &'static str {
    use bilidown_core::api::types::DownloadStage;
    match stage {
        DownloadStage::Resolving => "resolve",
        DownloadStage::Downloading => "video",
        DownloadStage::Merging => "merge",
        DownloadStage::PostProcess | DownloadStage::Done => "postprocess",
        DownloadStage::Idle => match phase {
            TaskPhase::Queued | TaskPhase::Resolving => "resolve",
            TaskPhase::Merging => "merge",
            TaskPhase::Done => "postprocess",
            _ => "resolve",
        },
    }
}

/// qn → UI 档位标签（与 web/src/lib/constants.ts QUALITY_TIERS 对齐）。
pub fn quality_label(qn: u32) -> String {
    match qn {
        127 => "8K".into(),
        126 => "杜比视界".into(),
        125 => "HDR".into(),
        120 => "4K".into(),
        116 => "1080P60".into(),
        112 => "1080P+".into(),
        80 => "1080P".into(),
        74 => "720P60".into(),
        64 => "720P".into(),
        32 => "480P".into(),
        16 => "360P".into(),
        other => format!("{other}qn"),
    }
}

/// UI 档位标签 → qn（`best`/空 = None）。
pub fn quality_qn(label: &str) -> Option<u32> {
    let label = label.trim();
    if label.is_empty() || label.eq_ignore_ascii_case("best") {
        return None;
    }
    let known = [
        ("8K", 127),
        ("杜比视界", 126),
        ("Dolby", 126),
        ("Dolby Vision", 126),
        ("HDR", 125),
        ("4K", 120),
        ("1080P60", 116),
        ("1080P+", 112),
        ("1080P", 80),
        ("720P60", 74),
        ("720P", 64),
        ("480P", 32),
        ("360P", 16),
    ];
    if let Some((_, qn)) = known.iter().find(|(name, _)| *name == label) {
        return Some(*qn);
    }
    label
        .trim_end_matches(['P', 'p', 'q', 'n'])
        .parse::<u32>()
        .ok()
}

fn codec_label(codec: Codec) -> String {
    match codec {
        Codec::Avc => "AVC",
        Codec::Hevc => "HEVC",
        Codec::Av1 => "AV1",
        Codec::Unknown => "UNKNOWN",
    }
    .into()
}

fn codec_from_label(label: &str) -> Option<Codec> {
    match label.to_ascii_lowercase().as_str() {
        "avc" => Some(Codec::Avc),
        "hevc" => Some(Codec::Hevc),
        "av1" => Some(Codec::Av1),
        _ => None,
    }
}

fn artifact_kind_str(kind: bilidown_core::ArtifactKind) -> &'static str {
    match kind {
        bilidown_core::ArtifactKind::Media => "media",
        bilidown_core::ArtifactKind::VideoStream => "video-stream",
        bilidown_core::ArtifactKind::AudioStream => "audio-stream",
    }
}

/// 单次 DTO 转换内的文件状态缓存（design D10/7.1）：
/// 同一路径（同一批次内）只做一次 metadata 探测，避免每帧重复 stat。
#[derive(Default)]
pub struct FileProbe {
    entries: HashMap<String, Option<(bool, u64)>>,
    /// 跨批次展示缓存（design D9/7.2）；权威操作不走此缓存。
    cache: Option<std::sync::Arc<FileStatusCache>>,
}

impl FileProbe {
    pub fn with_cache(cache: std::sync::Arc<FileStatusCache>) -> Self {
        Self {
            cache: Some(cache),
            ..Self::default()
        }
    }
}

impl FileProbe {
    fn lookup(&mut self, path: &str) -> Option<(bool, u64)> {
        if let Some(hit) = self.entries.get(path) {
            return *hit;
        }
        if let Some(cached) = self.cache.as_ref().and_then(|cache| cache.lookup(path)) {
            self.entries.insert(path.to_string(), cached);
            return cached;
        }
        let probed = std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.is_file(), metadata.len()));
        if let Some(cache) = &self.cache {
            cache.remember(path, probed);
        }
        self.entries.insert(path.to_string(), probed);
        probed
    }

    pub fn exists(&mut self, path: &str) -> bool {
        self.lookup(path).is_some_and(|(is_file, _)| is_file)
    }

    fn size_matches(&mut self, artifact: &DownloadArtifact) -> bool {
        self.lookup(&artifact.path)
            .is_some_and(|(is_file, len)| is_file && len == artifact.size)
    }

    fn valid(&mut self, artifact: &DownloadArtifact) -> bool {
        self.size_matches(artifact)
    }
}

fn artifact_dtos_with(
    artifacts: &[DownloadArtifact],
    selected: &HashSet<usize>,
    probe: &mut FileProbe,
) -> Vec<TaskArtifactDTO> {
    let mut counts: HashMap<(Option<usize>, bilidown_core::ArtifactKind), usize> = HashMap::new();
    for artifact in artifacts {
        if probe.valid(artifact) {
            *counts
                .entry((artifact.entry_index, artifact.kind))
                .or_default() += 1;
        }
    }
    artifacts
        .iter()
        .map(|artifact| {
            let exists = probe.exists(&artifact.path);
            let size_matches = probe.size_matches(artifact);
            let entry_valid = artifact
                .entry_index
                .is_none_or(|entry| selected.is_empty() || selected.contains(&entry));
            let unique = counts.get(&(artifact.entry_index, artifact.kind)) == Some(&1);
            TaskArtifactDTO {
                path: artifact.path.clone(),
                size: artifact.size,
                entry_index: artifact.entry_index,
                entry_title: artifact.entry_title.clone(),
                kind: artifact_kind_str(artifact.kind).into(),
                exists,
                size_matches,
                valid: size_matches && entry_valid && unique,
            }
        })
        .collect()
}

fn task_artifact_dtos_with(view: &TaskView, probe: &mut FileProbe) -> Vec<TaskArtifactDTO> {
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
    let completed = completed_entry_indices(view);
    let mut artifacts = artifact_dtos_with(&view.artifacts, &selected, probe);
    for artifact in &mut artifacts {
        if !(view.artifacts_legacy && artifact.entry_index.is_none()) {
            artifact.valid = artifact.valid
                && artifact
                    .entry_index
                    .is_some_and(|entry| completed.contains(&entry));
        }
    }
    artifacts
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn task_dto(view: &TaskView, settings: &Settings, sort_index: i64) -> TaskDTO {
    task_dto_with_probe(view, settings, sort_index, &mut FileProbe::default())
}

fn task_dto_with_probe(
    view: &TaskView,
    settings: &Settings,
    sort_index: i64,
    probe: &mut FileProbe,
) -> TaskDTO {
    let progress = &view.progress;
    let bytes_total = progress.total;
    let entry_count = selected_entry_count(view);
    let percent = if bytes_total == 0 {
        0.0
    } else {
        (progress.downloaded as f64 / bytes_total as f64).clamp(0.0, 1.0)
    };
    let quality = view
        .actual_quality
        .or(view.spec.quality)
        .map(|q| quality_label(q.qn()))
        .unwrap_or_default();
    let codec = view
        .actual_codec
        .or(view.spec.codec)
        .map(|c| c.as_str().to_string());
    let audio_format = if view.spec.to_mp3 {
        Some("mp3".to_string())
    } else {
        view.actual_audio
            .map(|a| AudioFormat::from_quality(a).as_str().to_string())
            .or(Some("original".to_string()))
    };
    let output_dir = view
        .spec
        .output_dir
        .clone()
        .unwrap_or_else(|| settings.effective_download_dir())
        .to_string_lossy()
        .into_owned();
    let artifacts = task_artifact_dtos_with(view, probe);
    TaskDTO {
        id: view.id.clone(),
        status: view.phase.as_str().to_string(),
        phase: phase_to_contract(view.phase, progress.stage).to_string(),
        source_kind: content_kind(view.content_kind).to_string(),
        source_id: view.spec.id.to_string(),
        title: view.title.clone(),
        cover_url: normalize_cover(view.cover_url.clone()),
        page_url: view.page_url.clone(),
        quality,
        output_path: view.output_path.clone(),
        artifacts,
        artifact_coverage: if view.artifacts_legacy {
            "legacy-single".into()
        } else {
            "complete".into()
        },
        error: view.error.clone().map(AppErrorDTO::from),
        restored: view.restored,
        sort_index,
        created_at: view.created_at,
        updated_at: view.updated_at,
        progress: TaskProgressDTO {
            bytes_done: progress.downloaded,
            bytes_total,
            stage_done: progress.entries_done as u64,
            stage_total: entry_count as u64,
            percent,
            speed_bps: view.speed_bps,
            eta_secs: None,
        },
        owner: (!view.upper.is_empty()).then(|| view.upper.clone()),
        entries: Some(view.entries.clone()),
        entry_count: Some(entry_count),
        codec,
        audio_format,
        output_dir: Some(output_dir),
        duration_secs: None,
        extras: TaskExtrasDTO::from(&view.extras),
        persist_error: view.persist_error.clone(),
    }
}

fn selected_entry_count(view: &TaskView) -> usize {
    if !view.selected_entries.is_empty() {
        return view.selected_entries.len();
    }
    if let Some(selected) = view
        .spec
        .selected_entries
        .as_ref()
        .filter(|selected| !selected.is_empty())
    {
        return selected.len();
    }
    view.entries.len().max(1)
}

pub fn stats_dto(snapshot: &QueueSnapshot) -> QueueStatsDTO {
    let cancelled = snapshot
        .tasks
        .iter()
        .filter(|t| t.phase == TaskPhase::Cancelled)
        .count() as u32;
    QueueStatsDTO {
        total: snapshot.tasks.len() as u32,
        queued: snapshot.queued_count,
        active: snapshot.active_count,
        paused: snapshot.paused_count,
        failed: snapshot.failed_count,
        finished: snapshot.done_count,
        cancelled,
        bytes_done: snapshot.tasks.iter().map(|t| t.progress.downloaded).sum(),
        bytes_total: snapshot.tasks.iter().map(|t| t.progress.total).sum(),
    }
}

pub fn capability_dto(account: &AccountState) -> QueueCapabilityDTO {
    let (signed_in, vip) = if account.logged_in {
        (
            "1080P",
            if account.user.vip_status {
                "4K"
            } else {
                "1080P"
            },
        )
    } else {
        ("480P", "1080P")
    };
    QueueCapabilityDTO {
        anonymous_max: "480P".into(),
        signed_in_max: signed_in.into(),
        vip_max: vip.into(),
    }
}

pub fn account_dto(account: &AccountState) -> Option<AccountInfoDTO> {
    if !account.logged_in || account.user.mid == 0 {
        return None;
    }
    let vip = if !account.user.vip_status {
        "none"
    } else if account.user.vip_type == 2 {
        "annual"
    } else {
        "monthly"
    };
    Some(AccountInfoDTO {
        mid: account.user.mid,
        uname: account.user.uname.clone(),
        avatar_url: account.user.avatar_url.clone(),
        vip: vip.into(),
        level: account.user.level,
    })
}

pub fn snapshot_dto(
    snapshot: &QueueSnapshot,
    account: &AccountState,
    settings: &Settings,
    cache: Option<&std::sync::Arc<FileStatusCache>>,
) -> QueueSnapshotDTO {
    // 同一批次内按路径复用文件状态探测（design D10/7.1），并命中跨批次展示缓存。
    let mut probe = match cache {
        Some(cache) => FileProbe::with_cache(std::sync::Arc::clone(cache)),
        None => FileProbe::default(),
    };
    QueueSnapshotDTO {
        revision: snapshot.seq,
        tasks: snapshot
            .tasks
            .iter()
            .enumerate()
            .map(|(index, view)| task_dto_with_probe(view, settings, index as i64, &mut probe))
            .collect(),
        stats: stats_dto(snapshot),
        account: account_dto(account),
        capability: capability_dto(account),
        updated_at: now_ms(),
    }
}

/// core TaskEvent → 前端 TaskEventDTO（HistoryChanged/QueueChanged 由各自域事件承载，不转发）。
pub fn task_event_dto(event: &TaskEvent) -> Option<TaskEventDTO> {
    let at = now_ms();
    let (task_id, kind, message): (String, &str, Option<String>) = match event {
        TaskEvent::Created { view } => (view.id.clone(), "enqueued", None),
        TaskEvent::Updated { view } => (view.id.clone(), "started", None),
        TaskEvent::PhaseChanged { id, phase, .. } => {
            let kind = match phase {
                TaskPhase::Resolving | TaskPhase::Downloading => "started",
                TaskPhase::Merging => "phase-changed",
                TaskPhase::Paused => "paused",
                TaskPhase::Queued => "enqueued",
                TaskPhase::Done => "completed",
                TaskPhase::Failed => "failed",
                TaskPhase::Cancelled => "cancelled",
            };
            (id.clone(), kind, None)
        }
        TaskEvent::Done { view } => (view.id.clone(), "completed", None),
        TaskEvent::Failed { view, error } => (view.id.clone(), "failed", Some(error.describe())),
        TaskEvent::Cancelled { id } => (id.clone(), "cancelled", None),
        TaskEvent::Paused { id } => (id.clone(), "paused", None),
        TaskEvent::Resumed { id } => (id.clone(), "resumed", None),
        TaskEvent::Removed { id } => (id.clone(), "removed", None),
        TaskEvent::HistoryChanged | TaskEvent::QueueChanged => return None,
    };
    Some(TaskEventDTO {
        task_id,
        kind: kind.to_string(),
        message,
        at,
    })
}

// ---------------------------------------------------------------------------
// 转换：解析 / 入队
// ---------------------------------------------------------------------------

fn unavailable_reason(reason: TierUnavailable) -> String {
    match reason {
        TierUnavailable::NotProvided => "接口未提供".into(),
        TierUnavailable::LoginRequired => "登录后可解锁".into(),
        TierUnavailable::VipRequired => "需大会员".into(),
        TierUnavailable::PaidRequired => "需付费".into(),
        TierUnavailable::Region => "地区限制".into(),
    }
}

fn quality_options(play: &PlayUrl) -> Vec<QualityOptionDTO> {
    let mut out = Vec::new();
    for tier in &play.quality_tiers {
        let matched = play.qualities.iter().find(|q| q.quality == tier.quality);
        let (resolution, fps) = matched
            .and_then(|quality| quality.pick_stream(None))
            .map(|stream| {
                let resolution = match (stream.width, stream.height) {
                    (Some(w), Some(h)) => format!("{w}×{h}"),
                    _ => String::new(),
                };
                let fps = stream
                    .frame_rate
                    .as_deref()
                    .and_then(|value| value.split('/').next())
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(|value| value.round() as u32)
                    .unwrap_or(30);
                (resolution, fps)
            })
            .unwrap_or((String::new(), 30));
        out.push(QualityOptionDTO {
            id: tier.quality.qn(),
            label: tier
                .name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| quality_label(tier.quality.qn())),
            resolution,
            fps,
            vip: tier.quality.is_vip_tier(),
            available: tier.unavailable.is_none(),
            unavailable_reason: tier.unavailable.map(unavailable_reason),
        });
    }
    if out.is_empty() {
        for quality in &play.qualities {
            out.push(QualityOptionDTO {
                id: quality.quality.qn(),
                label: quality_label(quality.quality.qn()),
                resolution: match quality.pick_stream(None).and_then(|s| s.height) {
                    Some(h) => format!("h{h}"),
                    None => String::new(),
                },
                fps: 30,
                vip: quality.quality.is_vip_tier(),
                available: true,
                unavailable_reason: None,
            });
        }
    }
    out
}

fn codec_options(play: &PlayUrl) -> Vec<CodecOptionDTO> {
    let mut seen: Vec<Codec> = Vec::new();
    for quality in &play.qualities {
        for codec in quality.codecs() {
            if codec != Codec::Unknown && !seen.contains(&codec) {
                seen.push(codec);
            }
        }
    }
    if seen.is_empty() {
        seen.push(Codec::Avc);
    }
    seen.into_iter()
        .map(|codec| CodecOptionDTO {
            id: codec.as_str().to_string(),
            label: codec_label(codec),
            available: true,
        })
        .collect()
}

fn audio_quality_options(play: &PlayUrl) -> Vec<AudioQualityOptionDTO> {
    let mut out: Vec<AudioQualityOptionDTO> = Vec::new();
    for track in &play.audios {
        let qn = track.quality.id();
        if out.iter().any(|item| item.id == qn) {
            continue;
        }
        let label = match qn {
            30216 => "64K".to_string(),
            30228 => "128K".to_string(),
            30232 => "132K".to_string(),
            30280 => "192K".to_string(),
            30250 => "杜比全景声".to_string(),
            30251 => "FLAC".to_string(),
            30252 => "Hi-Res".to_string(),
            other => format!("{other}"),
        };
        out.push(AudioQualityOptionDTO { id: qn, label });
    }
    out
}

fn audio_format_options(play: &PlayUrl) -> Vec<AudioFormatOptionDTO> {
    let mut formats: Vec<String> = Vec::new();
    for track in &play.audios {
        let format = track.format.as_str().to_string();
        if !formats.contains(&format) {
            formats.push(format);
        }
    }
    let mut out: Vec<AudioFormatOptionDTO> = formats
        .iter()
        .map(|format| AudioFormatOptionDTO {
            id: format.clone(),
            label: format.to_uppercase(),
            available: true,
            hint: None,
        })
        .collect();
    out.push(AudioFormatOptionDTO {
        id: "original".into(),
        label: "原格式".into(),
        available: true,
        hint: Some("m4a / flac".into()),
    });
    out.push(AudioFormatOptionDTO {
        id: "mp3".into(),
        label: "MP3".into(),
        available: true,
        hint: Some("需 ffmpeg".into()),
    });
    out
}

/// 解析结果 → 前端 ParseResultDTO。
pub fn parse_result_dto(
    raw_input: &str,
    _parsed: &ParsedInput,
    content: &Content,
    page_hint: Option<usize>,
    play: Option<&PlayUrl>,
) -> ParseResultDTO {
    let defaults = bilidown_core::api::video::default_entries(&content.id, content, page_hint);
    ParseResultDTO {
        kind: content_kind(content.id.kind()).to_string(),
        source_id: content.id.to_string(),
        title: content.title.clone(),
        cover_url: normalize_cover(content.cover_url.clone()),
        page_url: content.id.page_url(),
        owner: content.upper.clone(),
        entries: content
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| ParseEntryDTO {
                index,
                title: if entry.title.is_empty() {
                    format!("P{}", index + 1)
                } else {
                    entry.title.clone()
                },
                duration_secs: entry.duration_secs,
                selected_default: defaults.contains(&index),
                size_bytes: None,
                unavailable_reason: None,
            })
            .collect(),
        raw_input: raw_input.to_string(),
        duration_secs: content.duration_secs,
        published_at: content.pub_time,
        qualities: play.map(quality_options),
        codecs: play.map(codec_options),
        audio_formats: play.map(audio_format_options),
        audio_qualities: play.map(audio_quality_options),
    }
}

/// AddOptionsDTO → TaskSpec 覆盖。
pub fn apply_add_options(spec: &mut TaskSpec, options: &AddOptionsDTO, single_input: bool) {
    spec.quality = options
        .quality_qn
        .filter(|qn| *qn > 0)
        .map(Quality)
        .or_else(|| quality_qn(&options.quality).map(Quality));
    spec.codec = codec_from_label(&options.codec);
    spec.audio_quality = options.audio_qn.filter(|qn| *qn > 0).map(AudioQuality);
    spec.to_mp3 = options.audio_format.eq_ignore_ascii_case("mp3");
    spec.output_dir = {
        let dir = options.output_dir.trim();
        (!dir.is_empty()).then(|| PathBuf::from(dir))
    };
    if single_input {
        spec.selected_entries = options.entry_indexes.clone();
    }
    spec.note = options.note.clone();
}

// ---------------------------------------------------------------------------
// 转换：登录
// ---------------------------------------------------------------------------

pub fn login_start_dto(flow: &QrLogin) -> LoginStartDTO {
    let size = flow.matrix.size;
    let rows: Vec<Vec<bool>> = (0..size)
        .map(|y| (0..size).map(|x| flow.matrix.is_dark(x, y)).collect())
        .collect();
    LoginStartDTO {
        key: flow.qrcode_key.clone(),
        qr_matrix: rows,
        qr_url: flow.qr_url.clone(),
        expires_in_secs: 180,
        status: Some(login_state_str(&flow.state).to_string()),
    }
}

pub fn login_state_str(state: &LoginState) -> &'static str {
    match state {
        LoginState::Idle => "waiting",
        LoginState::Waiting => "waiting",
        LoginState::Scanned => "scanned",
        LoginState::Expired => "expired",
        LoginState::Success => "success",
        LoginState::Failed => "error",
    }
}

pub fn login_poll_dto(outcome: LoginOutcome) -> LoginPollDTO {
    match outcome {
        LoginOutcome::Pending(state) => {
            let message = match state {
                LoginState::Scanned => "已扫码，等待手机确认",
                _ => "等待扫码",
            };
            LoginPollDTO {
                status: login_state_str(&state).into(),
                message: message.into(),
                account: None,
            }
        }
        LoginOutcome::Success(account) => LoginPollDTO {
            status: "success".into(),
            message: "登录成功".into(),
            account: account_dto(&account),
        },
        LoginOutcome::Ended(state) => {
            let message = match state {
                LoginState::Expired => "二维码已过期，请刷新",
                LoginState::Failed => "登录失败",
                _ => "登录已结束",
            };
            LoginPollDTO {
                status: login_state_str(&state).into(),
                message: message.into(),
                account: None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 转换：设置（双向）
// ---------------------------------------------------------------------------

fn proxy_dto(proxy: &Option<String>) -> ProxyDTO {
    match proxy {
        None => ProxyDTO {
            mode: "direct".into(),
            host: None,
            port: None,
            user: None,
            pass: None,
        },
        Some(raw) => {
            let trimmed = raw.trim();
            let without_scheme = trimmed
                .strip_prefix("http://")
                .or_else(|| trimmed.strip_prefix("https://"))
                .or_else(|| trimmed.strip_prefix("socks5://"))
                .unwrap_or(trimmed);
            let (auth, host_port) = match without_scheme.rsplit_once('@') {
                Some((auth, host_port)) => (Some(auth), host_port),
                None => (None, without_scheme),
            };
            let (host, port) = match host_port.rsplit_once(':') {
                Some((host, port)) => (host.to_string(), port.parse::<u16>().ok()),
                None => (host_port.to_string(), None),
            };
            let (user, pass) = match auth {
                Some(auth) => match auth.split_once(':') {
                    Some((user, pass)) => (Some(user.to_string()), Some(pass.to_string())),
                    None => (Some(auth.to_string()), None),
                },
                None => (None, None),
            };
            ProxyDTO {
                mode: "manual".into(),
                host: (!host.is_empty()).then_some(host),
                port,
                user,
                pass,
            }
        }
    }
}

fn proxy_url(proxy: &ProxyDTO) -> Option<String> {
    if proxy.mode != "manual" {
        return None;
    }
    let host = proxy.host.as_deref().unwrap_or_default().trim();
    if host.is_empty() {
        return None;
    }
    let port = proxy.port.unwrap_or(7890);
    let auth = match (proxy.user.as_deref(), proxy.pass.as_deref()) {
        (Some(user), Some(pass)) if !user.is_empty() => format!("{user}:{pass}@"),
        (Some(user), None) if !user.is_empty() => format!("{user}@"),
        _ => String::new(),
    };
    Some(format!("http://{auth}{host}:{port}"))
}

fn theme_str(theme: ThemePreference) -> &'static str {
    match theme {
        ThemePreference::System => "system",
        ThemePreference::Light => "light",
        ThemePreference::Dark => "dark",
    }
}

fn theme_from_str(theme: &str) -> ThemePreference {
    match theme.to_ascii_lowercase().as_str() {
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => ThemePreference::System,
    }
}

pub fn ffmpeg_version(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new(path)
        .arg("-version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?;
    // "ffmpeg version 9.0.2 Copyright (c) ..."
    first.split_whitespace().nth(2).map(|v| v.to_string())
}

pub fn detect_ffmpeg(enabled: Option<&str>) -> FfmpegDetectionDTO {
    match Ffmpeg::detect(enabled) {
        Some(ffmpeg) => {
            let path = ffmpeg.path().to_path_buf();
            let version = ffmpeg_version(&path);
            FfmpegDetectionDTO {
                detected: true,
                version,
                path: Some(path.to_string_lossy().into_owned()),
            }
        }
        None => FfmpegDetectionDTO {
            detected: false,
            version: None,
            path: None,
        },
    }
}

fn motion_str(motion: MotionPreference) -> &'static str {
    match motion {
        MotionPreference::System => "system",
        MotionPreference::Reduce => "reduce",
        MotionPreference::Normal => "normal",
    }
}

fn motion_from_str(value: &str) -> MotionPreference {
    match value.trim().to_ascii_lowercase().as_str() {
        "reduce" => MotionPreference::Reduce,
        "normal" => MotionPreference::Normal,
        _ => MotionPreference::System,
    }
}

pub fn is_valid_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn settings_dto(settings: &Settings) -> SettingsDTO {
    let ffmpeg = Ffmpeg::detect(settings.ffmpeg_path.as_deref());
    let (ffmpeg_detected, ffmpeg_version, detected_path) = match &ffmpeg {
        Some(ffmpeg) => (
            true,
            ffmpeg_version(ffmpeg.path()),
            Some(ffmpeg.path().to_string_lossy().into_owned()),
        ),
        None => (false, None, None),
    };
    SettingsDTO {
        download_dir: settings
            .effective_download_dir()
            .to_string_lossy()
            .into_owned(),
        max_concurrent: settings.max_concurrent,
        quality: settings
            .default_quality
            .map(quality_label)
            .unwrap_or_else(|| "best".into()),
        codec: settings
            .default_codec
            .map(|codec| codec.as_str().to_string())
            .unwrap_or_else(|| "auto".into()),
        audio_format: if settings.audio_to_mp3 {
            "mp3"
        } else {
            "original"
        }
        .into(),
        ffmpeg_path: settings
            .ffmpeg_path
            .clone()
            .or(detected_path)
            .unwrap_or_default(),
        proxy: proxy_dto(&settings.proxy),
        theme: theme_str(settings.theme).into(),
        notifications: settings.notify_on_done,
        watch_clipboard: settings.watch_clipboard,
        parse_concurrency: Some(2),
        history_limit: Some(settings.history_limit),
        history_clear_policy: Some("count".into()),
        dock_badge: Some(true),
        reduce_motion: Some(false),
        show_speed_in_status: Some(true),
        write_metadata: Some(settings.write_metadata),
        download_subtitle: Some(settings.download_subtitle),
        download_cover: Some(settings.download_cover),
        check_updates_on_startup: Some(settings.check_updates_on_startup),
        accent_color: settings.accent_color.clone(),
        motion: Some(motion_str(settings.motion).to_string()),
        quality_fallback: Some(settings.quality_fallback),
        glass_background: Some(settings.window_glass),
        sound_on_complete: Some(false),
        reveal_in_finder_on_complete: Some(false),
        keep_queue_on_exit: Some(true),
        ffmpeg_detected: Some(ffmpeg_detected),
        ffmpeg_version,
        revision: None,
    }
}

/// SettingsDTO → core Settings（保留 core 支持但 UI 未暴露的字段）。
pub fn settings_from_dto(dto: &SettingsDTO, previous: &Settings) -> Settings {
    let quality = quality_qn(&dto.quality);
    let codec = if dto.codec.eq_ignore_ascii_case("best") || dto.codec.eq_ignore_ascii_case("auto")
    {
        None
    } else {
        codec_from_label(&dto.codec)
    };
    let download_dir = dto.download_dir.trim();
    Settings {
        download_dir: (!download_dir.is_empty()).then(|| download_dir.to_string()),
        max_concurrent: dto.max_concurrent.clamp(1, 8),
        default_quality: quality,
        default_codec: codec,
        audio_to_mp3: dto.audio_format.eq_ignore_ascii_case("mp3"),
        write_metadata: dto.write_metadata.unwrap_or(previous.write_metadata),
        ffmpeg_path: {
            let path = dto.ffmpeg_path.trim();
            (!path.is_empty()).then(|| path.to_string())
        },
        proxy: proxy_url(&dto.proxy),
        theme: theme_from_str(&dto.theme),
        notify_on_done: dto.notifications,
        watch_clipboard: dto.watch_clipboard,
        history_limit: dto
            .history_limit
            .unwrap_or(previous.history_limit)
            .clamp(1, 5000),
        pcdn_filter: previous.pcdn_filter,
        allow_streams_only: previous.allow_streams_only,
        download_subtitle: dto.download_subtitle.unwrap_or(previous.download_subtitle),
        download_cover: dto.download_cover.unwrap_or(previous.download_cover),
        check_updates_on_startup: dto
            .check_updates_on_startup
            .unwrap_or(previous.check_updates_on_startup),
        accent_color: match dto.accent_color.as_deref().map(str::trim) {
            Some("") => None,
            Some(value) if is_valid_hex_color(value) => Some(value.to_ascii_uppercase()),
            Some(_) => previous.accent_color.clone(),
            None => previous.accent_color.clone(),
        },
        motion: dto
            .motion
            .as_deref()
            .map(motion_from_str)
            .unwrap_or(previous.motion),
        quality_fallback: dto.quality_fallback.unwrap_or(previous.quality_fallback),
        window_glass: dto.glass_background.unwrap_or(previous.window_glass),
    }
}

/// 设置增量提交（design D3）：只包含可编辑业务字段；省略 = 保持，null = 清空可空字段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatchDTO {
    #[serde(default, deserialize_with = "double_option")]
    pub download_dir: Option<Option<String>>,
    pub max_concurrent: Option<usize>,
    pub quality: Option<String>,
    pub codec: Option<String>,
    pub audio_format: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub ffmpeg_path: Option<Option<String>>,
    /// 省略 = 保持；`{"mode":"direct"}` 表示切回直连（design D4 代理草稿）。
    pub proxy: Option<ProxyDTO>,
    pub theme: Option<String>,
    pub notifications: Option<bool>,
    pub watch_clipboard: Option<bool>,
    pub history_limit: Option<usize>,
    pub write_metadata: Option<bool>,
    pub download_subtitle: Option<bool>,
    pub download_cover: Option<bool>,
    pub check_updates_on_startup: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub accent_color: Option<Option<String>>,
    pub motion: Option<String>,
    pub quality_fallback: Option<bool>,
    pub glass_background: Option<bool>,
}

/// 区分「字段省略」与「显式 null」（design D3）。
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

/// 把补丁合并到当前有效设置上；只改补丁显式包含的字段（design D3）。
pub fn apply_settings_patch(patch: &SettingsPatchDTO, previous: &Settings) -> Settings {
    let mut next = previous.clone();
    if let Some(value) = &patch.download_dir {
        next.download_dir = value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }
    if let Some(value) = patch.max_concurrent {
        // 不在合并阶段静默纠正：交给 Settings::validate 拒绝并提示（design D3）。
        next.max_concurrent = value;
    }
    if let Some(value) = &patch.quality {
        next.default_quality = quality_qn(value);
    }
    if let Some(value) = &patch.codec {
        next.default_codec = if value.eq_ignore_ascii_case("best") || value.eq_ignore_ascii_case("auto")
        {
            None
        } else {
            codec_from_label(value)
        };
    }
    if let Some(value) = &patch.audio_format {
        next.audio_to_mp3 = value.eq_ignore_ascii_case("mp3");
    }
    if let Some(value) = &patch.ffmpeg_path {
        next.ffmpeg_path = value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }
    if let Some(value) = &patch.proxy {
        next.proxy = proxy_url(value);
    }
    if let Some(value) = &patch.theme {
        next.theme = theme_from_str(value);
    }
    if let Some(value) = patch.notifications {
        next.notify_on_done = value;
    }
    if let Some(value) = patch.watch_clipboard {
        next.watch_clipboard = value;
    }
    if let Some(value) = patch.history_limit {
        next.history_limit = value;
    }
    if let Some(value) = patch.write_metadata {
        next.write_metadata = value;
    }
    if let Some(value) = patch.download_subtitle {
        next.download_subtitle = value;
    }
    if let Some(value) = patch.download_cover {
        next.download_cover = value;
    }
    if let Some(value) = patch.check_updates_on_startup {
        next.check_updates_on_startup = value;
    }
    if let Some(value) = &patch.accent_color {
        // 非法值保留原样，由 validate 拒绝（不静默回退）。
        next.accent_color = match value.as_deref().map(str::trim) {
            Some("") | None => None,
            Some(value) => Some(value.to_ascii_uppercase()),
        };
    }
    if let Some(value) = &patch.motion {
        next.motion = motion_from_str(value);
    }
    if let Some(value) = patch.quality_fallback {
        next.quality_fallback = value;
    }
    if let Some(value) = patch.glass_background {
        next.window_glass = value;
    }
    next
}

// ---------------------------------------------------------------------------
// 转换：历史
// ---------------------------------------------------------------------------

#[cfg_attr(not(test), allow(dead_code))]
pub fn history_dto(record: HistoryRecord) -> HistoryEntryDTO {
    history_dto_with_probe(record, &mut FileProbe::default())
}

pub fn history_dto_with_probe(record: HistoryRecord, probe: &mut FileProbe) -> HistoryEntryDTO {
    let status = match record.status.as_str() {
        "done" | "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        other => other,
    };
    let mut artifacts: Vec<DownloadArtifact> =
        serde_json::from_str(&record.artifacts).unwrap_or_default();
    let legacy = artifacts.is_empty() && record.output_path.is_some();
    if legacy && let Some(path) = &record.output_path {
        artifacts.push(DownloadArtifact::media(
            PathBuf::from(path),
            record.file_size.unwrap_or(0).max(0) as u64,
            None,
            None,
        ));
    }
    let artifact_dtos = artifact_dtos_with(&artifacts, &HashSet::new(), probe);
    let primary_path = artifact_dtos
        .first()
        .map(|artifact| artifact.path.clone())
        .or(record.output_path.clone())
        .unwrap_or_default();
    let file_exists = !primary_path.is_empty() && probe.exists(&primary_path);
    let total_size = if artifact_dtos.is_empty() {
        record.file_size.unwrap_or(0).max(0) as u64
    } else {
        artifact_dtos.iter().map(|artifact| artifact.size).sum()
    };
    HistoryEntryDTO {
        id: record.id,
        task_id: record.task_id,
        title: record.title,
        cover_url: normalize_cover(record.cover_url),
        source_kind: record.source_kind,
        source_id: record.source_id,
        page_url: record.page_url.unwrap_or_default(),
        quality: record.quality.unwrap_or_default(),
        output_path: primary_path,
        file_size: total_size,
        status: status.into(),
        error: record.error,
        finished_at: record.finished_at,
        owner: None,
        file_exists,
        artifacts: artifact_dtos,
        artifact_coverage: if legacy {
            "legacy-single".into()
        } else {
            "complete".into()
        },
    }
}

// ---------------------------------------------------------------------------
// 内容源扩展（change add-download-features D3；core `api::source` 等）
// ---------------------------------------------------------------------------

/// UGC 合集条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UgcEpisodeDTO {
    pub bvid: String,
    pub aid: Option<u64>,
    pub cid: u64,
    pub title: String,
    pub cover: Option<String>,
    pub duration: Option<u64>,
    pub section_title: Option<String>,
    pub index: usize,
    pub is_current: bool,
}

/// UGC 合集元信息 + 条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UgcSeasonDTO {
    pub season_id: u64,
    pub title: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub upper: String,
    pub current_index: usize,
    pub episodes: Vec<UgcEpisodeDTO>,
}

/// UGC 合集 → DTO（`currentIndex` 找不到时回退 0）。
pub fn ugc_season_dto(season: &bilidown_core::api::video::UgcSeason) -> UgcSeasonDTO {
    UgcSeasonDTO {
        season_id: season.season_id,
        title: season.title.clone(),
        cover_url: normalize_cover(season.cover_url.clone()),
        intro: season
            .intro
            .clone()
            .filter(|intro| !intro.trim().is_empty()),
        upper: season.upper.clone(),
        current_index: season.current_index().unwrap_or(0),
        episodes: season
            .episodes
            .iter()
            .map(|episode| UgcEpisodeDTO {
                bvid: episode.bvid.clone(),
                aid: episode.aid,
                cid: episode.cid,
                title: episode.title.clone(),
                cover: normalize_cover(episode.cover.clone()),
                duration: episode.duration,
                section_title: (!episode.section_title.trim().is_empty())
                    .then(|| episode.section_title.clone()),
                index: episode.index,
                is_current: episode.is_current,
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

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
    fn settings_patch_only_changes_present_fields() {
        let previous = Settings {
            max_concurrent: 3,
            history_limit: 500,
            theme: bilidown_core::ThemePreference::System,
            ..Default::default()
        };
        // 只改并发：主题/历史等其他字段保持。
        let patch: SettingsPatchDTO = serde_json::from_value(serde_json::json!({
            "maxConcurrent": 5
        }))
        .unwrap();
        let next = apply_settings_patch(&patch, &previous);
        assert_eq!(next.max_concurrent, 5);
        assert_eq!(next.history_limit, 500);
        assert_eq!(next.theme, previous.theme);
        assert_eq!(next.download_dir, previous.download_dir);

        // 并发 + 主题连续修改都保留（design D3 原子 patch）。
        let patch: SettingsPatchDTO = serde_json::from_value(serde_json::json!({
            "theme": "dark",
            "downloadSubtitle": true
        }))
        .unwrap();
        let next = apply_settings_patch(&patch, &next);
        assert_eq!(next.max_concurrent, 5);
        assert!(next.download_subtitle);
    }

    #[test]
    fn settings_patch_null_clears_nullable_fields() {
        let previous = Settings {
            download_dir: Some("/tmp/a".into()),
            ffmpeg_path: Some("/opt/ffmpeg".into()),
            accent_color: Some("#FB7299".into()),
            proxy: Some("http://127.0.0.1:7897".into()),
            ..Default::default()
        };
        // 显式 null 清空可空字段；proxy 用 mode=direct 切回直连。
        let patch: SettingsPatchDTO = serde_json::from_value(serde_json::json!({
            "downloadDir": null,
            "ffmpegPath": null,
            "accentColor": null,
            "proxy": { "mode": "direct", "url": "" }
        }))
        .unwrap();
        let next = apply_settings_patch(&patch, &previous);
        assert!(next.download_dir.is_none());
        assert!(next.ffmpeg_path.is_none());
        assert!(next.accent_color.is_none());
        assert!(next.proxy.is_none());
        // 省略字段保持：再次 patch 空对象不改任何东西。
        let empty: SettingsPatchDTO = serde_json::from_value(serde_json::json!({})).unwrap();
        let again = apply_settings_patch(&empty, &next);
        assert_eq!(again, next);
    }

    #[test]
    fn settings_patch_invalid_values_are_rejected_by_validation() {
        let previous = Settings::default();
        for (json, field) in [
            (serde_json::json!({"maxConcurrent": 0}), "max_concurrent"),
            (serde_json::json!({"maxConcurrent": 9}), "max_concurrent"),
            (serde_json::json!({"historyLimit": 3}), "history_limit"),
            (
                serde_json::json!({"proxy": {"mode": "manual", "host": "bad host"}}),
                "proxy",
            ),
        ] {
            let patch: SettingsPatchDTO = serde_json::from_value(json.clone()).unwrap();
            let next = apply_settings_patch(&patch, &previous);
            let errors = match next.validate() {
                Err(errors) => errors,
                Ok(()) => panic!("非法补丁未被拒绝: {json} -> {next:?}"),
            };
            assert!(
                errors.iter().any(|error| error.field == field),
                "缺失字段 {field}: {errors:?}"
            );
        }
    }

    #[test]
    fn file_probe_reuses_metadata_within_one_batch() {
        let dir = std::env::temp_dir().join(format!("bilidown-dto-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.mp4");
        std::fs::write(&path, b"abc").unwrap();
        let path_str = path.to_string_lossy().into_owned();
        let artifacts = vec![
            DownloadArtifact::media(PathBuf::from(&path_str), 3, Some(0), Some("P1".into())),
            DownloadArtifact::media(PathBuf::from(&path_str), 3, Some(1), Some("P2".into())),
            DownloadArtifact::media(
                dir.join("missing.mp4"),
                3,
                Some(2),
                Some("P3".into()),
            ),
        ];
        let mut probe = FileProbe::default();
        let dtos = artifact_dtos_with(&artifacts, &HashSet::new(), &mut probe);
        assert!(dtos[0].valid && dtos[1].valid);
        assert!(!dtos[2].exists);
        assert_eq!(
            probe.entries.len(),
            2,
            "同一路径只探测一次，缺失路径也只探测一次"
        );
        // 同批次内第二个统计复用命中缓存，不重复 stat。
        let again = artifact_dtos_with(&artifacts, &HashSet::new(), &mut probe);
        assert_eq!(again.len(), 3);
        assert_eq!(probe.entries.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quality_qn_covers_all_ui_tiers() {
        for (label, qn) in [
            ("8K", 127),
            ("杜比视界", 126),
            ("Dolby", 126),
            ("HDR", 125),
            ("4K", 120),
            ("1080P60", 116),
            ("1080P+", 112),
            ("1080P", 80),
            ("720P60", 74),
            ("720P", 64),
            ("480P", 32),
            ("360P", 16),
        ] {
            assert_eq!(quality_qn(label), Some(qn), "label={label}");
        }
        assert_eq!(quality_qn("126"), Some(126));
        assert_eq!(quality_qn("125"), Some(125));
        assert_eq!(quality_qn(""), None);
        assert_eq!(quality_qn("best"), None);
        assert_eq!(quality_qn("unknown"), None);
    }

    #[test]
    fn settings_appearance_roundtrip() {
        let previous = Settings::default();
        let mut dto = settings_dto(&previous);
        assert_eq!(dto.accent_color, None);
        assert_eq!(dto.motion.as_deref(), Some("system"));
        assert_eq!(dto.quality_fallback, Some(true));
        assert_eq!(dto.glass_background, Some(true));

        dto.accent_color = Some("#3b82f6".into());
        dto.motion = Some("reduce".into());
        dto.quality_fallback = Some(false);
        dto.glass_background = Some(false);
        let next = settings_from_dto(&dto, &previous);
        assert_eq!(next.accent_color.as_deref(), Some("#3B82F6"));
        assert_eq!(next.motion, MotionPreference::Reduce);
        assert!(!next.quality_fallback);
        assert!(!next.window_glass);

        // 显式清空强调色 = 回退默认
        dto.accent_color = Some(String::new());
        dto.motion = Some("normal".into());
        let reset = settings_from_dto(&dto, &next);
        assert_eq!(reset.accent_color, None);
        assert_eq!(reset.motion, MotionPreference::Normal);

        // 非法值不覆盖旧值
        dto.accent_color = Some("#zzzzzz".into());
        let invalid = settings_from_dto(&dto, &next);
        assert_eq!(invalid.accent_color.as_deref(), Some("#3B82F6"));
    }

    #[test]
    fn settings_extras_flags_roundtrip() {
        let previous = Settings::default();
        let mut dto = settings_dto(&previous);
        assert_eq!(dto.download_subtitle, Some(false));
        assert_eq!(dto.download_cover, Some(false));
        assert_eq!(dto.check_updates_on_startup, Some(true));

        dto.download_subtitle = Some(true);
        dto.download_cover = Some(true);
        dto.check_updates_on_startup = Some(false);
        let next = settings_from_dto(&dto, &previous);
        assert!(next.download_subtitle);
        assert!(next.download_cover);
        assert!(!next.check_updates_on_startup);
    }

    #[test]
    fn add_options_prefer_explicit_qn_and_audio() {
        let mut spec = TaskSpec::new(bilidown_core::api::types::BiliId::Video {
            bvid: "BV1xx411c7mD".into(),
        });
        let options = AddOptionsDTO {
            quality: "1080P".into(),
            quality_qn: Some(126),
            codec: "hevc".into(),
            audio_format: "original".into(),
            audio_qn: Some(30251),
            output_dir: String::new(),
            entry_indexes: None,
            note: None,
        };
        apply_add_options(&mut spec, &options, true);
        assert_eq!(spec.quality.map(|q| q.qn()), Some(126));
        assert_eq!(spec.audio_quality.map(|q| q.id()), Some(30251));
        assert_eq!(spec.codec, Some(bilidown_core::api::types::Codec::Hevc));

        let mut explicit_empty = options.clone();
        explicit_empty.entry_indexes = Some(Vec::new());
        apply_add_options(&mut spec, &explicit_empty, true);
        assert_eq!(spec.selected_entries, Some(Vec::new()));

        let mut explicit = options;
        explicit.entry_indexes = Some(vec![2, 7, 12]);
        apply_add_options(&mut spec, &explicit, true);
        assert_eq!(spec.selected_entries, Some(vec![2, 7, 12]));

        assert_eq!(
            validate_add_options(&explicit_empty),
            Err("至少选择一个条目".into())
        );
        explicit.entry_indexes = None;
        assert!(validate_add_options(&explicit).is_ok());
    }

    #[test]
    fn task_dto_reports_effective_selected_entry_count() {
        let mut view = TaskView::new(TaskSpec::new(bilidown_core::api::types::BiliId::Video {
            bvid: "BV1xx411c7mD".into(),
        }));
        view.entries = (0..30).map(|index| format!("P{index}")).collect();
        view.selected_entries = vec![18, 24];

        let dto = task_dto(&view, &Settings::default(), 0);
        assert_eq!(dto.entry_count, Some(2));
        assert_eq!(dto.progress.stage_total, 2);

        view.selected_entries = (0..30).collect();
        assert_eq!(
            task_dto(&view, &Settings::default(), 0).entry_count,
            Some(30)
        );

        view.entries = vec!["单集".into()];
        view.selected_entries = vec![0];
        assert_eq!(
            task_dto(&view, &Settings::default(), 0).entry_count,
            Some(1)
        );

        // 重启恢复时视图条目标题可能为空，任务规格仍保留本次选择。
        view.entries.clear();
        view.selected_entries.clear();
        view.spec.selected_entries = Some(vec![18, 24]);
        let restored = task_dto(&view, &Settings::default(), 0);
        assert_eq!(restored.entry_count, Some(2));
        assert_eq!(restored.progress.stage_total, 2);

        // 更旧的持久化数据没有选择下标时回退到已知总条目数，避免显示 0。
        view.entries = (0..30).map(|index| format!("P{index}")).collect();
        view.spec.selected_entries = None;
        let legacy = task_dto(&view, &Settings::default(), 0);
        assert_eq!(legacy.entry_count, Some(30));
        assert_eq!(legacy.progress.stage_total, 30);
    }

    #[test]
    fn history_dto_marks_legacy_single_and_sums_new_artifacts() {
        let legacy = history_dto(HistoryRecord {
            id: "legacy".into(),
            task_id: Some("task".into()),
            title: "旧任务".into(),
            cover_url: None,
            source_kind: "video".into(),
            source_id: "BV1".into(),
            page_url: None,
            quality: Some("1080P".into()),
            output_path: Some("/tmp/old.mp4".into()),
            file_size: Some(1024),
            artifacts: "[]".into(),
            status: "done".into(),
            error: None,
            finished_at: 1,
        });
        assert_eq!(legacy.artifact_coverage, "legacy-single");
        assert_eq!(legacy.artifacts.len(), 1);
        assert_eq!(legacy.file_size, 1024);

        let artifacts = vec![
            DownloadArtifact::media(PathBuf::from("/tmp/a.mp4"), 10, Some(0), Some("P1".into())),
            DownloadArtifact::media(PathBuf::from("/tmp/b.mp4"), 20, Some(1), Some("P2".into())),
        ];
        let current = history_dto(HistoryRecord {
            id: "current".into(),
            task_id: Some("task".into()),
            title: "新任务".into(),
            cover_url: None,
            source_kind: "video".into(),
            source_id: "BV2".into(),
            page_url: None,
            quality: Some("1080P".into()),
            output_path: Some("/tmp/a.mp4".into()),
            file_size: Some(30),
            artifacts: serde_json::to_string(&artifacts).unwrap(),
            status: "done".into(),
            error: None,
            finished_at: 2,
        });
        assert_eq!(current.artifact_coverage, "complete");
        assert_eq!(current.artifacts.len(), 2);
        assert_eq!(current.file_size, 30);
    }

    #[test]
    fn legacy_task_artifact_remains_openable_when_file_matches_metadata() {
        let path = std::env::temp_dir().join(format!("bilidown-legacy-{}.mp4", now_ms()));
        std::fs::write(&path, b"legacy").unwrap();
        let mut spec = TaskSpec::new(bilidown_core::api::types::BiliId::Video {
            bvid: "BVlegacy".into(),
        });
        spec.selected_entries = Some(vec![0]);
        let mut view = TaskView::new(spec);
        view.output_path = Some(path.to_string_lossy().into_owned());
        view.artifacts_legacy = true;
        view.artifacts.push(DownloadArtifact::media(
            path.clone(),
            6,
            None,
            None,
        ));

        let dto = task_dto(&view, &Settings::default(), 0);
        assert_eq!(dto.artifact_coverage, "legacy-single");
        assert_eq!(dto.artifacts.len(), 1);
        assert!(dto.artifacts[0].valid);
        let _ = std::fs::remove_file(path);
    }

    // ---- 内容源扩展（add-download-features D3）----

    #[test]
    fn ugc_season_dto_maps_current_and_sections() {
        use bilidown_core::api::video::{UgcEpisode, UgcSeason};
        let season = UgcSeason {
            season_id: 777,
            title: "合集标题".into(),
            cover_url: Some("http://i0.hdslb.com/s.jpg".into()),
            intro: Some("简介".into()),
            upper: "UP主".into(),
            sections: Vec::new(),
            episodes: vec![
                UgcEpisode {
                    bvid: "BV1xx411c7mD".into(),
                    aid: Some(1),
                    cid: 11,
                    title: "第一话".into(),
                    cover: None,
                    duration: Some(100),
                    section_index: 0,
                    section_title: "正片".into(),
                    index: 0,
                    is_current: false,
                },
                UgcEpisode {
                    bvid: "BV1yy411c7mE".into(),
                    aid: None,
                    cid: 22,
                    title: "第二话".into(),
                    cover: None,
                    duration: None,
                    section_index: 0,
                    section_title: String::new(),
                    index: 1,
                    is_current: true,
                },
            ],
            current_bvid: "BV1yy411c7mE".into(),
        };
        let dto = ugc_season_dto(&season);
        assert_eq!(dto.season_id, 777);
        assert_eq!(dto.current_index, 1);
        assert_eq!(dto.episodes.len(), 2);
        assert_eq!(dto.episodes[0].section_title.as_deref(), Some("正片"));
        assert!(dto.episodes[1].section_title.is_none());
        assert!(dto.episodes[1].is_current);
        assert_eq!(dto.cover_url.as_deref(), Some("https://i0.hdslb.com/s.jpg"));
    }


}
