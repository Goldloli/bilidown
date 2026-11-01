//! 输入解析：任意粘贴文本 → BiliId（支持多行批量与 b23.tv 短链展开）。
//!
//! 除内容标识外还提取「分 P 提示」(`?p=N`)，供入队默认选择语义使用
//! （batch-download spec：链接带 `?p=N` 时只默认勾选该 P）。

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::client::BiliClient;
use super::error::{BiliError, BiliResult};
use super::types::BiliId;

/// 一条解析结果：内容标识 + 默认条目提示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedInput {
    pub id: BiliId,
    /// 链接携带的分 P 提示（0 基）。
    pub page_hint: Option<usize>,
}

impl ParsedInput {
    pub fn new(id: BiliId) -> Self {
        Self {
            id,
            page_hint: None,
        }
    }
}

/// 判断输入是否为合法裸 ID（av/BV/ep/ss/md/au/am/cv）。
pub fn parse_bare(raw: &str) -> Option<BiliId> {
    let raw = raw.trim();
    let lower = raw.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("bv") {
        let orig_len = raw
            .strip_prefix("BV")
            .or_else(|| raw.strip_prefix("bv"))?
            .len();
        if orig_len == 10 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(BiliId::Video {
                bvid: raw.to_string(),
            });
        }
    }
    let (prefix, num) = split_prefix_num(&lower, &["av", "ep", "ss", "md", "au", "am", "cv"])?;
    let n: String = num.chars().filter(|c| c.is_ascii_digit()).collect();
    if n.is_empty() || n.len() != num.len() {
        return None;
    }
    Some(match prefix {
        "av" => BiliId::Video {
            bvid: format!("av{n}"),
        },
        "ep" => BiliId::Bangumi {
            id: format!("ep{n}"),
        },
        "ss" => BiliId::Bangumi {
            id: format!("ss{n}"),
        },
        "md" => BiliId::Bangumi {
            id: format!("md{n}"),
        },
        "au" => BiliId::Audio { song_id: n },
        "am" => BiliId::AudioList { menu_id: n },
        _ => return None,
    })
}

fn split_prefix_num<'a>(s: &'a str, prefixes: &[&'a str]) -> Option<(&'a str, &'a str)> {
    for p in prefixes {
        if let Some(rest) = s.strip_prefix(p) {
            return Some((p, rest));
        }
    }
    None
}

/// 从 URL query 提取 `p=N`（1 基 → 0 基）。
fn page_from_query(query: &str) -> Option<usize> {
    for seg in query.split('&') {
        if let Some(v) = seg.strip_prefix("p=")
            && let Ok(n) = v.parse::<usize>()
            && n >= 1
        {
            return Some(n - 1);
        }
    }
    None
}

/// 解析 bilibili.com 站内 URL（path + query）。
pub fn parse_url_path(host: &str, path: &str) -> Option<BiliId> {
    parse_url_full(host, path, "").map(|p| p.id)
}

/// 解析站内 URL，返回条目提示。
pub fn parse_url_full(host: &str, path: &str, query: &str) -> Option<ParsedInput> {
    let segs: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if host == "b23.tv" {
        return None; // 由上层先展开
    }
    if !host.ends_with("bilibili.com") {
        return None;
    }
    let id = match segs.as_slice() {
        ["video", id] => parse_bare(id)?,
        ["video", .., id] if segs.len() > 2 => parse_bare(id)?,
        ["bangumi", "play", id] => parse_bare(id)?,
        ["cheese", "play", id] => parse_bare(id).map(|_| BiliId::Cheese { id: id.to_string() })?,
        ["audio", id] => parse_bare(id)?,
        // 不支持的页面类型：明确拒绝，避免把路径末段误解析成视频 ID
        ["opus", _] => return None,
        _ => parse_bare(segs.last().copied().unwrap_or_default())?,
    };
    let page_hint = match &id {
        BiliId::Video { .. } => page_from_query(query),
        _ => None,
    };
    Some(ParsedInput { id, page_hint })
}

/// 展开短链（跟随重定向，取最终 URL）。
async fn expand_short(http: &Client, url: &str) -> BiliResult<String> {
    let resp = http.get(url).send().await.map_err(BiliError::from)?;
    Ok(resp.url().to_string())
}

/// 从一段文本提取第一个 bilibili.com / b23.tv 链接。
pub fn extract_url_text(raw: &str) -> Option<String> {
    for token in raw.split_whitespace() {
        let t = token.trim_matches(|c: char| {
            !c.is_ascii()
                && !c.is_ascii_alphanumeric()
                && c != ':'
                && c != '/'
                && c != '.'
                && c != '-'
        });
        if t.contains("b23.tv") || t.contains("bilibili.com") {
            let with_scheme = if t.starts_with("http") {
                t.to_string()
            } else {
                format!("https://{t}")
            };
            return Some(with_scheme);
        }
    }
    None
}

/// 解析单条输入（短链需要 HTTP 展开时用 `http`）。
pub async fn parse_one(http: &Client, raw: &str) -> BiliResult<BiliId> {
    Ok(parse_input_http(http, raw).await?.id)
}

/// 解析单条输入，保留分 P 提示。
pub async fn parse_input_http(http: &Client, raw: &str) -> BiliResult<ParsedInput> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(BiliError::InvalidInput);
    }
    if let Some(id) = parse_bare(raw) {
        return Ok(ParsedInput::new(id));
    }
    let url_text = extract_url_text(raw).ok_or(BiliError::InvalidInput)?;
    // b23.tv 短链：先展开
    let expanded = if url_text.contains("b23.tv") {
        expand_short(http, &url_text).await?
    } else {
        url_text.clone()
    };
    let parsed = split_url(&expanded).ok_or(BiliError::InvalidInput)?;
    let host = parsed.host.to_ascii_lowercase();
    parse_url_full(&host, &parsed.path, &parsed.query).ok_or(BiliError::InvalidInput)
}

/// 用 core 客户端解析（携带 cookie/UA/代理）。
pub async fn parse_input(client: &BiliClient, raw: &str) -> BiliResult<ParsedInput> {
    parse_input_http(client.http(), raw).await
}

/// 多行输入批量解析：返回 (成功, 失败行)。
pub async fn parse_many(http: &Client, text: &str) -> (Vec<ParsedInput>, Vec<(String, String)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_input_http(http, line).await {
            Ok(id) => ok.push(id),
            Err(e) => bad.push((line.to_string(), e.describe())),
        }
    }
    (ok, bad)
}

/// 旧 egui 应用兼容：只返回 BiliId 列表。
pub async fn parse_many_simple(http: &Client, text: &str) -> (Vec<BiliId>, Vec<(String, String)>) {
    let (ok, bad) = parse_many(http, text).await;
    (ok.into_iter().map(|p| p.id).collect(), bad)
}

pub(crate) struct SplitUrl {
    pub(crate) host: String,
    pub(crate) path: String,
    pub(crate) query: String,
}

/// 简易 URL 拆解（避免引入 url crate）：host / path / query。
pub(crate) fn split_url(s: &str) -> Option<SplitUrl> {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))?;
    let (authority, path_query) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let authority = authority.split('@').next().unwrap_or(authority);
    let host = authority.split(':').next().unwrap_or(authority).to_string();
    let (path, query) = match path_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path_query, ""),
    };
    Some(SplitUrl {
        host,
        path: path.to_string(),
        query: query.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> Option<BiliId> {
        parse_bare(raw)
    }

    #[test]
    fn bare_ids() {
        assert!(matches!(p("BV1GJ411x7h7"), Some(BiliId::Video { .. })));
        assert!(matches!(p("bv1GJ411x7h7"), Some(BiliId::Video { .. })));
        assert!(matches!(p("av170001"), Some(BiliId::Video { .. })));
        assert!(matches!(p("ep425850"), Some(BiliId::Bangumi { .. })));
        assert!(matches!(p("ss3398"), Some(BiliId::Bangumi { .. })));
        assert!(matches!(p("md4314552"), Some(BiliId::Bangumi { .. })));
        assert!(matches!(p("au912772"), Some(BiliId::Audio { .. })));
        assert!(matches!(p("am106240"), Some(BiliId::AudioList { .. })));
            assert_eq!(p("BV12345"), None);
        assert_eq!(p("hello"), None);
    }

    #[test]
    fn url_paths() {
        assert!(matches!(
            parse_url_path("www.bilibili.com", "/video/BV1GJ411x7h7"),
            Some(BiliId::Video { .. })
        ));
        assert!(matches!(
            parse_url_path("www.bilibili.com", "/bangumi/play/ep425850"),
            Some(BiliId::Bangumi { .. })
        ));
        assert!(matches!(
            parse_url_path("www.bilibili.com", "/audio/au912772"),
            Some(BiliId::Audio { .. })
        ));
        assert!(matches!(
            parse_url_path("www.bilibili.com", "/cheese/play/ep123"),
            Some(BiliId::Cheese { .. })
        ));
        assert_eq!(parse_url_path("example.com", "/video/BV1GJ411x7h7"), None);
        // 不支持的页面类型不会被误解析
        assert_eq!(parse_url_path("www.bilibili.com", "/opus/123456"), None);
    }

    #[test]
    fn page_hint_from_query() {
        let parsed = parse_url_full(
            "www.bilibili.com",
            "/video/BV1GJ411x7h7",
            "p=3&spm_id_from=333",
        )
        .unwrap();
        assert_eq!(parsed.page_hint, Some(2));
        let parsed = parse_url_full("www.bilibili.com", "/video/BV1GJ411x7h7", "p=0").unwrap();
        assert_eq!(parsed.page_hint, None);
        let parsed = parse_url_full("www.bilibili.com", "/bangumi/play/ep425850", "p=2").unwrap();
        assert_eq!(parsed.page_hint, None, "番剧 ep 链接不带分 P 语义");
    }

    #[test]
    fn extract_share_text() {
        let text = "【视频标题】https://b23.tv/abc1234 快来看";
        assert_eq!(
            extract_url_text(text).as_deref(),
            Some("https://b23.tv/abc1234")
        );
        let text2 = "https://www.bilibili.com/video/BV1GJ411x7h7?spm_id_from=333";
        assert!(
            extract_url_text(text2)
                .unwrap()
                .contains("bilibili.com/video")
        );
    }

    #[tokio::test]
    async fn parse_many_reports_bad_lines() {
        let c = Client::new();
        let (ok, bad) = parse_many(&c, "BV1GJ411x7h7\nhello\nav170001").await;
        assert_eq!(ok.len(), 2);
        assert_eq!(bad.len(), 1);
        assert!(ok[0].page_hint.is_none());
    }
}
