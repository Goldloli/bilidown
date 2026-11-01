//! 真实下载管线（design D3/D5/D6）：
//!
//! 解析 → playurl 选流（含回退）→ DASH 双流下载（Range 续传）→ ffmpeg 合并
//! → 命名去重 → 原子落到输出目录。音频/歌单/专栏走各自分支。
//!
//! 暂停语义：下载阶段立即停（保留 `.part` 与已完成临时文件），合并阶段 kill
//! ffmpeg 并删除半成品；恢复时重新取 playurl，按已下载字节续传/重新合并。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::api::client::BiliClient;
use crate::api::error::{BiliError, BiliResult};
use crate::api::types::{AudioFormat, BiliId, Content, DownloadStage, Entry, Progress, TaskSpec};
use crate::api::{audio, pcdn, subtitle, video};
use crate::download::{self, DownloadProgress, DownloadRequest};
use crate::media::cover;
use crate::media::ffmpeg::{Ffmpeg, MergeRequest};
use crate::media::quality::{self, StreamChoice, StreamSelection};
use crate::media::{filename, output};
use crate::storage::Settings;

use super::state::{
    ActualParams, ArtifactKind, DownloadArtifact, TaskExtras, TaskId, TaskMeta, TaskOutcome,
    TaskPhase, TaskResume, WorkerEvent,
};

/// 管线执行上下文。
pub struct TaskRunCtx {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub resume: TaskResume,
    pub cancel: CancellationToken,
    pub paused: Arc<std::sync::atomic::AtomicBool>,
    pub reporter: TaskReporter,
    pub client: Arc<BiliClient>,
    /// 下载专用 HTTP client（read idle 60s）
    pub http: reqwest::Client,
    pub settings: Settings,
    pub ffmpeg: Option<Ffmpeg>,
}

impl TaskRunCtx {
    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn pause_requested(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    fn check_cancel(&self) -> BiliResult<()> {
        if self.cancelled() {
            Err(BiliError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn output_dir(&self) -> PathBuf {
        self.spec
            .output_dir
            .clone()
            .unwrap_or_else(|| self.settings.effective_download_dir())
    }

    /// 任务临时目录：输出目录下 `.bilidown-tmp/<task-id>`（同卷，rename 原子）。
    fn task_temp_dir(&self) -> PathBuf {
        self.output_dir().join(".bilidown-tmp").join(&self.id)
    }
}

/// worker → manager 事件上报器。
#[derive(Debug, Clone)]
pub struct TaskReporter {
    id: TaskId,
    tx: mpsc::UnboundedSender<WorkerEvent>,
    cancel: CancellationToken,
}

impl TaskReporter {
    pub fn new(
        id: TaskId,
        tx: mpsc::UnboundedSender<WorkerEvent>,
        cancel: CancellationToken,
    ) -> Self {
        Self { id, tx, cancel }
    }

    pub fn meta(&self, meta: TaskMeta) {
        let _ = self.tx.send(WorkerEvent::Meta {
            id: self.id.clone(),
            meta,
        });
    }

    pub fn phase(&self, phase: TaskPhase) {
        let _ = self.tx.send(WorkerEvent::Phase {
            id: self.id.clone(),
            phase,
        });
    }

    pub fn progress(&self, progress: Progress, speed_bps: f64) {
        let _ = self.tx.send(WorkerEvent::Progress {
            id: self.id.clone(),
            progress,
            speed_bps,
        });
    }

    pub fn actual(&self, actual: ActualParams) {
        let _ = self.tx.send(WorkerEvent::Actual {
            id: self.id.clone(),
            actual,
        });
    }

    pub async fn output(&self, artifact: DownloadArtifact) -> BiliResult<()> {
        let (ack, reply) = tokio::sync::oneshot::channel();
        self.tx
            .send(WorkerEvent::Output {
                id: self.id.clone(),
                artifact,
                ack,
            })
            .map_err(|_| {
                BiliError::Other("task manager stopped before artifact checkpoint".into())
            })?;
        tokio::select! {
            reply = reply => {
                reply.map_err(|_| BiliError::Other("task manager dropped artifact checkpoint reply".into()))?
            }
            _ = self.cancel.cancelled() => Err(BiliError::Cancelled),
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                Err(BiliError::Other("artifact checkpoint acknowledgement timed out".into()))
            }
        }
    }
}

/// 可替换的下载管线（生产实现 = [`DownloadPipeline`]；测试用 mock）。
#[async_trait]
pub trait TaskRunner: Send + Sync {
    async fn run(&self, ctx: TaskRunCtx) -> BiliResult<TaskOutcome>;
}

/// 生产下载管线（无状态）。
#[derive(Debug, Default, Clone, Copy)]
pub struct DownloadPipeline;

#[async_trait]
impl TaskRunner for DownloadPipeline {
    async fn run(&self, ctx: TaskRunCtx) -> BiliResult<TaskOutcome> {
        run_pipeline(ctx).await
    }
}

async fn run_pipeline(ctx: TaskRunCtx) -> BiliResult<TaskOutcome> {
    ctx.reporter.phase(TaskPhase::Resolving);
    match ctx.spec.id.clone() {
        BiliId::Audio { song_id } => {
            if ctx.resume.completed_entries.contains(&0) {
                return Ok(TaskOutcome::default());
            }
            return run_audio(&ctx, &song_id, None, 0, 0, true).await;
        }
        BiliId::AudioList { menu_id } => return run_audio_list(&ctx, &menu_id).await,
        _ => {}
    }
    run_video_content(ctx).await
}

async fn run_video_content(ctx: TaskRunCtx) -> BiliResult<TaskOutcome> {
    let content = video::resolve_content(&ctx.client, &ctx.spec.id).await?;
    ctx.check_cancel()?;
    let selected = selected_entries(ctx.spec.selected_entries.as_deref(), || {
        video::default_entries(&ctx.spec.id, &content, ctx.spec.page_hint)
    })?;
    ctx.reporter.meta(TaskMeta {
        title: content.title.clone(),
        upper: content.upper.clone(),
        cover_url: content.cover_url.clone(),
        page_url: ctx.spec.id.page_url(),
        content_kind: ctx.spec.id.kind(),
        entries: content.entries.iter().map(|e| e.title.clone()).collect(),
        selected_entries: selected.clone(),
        duration_secs: content.duration_secs,
    });
    let logged_in = ctx.client.is_logged_in();
    let out_dir = ctx.output_dir();
    std::fs::create_dir_all(&out_dir)?;
    let tmp_dir = ctx.task_temp_dir();
    std::fs::create_dir_all(&tmp_dir)?;
    let multi = selected.len() > 1 || content.entries.len() > 1;
    let mut artifacts = Vec::new();
    let mut extras = TaskExtras::default();
    let mut completed_count = ctx.resume.completed_entries.len() as u32;
    for idx in &selected {
        ctx.check_cancel()?;
        if ctx.resume.completed_entries.contains(idx) {
            continue;
        }
        let entry = content
            .entries
            .get(*idx)
            .ok_or_else(|| BiliError::Other(format!("entry index {idx} out of range")))?;
        let (entry_artifacts, entry_extras) = run_video_entry(
            &ctx,
            &content,
            entry,
            multi,
            logged_in,
            &out_dir,
            &tmp_dir,
            completed_count,
        )
        .await?;
        artifacts.extend(entry_artifacts);
        extras.merge(entry_extras);
        completed_count += 1;
        ctx.reporter.progress(
            Progress {
                downloaded: 0,
                total: 0,
                stage: DownloadStage::Downloading,
                entries_done: completed_count,
            },
            0.0,
        );
    }
    Ok(TaskOutcome {
        output_path: artifacts
            .first()
            .map(|artifact| PathBuf::from(&artifact.path)),
        file_size: artifacts.iter().map(|artifact| artifact.size).sum(),
        artifacts,
        extras,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_video_entry(
    ctx: &TaskRunCtx,
    content: &Content,
    entry: &Entry,
    multi: bool,
    logged_in: bool,
    out_dir: &Path,
    tmp_dir: &Path,
    entry_pos: u32,
) -> BiliResult<(Vec<DownloadArtifact>, TaskExtras)> {
    let play = video::play_url(&ctx.client, &ctx.spec.id, entry, logged_in).await?;
    ctx.check_cancel()?;
    let choice = StreamChoice {
        quality: ctx.spec.quality,
        codec: ctx.spec.codec,
        audio_quality: ctx.spec.audio_quality,
    };
    let selection =
        quality::select(&play, choice, ctx.settings.quality_fallback).ok_or_else(|| {
            if !ctx.settings.quality_fallback && ctx.spec.quality.is_some() {
                BiliError::Other(
                    "指定清晰度不可用：设置中已关闭自动回退，请改选可用档位或开启自动回退".into(),
                )
            } else {
                BiliError::Other("no downloadable stream returned by playurl".into())
            }
        })?;
    ctx.reporter.actual(ActualParams {
        quality: Some(selection.quality),
        codec: Some(selection.codec),
        audio: selection.audio_quality(),
    });
    let entry_title = if multi {
        Some(entry.title.as_str())
    } else {
        None
    };
    let pcdn_filter = ctx.settings.pcdn_filter;
    let video_urls = filtered(&selection.video_urls(), pcdn_filter);
    if selection.single_stream {
        return download_single_stream(
            ctx,
            content,
            entry,
            entry_title,
            &selection,
            &video_urls,
            out_dir,
            tmp_dir,
            entry_pos,
        )
        .await;
    }
    let audio_track = selection
        .audio
        .as_ref()
        .ok_or_else(|| BiliError::Other("dash response has no audio stream".into()))?;
    let audio_urls = filtered(&selection.audio_urls(), pcdn_filter);
    let container = MergeRequest::container_for(Some(audio_track.format));
    let video_temp = tmp_dir.join(format!("entry-{}.video.m4s", entry.index));
    let audio_temp = tmp_dir.join(format!("entry-{}.audio.m4s", entry.index));
    let merged_temp = tmp_dir.join(format!("entry-{}.merged.{container}", entry.index));

    let resume_merge = ctx.resume.stage == DownloadStage::Merging
        && file_len(&video_temp).await > 0
        && file_len(&audio_temp).await > 0;
    if !resume_merge {
        ctx.reporter.phase(TaskPhase::Downloading);
        download_pair(
            ctx,
            &video_urls,
            &audio_urls,
            &video_temp,
            &audio_temp,
            entry_pos,
        )
        .await?;
    }
    ctx.check_cancel()?;

    let ffmpeg = match &ctx.ffmpeg {
        Some(f) => f.clone(),
        None => {
            return degrade_without_ffmpeg(
                ctx,
                content,
                entry,
                entry_title,
                &selection,
                &video_temp,
                &audio_temp,
                out_dir,
            )
            .await;
        }
    };
    ctx.reporter.phase(TaskPhase::Merging);
    let title_meta = ctx.settings.write_metadata.then(|| match entry_title {
        Some(t) => format!("{} - {t}", content.title),
        None => content.title.clone(),
    });
    let total_us = entry.duration_secs.map(|s| s * 1_000_000);
    let reporter = ctx.reporter.clone();
    let stage_done = entry_pos;
    merge_entry(
        &ffmpeg,
        &video_temp,
        &audio_temp,
        &merged_temp,
        title_meta.clone(),
        &container,
        total_us,
        &ctx.cancel,
        reporter.clone(),
        stage_done,
    )
    .await?;
    if !merged_temp.exists() {
        // 合并产物瞬时丢失（外部清理/索引竞态）：重试一次
        tracing::warn!(
            target: "bilidown_core::task",
            task = %ctx.id,
            out = %merged_temp.display(),
            "ffmpeg 合并产物缺失，重试一次合并"
        );
        let _ = tokio::fs::remove_file(&merged_temp).await;
        merge_entry(
            &ffmpeg,
            &video_temp,
            &audio_temp,
            &merged_temp,
            title_meta,
            &container,
            total_us,
            &ctx.cancel,
            reporter,
            stage_done,
        )
        .await?;
        if !merged_temp.exists() {
            return Err(BiliError::Disk(format!(
                "ffmpeg 两次合并后仍未找到产物 {}",
                merged_temp.display()
            )));
        }
    }
    let _ = tokio::fs::remove_file(&video_temp).await;
    let _ = tokio::fs::remove_file(&audio_temp).await;
    let file_name =
        filename::build_video_name(&content.title, entry_title, selection.quality, &container);
    let final_path = publish_file(&merged_temp, out_dir, &file_name).await?;
    let size = file_len(&final_path).await;
    let artifact = DownloadArtifact::media(
        final_path.clone(),
        size,
        Some(entry.index),
        entry_title.map(str::to_string),
    );
    ctx.reporter.output(artifact.clone()).await?;
    // 可选后处理（字幕/封面）：主产物已落盘，失败不影响任务结果
    let extras = post_process(ctx, content, entry, &final_path).await;
    Ok((vec![artifact], extras))
}

#[allow(clippy::too_many_arguments)]
async fn download_single_stream(
    ctx: &TaskRunCtx,
    content: &Content,
    entry: &Entry,
    entry_title: Option<&str>,
    selection: &StreamSelection,
    urls: &[String],
    out_dir: &Path,
    tmp_dir: &Path,
    entry_pos: u32,
) -> BiliResult<(Vec<DownloadArtifact>, TaskExtras)> {
    ctx.reporter.phase(TaskPhase::Downloading);
    let ext = if selection.video.mime.contains("flv") {
        "flv"
    } else {
        "mp4"
    };
    let temp = tmp_dir.join(format!("entry-{}.single.{ext}", entry.index));
    let tracker = PairProgress::new(ctx.reporter.clone(), DownloadStage::Downloading, entry_pos);
    let t = tracker.clone();
    download::download_stream(
        &DownloadRequest::new(&ctx.http, urls, &temp, &ctx.cancel),
        move |p: DownloadProgress| t.report_video(p),
    )
    .await?;
    ctx.check_cancel()?;
    let file_name = filename::build_video_name(&content.title, entry_title, selection.quality, ext);
    let final_path = publish_file(&temp, out_dir, &file_name).await?;
    let size = file_len(&final_path).await;
    let artifact = DownloadArtifact::media(
        final_path.clone(),
        size,
        Some(entry.index),
        entry_title.map(str::to_string),
    );
    ctx.reporter.output(artifact.clone()).await?;
    let extras = post_process(ctx, content, entry, &final_path).await;
    Ok((vec![artifact], extras))
}

#[allow(clippy::too_many_arguments)]
async fn degrade_without_ffmpeg(
    ctx: &TaskRunCtx,
    content: &Content,
    entry: &Entry,
    entry_title: Option<&str>,
    selection: &StreamSelection,
    video_temp: &Path,
    audio_temp: &Path,
    out_dir: &Path,
) -> BiliResult<(Vec<DownloadArtifact>, TaskExtras)> {
    if !ctx.settings.allow_streams_only {
        return Err(BiliError::Ffmpeg(
            "ffmpeg not found; enable streams-only fallback in settings".into(),
        ));
    }
    tracing::warn!(task = %ctx.id, "ffmpeg missing, saving raw streams (degraded)");
    let stem = filename::build_video_name(&content.title, entry_title, selection.quality, "m4s");
    let stem = stem.trim_end_matches(".m4s").to_string();
    let video_final = publish_file(video_temp, out_dir, &format!("{stem}.video.m4s")).await?;
    let video_size = file_len(&video_final).await;
    let video_artifact = DownloadArtifact::stream(
        video_final.clone(),
        video_size,
        entry.index,
        entry_title.map(str::to_string),
        ArtifactKind::VideoStream,
    );
    ctx.reporter.output(video_artifact.clone()).await?;
    let mut artifacts = vec![video_artifact];
    if file_len(audio_temp).await > 0 {
        let audio_final = publish_file(audio_temp, out_dir, &format!("{stem}.audio.m4s")).await?;
        let audio_size = file_len(&audio_final).await;
        let audio_artifact = DownloadArtifact::stream(
            audio_final,
            audio_size,
            entry.index,
            entry_title.map(str::to_string),
            ArtifactKind::AudioStream,
        );
        ctx.reporter.output(audio_artifact.clone()).await?;
        artifacts.push(audio_artifact);
    }
    let extras = post_process(ctx, content, entry, &video_final).await;
    Ok((artifacts, extras))
}

async fn download_pair(
    ctx: &TaskRunCtx,
    video_urls: &[String],
    audio_urls: &[String],
    video_temp: &Path,
    audio_temp: &Path,
    entry_pos: u32,
) -> BiliResult<()> {
    let tracker = PairProgress::new(ctx.reporter.clone(), DownloadStage::Downloading, entry_pos);
    let tv = tracker.clone();
    let ta = tracker.clone();
    let v = async {
        let dest = video_temp.to_path_buf();
        download::download_stream(
            &DownloadRequest::new(&ctx.http, video_urls, &dest, &ctx.cancel),
            move |p: DownloadProgress| tv.report_video(p),
        )
        .await
        .map(|_| ())
    };
    let a = async {
        let dest = audio_temp.to_path_buf();
        download::download_stream(
            &DownloadRequest::new(&ctx.http, audio_urls, &dest, &ctx.cancel),
            move |p: DownloadProgress| ta.report_audio(p),
        )
        .await
        .map(|_| ())
    };
    tokio::try_join!(v, a)?;
    Ok(())
}

/// 双流聚合进度（两个下载回调共享，节流由下载层 250ms 保证）。
#[derive(Clone)]
struct PairProgress {
    reporter: TaskReporter,
    stage: DownloadStage,
    entries_done: u32,
    v_done: Arc<AtomicU64>,
    v_total: Arc<AtomicU64>,
    a_done: Arc<AtomicU64>,
    a_total: Arc<AtomicU64>,
    last: Arc<Mutex<(Instant, u64)>>,
    speed: Arc<AtomicU64>,
}

impl PairProgress {
    fn new(reporter: TaskReporter, stage: DownloadStage, entries_done: u32) -> Self {
        Self {
            reporter,
            stage,
            entries_done,
            v_done: Arc::default(),
            v_total: Arc::default(),
            a_done: Arc::default(),
            a_total: Arc::default(),
            last: Arc::new(Mutex::new((Instant::now(), 0))),
            speed: Arc::default(),
        }
    }

    fn report_video(&self, p: DownloadProgress) {
        self.v_done.store(p.downloaded, Ordering::Relaxed);
        self.v_total.store(p.total, Ordering::Relaxed);
        self.emit();
    }

    fn report_audio(&self, p: DownloadProgress) {
        self.a_done.store(p.downloaded, Ordering::Relaxed);
        self.a_total.store(p.total, Ordering::Relaxed);
        self.emit();
    }

    fn emit(&self) {
        let vd = self.v_done.load(Ordering::Relaxed);
        let ad = self.a_done.load(Ordering::Relaxed);
        let vt = self.v_total.load(Ordering::Relaxed);
        let at = self.a_total.load(Ordering::Relaxed);
        let downloaded = vd + ad;
        let total = vt + at;
        if let Ok(mut last) = self.last.lock() {
            let elapsed = last.0.elapsed().as_secs_f64();
            if elapsed >= 0.2 {
                let delta = downloaded.saturating_sub(last.1);
                self.speed
                    .store((delta as f64 / elapsed) as u64, Ordering::Relaxed);
                *last = (Instant::now(), downloaded);
            }
        }
        self.reporter.progress(
            Progress {
                downloaded,
                total,
                stage: self.stage,
                entries_done: self.entries_done,
            },
            self.speed.load(Ordering::Relaxed) as f64,
        );
    }
}

// ---- 音频 / 歌单 / 专栏 ----

async fn run_audio(
    ctx: &TaskRunCtx,
    song_id: &str,
    entry_title: Option<&str>,
    entry_index: usize,
    completed_before: u32,
    emit_meta: bool,
) -> BiliResult<TaskOutcome> {
    let song = audio::resolve_song(&ctx.client, song_id).await?;
    ctx.check_cancel()?;
    if emit_meta {
        ctx.reporter.meta(TaskMeta {
            title: song.title.clone(),
            upper: song.upper.clone(),
            cover_url: None,
            page_url: ctx.spec.id.page_url(),
            content_kind: ctx.spec.id.kind(),
            entries: vec![song.title.clone()],
            selected_entries: vec![entry_index],
            duration_secs: None,
        });
    }
    ctx.reporter.phase(TaskPhase::Downloading);
    let stream = audio::play_url(&ctx.client, song_id).await?;
    let out_dir = ctx.output_dir();
    std::fs::create_dir_all(&out_dir)?;
    let tmp_dir = ctx.task_temp_dir();
    std::fs::create_dir_all(&tmp_dir)?;
    let src_ext = stream.format.extension();
    let temp = tmp_dir.join(format!("audio-{song_id}.{src_ext}"));
    let urls = filtered(&stream.urls(), ctx.settings.pcdn_filter);
    let tracker = PairProgress::new(
        ctx.reporter.clone(),
        DownloadStage::Downloading,
        completed_before,
    );
    let t = tracker.clone();
    download::download_stream(
        &DownloadRequest::new(&ctx.http, &urls, &temp, &ctx.cancel),
        move |p: DownloadProgress| t.report_video(p),
    )
    .await?;
    ctx.check_cancel()?;
    let to_mp3 = ctx.spec.to_mp3;
    let (final_source, format) = if to_mp3 {
        let ffmpeg = ctx
            .ffmpeg
            .clone()
            .ok_or_else(|| BiliError::Ffmpeg("ffmpeg not found; cannot transcode to mp3".into()))?;
        ctx.reporter.phase(TaskPhase::Merging);
        let mp3_temp = tmp_dir.join(format!("audio-{song_id}.mp3"));
        let title = song.title.clone();
        ffmpeg
            .transcode_mp3(&temp, &mp3_temp, Some(&title), &ctx.cancel, |_| {})
            .await?;
        let _ = tokio::fs::remove_file(&temp).await;
        (mp3_temp, AudioFormat::Mp3)
    } else {
        (temp, stream.format)
    };
    let file_name = filename::build_audio_name(&song.title, entry_title, None, format, to_mp3);
    let final_path = publish_file(&final_source, &out_dir, &file_name).await?;
    let size = file_len(&final_path).await;
    let artifact = DownloadArtifact::media(
        final_path.clone(),
        size,
        Some(entry_index),
        entry_title.map(str::to_string),
    );
    ctx.reporter.output(artifact.clone()).await?;
    Ok(TaskOutcome {
        output_path: Some(final_path),
        file_size: size,
        artifacts: vec![artifact],
        extras: TaskExtras::default(),
    })
}

async fn run_audio_list(ctx: &TaskRunCtx, menu_id: &str) -> BiliResult<TaskOutcome> {
    let content = audio::resolve_menu(&ctx.client, menu_id).await?;
    ctx.check_cancel()?;
    let selected = selected_entries(ctx.spec.selected_entries.as_deref(), || {
        (0..content.entries.len()).collect()
    })?;
    ctx.reporter.meta(TaskMeta {
        title: content.title.clone(),
        upper: content.upper.clone(),
        cover_url: content.cover_url.clone(),
        page_url: ctx.spec.id.page_url(),
        content_kind: ctx.spec.id.kind(),
        entries: content.entries.iter().map(|e| e.title.clone()).collect(),
        selected_entries: selected.clone(),
        duration_secs: content.duration_secs,
    });
    let multi = selected.len() > 1;
    let mut artifacts = Vec::new();
    let mut completed_count = ctx.resume.completed_entries.len() as u32;
    for idx in &selected {
        ctx.check_cancel()?;
        if ctx.resume.completed_entries.contains(idx) {
            continue;
        }
        let entry = content
            .entries
            .get(*idx)
            .ok_or_else(|| BiliError::Other(format!("track index {idx} out of range")))?;
        let title = multi.then_some(entry.title.as_str());
        let outcome = run_audio(
            ctx,
            &entry.cid.to_string(),
            title,
            *idx,
            completed_count,
            false,
        )
        .await?;
        artifacts.extend(outcome.artifacts);
        completed_count += 1;
        ctx.reporter.progress(
            Progress {
                downloaded: 0,
                total: 0,
                stage: DownloadStage::Downloading,
                entries_done: completed_count,
            },
            0.0,
        );
    }
    Ok(TaskOutcome {
        output_path: artifacts
            .first()
            .map(|artifact| PathBuf::from(&artifact.path)),
        file_size: artifacts.iter().map(|artifact| artifact.size).sum(),
        artifacts,
        extras: TaskExtras::default(),
    })
}

/// 视频产物落盘后的可选后处理（design D2：字幕 SRT + 封面）。
///
/// 语义：**绝不改变主任务结果**。单项失败只写 `tracing::warn` 与
/// [`TaskExtras`] 的 `*_error` 字段；无字幕/无封面属于正常情况（不报错）。
async fn post_process(
    ctx: &TaskRunCtx,
    content: &Content,
    entry: &Entry,
    video_path: &Path,
) -> TaskExtras {
    let mut extras = TaskExtras::default();
    if !ctx.settings.download_subtitle && !ctx.settings.download_cover {
        return extras; // 开关关闭：不发任何请求，行为与旧版本一致
    }
    if ctx.cancelled() {
        return extras;
    }
    let out_dir = video_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if ctx.settings.download_subtitle {
        let missing_refs = entry.aid.is_none() || entry.cid == 0;
        if missing_refs {
            tracing::warn!(
                target: "bilidown_core::post_process",
                task = %ctx.id,
                aid = ?entry.aid,
                cid = entry.cid,
                "字幕跳过：缺少 aid/cid"
            );
            extras.subtitle_error = Some("subtitle skipped: missing aid/cid".into());
        } else {
            let aid = entry.aid.unwrap_or_default();
            match subtitle::player_subtitles(&ctx.client, aid, entry.cid).await {
                Ok(tracks) if tracks.is_empty() => {
                    tracing::info!(
                        target: "bilidown_core::post_process",
                        task = %ctx.id,
                        aid,
                        cid = entry.cid,
                        "该条目没有可用字幕"
                    );
                }
                Ok(tracks) => {
                    let (paths, errors) = subtitle::save_subtitles(
                        &ctx.client,
                        &tracks,
                        video_path,
                        &out_dir,
                        Some(&ctx.cancel),
                    )
                    .await;
                    extras.subtitles = paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    tracing::info!(
                        target: "bilidown_core::post_process",
                        task = %ctx.id,
                        tracks = tracks.len(),
                        saved = extras.subtitles.len(),
                        errors = errors.len(),
                        "字幕后处理完成"
                    );
                    if !errors.is_empty() {
                        for err in &errors {
                            tracing::warn!(
                                target: "bilidown_core::post_process",
                                task = %ctx.id,
                                error = %err,
                                "字幕下载失败（不影响主任务）"
                            );
                        }
                        extras.subtitle_error = Some(errors.join("; "));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "bilidown_core::post_process",
                        task = %ctx.id,
                        error = %e.describe(),
                        "获取字幕列表失败（不影响主任务）"
                    );
                    extras.subtitle_error = Some(e.describe());
                }
            }
        }
    }

    if ctx.settings.download_cover && !ctx.cancelled() {
        match content.cover_url.as_deref() {
            None => {
                tracing::info!(
                    target: "bilidown_core::post_process",
                    task = %ctx.id,
                    "该内容没有封面 URL"
                );
            }
            Some(url) => match cover::download_cover(&ctx.http, url, video_path, &out_dir).await {
                Ok(path) => {
                    tracing::info!(
                        target: "bilidown_core::post_process",
                        task = %ctx.id,
                        cover = %path.display(),
                        "封面已保存"
                    );
                    extras.cover = Some(path.to_string_lossy().into_owned());
                }
                Err(e) => {
                    tracing::warn!(
                        target: "bilidown_core::post_process",
                        task = %ctx.id,
                        error = %e.describe(),
                        "封面下载失败（不影响主任务）"
                    );
                    extras.cover_error = Some(e.describe());
                }
            },
        }
    }
    extras
}

fn filtered(urls: &[String], pcdn: bool) -> Vec<String> {
    if pcdn {
        pcdn::filter_urls(urls)
    } else {
        urls.to_vec()
    }
}

fn selected_entries(
    configured: Option<&[usize]>,
    defaults: impl FnOnce() -> Vec<usize>,
) -> BiliResult<Vec<usize>> {
    let selected = match configured {
        Some(entries) => entries.to_vec(),
        None => defaults(),
    };
    if selected.is_empty() {
        Err(BiliError::Other("no entry selected".into()))
    } else {
        Ok(selected)
    }
}

async fn file_len(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

/// 同卷 rename；跨卷（EXDEV）时退化为 copy + remove。
#[allow(clippy::too_many_arguments)]
async fn merge_entry(
    ffmpeg: &Ffmpeg,
    video: &Path,
    audio: &Path,
    out: &Path,
    title: Option<String>,
    container: &str,
    total_us: Option<u64>,
    cancel: &CancellationToken,
    reporter: TaskReporter,
    entries_done: u32,
) -> BiliResult<()> {
    let req = MergeRequest {
        video: video.to_path_buf(),
        audio: Some(audio.to_path_buf()),
        out: out.to_path_buf(),
        title,
        container: container.to_string(),
        total_us,
    };
    ffmpeg
        .merge(&req, cancel, move |us| {
            reporter.progress(
                Progress {
                    downloaded: us,
                    total: total_us.unwrap_or(0),
                    stage: DownloadStage::Merging,
                    entries_done,
                },
                0.0,
            );
        })
        .await
}

async fn publish_file(src: &Path, dir: &Path, file_name: &str) -> BiliResult<PathBuf> {
    let src = src.to_path_buf();
    let dir = dir.to_path_buf();
    let file_name = file_name.to_string();
    tokio::task::spawn_blocking(move || output::publish_no_overwrite(&src, &dir, &file_name))
        .await
        .map_err(|error| BiliError::Disk(format!("发布产物任务异常终止: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_progress_aggregates_and_speeds() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let reporter = TaskReporter::new("t".into(), tx, CancellationToken::new());
        let p = PairProgress::new(reporter, DownloadStage::Downloading, 1);
        p.report_video(DownloadProgress {
            downloaded: 100,
            total: 1000,
        });
        p.report_audio(DownloadProgress {
            downloaded: 50,
            total: 500,
        });
        let mut last = None;
        while let Ok(ev) = rx.try_recv() {
            last = Some(ev);
        }
        match last.unwrap() {
            WorkerEvent::Progress { progress, .. } => {
                assert_eq!(progress.downloaded, 150);
                assert_eq!(progress.total, 1500);
                assert_eq!(progress.entries_done, 1);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn selection_keeps_explicit_empty_distinct_from_unspecified() {
        assert_eq!(
            selected_entries(None, || vec![2, 7, 12]).unwrap(),
            vec![2, 7, 12]
        );
        assert_eq!(
            selected_entries(Some(&[7]), || vec![2, 7, 12]).unwrap(),
            vec![7]
        );
        assert!(selected_entries(Some(&[]), || vec![2, 7, 12]).is_err());
    }

    #[tokio::test]
    async fn artifact_checkpoint_wait_cancels_with_task() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let reporter = TaskReporter::new("t".into(), tx, cancel.clone());
        let wait = tokio::spawn(async move {
            reporter
                .output(DownloadArtifact::media(
                    PathBuf::from("/tmp/a.mp4"),
                    1,
                    Some(0),
                    None,
                ))
                .await
        });
        let event = rx.recv().await.unwrap();
        let WorkerEvent::Output { ack, .. } = event else {
            panic!("expected artifact checkpoint");
        };
        cancel.cancel();
        assert!(matches!(wait.await.unwrap(), Err(BiliError::Cancelled)));
        drop(ack);
    }

    #[test]
    fn filename_helpers_used_by_pipeline() {
        // 冒烟：确保命名规则与容器规则可用
        let name = filename::build_video_name(
            "标题",
            Some("第一话"),
            crate::api::types::Quality::P1080,
            "mkv",
        );
        assert_eq!(name, "标题 - 第一话[1080P].mkv");
        assert_eq!(MergeRequest::container_for(Some(AudioFormat::Flac)), "mkv");
        assert_eq!(MergeRequest::container_for(Some(AudioFormat::M4a)), "mp4");
    }
}
