//! 应用内更新：GitHub Releases 检查 + DMG 下载（手动安装，不静默更新）。
//!
//! 设计约束（change `add-download-features` D5 / specs/app-update）：
//! - 只选非 draft / 非 prerelease 的最高 semver 版本；
//! - 资产固定命名：`bilidown_<version>_aarch64.dmg` + `SHA256SUMS.txt`；
//! - 下载到系统「下载」目录：`.part` → SHA256 校验 → 原子 rename；
//! - 事件 `app-update://progress`，payload 即 [`AppUpdateSnapshot`]；
//! - 安全：URL host 白名单、大小上限、拒绝符号链接、SHA256 不匹配不标记完成；
//! - 不自动安装：完成后只提供「打开安装包 / 在 Finder 中显示」。
//!
//! 行为参考 ASRbox 的更新体验（`ASRbox`），代码为本项目独立实现。

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

/// GitHub 仓库（当前为私密仓库；公开后 Releases API 可直接匿名访问）。
pub const REPO: &str = "Goldloli/bilidown";
pub const RELEASES_API: &str =
    "https://api.github.com/repos/Goldloli/bilidown/releases?per_page=30";
pub const RELEASES_PAGE: &str = "https://github.com/Goldloli/bilidown/releases";
/// 进度/状态事件名（payload = [`AppUpdateSnapshot`]）。
pub const UPDATE_PROGRESS_EVENT: &str = "app-update://progress";

const USER_AGENT: &str = "bilidown-desktop-updater";
const CHECKSUMS_ASSET: &str = "SHA256SUMS.txt";
const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(250);
const MAX_NOTES_CHARS: usize = 600;

/// 更新状态（事件 payload / `get_app_update_state` 返回值）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateSnapshot {
    /// idle / checking / available / downloading / verifying / completed / error / cancelled
    pub status: String,
    pub version: Option<String>,
    pub filename: Option<String>,
    pub path: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    /// 0–100 百分比（与 ASRbox 行为一致；前端可直接用于进度条）
    pub progress: Option<f64>,
    pub bytes_per_second: Option<f64>,
    pub eta_seconds: Option<u64>,
    pub error: Option<String>,
}

impl Default for AppUpdateSnapshot {
    fn default() -> Self {
        Self {
            status: "idle".into(),
            version: None,
            filename: None,
            path: None,
            downloaded_bytes: 0,
            total_bytes: None,
            progress: None,
            bytes_per_second: None,
            eta_seconds: None,
            error: None,
        }
    }
}

/// 版本信息（关于页：版本 / 目标平台 / 安装包类型）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVersionInfo {
    pub version: String,
    pub target: String,
    pub installer_kind: Option<String>,
}

/// 远端 Release 摘要。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateRelease {
    pub version: String,
    pub tag_name: String,
    pub name: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub html_url: String,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
}

/// `check_app_update` 返回值。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateCheck {
    pub current_version: String,
    pub update_available: bool,
    pub checked_at_ms: u64,
    pub release: Option<AppUpdateRelease>,
    pub releases_url: String,
}

/// 托管在应用内的更新状态（`tauri::Manager::manage`）。
pub struct AppUpdateState {
    cancel: AtomicBool,
    snapshot: Mutex<AppUpdateSnapshot>,
    /// 最近一次检查解析出的候选版本（start_app_update_download 使用，避免重复请求 API）。
    last_check: Mutex<Option<ResolvedRelease>>,
}

impl Default for AppUpdateState {
    fn default() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            snapshot: Mutex::new(AppUpdateSnapshot::default()),
            last_check: Mutex::new(None),
        }
    }
}

impl AppUpdateState {
    pub fn snapshot(&self) -> Result<AppUpdateSnapshot, String> {
        self.snapshot
            .lock()
            .map(|guard| guard.clone())
            .map_err(|error| format!("读取更新状态失败: {error}"))
    }
}

// ---------------------------------------------------------------------------
// GitHub 数据结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

/// 已解析的候选版本（版本 + Release + 安装包/校验文件资产）。
#[derive(Debug, Clone)]
struct ResolvedRelease {
    version: Version,
    release: GitHubRelease,
    installer: GitHubAsset,
    checksums: GitHubAsset,
}

/// 下载任务内部错误（取消与普通失败区分）。
enum UpdateError {
    Cancelled,
    Failed(String),
}

impl From<String> for UpdateError {
    fn from(message: String) -> Self {
        UpdateError::Failed(message)
    }
}

// ---------------------------------------------------------------------------
// 平台与资产命名
// ---------------------------------------------------------------------------

pub fn target_label() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macOS Apple Silicon"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macOS Intel"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Desktop"
    }
}

pub fn installer_kind() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("dmg")
    } else if cfg!(target_os = "windows") {
        Some("nsis")
    } else {
        None
    }
}

/// 正式版安装包文件名：macOS 为 `bilidown_<version>_<arch>.dmg`，Windows 为 NSIS
/// `bilidown_<version>_x64-setup.exe`（与 Tauri 打包产物名一致）。
fn expected_asset_name(version: &Version) -> String {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        format!("bilidown_{version}_aarch64.dmg")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        format!("bilidown_{version}_x64.dmg")
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        format!("bilidown_{version}_x64-setup.exe")
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        format!("bilidown_{version}_arm64-setup.exe")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        format!("bilidown_{version}_x86_64.AppImage")
    }
}

/// 资产名是否合法：无路径分隔、只允许 ASCII 安全字符、且与期望版本名一致。
fn is_expected_installer(name: &str, version: &Version) -> bool {
    !name.is_empty()
        && name == expected_asset_name(version)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn resolve_assets(
    release: &GitHubRelease,
    version: &Version,
) -> Result<(GitHubAsset, GitHubAsset), String> {
    let expected = expected_asset_name(version);
    let installer = release
        .assets
        .iter()
        .find(|asset| asset.name == expected)
        .cloned()
        .ok_or_else(|| {
            format!(
                "该版本没有适用于当前平台（{}）的安装包（期望 {expected}）。",
                target_label()
            )
        })?;
    if !is_expected_installer(&installer.name, version) {
        return Err(format!("安装包文件名不合法: {}", installer.name));
    }
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == CHECKSUMS_ASSET)
        .cloned()
        .ok_or_else(|| format!("该版本缺少 {CHECKSUMS_ASSET}，无法校验安装包。"))?;
    Ok((installer, checksums))
}

// ---------------------------------------------------------------------------
// semver / Release 选择
// ---------------------------------------------------------------------------

fn parse_release_version(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches('v')).ok()
}

/// 从 Release 列表选出「最高 semver 的正式版」，并解析安装包资产。
fn select_release(releases: &[GitHubRelease]) -> Result<Option<ResolvedRelease>, String> {
    let candidate = releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = parse_release_version(&release.tag_name)?;
            if !version.pre.is_empty() {
                return None;
            }
            Some((version, release.clone()))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right));
    let Some((version, release)) = candidate else {
        return Ok(None);
    };
    let (installer, checksums) = resolve_assets(&release, &version)?;
    Ok(Some(ResolvedRelease {
        version,
        release,
        installer,
        checksums,
    }))
}

fn public_release(resolved: &ResolvedRelease) -> AppUpdateRelease {
    AppUpdateRelease {
        version: resolved.version.to_string(),
        tag_name: resolved.release.tag_name.clone(),
        name: resolved
            .release
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| resolved.release.tag_name.clone()),
        notes: plain_text_summary(resolved.release.body.as_deref().unwrap_or("")),
        published_at: resolved.release.published_at.clone(),
        html_url: trusted_release_page(&resolved.release.html_url),
        asset_name: Some(resolved.installer.name.clone()),
        asset_size: Some(resolved.installer.size),
    }
}

fn trusted_release_page(value: &str) -> String {
    let Ok(url) = reqwest::Url::parse(value) else {
        return RELEASES_PAGE.to_string();
    };
    if url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.path().starts_with(&format!("/{REPO}/releases"))
    {
        value.to_string()
    } else {
        RELEASES_PAGE.to_string()
    }
}

fn plain_text_summary(value: &str) -> String {
    let joined = value
        .lines()
        .map(|line| line.trim().trim_start_matches(['#', '>', '-', '*']).trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.chars().count() > MAX_NOTES_CHARS {
        joined.chars().take(MAX_NOTES_CHARS - 1).collect::<String>() + "…"
    } else {
        joined
    }
}

/// 当前版本是否落后于远端版本（纯函数，单测覆盖）。
fn is_newer(current: &Version, remote: &Version) -> bool {
    remote > current
}

// ---------------------------------------------------------------------------
// URL / 安全校验
// ---------------------------------------------------------------------------

fn is_trusted_host(host: &str) -> bool {
    host == "api.github.com"
        || host == "github.com"
        || host == "objects.githubusercontent.com"
        // GitHub 新资产的 302 目标域名（release-assets.githubusercontent.com 等）
        || host.ends_with(".githubusercontent.com")
}

fn validate_asset_url(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value)
        .map_err(|error| format!("GitHub Release 资产 URL 非法: {error}"))?;
    if url.scheme() != "https" {
        return Err("安装包下载地址必须使用 https。".into());
    }
    if url.host_str() != Some("github.com") {
        return Err("安装包下载地址不在 GitHub 官方域名下。".into());
    }
    let expected_prefix = format!("/{REPO}/releases/download/");
    if !url.path().starts_with(&expected_prefix) {
        return Err(format!("安装包下载地址不属于 {REPO} 的正式 Release。"));
    }
    Ok(())
}

fn validate_response_url(url: &reqwest::Url) -> Result<(), String> {
    if url.scheme() == "https"
        && let Some(host) = url.host_str()
        && is_trusted_host(host)
    {
        return Ok(());
    }
    Err("下载请求被重定向到非可信域名，已拒绝。".into())
}

fn github_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.stop();
            }
            match attempt.url().host_str() {
                Some(host) if is_trusted_host(host) => attempt.follow(),
                _ => attempt.stop(),
            }
        }))
        .build()
        .map_err(|error| format!("创建 GitHub 客户端失败: {error}"))
}

// ---------------------------------------------------------------------------
// GitHub API
// ---------------------------------------------------------------------------

async fn fetch_releases() -> Result<Vec<GitHubRelease>, String> {
    let response = github_client()?
        .get(RELEASES_API)
        .timeout(API_TIMEOUT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|error| format!("检查更新失败（网络）: {error}"))?;
    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(
            "GitHub API 触发限流（HTTP 403/429），请稍后重试或在 Releases 页面手动下载。".into(),
        );
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "未找到 GitHub 仓库 {REPO}（HTTP 404），请确认仓库已公开。"
        ));
    }
    if !status.is_success() {
        return Err(format!("检查更新失败：GitHub API 返回 HTTP {status}。"));
    }
    validate_response_url(response.url())?;
    if response
        .content_length()
        .is_some_and(|length| length > 8 * 1024 * 1024)
    {
        return Err("GitHub Releases 响应体过大，已拒绝。".into());
    }
    response
        .json::<Vec<GitHubRelease>>()
        .await
        .map_err(|error| format!("解析 GitHub Releases 失败: {error}"))
}

async fn fetch_expected_checksum(asset: &GitHubAsset) -> Result<String, String> {
    validate_asset_url(&asset.browser_download_url)?;
    let response = github_client()?
        .get(&asset.browser_download_url)
        .timeout(API_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("下载 {CHECKSUMS_ASSET} 失败: {error}"))?
        .error_for_status()
        .map_err(|error| format!("下载 {CHECKSUMS_ASSET} 失败: {error}"))?;
    validate_response_url(response.url())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CHECKSUM_BYTES as u64)
    {
        return Err(format!("{CHECKSUMS_ASSET} 体积异常（超过 1MB），已拒绝。"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取 {CHECKSUMS_ASSET} 失败: {error}"))?;
    if bytes.len() > MAX_CHECKSUM_BYTES {
        return Err(format!("{CHECKSUMS_ASSET} 体积异常（超过 1MB），已拒绝。"));
    }
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| format!("{CHECKSUMS_ASSET} 不是 UTF-8 文本。"))?;
    Ok(text)
}

/// 从 SHA256SUMS.txt 精确匹配文件名，返回小写十六进制摘要。
fn parse_checksum(contents: &str, filename: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        let rest = parts.next();
        if rest.is_none()
            && name == filename
            && digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            Some(digest.to_ascii_lowercase())
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// 状态/事件
// ---------------------------------------------------------------------------

fn mutate_snapshot(
    app: &AppHandle,
    mutate: impl FnOnce(&mut AppUpdateSnapshot),
) -> Result<AppUpdateSnapshot, String> {
    let state = app.state::<AppUpdateState>();
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .map_err(|error| format!("更新状态锁失败: {error}"))?;
        mutate(&mut guard);
        guard.clone()
    };
    app.emit(UPDATE_PROGRESS_EVENT, snapshot.clone())
        .map_err(|error| format!("发送更新进度事件失败: {error}"))?;
    Ok(snapshot)
}

fn cancel_requested(app: &AppHandle) -> bool {
    app.state::<AppUpdateState>().cancel.load(Ordering::SeqCst)
}

fn ensure_not_cancelled(app: &AppHandle) -> Result<(), UpdateError> {
    if cancel_requested(app) {
        Err(UpdateError::Cancelled)
    } else {
        Ok(())
    }
}

fn emit_progress(
    app: &AppHandle,
    downloaded: u64,
    total: Option<u64>,
    elapsed: Duration,
) -> Result<(), String> {
    let seconds = elapsed.as_secs_f64();
    let bytes_per_second = (seconds > 0.0).then_some(downloaded as f64 / seconds);
    let progress = total
        .filter(|value| *value > 0)
        .map(|value| (downloaded as f64 / value as f64 * 100.0).clamp(0.0, 100.0));
    let eta_seconds = match (total, bytes_per_second) {
        (Some(total), Some(speed)) if speed > 0.0 && total > downloaded => {
            Some(((total - downloaded) as f64 / speed).ceil() as u64)
        }
        (Some(_), Some(_)) => Some(0),
        _ => None,
    };
    mutate_snapshot(app, |snapshot| {
        snapshot.status = "downloading".into();
        snapshot.downloaded_bytes = downloaded;
        snapshot.total_bytes = total;
        snapshot.progress = progress;
        snapshot.bytes_per_second = bytes_per_second;
        snapshot.eta_seconds = eta_seconds;
    })?;
    Ok(())
}

fn is_active_download(snapshot: &AppUpdateSnapshot) -> bool {
    matches!(snapshot.status.as_str(), "downloading" | "verifying")
}

// ---------------------------------------------------------------------------
// 下载实现
// ---------------------------------------------------------------------------

fn downloads_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(dir) = app.path().download_dir() {
        return Ok(dir);
    }
    dirs::download_dir().ok_or_else(|| "无法定位系统「下载」目录。".into())
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("拒绝符号链接路径: {}", path.display()))
        }
        Ok(metadata) if metadata.file_type().is_dir() => {
            Err(format!("期望文件但发现目录: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("检查路径失败 {}: {error}", path.display())),
    }
}

fn regular_file_without_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

async fn remove_file_if_present(path: &Path, description: &str) -> Result<(), String> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_dir() => {
            Err(format!("{description} 路径是目录: {}", path.display()))
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{description} 路径是符号链接: {}", path.display()))
        }
        Ok(_) => tokio::fs::remove_file(path)
            .await
            .map_err(|error| format!("删除{description}失败: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("检查{description}失败: {error}")),
    }
}

async fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("读取已有安装包失败: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = tokio::io::AsyncReadExt::read(&mut file, &mut buffer)
            .await
            .map_err(|error| format!("计算已有安装包 SHA256 失败: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// 执行下载：进入前 snapshot 已置为 downloading；结束后自行写 completed/error/cancelled。
async fn run_download(app: AppHandle, resolved: ResolvedRelease) -> Result<(), UpdateError> {
    let result = download_inner(&app, &resolved).await;
    match result {
        Ok(()) => Ok(()),
        Err(UpdateError::Cancelled) => {
            let _ = mutate_snapshot(&app, |snapshot| {
                snapshot.status = "cancelled".into();
                snapshot.progress = None;
                snapshot.bytes_per_second = None;
                snapshot.eta_seconds = None;
                snapshot.error = None;
            });
            Err(UpdateError::Cancelled)
        }
        Err(UpdateError::Failed(message)) => {
            let _ = mutate_snapshot(&app, |snapshot| {
                snapshot.status = "error".into();
                snapshot.progress = None;
                snapshot.bytes_per_second = None;
                snapshot.eta_seconds = None;
                snapshot.error = Some(message.clone());
            });
            Err(UpdateError::Failed(message))
        }
    }
}

async fn download_inner(app: &AppHandle, resolved: &ResolvedRelease) -> Result<(), UpdateError> {
    validate_asset_url(&resolved.installer.browser_download_url)?;
    validate_asset_url(&resolved.checksums.browser_download_url)?;
    if resolved.installer.size == 0 || resolved.installer.size > MAX_INSTALLER_BYTES {
        return Err(UpdateError::Failed("安装包体积异常，已拒绝下载。".into()));
    }
    ensure_not_cancelled(app)?;

    let checksum_text = fetch_expected_checksum(&resolved.checksums).await?;
    let expected_checksum =
        parse_checksum(&checksum_text, &resolved.installer.name).ok_or_else(|| {
            UpdateError::Failed(format!(
                "{CHECKSUMS_ASSET} 中没有 {} 的 SHA256 记录。",
                resolved.installer.name
            ))
        })?;
    ensure_not_cancelled(app)?;

    let download_dir = downloads_dir(app)?;
    tokio::fs::create_dir_all(&download_dir)
        .await
        .map_err(|error| UpdateError::Failed(format!("创建下载目录失败: {error}")))?;
    if std::fs::symlink_metadata(&download_dir)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(UpdateError::Failed("下载目录是符号链接，已拒绝。".into()));
    }
    let final_path = download_dir.join(&resolved.installer.name);
    let part_path = download_dir.join(format!("{}.part", resolved.installer.name));
    reject_symlink(&final_path)?;
    remove_file_if_present(&part_path, "上次未完成的临时文件").await?;

    // 已存在且 SHA256 匹配：直接复用，不重复下载
    if regular_file_without_symlink(&final_path)
        && hash_file(&final_path).await? == expected_checksum
    {
        let size = tokio::fs::metadata(&final_path)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or(resolved.installer.size);
        mutate_snapshot(app, |snapshot| {
            snapshot.status = "completed".into();
            snapshot.version = Some(resolved.version.to_string());
            snapshot.filename = Some(resolved.installer.name.clone());
            snapshot.path = Some(final_path.to_string_lossy().to_string());
            snapshot.downloaded_bytes = size;
            snapshot.total_bytes = Some(size);
            snapshot.progress = Some(100.0);
            snapshot.bytes_per_second = None;
            snapshot.eta_seconds = Some(0);
            snapshot.error = None;
        })?;
        return Ok(());
    }

    mutate_snapshot(app, |snapshot| {
        snapshot.status = "downloading".into();
        snapshot.version = Some(resolved.version.to_string());
        snapshot.filename = Some(resolved.installer.name.clone());
        snapshot.path = None;
        snapshot.downloaded_bytes = 0;
        snapshot.total_bytes = Some(resolved.installer.size);
        snapshot.progress = Some(0.0);
        snapshot.bytes_per_second = None;
        snapshot.eta_seconds = None;
        snapshot.error = None;
    })?;

    let client = github_client()?;
    let mut response = match client
        .get(&resolved.installer.browser_download_url)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return Err(UpdateError::Failed(format!("下载安装包失败: {error}")));
        }
    };
    validate_response_url(response.url())?;
    if let Some(length) = response.content_length()
        && length != resolved.installer.size
    {
        return Err(UpdateError::Failed(
            "安装包下载响应大小与 Release 不一致。".into(),
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part_path)
        .await
        .map_err(|error| UpdateError::Failed(format!("创建临时文件失败: {error}")))?;
    let mut hasher = Sha256::new();
    let started = Instant::now();
    let mut last_emit = Instant::now() - PROGRESS_EMIT_INTERVAL;
    let mut downloaded: u64 = 0;

    loop {
        ensure_not_cancelled(app)?;
        let chunk = match tokio::time::timeout(RESPONSE_IDLE_TIMEOUT, response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                return Err(UpdateError::Failed(format!("下载安装包中断: {error}")));
            }
            Err(_) => {
                return Err(UpdateError::Failed(
                    "安装包下载超过 60 秒无数据，已中止。".into(),
                ));
            }
        };
        file.write_all(&chunk)
            .await
            .map_err(|error| UpdateError::Failed(format!("写入临时文件失败: {error}")))?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        if downloaded > resolved.installer.size {
            return Err(UpdateError::Failed(
                "下载数据超过 Release 声明的大小，已中止。".into(),
            ));
        }
        if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            emit_progress(
                app,
                downloaded,
                Some(resolved.installer.size),
                started.elapsed(),
            )?;
            last_emit = Instant::now();
        }
    }
    ensure_not_cancelled(app)?;
    if downloaded != resolved.installer.size {
        return Err(UpdateError::Failed(
            "安装包下载大小与 Release 声明不一致。".into(),
        ));
    }
    file.flush()
        .await
        .map_err(|error| UpdateError::Failed(format!("刷新临时文件失败: {error}")))?;
    drop(file);
    emit_progress(
        app,
        downloaded,
        Some(resolved.installer.size),
        started.elapsed(),
    )?;

    mutate_snapshot(app, |snapshot| {
        snapshot.status = "verifying".into();
        snapshot.bytes_per_second = None;
        snapshot.eta_seconds = None;
    })?;
    ensure_not_cancelled(app)?;
    let actual_checksum: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual_checksum != expected_checksum {
        let _ = remove_file_if_present(&part_path, "校验失败的临时文件").await;
        return Err(UpdateError::Failed(format!(
            "SHA256 校验失败：期望 {expected_checksum}，实际 {actual_checksum}。安装包已删除。"
        )));
    }

    reject_symlink(&final_path)?;
    remove_file_if_present(&final_path, "旧的安装包").await?;
    tokio::fs::rename(&part_path, &final_path)
        .await
        .map_err(|error| UpdateError::Failed(format!("保存安装包失败: {error}")))?;
    let size = tokio::fs::metadata(&final_path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(resolved.installer.size);
    mutate_snapshot(app, |snapshot| {
        snapshot.status = "completed".into();
        snapshot.version = Some(resolved.version.to_string());
        snapshot.filename = Some(resolved.installer.name.clone());
        snapshot.path = Some(final_path.to_string_lossy().to_string());
        snapshot.downloaded_bytes = size;
        snapshot.total_bytes = Some(size);
        snapshot.progress = Some(100.0);
        snapshot.bytes_per_second = None;
        snapshot.eta_seconds = Some(0);
        snapshot.error = None;
    })?;
    tracing::info!(target: "shell::update", version = %resolved.version, path = %final_path.display(), "更新包下载完成并通过 SHA256 校验");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

pub fn get_app_update_state(
    state: tauri::State<'_, AppUpdateState>,
) -> Result<AppUpdateSnapshot, String> {
    state.snapshot()
}

pub fn get_app_version(app: AppHandle) -> AppVersionInfo {
    AppVersionInfo {
        version: app.package_info().version.to_string(),
        target: target_label().to_string(),
        installer_kind: installer_kind().map(str::to_string),
    }
}

pub async fn check_app_update(
    app: AppHandle,
    _state: tauri::State<'_, AppUpdateState>,
) -> Result<AppUpdateCheck, String> {
    check_in_background(app).await
}

/// 检查更新（不依赖 `tauri::State` 参数；供启动后台检查与命令复用）。
pub async fn check_in_background(app: AppHandle) -> Result<AppUpdateCheck, String> {
    let state = app.state::<AppUpdateState>();
    {
        let snapshot = state.snapshot()?;
        if is_active_download(&snapshot) {
            return Err("正在下载更新包，请先完成或取消下载后再检查更新。".into());
        }
    }
    mutate_snapshot(&app, |snapshot| {
        snapshot.status = "checking".into();
        snapshot.error = None;
        snapshot.bytes_per_second = None;
        snapshot.eta_seconds = None;
    })?;

    let result = async {
        let releases = fetch_releases().await?;
        let selected = select_release(&releases)?
            .ok_or_else(|| format!("仓库 {REPO} 还没有正式 Release。"))?;
        let current = app.package_info().version.clone();
        let update_available = is_newer(&current, &selected.version);
        let check = AppUpdateCheck {
            current_version: current.to_string(),
            update_available,
            checked_at_ms: now_ms(),
            release: Some(public_release(&selected)),
            releases_url: RELEASES_PAGE.to_string(),
        };
        Ok::<_, String>((check, selected))
    }
    .await;

    match result {
        Ok((check, selected)) => {
            if let Ok(mut guard) = state.last_check.lock() {
                *guard = Some(selected.clone());
            }
            mutate_snapshot(&app, |snapshot| {
                if check.update_available {
                    snapshot.status = "available".into();
                    snapshot.version = Some(selected.version.to_string());
                    snapshot.filename = Some(selected.installer.name.clone());
                } else {
                    snapshot.status = "idle".into();
                    snapshot.version = None;
                    snapshot.filename = None;
                }
                snapshot.error = None;
            })?;
            Ok(check)
        }
        Err(message) => {
            let _ = mutate_snapshot(&app, |snapshot| {
                snapshot.status = "error".into();
                snapshot.error = Some(message.clone());
            });
            Err(message)
        }
    }
}

pub fn start_app_update_download(
    app: AppHandle,
    state: tauri::State<'_, AppUpdateState>,
    version: String,
) -> Result<AppUpdateSnapshot, String> {
    let requested = parse_release_version(&version)
        .ok_or_else(|| format!("要下载的版本号不合法: {version}"))?;
    let current = app.package_info().version.clone();
    {
        let snapshot = state.snapshot()?;
        if is_active_download(&snapshot) {
            return Err("已有更新包正在下载。".into());
        }
    }
    let cached = state
        .last_check
        .lock()
        .map_err(|error| format!("读取更新缓存失败: {error}"))?
        .clone()
        .ok_or_else(|| "请先检查更新，再开始下载。".to_string())?;
    if cached.version != requested {
        return Err(format!(
            "要下载的版本 {} 不是最近检查到的最新正式版 {}，请重新检查更新。",
            requested, cached.version
        ));
    }
    if !is_newer(&current, &cached.version) {
        return Err("该版本不高于当前版本，无需下载。".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let initial = mutate_snapshot(&app, |snapshot| {
        *snapshot = AppUpdateSnapshot {
            status: "downloading".into(),
            version: Some(cached.version.to_string()),
            filename: Some(cached.installer.name.clone()),
            total_bytes: Some(cached.installer.size),
            progress: Some(0.0),
            ..AppUpdateSnapshot::default()
        };
    })?;

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_download(app_handle, cached).await;
        match result {
            Ok(()) | Err(UpdateError::Cancelled) => {}
            Err(UpdateError::Failed(message)) => {
                tracing::warn!(target: "shell::update", error = %message, "更新包下载失败");
            }
        }
    });
    Ok(initial)
}

pub fn cancel_app_update_download(
    app: AppHandle,
    state: tauri::State<'_, AppUpdateState>,
) -> Result<AppUpdateSnapshot, String> {
    let snapshot = state.snapshot()?;
    if !is_active_download(&snapshot) {
        return Ok(snapshot);
    }
    state.cancel.store(true, Ordering::SeqCst);
    mutate_snapshot(&app, |snapshot| {
        snapshot.status = "cancelled".into();
        snapshot.bytes_per_second = None;
        snapshot.eta_seconds = None;
        snapshot.error = None;
    })
}

pub fn open_downloaded_update(
    app: AppHandle,
    state: tauri::State<'_, AppUpdateState>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = completed_installer_path(&state)?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| format!("打开安装包失败: {error}"))
}

pub fn open_update_file_location(
    app: AppHandle,
    state: tauri::State<'_, AppUpdateState>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = completed_installer_path(&state)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|error| format!("{}安装包失败: {error}", crate::platform::REVEAL_LABEL))
}

pub fn open_about_link(app: AppHandle, link: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = about_link_url(&link).ok_or_else(|| format!("未知的关于页链接: {link}"))?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| format!("打开链接失败: {error}"))
}

/// Windows：先优雅停止任务并退出应用，再以静默更新模式启动 NSIS 安装器。
/// 安装器参数：`/S` 静默、`/UPDATE` 更新模式（不重复建快捷方式）、`/R` 完成后重启应用、
/// `/ARGS=--updated` 让新版本启动时提示「已更新」并跳过启动检查。
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    state: tauri::State<'_, AppUpdateState>,
) -> Result<(), String> {
    let path = completed_installer_path(&state)?;
    #[cfg(target_os = "windows")]
    {
        // 安装器启动失败时返回错误且应用保持运行，由前端提示；成功后再退出。
        crate::window::quit_and_install(&app, path).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, path);
        Err("当前平台请手动安装更新包（打开安装包后按提示操作）".into())
    }
}

fn completed_installer_path(state: &AppUpdateState) -> Result<PathBuf, String> {
    let snapshot = state.snapshot()?;
    if snapshot.status != "completed" {
        return Err("还没有已校验完成的更新包。".into());
    }
    let path = snapshot
        .path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "更新包路径缺失。".to_string())?;
    let version = snapshot
        .version
        .as_deref()
        .and_then(parse_release_version)
        .ok_or_else(|| "更新包版本信息缺失。".to_string())?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !is_expected_installer(filename, &version) {
        return Err("更新包文件名与版本不匹配，已拒绝打开。".into());
    }
    if !regular_file_without_symlink(&path) {
        return Err("更新包文件不存在或不是普通文件。".into());
    }
    Ok(path)
}

/// 关于页链接白名单（固定 key，不接受任意 URL）。
fn about_link_url(link: &str) -> Option<&'static str> {
    match link {
        "repository" => Some("https://github.com/Goldloli/bilidown"),
        "releases" => Some(RELEASES_PAGE),
        "issues" => Some("https://github.com/Goldloli/bilidown/issues"),
        "license" => Some("https://github.com/Goldloli/bilidown/blob/main/LICENSE"),
        "author" => Some("https://github.com/Goldloli"),
        "acknowledgements" => {
            Some("https://github.com/Goldloli/bilidown/blob/main/ACKNOWLEDGEMENTS.md")
        }
        _ => None,
    }
}

/// 启动时检查更新的入口占位（实际调度在 `state::spawn_init`：core ready 后读取
/// `Settings.check_updates_on_startup`，为 true 时后台调用 [`check_in_background`]，
/// 仅更新状态/事件，不发通知、不自动下载；失败只 warn）。
pub fn startup_check_interface() {}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------
// 单测（不联网）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, url: &str, size: u64) -> GitHubAsset {
        GitHubAsset {
            name: name.into(),
            browser_download_url: url.into(),
            size,
        }
    }

    fn release(
        tag: &str,
        prerelease: bool,
        draft: bool,
        assets: Vec<GitHubAsset>,
    ) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.into(),
            name: Some(format!("Release {tag}")),
            body: Some("# 更新\n- 修复问题".into()),
            draft,
            prerelease,
            published_at: Some("2026-10-29T00:00:00Z".into()),
            html_url: format!("https://github.com/Goldloli/bilidown/releases/tag/{tag}"),
            assets,
        }
    }

    fn expected_name(version: &str) -> String {
        expected_asset_name(&Version::parse(version).unwrap())
    }

    fn good_assets(version: &str) -> Vec<GitHubAsset> {
        let name = expected_name(version);
        vec![
            asset(
                &name,
                &format!(
                    "https://github.com/Goldloli/bilidown/releases/download/v{version}/{name}"
                ),
                11_000_000,
            ),
            asset(
                "SHA256SUMS.txt",
                &format!(
                    "https://github.com/Goldloli/bilidown/releases/download/v{version}/SHA256SUMS.txt"
                ),
                100,
            ),
        ]
    }

    #[test]
    fn selects_highest_stable_release_and_ignores_drafts_prereleases() {
        let releases = vec![
            release("v2.2.0", true, false, good_assets("2.2.0")),
            release("v2.1.0", false, true, good_assets("2.1.0")),
            release("v2.0.0", false, false, good_assets("2.0.0")),
            release("not-a-version", false, false, vec![]),
        ];
        let selected = select_release(&releases).unwrap().unwrap();
        assert_eq!(selected.version, Version::parse("2.0.0").unwrap());
        assert_eq!(selected.installer.name, expected_name("2.0.0"));
    }

    #[test]
    fn no_stable_release_returns_none() {
        let releases = vec![
            release("v2.2.0", true, false, good_assets("2.2.0")),
            release("v2.1.0", false, true, good_assets("2.1.0")),
        ];
        assert!(select_release(&releases).unwrap().is_none());
    }

    #[test]
    fn release_without_matching_assets_is_rejected() {
        let name = expected_name("2.0.0");
        let url = format!("https://github.com/Goldloli/bilidown/releases/download/v2.0.0/{name}");
        let missing_checksum = vec![asset(&name, &url, 1)];
        let err = select_release(&[release("v2.0.0", false, false, missing_checksum)]).unwrap_err();
        assert!(err.contains("SHA256SUMS"));

        let wrong_name = expected_name("2.1.0");
        let wrong_url =
            format!("https://github.com/Goldloli/bilidown/releases/download/v2.0.0/{wrong_name}");
        let wrong_version_name = vec![
            asset(&wrong_name, &wrong_url, 1),
            asset(
                "SHA256SUMS.txt",
                "https://github.com/Goldloli/bilidown/releases/download/v2.0.0/SHA256SUMS.txt",
                1,
            ),
        ];
        let err =
            select_release(&[release("v2.0.0", false, false, wrong_version_name)]).unwrap_err();
        assert!(err.contains("没有适用于当前平台"));
    }

    #[test]
    fn installer_name_rejects_path_traversal_and_other_projects() {
        let version = Version::parse("2.0.0").unwrap();
        let valid = expected_name("2.0.0");
        assert!(is_expected_installer(&valid, &version));
        for bad in [
            format!("../{valid}"),
            expected_name("2.1.0"),
            "asrbox_2.0.0_aarch64.dmg".into(),
            format!("{valid}/evil"),
            String::new(),
        ] {
            assert!(
                !is_expected_installer(&bad, &version),
                "should reject {bad}"
            );
        }
    }

    #[test]
    fn checksum_parser_requires_exact_filename_and_hex_digest() {
        let digest = "a".repeat(64);
        let contents = format!(
            "{digest}  bilidown_2.0.0_aarch64.dmg\n{}  other.dmg\n",
            "b".repeat(64)
        );
        assert_eq!(
            parse_checksum(&contents, "bilidown_2.0.0_aarch64.dmg"),
            Some(digest.clone())
        );
        assert_eq!(
            parse_checksum(&contents, "bilidown_2.1.0_aarch64.dmg"),
            None
        );
        for bad in [
            format!("{digest}  ./bilidown_2.0.0_aarch64.dmg\n"),
            format!("{digest}  nested/bilidown_2.0.0_aarch64.dmg\n"),
            format!("{}  bilidown_2.0.0_aarch64.dmg\n", "a".repeat(63)),
            format!("{}  bilidown_2.0.0_aarch64.dmg\n", "g".repeat(64)),
            format!("{digest} *bilidown_2.0.0_aarch64.dmg extra\n"),
        ] {
            assert_eq!(parse_checksum(&bad, "bilidown_2.0.0_aarch64.dmg"), None);
        }
    }

    #[test]
    fn asset_urls_are_scoped_to_official_release_downloads() {
        assert!(validate_asset_url(
            "https://github.com/Goldloli/bilidown/releases/download/v2.0.0/bilidown_2.0.0_aarch64.dmg"
        )
        .is_ok());
        for bad in [
            "http://github.com/Goldloli/bilidown/releases/download/v2.0.0/bilidown_2.0.0_aarch64.dmg",
            "https://example.com/Goldloli/bilidown/releases/download/v2.0.0/bilidown_2.0.0_aarch64.dmg",
            "https://github.com/other/repo/releases/download/v2.0.0/bilidown_2.0.0_aarch64.dmg",
            "https://github.com/Goldloli/bilidown/archive/v2.0.0.zip",
            "not a url",
        ] {
            assert!(validate_asset_url(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn semver_comparison_only_flags_newer_remote() {
        let current = Version::parse("2.0.0").unwrap();
        assert!(is_newer(&current, &Version::parse("2.0.1").unwrap()));
        assert!(is_newer(&current, &Version::parse("2.1.0").unwrap()));
        assert!(is_newer(&current, &Version::parse("3.0.0").unwrap()));
        assert!(!is_newer(&current, &Version::parse("2.0.0").unwrap()));
        assert!(!is_newer(&current, &Version::parse("1.9.9").unwrap()));
    }

    #[test]
    fn only_stable_tag_versions_are_parsed() {
        assert_eq!(
            parse_release_version("v2.0.0"),
            Some(Version::parse("2.0.0").unwrap())
        );
        assert_eq!(
            parse_release_version("2.0.0"),
            Some(Version::parse("2.0.0").unwrap())
        );
        assert_eq!(parse_release_version("release-2.0.0"), None);
        assert_eq!(parse_release_version(""), None);
    }

    #[test]
    fn release_notes_are_plain_and_bounded() {
        let input = format!("# 标题\n- 条目\n{}", "x".repeat(700));
        let output = plain_text_summary(&input);
        assert!(output.starts_with("标题 条目"));
        assert!(output.chars().count() <= MAX_NOTES_CHARS);
        assert!(!output.contains('#'));
    }

    #[test]
    fn about_links_are_allowlisted() {
        for key in [
            "repository",
            "releases",
            "issues",
            "license",
            "author",
            "acknowledgements",
        ] {
            assert!(about_link_url(key).is_some(), "missing {key}");
        }
        assert_eq!(about_link_url("https://example.com"), None);
        assert_eq!(about_link_url("unknown"), None);
    }

    #[test]
    fn default_snapshot_is_idle() {
        let snapshot = AppUpdateSnapshot::default();
        assert_eq!(snapshot.status, "idle");
        assert_eq!(snapshot.downloaded_bytes, 0);
        assert!(snapshot.total_bytes.is_none());
        assert!(snapshot.error.is_none());
    }

    #[test]
    fn active_states_are_downloading_and_verifying() {
        let mut snapshot = AppUpdateSnapshot::default();
        assert!(!is_active_download(&snapshot));
        snapshot.status = "downloading".into();
        assert!(is_active_download(&snapshot));
        snapshot.status = "verifying".into();
        assert!(is_active_download(&snapshot));
        snapshot.status = "completed".into();
        assert!(!is_active_download(&snapshot));
    }

    #[test]
    fn target_and_installer_kind_are_defined() {
        assert!(!target_label().is_empty());
        #[cfg(target_os = "macos")]
        assert_eq!(installer_kind(), Some("dmg"));
        #[cfg(target_os = "windows")]
        assert_eq!(installer_kind(), Some("nsis"));
        let expected = expected_asset_name(&Version::parse("2.0.0").unwrap());
        assert!(expected.starts_with("bilidown_2.0.0_"));
        #[cfg(target_os = "macos")]
        assert!(expected.ends_with(".dmg"));
        #[cfg(target_os = "windows")]
        assert!(expected.ends_with("-setup.exe"));
    }
}
