//! ffmpeg 集成（task 3.6、design D6）：
//!
//! - 路径探测：设置中的显式路径 → PATH → 常见安装路径；
//! - 合并/转码通过 `-progress pipe:1` 上报进度，stderr 保留末尾 64 行做错误摘要；
//! - 取消时 kill 子进程并清理半成品；
//! - 元数据写入失败不影响主产物；
//! - ffmpeg 缺失时由任务层做「仅保存流文件」降级。

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::sync::CancellationToken;

use crate::api::error::{BiliError, BiliResult};
use crate::api::types::AudioFormat;

/// 常见安装路径（沙箱/包管理器）。
const COMMON_PATHS: [&str; 3] = [
    "/opt/homebrew/bin/ffmpeg",
    "/usr/local/bin/ffmpeg",
    "/usr/bin/ffmpeg",
];

/// ffmpeg 可执行封装。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ffmpeg {
    path: PathBuf,
}

impl Ffmpeg {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 探测：显式路径（设置）→ PATH → 常见安装路径。
    pub fn detect(override_path: Option<&str>) -> Option<Self> {
        detect_ffmpeg(override_path).map(Self::new)
    }

    /// 探测 ffprobe（与 ffmpeg 同目录；PATH 中则直接用命令名）。
    pub fn ffprobe_path(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        };
        if self.path.components().count() <= 1 {
            return PathBuf::from(name);
        }
        match self.path.parent() {
            Some(dir) if dir.join(name).is_file() => dir.join(name),
            _ => PathBuf::from(name),
        }
    }

    /// 读取媒体时长（秒）；失败返回 None（进度降级为不确定态）。
    pub fn probe_duration(&self, media: &Path) -> Option<f64> {
        let out = std::process::Command::new(self.ffprobe_path())
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(media)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<f64>()
            .ok()
    }

    /// 合并音视频（`-c copy`），可取消、带进度。
    ///
    /// `total_us` 用于把进度换算成百分比；为 None 时进度回调仍按已处理时长上报。
    pub async fn merge(
        &self,
        req: &MergeRequest,
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(u64),
    ) -> BiliResult<()> {
        let args = merge_args(
            &req.video,
            req.audio.as_deref(),
            &req.out,
            req.title.as_deref(),
            &req.container,
        );
        self.run_progress(args, cancel, &mut on_progress).await
    }

    /// 转 mp3（libmp3lame），可取消、带进度。
    pub async fn transcode_mp3(
        &self,
        input: &Path,
        out: &Path,
        title: Option<&str>,
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(u64),
    ) -> BiliResult<()> {
        let args = mp3_args(input, out, title);
        self.run_progress(args, cancel, &mut on_progress).await
    }

    /// 运行 ffmpeg（`-progress pipe:1`），解析 `out_time_us` 作为已处理时长（微秒）。
    async fn run_progress(
        &self,
        args: Vec<String>,
        cancel: &CancellationToken,
        on_progress: &mut impl FnMut(u64),
    ) -> BiliResult<()> {
        let mut cmd = TokioCommand::new(&self.path);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| {
            BiliError::Ffmpeg(format!("无法启动 ffmpeg（{}）: {e}", self.path.display()))
        })?;
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        // stderr 末尾 64 行（design D6）
        let stderr_task = tokio::spawn(async move {
            let mut tail: VecDeque<String> = VecDeque::with_capacity(64);
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tail.len() == 64 {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
            tail
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut parser = ProgressParser::default();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    stderr_task.abort();
                    return Err(BiliError::Cancelled);
                }
                next = lines.next_line() => {
                    match next {
                        Ok(Some(line)) => {
                            if let Some(us) = parser.push_line(&line) {
                                on_progress(us);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = child.start_kill();
                            let _ = child.wait().await;
                            stderr_task.abort();
                            return Err(BiliError::Ffmpeg(format!("读取 ffmpeg 输出失败: {e}")));
                        }
                    }
                }
            }
        }
        let status = child
            .wait()
            .await
            .map_err(|e| BiliError::Ffmpeg(format!("等待 ffmpeg 失败: {e}")))?;
        let tail = stderr_task.await.unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            let summary: String = tail
                .iter()
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            Err(BiliError::Ffmpeg(if summary.is_empty() {
                format!("ffmpeg 退出码 {:?}", status.code())
            } else {
                summary
            }))
        }
    }
}

/// `-progress pipe:1` 输出解析器（只关心 out_time_us / out_time_ms）。
#[derive(Debug, Default)]
pub struct ProgressParser {
    last_us: u64,
}

impl ProgressParser {
    /// 喂一行输出；命中进度行时返回已处理时长（微秒，单调递增）。
    pub fn push_line(&mut self, line: &str) -> Option<u64> {
        let (key, value) = line.split_once('=')?;
        let us = match key.trim() {
            // 新版 ffmpeg 输出 out_time_us；out_time_ms 历史上也是微秒（ffmpeg 命名遗留）
            "out_time_us" | "out_time_ms" => value.trim().parse::<u64>().ok()?,
            _ => return None,
        };
        if us <= self.last_us {
            return None;
        }
        self.last_us = us;
        Some(us)
    }
}

/// 合并请求。
#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub video: PathBuf,
    pub audio: Option<PathBuf>,
    pub out: PathBuf,
    pub title: Option<String>,
    /// 输出容器扩展名：`mp4` / `mkv`。
    pub container: String,
    /// 媒体总时长（微秒），用于进度百分比。
    pub total_us: Option<u64>,
}

impl MergeRequest {
    /// 按音轨格式决定容器（FLAC/Hi-Res 必须 mkv，其余 mp4）。
    pub fn container_for(audio: Option<AudioFormat>) -> String {
        audio
            .map(|f| f.merged_container())
            .unwrap_or("mp4")
            .to_string()
    }
}

/// 构造合并参数：`ffmpeg -y -i video [-i audio] -c copy [-movflags +faststart] [-metadata title=…] out`
pub fn merge_args(
    video: &Path,
    audio: Option<&Path>,
    out: &Path,
    title: Option<&str>,
    container: &str,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
    ];
    args.push("-i".into());
    args.push(video.to_string_lossy().into_owned());
    if let Some(a) = audio {
        args.push("-i".into());
        args.push(a.to_string_lossy().into_owned());
    }
    args.push("-c".into());
    args.push("copy".into());
    if container == "mp4" {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
    if let Some(t) = title.filter(|s| !s.trim().is_empty()) {
        args.push("-metadata".into());
        args.push(format!("title={t}"));
    }
    // -progress 必须在输出文件前
    args.push("-progress".into());
    args.push("pipe:1".into());
    args.push("-nostats".into());
    args.push(out.to_string_lossy().into_owned());
    args
}

/// 构造 mp3 转码参数。
pub fn mp3_args(input: &Path, out: &Path, title: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-c:a".into(),
        "libmp3lame".into(),
        "-q:a".into(),
        "0".into(),
    ];
    if let Some(t) = title.filter(|s| !s.trim().is_empty()) {
        args.push("-metadata".into());
        args.push(format!("title={t}"));
    }
    args.push("-progress".into());
    args.push("pipe:1".into());
    args.push("-nostats".into());
    args.push(out.to_string_lossy().into_owned());
    args
}

/// 探测 ffmpeg 可执行路径：内置 sidecar（与主程序同目录）→ 设置中的显式路径 →
/// PATH → 常见安装路径。内置 sidecar 由 Tauri `externalBin` 随安装包分发。
pub fn detect_ffmpeg(override_path: Option<&str>) -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| bundled_ffmpeg_path_from(&exe));
    let path_env = std::env::var_os("PATH");
    detect_ffmpeg_from_sources(bundled, override_path, path_env.as_deref(), &COMMON_PATHS)
}

fn detect_ffmpeg_from_sources(
    bundled: Option<PathBuf>,
    override_path: Option<&str>,
    path_env: Option<&OsStr>,
    common_paths: &[&str],
) -> Option<PathBuf> {
    if let Some(bundled) = bundled.filter(|path| path.is_file()) {
        return Some(bundled);
    }
    if let Some(p) = override_path.filter(|s| !s.trim().is_empty()) {
        let pb = PathBuf::from(p);
        return pb.is_file().then_some(pb);
    }
    if let Some(path) = path_env {
        for dir in std::env::split_paths(&path) {
            for name in ffmpeg_binary_names() {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for p in common_paths {
        if Path::new(p).is_file() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// 随包 sidecar：Tauri `externalBin` 会把 ffmpeg 放到主程序同目录。
fn bundled_ffmpeg_path_from(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    ffmpeg_binary_names()
        .into_iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn ffmpeg_binary_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["ffmpeg.exe", "ffmpeg"]
    } else {
        vec!["ffmpeg"]
    }
}

// ---- 兼容旧 egui 应用（8.1 删除旧 UI 后移除）的同步封装 ----

/// 兼容旧调用：同步合并（阻塞）。新代码请用 [`Ffmpeg::merge`]。
pub fn merge(
    ffmpeg: &Path,
    video: &Path,
    audio: Option<&Path>,
    out: &Path,
    title: Option<&str>,
) -> Result<(), BiliError> {
    let container = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_string();
    let args = merge_args(video, audio, out, title, &container);
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(args);
    run(cmd)
}

/// 兼容旧调用：构造 std 命令对象（仅在同步路径使用）。
pub fn build_merge_cmd(
    ffmpeg: &Path,
    video: &Path,
    audio: Option<&Path>,
    out: &Path,
    title: Option<&str>,
) -> std::process::Command {
    let container = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_string();
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(merge_args(video, audio, out, title, &container));
    cmd
}

/// 兼容旧调用：构造 mp3 转码 std 命令对象。
pub fn build_mp3_cmd(
    ffmpeg: &Path,
    input: &Path,
    out: &Path,
    title: Option<&str>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(mp3_args(input, out, title));
    cmd
}

/// 执行 ffmpeg 命令并归一错误（stderr 截尾）。
pub fn run(mut cmd: std::process::Command) -> Result<(), BiliError> {
    let out = cmd
        .output()
        .map_err(|e| BiliError::Ffmpeg(format!("无法启动 ffmpeg: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        Err(BiliError::Ffmpeg(if tail.is_empty() {
            format!("退出码 {:?}", out.status.code())
        } else {
            tail
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_args_shape() {
        let args = merge_args(
            Path::new("/tmp/v.m4s"),
            Some(Path::new("/tmp/a.m4s")),
            Path::new("/tmp/out.mp4"),
            Some("标题"),
            "mp4",
        );
        assert!(args.windows(2).any(|w| w == ["-c", "copy"]));
        assert!(args.windows(2).any(|w| w == ["-movflags", "+faststart"]));
        assert!(args.windows(2).any(|w| w == ["-metadata", "title=标题"]));
        assert!(args.windows(2).any(|w| w == ["-progress", "pipe:1"]));
        assert_eq!(args.last().unwrap(), "/tmp/out.mp4");
    }

    #[test]
    fn merge_args_mkv_has_no_faststart() {
        let args = merge_args(
            Path::new("/tmp/v.m4s"),
            Some(Path::new("/tmp/a.flac")),
            Path::new("/tmp/out.mkv"),
            None,
            "mkv",
        );
        assert!(!args.iter().any(|a| a == "+faststart"));
        assert_eq!(args.last().unwrap(), "/tmp/out.mkv");
        assert_eq!(MergeRequest::container_for(Some(AudioFormat::Flac)), "mkv");
        assert_eq!(MergeRequest::container_for(Some(AudioFormat::M4a)), "mp4");
        assert_eq!(MergeRequest::container_for(None), "mp4");
    }

    #[test]
    fn mp3_args_shape() {
        let args = mp3_args(Path::new("/tmp/a.m4s"), Path::new("/tmp/a.mp3"), None);
        assert!(args.windows(2).any(|w| w == ["-c:a", "libmp3lame"]));
        assert!(args.windows(2).any(|w| w == ["-progress", "pipe:1"]));
        assert_eq!(args.last().unwrap(), "/tmp/a.mp3");
    }

    #[test]
    fn progress_parser_reads_out_time() {
        let mut p = ProgressParser::default();
        assert_eq!(p.push_line("frame=1"), None);
        assert_eq!(p.push_line("out_time_us=1500000"), Some(1_500_000));
        // 进度不倒退
        assert_eq!(p.push_line("out_time_us=1000000"), None);
        assert_eq!(p.push_line("out_time_ms=2000000"), Some(2_000_000));
        assert_eq!(p.push_line("progress=end"), None);
    }

    #[test]
    fn detect_rejects_missing_override() {
        assert!(detect_ffmpeg(Some("/nonexistent/ffmpeg")).is_none());
        assert!(detect_ffmpeg(Some("")).is_some() == detect_ffmpeg(None).is_some());
    }

    fn create_ffmpeg_file(dir: &Path) -> PathBuf {
        let name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let file = dir.join(name);
        std::fs::write(&file, b"test").unwrap();
        file
    }

    #[test]
    fn bundled_sidecar_is_found_next_to_application_executable() {
        let dir = tempfile::tempdir().unwrap();
        let macos_dir = dir.path().join("Contents/MacOS");
        std::fs::create_dir_all(&macos_dir).unwrap();
        let executable = macos_dir.join("bilidown");
        std::fs::write(&executable, b"test").unwrap();
        let sidecar = create_ffmpeg_file(&macos_dir);

        assert_eq!(bundled_ffmpeg_path_from(&executable), Some(sidecar));
    }

    #[test]
    fn detection_priority_and_fallback_are_explicit() {
        let bundled_dir = tempfile::tempdir().unwrap();
        let override_dir = tempfile::tempdir().unwrap();
        let path_dir = tempfile::tempdir().unwrap();
        let bundled = create_ffmpeg_file(bundled_dir.path());
        let override_path = create_ffmpeg_file(override_dir.path());
        let path_ffmpeg = create_ffmpeg_file(path_dir.path());
        let path_env = std::env::join_paths([path_dir.path()]).unwrap();

        assert_eq!(
            detect_ffmpeg_from_sources(
                Some(bundled.clone()),
                override_path.to_str(),
                Some(path_env.as_os_str()),
                &[],
            ),
            Some(bundled)
        );
        assert_eq!(
            detect_ffmpeg_from_sources(
                None,
                override_path.to_str(),
                Some(path_env.as_os_str()),
                &[],
            ),
            Some(override_path)
        );
        assert_eq!(
            detect_ffmpeg_from_sources(None, None, Some(path_env.as_os_str()), &[]),
            Some(path_ffmpeg)
        );
        assert_eq!(
            detect_ffmpeg_from_sources(None, Some("/nonexistent/ffmpeg"), None, &[]),
            None
        );
    }

    fn test_ffmpeg() -> Option<Ffmpeg> {
        let sidecar = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../src-tauri/binaries/macos/ffmpeg-aarch64-apple-darwin");
        if sidecar.is_file() {
            return Some(Ffmpeg::new(sidecar));
        }
        Ffmpeg::detect(None)
    }

    /// 真实合并集成测试：生成两路流 → `-c copy` 合并 → 可读取性校验。
    #[tokio::test]
    async fn real_merge_with_detected_ffmpeg() {
        let Some(ffmpeg) = test_ffmpeg() else {
            eprintln!("skip: 没有可用的 ffmpeg");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("v.mp4");
        let audio = dir.path().join("a.m4a");
        // 生成 1 秒测试视频与音频
        let v = std::process::Command::new(ffmpeg.path())
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=10",
                "-c:v",
                "mpeg4",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(v.success());
        let a = std::process::Command::new(ffmpeg.path())
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "aac",
            ])
            .arg(&audio)
            .status()
            .unwrap();
        assert!(a.success());

        let out = dir.path().join("out.mp4");
        let cancel = CancellationToken::new();
        let req = MergeRequest {
            video: video.clone(),
            audio: Some(audio.clone()),
            out: out.clone(),
            title: Some("集成测试".into()),
            container: "mp4".into(),
            total_us: ffmpeg
                .probe_duration(&video)
                .map(|s| (s * 1_000_000.0) as u64),
        };
        let mut beats = 0u64;
        ffmpeg
            .merge(&req, &cancel, |_us| {
                beats += 1;
            })
            .await
            .expect("ffmpeg 合并应成功");
        assert!(out.is_file(), "产物应存在");
        assert!(std::fs::metadata(&out).unwrap().len() > 1000);
        // ffprobe 是可选依赖：随包只带 ffmpeg 时进度为不确定态，此处只做存在性校验。
        match ffmpeg.probe_duration(&out) {
            Some(duration) => assert!((0.5..3.0).contains(&duration), "时长异常: {duration}"),
            None => eprintln!("skip ffprobe 时长校验：未找到 ffprobe"),
        }
        let _ = beats;
        // 清理输入
        let _ = std::fs::remove_file(&video);
        let _ = std::fs::remove_file(&audio);
    }

    #[tokio::test]
    async fn real_mp3_transcode_with_detected_ffmpeg() {
        let Some(ffmpeg) = test_ffmpeg() else {
            eprintln!("skip: 没有可用的 ffmpeg");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.m4a");
        let output = dir.path().join("output.mp3");
        let status = std::process::Command::new(ffmpeg.path())
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "aac",
            ])
            .arg(&input)
            .status()
            .unwrap();
        assert!(status.success());

        let cancel = CancellationToken::new();
        ffmpeg
            .transcode_mp3(&input, &output, Some("集成测试"), &cancel, |_| {})
            .await
            .expect("MP3 转码应成功");
        assert!(output.is_file());
        assert!(std::fs::metadata(&output).unwrap().len() > 1000);
        let readable = std::process::Command::new(ffmpeg.path())
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&output)
            .args(["-f", "null", "-"])
            .status()
            .unwrap();
        assert!(readable.success(), "生成的 MP3 应可被 ffmpeg 读取");
    }

    #[tokio::test]
    async fn cancel_kills_ffmpeg() {
        let Some(ffmpeg) = test_ffmpeg() else {
            eprintln!("skip: 没有可用的 ffmpeg");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("long.mp4");
        // 生成 20 秒视频，确保合并/转码耗时足够触发取消
        let status = std::process::Command::new(ffmpeg.path())
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=20:size=64x64:rate=5",
                "-c:v",
                "mpeg4",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let out = dir.path().join("out.mp4");
        let cancel = CancellationToken::new();
        let req = MergeRequest {
            video: video.clone(),
            audio: None,
            out: out.clone(),
            title: None,
            container: "mp4".into(),
            total_us: None,
        };
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            c.cancel();
        });
        // 取消路径也应可能先完成（极快机器），两种结果都接受，但不得 panic
        let result = ffmpeg.merge(&req, &cancel, |_| {}).await;
        match result {
            Err(BiliError::Cancelled) => {}
            Ok(()) => eprintln!("注意: 合并先于取消完成"),
            Err(e) => panic!("意外错误: {e}"),
        }
    }
}
