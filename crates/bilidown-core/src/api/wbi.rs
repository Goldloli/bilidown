//! WBI 签名：nav 取 img/sub key → mixinKey 重排 → md5 w_rid。
//! 算法依据 B 站 web 端公开实现（公开协议）。

use md5::{Digest, Md5};
use serde_json::Value;

use super::client::{BiliClient, WbiKeys};
use super::error::BiliResult;

/// mixinKey 混淆表（64 → 32）。
const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// 从 nav 响应的 wbi_img 取原始 key。
pub fn raw_keys_from_nav(nav: &Value) -> Option<(String, String)> {
    let img_url = nav.pointer("/wbi_img/img_url")?.as_str()?;
    let sub_url = nav.pointer("/wbi_img/sub_url")?.as_str()?;
    let stem = |u: &str| {
        u.rsplit('/')
            .next()
            .unwrap_or_default()
            .split('.')
            .next()
            .unwrap_or_default()
            .to_string()
    };
    Some((stem(img_url), stem(sub_url)))
}

/// 从 nav data 构造 [`WbiKeys`]。
pub fn keys_from_nav(nav: &Value) -> Option<WbiKeys> {
    raw_keys_from_nav(nav).map(|(img_key, sub_key)| WbiKeys { img_key, sub_key })
}

/// 混淆：img_key + sub_key 拼接 64 字符，按表取前 32 位。
pub fn mixin_key(img_key: &str, sub_key: &str) -> String {
    let raw: String = format!("{img_key}{sub_key}");
    MIXIN_KEY_ENC_TAB
        .iter()
        .filter_map(|&i| raw.chars().nth(i))
        .take(32)
        .collect()
}

/// 参数处理：过滤 value 中的 `!'()*` 字符（B 站 web 端实现），空值参数丢弃。
fn sanitize_value(v: &str) -> String {
    v.chars()
        .filter(|c| !matches!(c, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

/// 对参数做 WBI 签名，返回追加了 `wts` 与 `w_rid` 的查询串。
pub fn sign(params: &[(String, String)], mixin: &str) -> String {
    sign_at(params, mixin, unix_now())
}

/// 指定时间戳版本（测试可复现）。
pub fn sign_at(params: &[(String, String)], mixin: &str, wts: u64) -> String {
    let mut filtered: Vec<(String, String)> = params
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.clone(), sanitize_value(v)))
        .collect();
    filtered.push(("wts".to_string(), wts.to_string()));
    filtered.sort();
    let mut pairs: Vec<String> = filtered
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect();
    let query = pairs.join("&");
    let mut hasher = Md5::new();
    hasher.update(query.as_bytes());
    hasher.update(mixin.as_bytes());
    let w_rid = super::client::hex_lower(&hasher.finalize());
    pairs.push(format!("w_rid={w_rid}"));
    pairs.join("&")
}

/// 与 JS encodeURIComponent 兼容的百分号编码（含 !'()* ）。
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 设备指纹参数（B 站 2024+ 风控必需；无 WebGL 环境的等效常量，
/// 取常见 macOS Chrome 值，依据公开 web 端行为重推）。
pub fn fingerprint_params() -> Vec<(String, String)> {
    vec![
        ("dm_img_str".to_string(), "V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ".to_string()),
        (
            "dm_cover_img_str".to_string(),
            "QU5HTEUgKEFwcGxlLCBBTkdMRSBNZXRhbCBSZW5kZXJlcjogQXBwbGUgTTIsIFVuc3BlY2lmaWVkIFZlcnNpb24pQXBwbG".to_string(),
        ),
        ("dm_img_list".to_string(), "[]".to_string()),
    ]
}

/// 客户端便捷封装：拿（缓存的）WBI key → 注入指纹参数 → 签名 → GET。
pub async fn get_with_wbi(
    client: &BiliClient,
    url: &str,
    params: Vec<(String, String)>,
) -> BiliResult<Value> {
    let keys = client.wbi_keys().await?;
    let mixin = mixin_key(&keys.img_key, &keys.sub_key);
    let mut all = params;
    all.extend(fingerprint_params());
    let signed = sign(&all, &mixin);
    // 签名串已做过 URL 编码，直接拼到 URL
    let sep = if url.contains('?') { '&' } else { '?' };
    client.get_json(&format!("{url}{sep}{signed}"), &[]).await
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixin_key_known_vector() {
        let img = "7cd084941338484aae1ad9425b84077c";
        let sub = "4932caff0ff746eab6f01bf08b70ac45";
        let mk = mixin_key(img, sub);
        assert_eq!(mk.len(), 32);
        assert_eq!(mk, "ea1db124af3c7062474693fa704f4ff8");
    }

    #[test]
    fn sign_produces_wrid() {
        let params = vec![
            ("foo".to_string(), "114".to_string()),
            ("bar".to_string(), "514".to_string()),
            ("baz".to_string(), "1919810".to_string()),
        ];
        let mk = mixin_key(
            "7cd084941338484aae1ad9425b84077c",
            "4932caff0ff746eab6f01bf08b70ac45",
        );
        let out = sign(&params, &mk);
        assert!(out.contains("w_rid="));
        assert!(out.contains("wts="));
        assert!(out.starts_with("bar=514"), "参数需按 key 排序: {out}");
    }

    #[test]
    fn sign_filters_value_special_chars_and_empty() {
        let mk = mixin_key(
            "7cd084941338484aae1ad9425b84077c",
            "4932caff0ff746eab6f01bf08b70ac45",
        );
        let out = sign_at(
            &[
                ("k".to_string(), "a!'()*b".to_string()),
                ("empty".to_string(), String::new()),
            ],
            &mk,
            1700000000,
        );
        assert!(out.contains("k=ab"), "value 应过滤 !'()*: {out}");
        assert!(!out.contains("empty="));
        assert!(out.contains("wts=1700000000"));
    }

    #[test]
    fn urlencode_special() {
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn keys_from_nav_fixture() {
        let nav = serde_json::json!({
            "wbi_img": {
                "img_url": "https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png",
                "sub_url": "https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"
            }
        });
        let keys = keys_from_nav(&nav).unwrap();
        assert_eq!(keys.img_key.len(), 32);
        assert_eq!(keys.sub_key.len(), 32);
        assert!(keys_from_nav(&serde_json::json!({})).is_none());
    }
}
