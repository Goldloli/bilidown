//! 字幕：`/x/player/wbi/v2` 取字幕列表 → 拉取字幕 JSON → SRT（change add-download-features design D2）。
//!
//! 行为约定：
//! - 无字幕/未登录/接口失败返回空列表或错误，由任务层 warn 记录，**不使主任务失败**；
//! - 字幕 URL 可能是协议相对 `//host/path`，统一补 `https:`；
//! - 多语言分别保存为 `{视频文件名去扩展}.{lan}.srt`，时间格式 `HH:MM:SS,mmm`。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::BiliClient;
use super::error::{BiliError, BiliResult};
use super::wbi;
use crate::media::filename;

/// player/wbi/v2 端点（WBI 签名）。
pub const PLAYER_INFO_URL: &str = "https://api.bilibili.com/x/player/wbi/v2";

/// 一条可用字幕轨。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTrack {
    /// B 站语言代码（如 `zh-CN`、`ai-zh`、`en-US`）。
    pub lan: String,
    /// 语言显示名（如「中文（中国）」，前端展示用）。
    pub lan_doc: String,
    /// 字幕 JSON 直链（已规范化 `https:`）。
    pub subtitle_url: String,
}

/// 字幕的一条 cue（毫秒）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub from_ms: u64,
    pub to_ms: u64,
    pub content: String,
}

/// 从 player/wbi/v2 的 `data` 解析字幕轨列表（纯函数，fixture 可测）。
pub fn parse_player_subtitles(data: &Value) -> Vec<SubtitleTrack> {
    let Some(list) = data
        .pointer("/subtitle/subtitles")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out: Vec<SubtitleTrack> = Vec::new();
    for item in list {
        let Some(url) = item
            .get("subtitle_url")
            .and_then(Value::as_str)
            .and_then(normalize_subtitle_url)
        else {
            continue;
        };
        let lan = item
            .get("lan")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("und")
            .to_string();
        if out.iter().any(|t| t.lan == lan) {
            continue; // 同语言去重（接口偶有重复轨）
        }
        let lan_doc = item
            .get("lan_doc")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        out.push(SubtitleTrack {
            lan,
            lan_doc,
            subtitle_url: url,
        });
    }
    out
}

/// 规范化字幕 URL：`//host/path` → `https://host/path`；空/非 http(s) 返回 None。
pub fn normalize_subtitle_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(url.to_string());
    }
    None
}

/// 解析字幕 JSON（`{ "body": [{ "from": 0.0, "to": 2.5, "content": "..." }] }`）。
///
/// 容错：from/to 支持数字或字符串；缺 content/非法时间/空文本的条目跳过。
pub fn parse_subtitle_json(json: &str) -> BiliResult<Vec<SubtitleCue>> {
    let value: Value = serde_json::from_str(json)?;
    let body = value
        .get("body")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in &body {
        let (Some(from), Some(to)) = (seconds_of(item.get("from")), seconds_of(item.get("to")))
        else {
            continue;
        };
        let content = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if content.is_empty() {
            continue;
        }
        let from_ms = secs_to_ms(from);
        let to_ms = secs_to_ms(to).max(from_ms);
        out.push(SubtitleCue {
            from_ms,
            to_ms,
            content,
        });
    }
    Ok(out)
}

fn seconds_of(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn secs_to_ms(secs: f64) -> u64 {
    if !secs.is_finite() || secs <= 0.0 {
        0
    } else {
        (secs * 1000.0).round() as u64
    }
}

/// 毫秒 → SRT 时间戳 `HH:MM:SS,mmm`。
pub fn format_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// cue 列表 → SRT 文本（末尾保留一个空行）。
pub fn cues_to_srt(cues: &[SubtitleCue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_timestamp(cue.from_ms),
            format_timestamp(cue.to_ms),
            cue.content
        ));
    }
    out
}

/// 取某条目（aid + cid）的字幕轨列表。
pub async fn player_subtitles(
    client: &BiliClient,
    aid: u64,
    cid: u64,
) -> BiliResult<Vec<SubtitleTrack>> {
    let data = wbi::get_with_wbi(
        client,
        PLAYER_INFO_URL,
        vec![
            ("aid".to_string(), aid.to_string()),
            ("cid".to_string(), cid.to_string()),
        ],
    )
    .await?;
    Ok(parse_player_subtitles(&data))
}

/// 下载单条字幕并转换为 SRT 文本。
pub async fn download_srt(client: &BiliClient, track: &SubtitleTrack) -> BiliResult<String> {
    let url = normalize_subtitle_url(&track.subtitle_url)
        .ok_or_else(|| BiliError::Parse(format!("invalid subtitle url: {}", track.subtitle_url)))?;
    let text = client.get_text(&url).await?;
    let cues = parse_subtitle_json(&text)?;
    if cues.is_empty() {
        return Err(BiliError::Parse("subtitle body empty".into()));
    }
    Ok(cues_to_srt(&cues))
}

/// 字幕文件名：`{视频文件名去扩展}.{lan}.srt`。
pub fn srt_filename(video_stem: &str, lan: &str) -> String {
    let stem = filename::sanitize_component(video_stem);
    let lan = filename::sanitize_component(lan);
    format!("{stem}.{lan}.srt")
}

/// 批量下载并保存字幕到视频同目录（`dedupe_in_dir` 防覆盖）。
///
/// 返回 `(保存路径列表, 逐语言错误)`；无字幕轨时返回空结果且不发请求。
pub async fn save_subtitles(
    client: &BiliClient,
    tracks: &[SubtitleTrack],
    video_path: &Path,
    out_dir: &Path,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut saved = Vec::new();
    let mut errors = Vec::new();
    let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
        return (saved, vec!["invalid video path".to_string()]);
    };
    for track in tracks {
        if cancel.is_some_and(|c| c.is_cancelled()) {
            errors.push("cancelled before subtitle download".into());
            break;
        }
        match download_srt(client, track).await {
            Ok(srt) => {
                let path = filename::dedupe_in_dir(out_dir, &srt_filename(stem, &track.lan));
                match tokio::fs::write(&path, srt.as_bytes()).await {
                    Ok(()) => saved.push(path),
                    Err(e) => errors.push(format!("{}: {e}", track.lan)),
                }
            }
            Err(e) => errors.push(format!("{}: {}", track.lan, e.describe())),
        }
    }
    (saved, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_urls() {
        assert_eq!(
            normalize_subtitle_url("//aisubtitle.hdslb.com/bfs/ai_subtitle/prod/x.json"),
            Some("https://aisubtitle.hdslb.com/bfs/ai_subtitle/prod/x.json".to_string())
        );
        assert_eq!(
            normalize_subtitle_url("https://i0.hdslb.com/a.json"),
            Some("https://i0.hdslb.com/a.json".to_string())
        );
        assert_eq!(normalize_subtitle_url("  "), None);
        assert_eq!(normalize_subtitle_url("ftp://x/a.json"), None);
    }

    #[test]
    fn parse_player_fixture_filters_and_dedupes() {
        let data = serde_json::json!({
            "aid": 170001,
            "cid": 279786,
            "subtitle": {
                "subtitles": [
                    {"lan": "zh-CN", "lan_doc": "中文（中国）", "subtitle_url": "//aisubtitle.hdslb.com/x-zh.json"},
                    {"lan": "zh-CN", "lan_doc": "中文（中国）", "subtitle_url": "//aisubtitle.hdslb.com/x-zh-dup.json"},
                    {"lan": "ai-zh", "lan_doc": "中文（自动生成）", "subtitle_url": "https://aisubtitle.hdslb.com/x-ai.json"},
                    {"lan": "en-US", "lan_doc": "English", "subtitle_url": ""},
                    {"lan": "", "lan_doc": "unknown", "subtitle_url": "//aisubtitle.hdslb.com/x-und.json"}
                ]
            }
        });
        let tracks = parse_player_subtitles(&data);
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].lan, "zh-CN");
        assert!(tracks[0].subtitle_url.starts_with("https://"));
        assert_eq!(tracks[1].lan, "ai-zh");
        assert_eq!(tracks[2].lan, "und", "空 lan 回退 und");
    }

    #[test]
    fn no_subtitles_returns_empty() {
        assert!(parse_player_subtitles(&serde_json::json!({})).is_empty());
        assert!(
            parse_player_subtitles(&serde_json::json!({"subtitle": {"subtitles": []}})).is_empty()
        );
        assert!(parse_player_subtitles(&serde_json::json!({"subtitle": null})).is_empty());
    }

    #[test]
    fn parse_subtitle_body_fixture() {
        let json = r#"{
            "body": [
                {"from": 0.0, "to": 2.5, "content": "第一句"},
                {"from": 2.5, "to": 4.0, "content": "第二句\n第二行"},
                {"from": "4.0", "to": "5.234", "content": "字符串时间"},
                {"from": 6.0, "to": 7.0, "content": "   "},
                {"from": null, "to": 8.0, "content": "非法时间"}
            ]
        }"#;
        let cues = parse_subtitle_json(json).unwrap();
        assert_eq!(cues.len(), 3);
        assert_eq!(
            cues[0],
            SubtitleCue {
                from_ms: 0,
                to_ms: 2500,
                content: "第一句".into()
            }
        );
        assert_eq!(cues[1].from_ms, 2500);
        assert_eq!(cues[2].from_ms, 4000);
        assert_eq!(cues[2].to_ms, 5234);
    }

    #[test]
    fn srt_formatting() {
        assert_eq!(format_timestamp(0), "00:00:00,000");
        assert_eq!(format_timestamp(1_234), "00:00:01,234");
        assert_eq!(format_timestamp(3_599_999), "00:59:59,999");
        assert_eq!(format_timestamp(3_600_000), "01:00:00,000");
        assert_eq!(format_timestamp(366_1234), "01:01:01,234");
        let cues = vec![
            SubtitleCue {
                from_ms: 0,
                to_ms: 2500,
                content: "第一句".into(),
            },
            SubtitleCue {
                from_ms: 2500,
                to_ms: 4000,
                content: "第二句\n第二行".into(),
            },
        ];
        let srt = cues_to_srt(&cues);
        assert_eq!(
            srt,
            "1\n00:00:00,000 --> 00:00:02,500\n第一句\n\n2\n00:00:02,500 --> 00:00:04,000\n第二句\n第二行\n\n"
        );
    }

    #[test]
    fn bad_json_is_error_but_parsed_by_caller() {
        assert!(parse_subtitle_json("not json").is_err());
        // 无 body 字段 → 空 cue（调用方视为无字幕）
        assert!(parse_subtitle_json("{}").unwrap().is_empty());
    }

    #[test]
    fn srt_filename_shape() {
        assert_eq!(
            srt_filename("视频[1080P]", "zh-CN"),
            "视频[1080P].zh-CN.srt"
        );
        assert_eq!(
            srt_filename("视频[1080P]", "ai/zh"),
            "视频[1080P].ai zh.srt"
        );
    }

    #[tokio::test]
    async fn save_subtitles_empty_tracks_noop() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("视频[1080P].mp4");
        std::fs::write(&video, b"x").unwrap();
        let client = BiliClient::new(None, Default::default()).unwrap();
        let (saved, errors) = save_subtitles(&client, &[], &video, dir.path(), None).await;
        assert!(saved.is_empty());
        assert!(errors.is_empty());
    }

    fn test_client() -> (BiliClient, reqwest::Client) {
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        (
            BiliClient::with_http(http.clone(), Default::default()),
            http,
        )
    }

    #[tokio::test]
    async fn download_and_save_with_wiremock() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sub-zh.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"body":[{"from":0.0,"to":1.5,"content":"你好"},{"from":1.5,"to":3.0,"content":"世界"}]}"#,
            ))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("标题[1080P].mp4");
        std::fs::write(&video, b"x").unwrap();
        let (client, _http) = test_client();
        let track = SubtitleTrack {
            lan: "zh-CN".into(),
            lan_doc: "中文".into(),
            subtitle_url: format!("{}/sub-zh.json", server.uri()),
        };
        let (saved, errors) = save_subtitles(
            &client,
            std::slice::from_ref(&track),
            &video,
            dir.path(),
            None,
        )
        .await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(saved.len(), 1);
        assert_eq!(
            saved[0].file_name().unwrap().to_str().unwrap(),
            "标题[1080P].zh-CN.srt"
        );
        let text = std::fs::read_to_string(&saved[0]).unwrap();
        assert!(text.starts_with("1\n00:00:00,000 --> 00:00:01,500\n你好\n\n"));
        // 重复下载不覆盖：追加 (1)
        let (saved2, _) = save_subtitles(&client, &[track], &video, dir.path(), None).await;
        assert_eq!(
            saved2[0].file_name().unwrap().to_str().unwrap(),
            "标题[1080P].zh-CN (1).srt"
        );
    }

    #[tokio::test]
    async fn subtitle_fetch_failure_is_reported_not_panicked() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bad.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("v.mp4");
        let (client, _http) = test_client();
        let track = SubtitleTrack {
            lan: "en-US".into(),
            lan_doc: "English".into(),
            subtitle_url: format!("{}/bad.json", server.uri()),
        };
        let (saved, errors) = save_subtitles(&client, &[track], &video, dir.path(), None).await;
        assert!(saved.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("en-US"));
    }
}
