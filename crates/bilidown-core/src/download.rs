//! 流式下载器（task 3.5、design D5）：
//!
//! - Range 续传写入 `.part`，完成后同目录原子 rename；
//! - 每个 URL 尝试前**重新读取** `.part` 长度作为续传偏移（修复 v1 换备用 URL 时沿用旧偏移的 bug）；
//! - 403/404 视为 CDN 拒绝，继续尝试备用 URL；
//! - 读空闲超时（默认 60s）与取消响应；取消时保留 `.part`（暂停续传），
//!   真正取消的清理由上层调用 [`cleanup`] 决定；
//! - 进度以回调上报（结构化，由任务层节流聚合）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::api::client::{REFERER, UA};
use crate::api::error::{BiliError, BiliResult};

/// 读空闲超时（两次收到数据之间允许的最长间隔）。
pub const DEFAULT_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// 进度回调节流间隔（下载层不做高频回调）。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// 下载进度（单条流）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

/// 下载参数。
pub struct DownloadRequest<'a> {
    pub http: &'a reqwest::Client,
    /// 主 URL + 备用 URL（已做 PCDN 过滤）。
    pub urls: &'a [String],
    /// 最终目标路径（先写 `dest.part`）。
    pub dest: &'a Path,
    pub cancel: &'a CancellationToken,
    /// 读空闲超时（测试可调小）。
    pub read_idle_timeout: Duration,
}

impl<'a> DownloadRequest<'a> {
    pub fn new(
        http: &'a reqwest::Client,
        urls: &'a [String],
        dest: &'a Path,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            http,
            urls,
            dest,
            cancel,
            read_idle_timeout: DEFAULT_READ_IDLE_TIMEOUT,
        }
    }
}

/// 下载一个流到 `req.dest`，返回最终字节数。`.part` 在同目录，rename 原子。
///
/// 取消时返回 [`BiliError::Cancelled`]，并**保留** `.part` 供续传/清理决策。
pub async fn download_stream(
    req: &DownloadRequest<'_>,
    mut on_progress: impl FnMut(DownloadProgress),
) -> BiliResult<u64> {
    if req.urls.is_empty() {
        return Err(BiliError::Other("no download url".into()));
    }
    if let Some(parent) = req.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = part_of(req.dest);
    let mut last_err: Option<BiliError> = None;
    for url in req.urls {
        if req.cancel.is_cancelled() {
            return Err(BiliError::Cancelled);
        }
        // 每次尝试前重新读取续传偏移（备用 URL 场景修正）
        let offset = file_len(&part).await;
        let mut allow_redownload = true;
        loop {
            match download_one(req, url, &part, offset, &mut on_progress).await {
                Ok(n) => match tokio::fs::rename(&part, req.dest).await {
                    Ok(()) => {
                        on_progress(DownloadProgress {
                            downloaded: n,
                            total: n,
                        });
                        return Ok(n);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && allow_redownload => {
                        // `.part` 在下载完成与重命名之间消失（索引/外部清理/同目录冲突）：
                        // 从头重下一次当前 URL，避免把一次瞬时文件丢失直接判定为任务失败。
                        tracing::warn!(
                            target: "bilidown_core::download",
                            part = %part.display(),
                            dest = %req.dest.display(),
                            error = %e,
                            "临时文件在重命名时丢失，重新下载当前 URL"
                        );
                        allow_redownload = false;
                        let _ = tokio::fs::remove_file(&part).await;
                    }
                    Err(e) => {
                        return Err(BiliError::Disk(format!(
                            "rename {} -> {} 失败: {e}",
                            part.display(),
                            req.dest.display()
                        )));
                    }
                },
                Err(BiliError::Cancelled) => return Err(BiliError::Cancelled),
                Err(e) => {
                    last_err = Some(e);
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| BiliError::Network("all urls failed".into())))
}

async fn download_one(
    req: &DownloadRequest<'_>,
    url: &str,
    part: &Path,
    offset: u64,
    on_progress: &mut impl FnMut(DownloadProgress),
) -> BiliResult<u64> {
    let mut attempt_offset = offset;
    // 416/服务器忽略 Range 时允许一次从头重试
    let mut restarted = false;
    loop {
        let mut builder = req
            .http
            .get(url)
            .header("Referer", REFERER)
            .header("User-Agent", UA);
        if attempt_offset > 0 {
            builder = builder.header("Range", format!("bytes={attempt_offset}-"));
        }
        let resp = builder.send().await.map_err(BiliError::from_reqwest)?;
        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            return Err(BiliError::CdnDenied);
        }
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // `.part` 可能已完整：`bytes */total` 且 offset == total
            let total = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total);
            if let (Some(total), true) = (total, !restarted)
                && attempt_offset == total
                && attempt_offset > 0
            {
                return Ok(attempt_offset);
            }
            if restarted {
                return Err(BiliError::Network("range not satisfiable".into()));
            }
            let _ = tokio::fs::remove_file(part).await;
            attempt_offset = 0;
            restarted = true;
            continue;
        }
        if !status.is_success() {
            return Err(BiliError::Network(format!("HTTP {status}")));
        }

        // 服务器是否接受续传
        let mut resumed = false;
        let mut total_hint = 0u64;
        if attempt_offset > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            let range = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_start);
            match range {
                Some(start) if start == attempt_offset => resumed = true,
                _ => {
                    // Content-Range 与期望不符：丢弃半成品，从头下载
                    if restarted {
                        return Err(BiliError::Network("bad content-range".into()));
                    }
                    let _ = tokio::fs::remove_file(part).await;
                    attempt_offset = 0;
                    restarted = true;
                    continue;
                }
            }
            if let Some(total) = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total)
            {
                total_hint = total;
            }
        } else if attempt_offset > 0 && status == reqwest::StatusCode::OK {
            // 服务器忽略 Range：从 0 重写
            let _ = tokio::fs::remove_file(part).await;
            attempt_offset = 0;
        }
        if resumed {
            total_hint = total_hint.max(attempt_offset + resp.content_length().unwrap_or(0));
        } else {
            total_hint = resp.content_length().unwrap_or(0);
        }

        let mut file = if resumed {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(part)
                .await?
        } else {
            tokio::fs::File::create(part).await?
        };
        let mut downloaded = if resumed { attempt_offset } else { 0 };
        let mut stream = resp.bytes_stream();
        let mut last_emit = Instant::now();
        let mut emitted_initial = false;
        loop {
            let next = tokio::select! {
                _ = req.cancel.cancelled() => {
                    file.flush().await.ok();
                    return Err(BiliError::Cancelled);
                }
                res = tokio::time::timeout(req.read_idle_timeout, stream.next()) => match res {
                    Err(_) => {
                        file.flush().await.ok();
                        return Err(BiliError::Timeout(format!("read idle over {:?}", req.read_idle_timeout)));
                    }
                    Ok(item) => item,
                },
            };
            match next {
                Some(Ok(bytes)) => {
                    file.write_all(&bytes).await?;
                    downloaded += bytes.len() as u64;
                    let now = Instant::now();
                    if !emitted_initial || now.duration_since(last_emit) >= PROGRESS_INTERVAL {
                        emitted_initial = true;
                        last_emit = now;
                        on_progress(DownloadProgress {
                            downloaded,
                            total: total_hint.max(downloaded),
                        });
                    }
                }
                Some(Err(e)) => {
                    // 网络中断：flush 后保留 .part 供续传
                    file.flush().await.ok();
                    return Err(BiliError::from_reqwest(e));
                }
                None => break,
            }
        }
        file.flush().await?;
        file.sync_all().await.ok();
        drop(file);
        on_progress(DownloadProgress {
            downloaded,
            total: total_hint.max(downloaded),
        });
        return Ok(downloaded);
    }
}

/// `.part` 路径。
pub fn part_of(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// 清理下载残留（真正取消时由上层调用；暂停时保留以续传）。
pub async fn cleanup(dest: &Path) {
    let _ = tokio::fs::remove_file(part_of(dest)).await;
    let _ = tokio::fs::remove_file(dest).await;
}

async fn file_len(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

/// 解析 `Content-Range: bytes 100-199/1234` 的起始偏移。
fn parse_content_range_start(value: &str) -> Option<u64> {
    let rest = value.strip_prefix("bytes")?.trim();
    let (range, _total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    start.trim().parse().ok()
}

/// 解析 `Content-Range: bytes 100-199/1234` 或 `bytes */1234` 的总长度。
fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.rsplit_once('/')?;
    let total = total.trim();
    if total == "*" {
        return None;
    }
    total.parse().ok()
}

/// 旧接口兼容：v1 的 mpsc 事件通道（旧 egui 队列仍用；8.1 后删除）。
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    Progress { downloaded: u64, total: u64 },
    Finished(PathBuf),
    Failed(String),
    Cancelled,
}

/// 旧接口兼容：把下载进度转发到 mpsc 通道。
pub async fn download_stream_to_channel(
    http: &reqwest::Client,
    urls: &[String],
    dest: &Path,
    cancel: &CancellationToken,
    progress: &mpsc::Sender<DownloadEvent>,
    _already: u64,
) -> BiliResult<u64> {
    let req = DownloadRequest::new(http, urls, dest, cancel);
    let n = download_stream(&req, |p| {
        let _ = progress.try_send(DownloadEvent::Progress {
            downloaded: p.downloaded,
            total: p.total,
        });
    })
    .await?;
    let _ = progress.try_send(DownloadEvent::Finished(dest.to_path_buf()));
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    async fn run(
        urls: &[String],
        dest: &Path,
        cancel: &CancellationToken,
        idle_ms: u64,
        seen: &mut Vec<DownloadProgress>,
    ) -> BiliResult<u64> {
        let c = client();
        let req = DownloadRequest {
            http: &c,
            urls,
            dest,
            cancel,
            read_idle_timeout: Duration::from_millis(idle_ms),
        };
        download_stream(&req, |p| seen.push(p)).await
    }

    #[tokio::test]
    async fn downloads_200_with_progress() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v.m4s"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![7u8; 4096]))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("v.m4s");
        let urls = vec![format!("{}/v.m4s", server.uri())];
        let cancel = CancellationToken::new();
        let mut seen = Vec::new();
        let n = run(&urls, &dest, &cancel, 5000, &mut seen).await.unwrap();
        assert_eq!(n, 4096);
        assert_eq!(std::fs::read(&dest).unwrap().len(), 4096);
        assert!(seen.iter().any(|p| p.total == 4096));
        assert!(!part_of(&dest).exists(), ".part 应已 rename");
    }

    #[tokio::test]
    async fn resumes_from_part_with_range() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v.m4s"))
            .and(header("range", "bytes=100-"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 100-199/200")
                    .set_body_bytes(vec![2u8; 100]),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("v.m4s");
        let mut part = std::fs::File::create(part_of(&dest)).unwrap();
        use std::io::Write;
        part.write_all(&[1u8; 100]).unwrap();
        drop(part);
        let urls = vec![format!("{}/v.m4s", server.uri())];
        let cancel = CancellationToken::new();
        let mut seen = Vec::new();
        let n = run(&urls, &dest, &cancel, 5000, &mut seen).await.unwrap();
        assert_eq!(n, 200);
        let data = std::fs::read(&dest).unwrap();
        assert_eq!(data.len(), 200);
        assert!(data[..100].iter().all(|b| *b == 1));
        assert!(data[100..].iter().all(|b| *b == 2));
    }

    #[tokio::test]
    async fn forbidden_falls_back_to_backup_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bad.m4s"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/good.m4s"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![3u8; 512]))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("v.m4s");
        let urls = vec![
            format!("{}/bad.m4s", server.uri()),
            format!("{}/good.m4s", server.uri()),
        ];
        let cancel = CancellationToken::new();
        let mut seen = Vec::new();
        let n = run(&urls, &dest, &cancel, 5000, &mut seen).await.unwrap();
        assert_eq!(n, 512);
    }

    #[tokio::test]
    async fn backup_url_resumes_with_fresh_offset() {
        // URL1 403，URL2 支持 Range：必须用「URL2 尝试前重新读取的 part 长度」而不是旧偏移
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bad.m4s"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/good.m4s"))
            .and(header("range", "bytes=50-"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 50-99/100")
                    .set_body_bytes(vec![9u8; 50]),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("v.m4s");
        std::fs::write(part_of(&dest), vec![8u8; 50]).unwrap();
        let urls = vec![
            format!("{}/bad.m4s", server.uri()),
            format!("{}/good.m4s", server.uri()),
        ];
        let cancel = CancellationToken::new();
        let mut seen = Vec::new();
        let n = run(&urls, &dest, &cancel, 5000, &mut seen).await.unwrap();
        assert_eq!(n, 100, "应从 part 偏移续传，不得重复写");
        let data = std::fs::read(&dest).unwrap();
        assert!(data[..50].iter().all(|b| *b == 8));
        assert!(data[50..].iter().all(|b| *b == 9));
    }

    #[tokio::test]
    async fn read_idle_timeout_classified() {
        // 手写一个「发完响应头后卡住」的 HTTP 服务，验证读空闲超时（wiremock 的 delay 在响应头之前）
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nContent-Type: application/octet-stream\r\n\r\n",
                );
                let _ = sock.write_all(&[0u8; 10]);
                let _ = sock.flush();
                std::thread::sleep(Duration::from_secs(2));
                drop(sock);
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("v.m4s");
        let urls = vec![format!("http://{addr}/slow.m4s")];
        let cancel = CancellationToken::new();
        let mut seen = Vec::new();
        let err = run(&urls, &dest, &cancel, 100, &mut seen)
            .await
            .expect_err("应超时");
        assert_eq!(err.kind(), crate::api::error::ErrorKind::Timeout, "{err}");
    }

    #[tokio::test]
    async fn cancel_keeps_part_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v.m4s"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(vec![5u8; 8 * 1024 * 1024])
                    .set_delay(Duration::from_millis(300)),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("v.m4s");
        let urls = vec![format!("{}/v.m4s", server.uri())];
        let cancel = CancellationToken::new();
        let c = cancel.clone();
        let dest2 = dest.clone();
        let handle = tokio::spawn(async move {
            let mut seen = Vec::new();
            let r = run(&urls, &dest2, &c, 5000, &mut seen).await;
            (r, dest2)
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        let (r, d) = handle.await.unwrap();
        assert!(matches!(r, Err(BiliError::Cancelled)), "{r:?}");
        assert!(part_of(&d).exists(), "暂停/取消时保留 .part 由上层清理");
        cleanup(&d).await;
        assert!(!part_of(&d).exists());
    }

    #[tokio::test]
    async fn content_range_parsing() {
        assert_eq!(parse_content_range_start("bytes 100-199/1234"), Some(100));
        assert_eq!(parse_content_range_total("bytes 100-199/1234"), Some(1234));
        assert_eq!(parse_content_range_total("bytes */1234"), Some(1234));
        assert_eq!(parse_content_range_total("bytes */"), None);
    }
}
