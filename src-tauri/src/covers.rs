//! 远程图片抓取与缓存（B 站头像/封面图床有 Referer 防盗链，WebView 直连会破图）。
//!
//! 请求不携带登录 Cookie；返回 `data:` URL 给前端 `<img>` 使用；缓存到 `<appData>/covers/<hash>.<ext>`，
//! 启动时清理 N 天未用/超量的旧缓存。

use base64::Engine;
use futures_util::StreamExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

const REFERER: &str = "https://www.bilibili.com/";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
/// 单张封面上限（B 站封面通常 < 1MB）。
const MAX_BYTES: usize = 8 * 1024 * 1024;
/// 支持的图片扩展名（磁盘缓存直接按 hash + 扩展名定位，不遍历目录，design D11/7.7）。
const IMAGE_EXTS: [&str; 4] = ["jpg", "png", "webp", "gif"];

type CoverClientCache = Mutex<Option<(Option<String>, reqwest::Client)>>;

/// 按代理配置复用的图片 HTTP 客户端（不带登录 Cookie，design D11/7.7）。
static COVER_CLIENT: OnceLock<CoverClientCache> = OnceLock::new();

/// 测试用：当前缓存的代理配置键。
#[cfg(test)]
fn cached_cover_proxy() -> Option<Option<String>> {
    COVER_CLIENT
        .get()
        .and_then(|cache| cache.lock().ok())
        .and_then(|guard| guard.as_ref().map(|(key, _)| key.clone()))
}

fn cover_client(proxy: Option<&str>) -> Option<reqwest::Client> {
    let key = proxy
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let cache = COVER_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    if let Some((cached_key, client)) = guard.as_ref()
        && cached_key == &key
    {
        return Some(client.clone());
    }
    let client = bilidown_core::api::client::download_client(key.as_deref()).ok()?;
    *guard = Some((key, client.clone()));
    Some(client)
}

pub fn hash_url(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(("image/png", "png"))
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else if bytes.starts_with(b"GIF8") {
        Some(("image/gif", "gif"))
    } else {
        None
    }
}

fn data_url(bytes: &[u8]) -> String {
    let Some((mime, _)) = sniff(bytes) else {
        return String::new();
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn cache_hit(dir: &Path, hash: &str) -> Option<Vec<u8>> {
    // 直接按已知扩展名定位，避免每次遍历缓存目录（design D11/7.7）。
    for ext in IMAGE_EXTS {
        let path = dir.join(format!("{hash}.{ext}"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if !bytes.is_empty() && bytes.len() <= MAX_BYTES && sniff(&bytes).is_some() {
            return Some(bytes);
        }
    }
    None
}

fn write_cache(dir: &Path, hash: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
    let (_, ext) = sniff(bytes)?;
    let path = dir.join(format!("{hash}.{ext}"));
    let _ = std::fs::write(&path, bytes);
    Some(path)
}

fn log_identity(url: &str) -> (&str, String) {
    let host = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split('/').next())
        .filter(|host| !host.is_empty())
        .unwrap_or("invalid");
    (host, hash_url(url))
}

/// 抓取远程图片：缓存命中 → 读取；否则带 Referer/UA 经设置代理下载 → 校验 → 落盘 → data URL。
pub async fn fetch(proxy: Option<&str>, cache_dir: &Path, url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return String::new();
    }
    let _ = std::fs::create_dir_all(cache_dir);
    let hash = hash_url(url);
    if let Some(bytes) = cache_hit(cache_dir, &hash) {
        return data_url(&bytes);
    }
    let Some(client) = cover_client(proxy) else {
        tracing::warn!(target: "shell::covers", "创建封面下载客户端失败");
        return String::new();
    };
    let response = match client
        .get(url)
        .header("Referer", REFERER)
        .header("User-Agent", UA)
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let (host, hash) = log_identity(url);
            tracing::debug!(target: "shell::covers", url_host = host, url_hash = %hash, %err, "远程图片下载失败");
            return String::new();
        }
    };
    if !response.status().is_success() {
        let (host, hash) = log_identity(url);
        tracing::debug!(target: "shell::covers", url_host = host, url_hash = %hash, status = %response.status(), "远程图片 HTTP 失败");
        return String::new();
    }
    // 流式读取并执行 8 MiB 上限，避免先把任意大小的响应完整装入内存（design D11/7.7）。
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return String::new();
        };
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            tracing::debug!(target: "shell::covers", limit = MAX_BYTES, "远程图片超过大小上限，已中止");
            return String::new();
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() || sniff(&bytes).is_none() {
        return String::new();
    }
    let _ = write_cache(cache_dir, &hash, &bytes);
    data_url(&bytes)
}

/// 启动时清理：删除超过 `keep_days` 的缓存；仍超过 `max_files` 时按 mtime 删最旧。
pub fn prune(cache_dir: &Path, keep_days: u64, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let now = SystemTime::now();
    let max_age = Duration::from_secs(keep_days * 24 * 3600);
    let mut files: Vec<(SystemTime, std::path::PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(now);
        if now.duration_since(modified).unwrap_or_default() > max_age {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        files.push((modified, path));
    }
    if files.len() > max_files {
        files.sort_by_key(|(modified, _)| *modified);
        let remove = files.len() - max_files;
        for (_, path) in files.into_iter().take(remove) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_image_containers() {
        assert_eq!(
            sniff(&[0xFF, 0xD8, 0xFF, 0x00]),
            Some(("image/jpeg", "jpg"))
        );
        assert_eq!(
            sniff(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A]),
            Some(("image/png", "png"))
        );
        assert_eq!(sniff(b"GIF89a"), Some(("image/gif", "gif")));
        let mut webp = b"RIFF0000WEBP!".to_vec();
        webp[4..8].copy_from_slice(&16u32.to_le_bytes());
        assert_eq!(sniff(&webp), Some(("image/webp", "webp")));
    }

    #[test]
    fn rejects_non_image_payloads() {
        assert_eq!(sniff(b"<html>not an image</html>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn cover_client_cache_reuses_and_follows_proxy_changes() {
        // 同一代理复用同一客户端缓存；代理切换后缓存键更新（design D11/7.7）。
        assert!(cover_client(Some("http://127.0.0.1:7897")).is_some());
        assert_eq!(
            cached_cover_proxy(),
            Some(Some("http://127.0.0.1:7897".to_string()))
        );
        assert!(cover_client(None).is_some());
        assert_eq!(cached_cover_proxy(), Some(None));
        assert!(cover_client(Some("  ")).is_some());
        assert_eq!(cached_cover_proxy(), Some(None), "空白代理视为直连");
    }

    #[test]
    fn cache_hit_uses_direct_ext_lookup() {
        let dir = std::env::temp_dir().join(format!("bilidown-covers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hash = hash_url("https://i0.hdslb.com/bfs/archive/cover.jpg");
        // 直接命中 hash.jpg，不需要遍历目录。
        std::fs::write(dir.join(format!("{hash}.jpg")), [0xFF, 0xD8, 0xFF, 0x00]).unwrap();
        assert!(cache_hit(&dir, &hash).is_some());
        // PNG 扩展名同样可命中。
        std::fs::write(dir.join(format!("{hash}.png")), [0x89, b'P', b'N', b'G']).unwrap();
        assert!(cache_hit(&dir, &hash).is_some());
        // 非图片内容不被当作缓存命中。
        let bad_hash = hash_url("https://i0.hdslb.com/bfs/archive/bad.jpg");
        std::fs::write(dir.join(format!("{bad_hash}.jpg")), b"<html>nope</html>").unwrap();
        assert!(cache_hit(&dir, &bad_hash).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn logs_only_host_and_hash_for_sensitive_image_urls() {
        let (host, hash) = log_identity("https://i0.hdslb.com/bfs/face/account.jpg?uid=123");
        assert_eq!(host, "i0.hdslb.com");
        assert_eq!(hash.len(), 16);
        assert!(!hash.contains("account"));
        assert!(!hash.contains("uid"));
    }

    #[tokio::test]
    async fn rejects_non_http_image_urls_without_touching_cache() {
        let result = fetch(None, Path::new("unused"), "file:///tmp/avatar.jpg").await;
        assert!(result.is_empty());
    }
}
