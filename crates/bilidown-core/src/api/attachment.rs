//! 公开附件下载：URL 策略、无凭据客户端、有界响应与图片签名。

use futures_util::StreamExt;
use reqwest::{Client, Response, Url};

use super::client::{DOWNLOAD_CONNECT_TIMEOUT, DOWNLOAD_READ_TIMEOUT, REFERER, UA, apply_proxy};
use super::error::{BiliError, BiliResult};

pub const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;
const ALLOWED_ROOTS: [&str; 2] = ["bilibili.com", "hdslb.com"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachmentUrlPolicy {
    allow_loopback: bool,
}

impl AttachmentUrlPolicy {
    /// 仅供集成测试显式注入的环回策略；生产调用统一使用 `default()`。
    pub fn loopback_for_tests() -> Self {
        Self {
            allow_loopback: true,
        }
    }

    pub fn normalize(&self, raw: &str) -> BiliResult<Url> {
        let raw = raw.trim();
        let absolute;
        let raw = if let Some(rest) = raw.strip_prefix("//") {
            absolute = format!("https://{rest}");
            &absolute
        } else {
            raw
        };
        let mut url = Url::parse(raw).map_err(|_| BiliError::InvalidInput)?;
        self.validate_credentials(&url)?;
        if url.scheme() == "http" && !is_loopback(&url) {
            url.set_scheme("https")
                .map_err(|_| BiliError::InvalidInput)?;
        }
        self.validate(&url)?;
        Ok(url)
    }

    fn validate(&self, url: &Url) -> BiliResult<()> {
        self.validate_credentials(url)?;
        let loopback = is_loopback(url);
        if loopback {
            if !self.allow_loopback {
                return Err(BiliError::Permission {
                    message: "attachment host is not allowed".into(),
                });
            }
        } else if !url.host_str().is_some_and(is_allowed_bilibili_host) {
            return Err(BiliError::Permission {
                message: "attachment host is not allowed".into(),
            });
        }
        if url.scheme() == "https" || (loopback && self.allow_loopback && url.scheme() == "http") {
            return Ok(());
        }
        Err(BiliError::Permission {
            message: "attachment URL must use https".into(),
        })
    }

    fn validate_credentials(&self, url: &Url) -> BiliResult<()> {
        if !url.username().is_empty() || url.password().is_some() {
            return Err(BiliError::Permission {
                message: "attachment URL must not contain user info".into(),
            });
        }
        if url.host_str().is_none() {
            return Err(BiliError::InvalidInput);
        }
        Ok(())
    }
}

fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

fn is_allowed_bilibili_host(host: &str) -> bool {
    ALLOWED_ROOTS.iter().any(|root| {
        host == *root
            || host
                .strip_suffix(root)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

pub fn attachment_client(proxy: Option<&str>, policy: AttachmentUrlPolicy) -> BiliResult<Client> {
    let mut builder = Client::builder()
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::custom(
            move |attempt: reqwest::redirect::Attempt<'_>| {
                if attempt.previous().len() > MAX_REDIRECTS {
                    return attempt.error("too many attachment redirects");
                }
                match policy.validate(attempt.url()) {
                    Ok(()) => attempt.follow(),
                    Err(error) => attempt.error(error),
                }
            },
        ))
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT);
    builder = apply_proxy(builder, proxy)?;
    builder.build().map_err(BiliError::from_reqwest)
}

pub async fn send_attachment(
    client: &Client,
    policy: AttachmentUrlPolicy,
    url: &str,
) -> BiliResult<Response> {
    let url = policy.normalize(url)?;
    client
        .get(url)
        .header("Referer", REFERER)
        .header("User-Agent", UA)
        .send()
        .await
        .map_err(BiliError::from_reqwest)
}

pub async fn read_attachment_body(response: Response) -> BiliResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ATTACHMENT_BYTES as u64)
    {
        return Err(BiliError::Parse("attachment exceeds 8 MiB".into()));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BiliError::from_reqwest)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_ATTACHMENT_BYTES {
            return Err(BiliError::Parse("attachment exceeds 8 MiB".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub fn image_format(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn serve_once(content_length: Option<usize>, body: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                let headers = match content_length {
                    Some(length) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                    ),
                    None => "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string(),
                };
                let _ = socket.write_all(headers.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });
        format!("http://{addr}/attachment")
    }

    #[test]
    fn production_policy_normalizes_only_expected_bilibili_urls() {
        let policy = AttachmentUrlPolicy::default();
        assert_eq!(
            policy
                .normalize("//i0.hdslb.com/bfs/a.jpg")
                .unwrap()
                .as_str(),
            "https://i0.hdslb.com/bfs/a.jpg"
        );
        assert_eq!(
            policy
                .normalize("http://www.bilibili.com/a")
                .unwrap()
                .as_str(),
            "https://www.bilibili.com/a"
        );
        assert!(policy.normalize("https://hdslb.com/a").is_ok());
        assert!(policy.normalize("https://aisubtitle.hdslb.com/a").is_ok());
    }

    #[test]
    fn production_policy_rejects_untrusted_and_local_urls() {
        let policy = AttachmentUrlPolicy::default();
        for url in [
            "not a url",
            "file:///tmp/a.jpg",
            "https://user:pass@i0.hdslb.com/a",
            "https://127.0.0.1/a",
            "http://localhost/a",
            "https://192.168.1.2/a",
            "https://169.254.1.1/a",
            "https://example.com/a",
            "https://evilhdslb.com/a",
            "https://hdslb.com.example.com/a",
        ] {
            assert!(policy.normalize(url).is_err(), "should reject {url}");
        }
    }

    #[test]
    fn test_policy_only_relaxes_loopback_http() {
        let policy = AttachmentUrlPolicy::loopback_for_tests();
        assert!(policy.normalize("http://127.0.0.1:1234/a").is_ok());
        assert!(policy.normalize("http://localhost/a").is_ok());
        assert!(policy.normalize("http://192.168.1.2/a").is_err());
        assert!(policy.normalize("http://example.com/a").is_err());
    }

    #[test]
    fn image_signatures_cover_existing_formats_and_reject_html() {
        assert_eq!(image_format(&[0xFF, 0xD8, 0xFF, 0]).unwrap().1, "jpg");
        assert_eq!(image_format(b"\x89PNG\r\n").unwrap().1, "png");
        assert_eq!(image_format(b"RIFF0000WEBP!").unwrap().1, "webp");
        assert_eq!(image_format(b"GIF89a").unwrap().1, "gif");
        assert!(image_format(b"<html>nope</html>").is_none());
    }

    #[tokio::test]
    async fn bounded_reader_rejects_declared_oversize() {
        let url = serve_once(Some(MAX_ATTACHMENT_BYTES + 1), Vec::new()).await;
        let client = attachment_client(None, AttachmentUrlPolicy::loopback_for_tests()).unwrap();
        let response = send_attachment(&client, AttachmentUrlPolicy::loopback_for_tests(), &url)
            .await
            .unwrap();
        assert!(read_attachment_body(response).await.is_err());
    }

    #[tokio::test]
    async fn bounded_reader_rejects_streamed_oversize_without_content_length() {
        let url = serve_once(None, vec![1; MAX_ATTACHMENT_BYTES + 1]).await;
        let client = attachment_client(None, AttachmentUrlPolicy::loopback_for_tests()).unwrap();
        let response = send_attachment(&client, AttachmentUrlPolicy::loopback_for_tests(), &url)
            .await
            .unwrap();
        assert!(read_attachment_body(response).await.is_err());
    }

    #[tokio::test]
    async fn bounded_reader_accepts_exact_limit() {
        let url = serve_once(Some(MAX_ATTACHMENT_BYTES), vec![1; MAX_ATTACHMENT_BYTES]).await;
        let client = attachment_client(None, AttachmentUrlPolicy::loopback_for_tests()).unwrap();
        let response = send_attachment(&client, AttachmentUrlPolicy::loopback_for_tests(), &url)
            .await
            .unwrap();
        let bytes = read_attachment_body(response).await.unwrap();
        assert_eq!(bytes.len(), MAX_ATTACHMENT_BYTES);
    }

    #[tokio::test]
    async fn bounded_reader_preserves_stream_errors() {
        let url = serve_once(Some(100), vec![1; 10]).await;
        let client = attachment_client(None, AttachmentUrlPolicy::loopback_for_tests()).unwrap();
        let response = send_attachment(&client, AttachmentUrlPolicy::loopback_for_tests(), &url)
            .await
            .unwrap();
        let error = read_attachment_body(response).await.unwrap_err();
        assert!(matches!(error, BiliError::Network(_)), "{error:?}");
    }

    #[tokio::test]
    async fn follows_valid_redirects_without_cookie() {
        let source = MockServer::start().await;
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/final", target.uri())),
            )
            .expect(1)
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&target)
            .await;
        let client = attachment_client(None, AttachmentUrlPolicy::loopback_for_tests()).unwrap();
        let response = send_attachment(
            &client,
            AttachmentUrlPolicy::loopback_for_tests(),
            &format!("{}/start", source.uri()),
        )
        .await
        .unwrap();
        let bytes = read_attachment_body(response).await.unwrap();
        assert_eq!(bytes, b"ok");
        let requests = target.received_requests().await.unwrap();
        assert!(!requests[0].headers.contains_key("cookie"));
    }

    #[tokio::test]
    async fn rejects_redirect_target_before_request() {
        let source = MockServer::start().await;
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/final", target.uri())),
            )
            .expect(1)
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&target)
            .await;
        let client = attachment_client(None, AttachmentUrlPolicy::default()).unwrap();
        let result = client.get(format!("{}/start", source.uri())).send().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_initial_target_before_request_and_never_falls_back_to_direct() {
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&target)
            .await;
        let client = attachment_client(None, AttachmentUrlPolicy::default()).unwrap();
        let result = send_attachment(
            &client,
            AttachmentUrlPolicy::default(),
            &format!("{}/final", target.uri()),
        )
        .await;
        assert!(result.is_err());

        let proxy = "http://127.0.0.1:1";
        let client =
            attachment_client(Some(proxy), AttachmentUrlPolicy::loopback_for_tests()).unwrap();
        let result = send_attachment(
            &client,
            AttachmentUrlPolicy::loopback_for_tests(),
            &format!("{}/final", target.uri()),
        )
        .await;
        assert!(result.is_err());
    }
}
