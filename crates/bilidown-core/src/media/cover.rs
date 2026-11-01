//! 封面下载（change add-download-features design D2）：
//!
//! - 优先 B 站图片服务的 `@.jpg` 缩放形式，失败回退原图/base；
//! - 与视频同目录保存 `{视频文件名去扩展}.jpg`，重名走 `dedupe_in_dir`；
//! - 失败返回错误由任务层 warn 记录，不影响主产物。

use std::path::{Path, PathBuf};

use crate::api::client::{REFERER, UA};
use crate::api::error::{BiliError, BiliResult};

use super::filename;

/// 缩放后缀（B 站图片服务格式转换/缩放操作符）。
pub const COVER_SUFFIX: &str = "@.jpg";

/// 候选封面 URL：`@.jpg` 缩放优先，其次原图（去掉旧缩放参数）兜底。
pub fn cover_url_candidates(url: &str) -> Vec<String> {
    let raw = url.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let normalized = if let Some(rest) = raw.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        raw.to_string()
    };
    if !(normalized.starts_with("https://") || normalized.starts_with("http://")) {
        return Vec::new();
    }
    // 去掉已有的 `@…` 处理参数，避免 `@.jpg@672w…` 这类非法组合
    let base = normalized
        .split('@')
        .next()
        .unwrap_or(&normalized)
        .to_string();
    let mut out = vec![format!("{base}{COVER_SUFFIX}"), base];
    out.dedup();
    out
}

/// 封面文件名：`{stem}.jpg`。
pub fn cover_filename(stem: &str) -> String {
    format!("{}.jpg", filename::sanitize_component(stem))
}

/// 下载封面到 `out_dir`，返回最终文件路径。
///
/// `video_path` 用于派生文件名（与视频同名）；`http` 建议传下载 client
/// （读超时 60s）。所有候选 URL 都失败时返回最后一个错误。
pub async fn download_cover(
    http: &reqwest::Client,
    url: &str,
    video_path: &Path,
    out_dir: &Path,
) -> BiliResult<PathBuf> {
    let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
        return Err(BiliError::InvalidInput);
    };
    download_cover_named(http, url, stem, out_dir).await
}

/// 按给定 stem 下载封面（测试/特殊命名场景）。
pub async fn download_cover_named(
    http: &reqwest::Client,
    url: &str,
    stem: &str,
    out_dir: &Path,
) -> BiliResult<PathBuf> {
    let candidates = cover_url_candidates(url);
    if candidates.is_empty() {
        return Err(BiliError::InvalidInput);
    }
    tokio::fs::create_dir_all(out_dir).await?;
    let mut last_err: Option<BiliError> = None;
    for candidate in candidates {
        let resp = match http
            .get(&candidate)
            .header("Referer", REFERER)
            .header("User-Agent", UA)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                last_err = Some(BiliError::from_reqwest(e));
                continue;
            }
        };
        let status = resp.status();
        if !status.is_success() {
            last_err = Some(BiliError::Network(format!(
                "cover HTTP {status}: {candidate}"
            )));
            continue;
        }
        match resp.bytes().await {
            Ok(bytes) if !bytes.is_empty() => {
                let path = filename::dedupe_in_dir(out_dir, &cover_filename(stem));
                tokio::fs::write(&path, &bytes).await?;
                tracing::debug!(path = %path.display(), bytes = bytes.len(), "cover saved");
                return Ok(path);
            }
            Ok(_) => last_err = Some(BiliError::Parse("empty cover body".into())),
            Err(e) => last_err = Some(BiliError::from_reqwest(e)),
        }
    }
    Err(last_err.unwrap_or_else(|| BiliError::Network("cover download failed".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_prefer_jpg_suffix_then_original() {
        let c = cover_url_candidates("https://i0.hdslb.com/bfs/archive/abc.jpg");
        assert_eq!(c[0], "https://i0.hdslb.com/bfs/archive/abc.jpg@.jpg");
        assert_eq!(c[1], "https://i0.hdslb.com/bfs/archive/abc.jpg");
    }

    #[test]
    fn candidates_normalize_protocol_relative_and_strip_old_params() {
        let c = cover_url_candidates("//i0.hdslb.com/bfs/archive/abc.jpg@672w_378h_1c.webp");
        assert_eq!(c.len(), 2);
        assert!(c[0].starts_with("https://i0.hdslb.com/"));
        assert_eq!(c[0], "https://i0.hdslb.com/bfs/archive/abc.jpg@.jpg");
        assert_eq!(c[1], "https://i0.hdslb.com/bfs/archive/abc.jpg");
    }

    #[test]
    fn invalid_urls_yield_no_candidates() {
        assert!(cover_url_candidates("").is_empty());
        assert!(cover_url_candidates("   ").is_empty());
        assert!(cover_url_candidates("ftp://x/a.jpg").is_empty());
    }

    #[test]
    fn cover_filename_shape() {
        assert_eq!(cover_filename("视频[1080P]"), "视频[1080P].jpg");
        assert_eq!(cover_filename("a/b:c"), "a b c.jpg");
    }

    fn image_bytes() -> Vec<u8> {
        // 最小 JPEG 头部即可（不解析图片，只验证写盘）
        vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46]
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    #[tokio::test]
    async fn downloads_cover_with_jpg_suffix() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bfs/archive/abc.jpg@.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(image_bytes()))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("标题[1080P].mp4");
        let url = format!("{}/bfs/archive/abc.jpg", server.uri());
        let path = download_cover(&client(), &url, &video, dir.path())
            .await
            .expect("封面应下载成功");
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "标题[1080P].jpg"
        );
        assert_eq!(std::fs::read(&path).unwrap(), image_bytes());
        // 重名去重
        let path2 = download_cover(&client(), &url, &video, dir.path())
            .await
            .unwrap();
        assert_eq!(
            path2.file_name().unwrap().to_str().unwrap(),
            "标题[1080P] (1).jpg"
        );
    }

    #[tokio::test]
    async fn falls_back_to_original_when_suffix_missing() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/abc.jpg@.jpg"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/abc.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(image_bytes()))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("v.mp4");
        let url = format!("{}/abc.jpg", server.uri());
        let path = download_cover(&client(), &url, &video, dir.path())
            .await
            .expect("回退原图应成功");
        assert_eq!(std::fs::read(&path).unwrap(), image_bytes());
    }

    #[tokio::test]
    async fn empty_body_is_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/empty.jpg@.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(Vec::new()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/empty.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(Vec::new()))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("v.mp4");
        let url = format!("{}/empty.jpg", server.uri());
        let err = download_cover(&client(), &url, &video, dir.path())
            .await
            .expect_err("空响应应失败");
        assert!(matches!(err, BiliError::Parse(_)), "{err:?}");
    }

    #[tokio::test]
    async fn all_candidates_fail_returns_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gone.jpg@.jpg"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gone.jpg"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("v.mp4");
        let url = format!("{}/gone.jpg", server.uri());
        let err = download_cover(&client(), &url, &video, dir.path())
            .await
            .expect_err("应失败");
        assert!(matches!(err, BiliError::Network(_)), "{err:?}");
    }

    #[tokio::test]
    async fn write_failure_is_disk_error() {
        let dir = tempfile::tempdir().unwrap();
        // out_dir 是普通文件 → create_dir_all 失败
        let not_a_dir = dir.path().join("not-a-dir");
        std::fs::write(&not_a_dir, b"x").unwrap();
        let err = download_cover_named(
            &client(),
            "https://example.invalid/a.jpg",
            "stem",
            &not_a_dir,
        )
        .await
        .expect_err("写盘失败应是错误");
        assert!(
            matches!(err, BiliError::Disk(_) | BiliError::Io(_)),
            "{err:?}"
        );
    }
}
