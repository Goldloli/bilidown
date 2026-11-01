//! 音频：单曲信息 / 歌单（信息 + 分页曲目）/ 播放直链（music-service-c 端点）。
//!
//! 歌单补全（change `add-download-features` D3 内容源扩展）：
//! - [`menu_info`] 取歌单元信息；
//! - [`menu_page`] 分页取曲目（供 UI「加载更多」）；
//! - [`menu_all`] 拉全所有页（[`resolve_menu`] 保持既有签名，内部改为全量）；
//! - 歌单/曲目可映射为统一 [`super::source::SourceItem`] 输出。

use serde_json::Value;

use super::client::BiliClient;
use super::error::{BiliError, BiliResult};
use super::source::{self, SourceCollection, SourceItem, SourceKind};
use super::types::{AudioEntry, AudioFormat, BiliId, Content, Entry};

const SONG_INFO_URL: &str = "https://www.bilibili.com/audio/music-service-c/web/song/info";
const MENU_INFO_URL: &str = "https://www.bilibili.com/audio/music-service-c/web/menu/info";
const MENU_SONGS_URL: &str = "https://www.bilibili.com/audio/music-service-c/web/song/of-menu";
const SONG_URL: &str = "https://www.bilibili.com/audio/music-service-c/web/url";

/// 歌单曲目分页默认页大小。
pub const MENU_PAGE_SIZE: u32 = 100;
/// 全量拉取歌单的最大页数（防御接口异常导致死循环）。
const MAX_MENU_PAGES: u32 = 100;

/// 音频直链信息。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioStream {
    pub url: String,
    pub backup_urls: Vec<String>,
    pub size: Option<u64>,
    pub format: AudioFormat,
}

impl AudioStream {
    pub fn urls(&self) -> Vec<String> {
        std::iter::once(self.url.clone())
            .chain(self.backup_urls.iter().cloned())
            .collect()
    }
}

/// 歌单元信息 + 曲目（[`menu_info`] 只返回元信息，[`menu_all`] 返回全量曲目）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioMenu {
    pub menu_id: u64,
    pub title: String,
    pub cover: Option<String>,
    pub upper: Option<String>,
    pub intro: Option<String>,
    pub songs: Vec<AudioEntry>,
}

impl AudioMenu {
    /// 歌单元信息 + 曲目 → 统一合集输出（曲目为 `SourceKind::Audio`）。
    pub fn to_collection(&self) -> SourceCollection {
        let items = self
            .songs
            .iter()
            .map(|song| {
                SourceItem::new(
                    SourceKind::Audio,
                    song.song_id.to_string(),
                    song.title.clone(),
                )
                .with_cover(song.cover.clone())
                .with_duration(song.duration)
                .with_upper((!song.upper.is_empty()).then(|| song.upper.clone()))
            })
            .collect();
        let mut collection = SourceCollection::new(
            SourceKind::AudioList,
            self.menu_id.to_string(),
            self.title.clone(),
            items,
        );
        collection.cover = self.cover.clone();
        collection.upper = self.upper.clone();
        collection.description = self.intro.clone();
        collection
    }
}

/// 歌单曲目一页（分页 UI 用）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioMenuPage {
    pub songs: Vec<AudioEntry>,
    /// 当前页码（1 基）。
    pub page: u32,
    /// 总页数（接口未给出时为 0）。
    pub page_count: u32,
    /// 曲目总数（接口未给出时为空）。
    pub total: Option<u64>,
}

/// 单曲信息（标题/作者/格式/封面/时长）。
pub async fn resolve_song(client: &BiliClient, song_id: &str) -> BiliResult<AudioEntry> {
    let data = client
        .get_json(SONG_INFO_URL, &[("sid".to_string(), song_id.to_string())])
        .await?;
    Ok(audio_entry(&data, song_id.parse().unwrap_or(0)))
}

/// 从歌曲 JSON 解析 [`AudioEntry`]（单曲信息与歌单曲目共用；纯函数）。
fn audio_entry(data: &Value, fallback_id: u64) -> AudioEntry {
    AudioEntry {
        song_id: data
            .get("id")
            .and_then(Value::as_u64)
            .unwrap_or(fallback_id),
        title: data
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        upper: data
            .pointer("/upper/username")
            .or_else(|| data.get("uname"))
            .or_else(|| data.get("author"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        format: format_from_type_field(data.get("type")),
        cover: source::url_value(data.get("cover")),
        duration: source::duration_secs(data.get("duration")),
    }
}

fn format_from_ext(ext: &str) -> AudioFormat {
    match ext.to_ascii_lowercase().as_str() {
        "flac" => AudioFormat::Flac,
        "mp3" => AudioFormat::Mp3,
        "eac3" => AudioFormat::Eac3,
        _ => AudioFormat::M4a,
    }
}

/// 接口 `type` 字段解析：旧接口给扩展名字符串，新接口给数字枚举
/// （0→128k m4a、1→192k m4a、2→其它 m4a、3→FLAC/Hi-Res）。
fn format_from_type_field(value: Option<&Value>) -> AudioFormat {
    match value {
        Some(Value::String(ext)) => format_from_ext(ext),
        Some(Value::Number(n)) => match n.as_u64() {
            Some(3) => AudioFormat::Flac,
            _ => AudioFormat::M4a,
        },
        _ => AudioFormat::M4a,
    }
}

/// 歌单元信息（不含曲目）。
pub async fn menu_info(client: &BiliClient, menu_id: u64) -> BiliResult<AudioMenu> {
    let data = client
        .get_json(MENU_INFO_URL, &[("sid".to_string(), menu_id.to_string())])
        .await?;
    Ok(parse_menu_info(&data, menu_id))
}

/// 从 `menu/info` 的 `data` 解析元信息（纯函数）。
pub fn parse_menu_info(data: &Value, menu_id: u64) -> AudioMenu {
    AudioMenu {
        menu_id: data
            .get("menuId")
            .and_then(Value::as_u64)
            .unwrap_or(menu_id),
        title: data
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cover: source::url_value(data.get("cover")),
        upper: data
            .get("uname")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from),
        intro: data
            .get("intro")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from),
        songs: Vec::new(),
    }
}

/// 从 `song/of-menu` 的 `data` 解析一页曲目（纯函数）。
pub fn parse_menu_songs(data: &Value, page: u32) -> AudioMenuPage {
    let songs: Vec<AudioEntry> = data
        .get("data")
        .or_else(|| data.get("list"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|raw| {
            let entry = audio_entry(raw, 0);
            (!entry.title.is_empty()).then_some(entry)
        })
        .collect();
    let page_count = data.get("pageCount").and_then(Value::as_u64).unwrap_or(0) as u32;
    let total = data.get("totalSize").and_then(Value::as_u64);
    AudioMenuPage {
        songs,
        page,
        page_count,
        total,
    }
}

/// 拉取歌单曲目一页（`page` 1 基）。
pub async fn menu_page(
    client: &BiliClient,
    menu_id: u64,
    page: u32,
    ps: u32,
) -> BiliResult<AudioMenuPage> {
    let data = client
        .get_json(
            MENU_SONGS_URL,
            &[
                ("sid".to_string(), menu_id.to_string()),
                ("pn".to_string(), page.to_string()),
                ("ps".to_string(), ps.to_string()),
            ],
        )
        .await?;
    Ok(parse_menu_songs(&data, page))
}

/// 拉全歌单（元信息 + 所有曲目；每页间隔 [`source::PAGE_THROTTLE`]）。
pub async fn menu_all(client: &BiliClient, menu_id: u64) -> BiliResult<AudioMenu> {
    let mut menu = menu_info(client, menu_id).await?;
    let mut page = 1u32;
    loop {
        let result = menu_page(client, menu_id, page, MENU_PAGE_SIZE).await?;
        let empty = result.songs.is_empty();
        menu.songs.extend(result.songs);
        // page_count == 0 时以「本页为空」为结束条件
        if empty || (result.page_count > 0 && page >= result.page_count) || page >= MAX_MENU_PAGES {
            break;
        }
        page += 1;
        tokio::time::sleep(source::PAGE_THROTTLE).await;
    }
    Ok(menu)
}

/// 歌单 → Content（entries 复用视频条目模型：cid 用 song_id 占位）。
pub async fn resolve_menu(client: &BiliClient, menu_id: &str) -> BiliResult<Content> {
    let menu_id: u64 = menu_id.parse().map_err(|_| BiliError::InvalidInput)?;
    let menu = menu_all(client, menu_id).await?;
    if menu.songs.is_empty() {
        return Err(BiliError::Other("audio menu empty or invisible".into()));
    }
    let mut entries = Vec::new();
    for song in &menu.songs {
        entries.push(Entry {
            cid: song.song_id,
            title: song.title.clone(),
            index: entries.len(),
            ep_id: None,
            season_id: None,
            aid: None,
            duration_secs: song.duration,
            qualities: Vec::new(),
        });
    }
    let total: u64 = menu
        .songs
        .iter()
        .map(|song| song.duration.unwrap_or(0))
        .sum();
    Ok(Content {
        id: BiliId::AudioList {
            menu_id: menu_id.to_string(),
        },
        title: menu.title,
        cover_url: menu.cover,
        upper: menu.upper.unwrap_or_default(),
        entries,
        duration_secs: (total > 0).then_some(total),
        pub_time: None,
    })
}

/// 播放直链（含备用 URL / 大小 / 格式）。
pub async fn play_url(client: &BiliClient, song_id: &str) -> BiliResult<AudioStream> {
    #[derive(serde::Deserialize)]
    struct UrlResp {
        code: i64,
        #[serde(default)]
        message: String,
        data: Option<UrlData>,
    }
    #[derive(serde::Deserialize)]
    struct UrlData {
        /// 可能是扩展名字符串，也可能是数字枚举（0/1/2/3）。
        #[serde(default)]
        r#type: Value,
        #[serde(default)]
        size: u64,
        #[serde(default)]
        url: String,
        #[serde(default)]
        backup_url: Option<Value>,
    }
    let resp = client
        .http()
        .get(SONG_URL)
        .header("Referer", super::client::REFERER)
        .query(&[("sid", song_id), ("privilege", "2"), ("quality", "0")])
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(BiliError::Network(format!("HTTP {status}")));
    }
    let body: UrlResp = resp.json().await?;
    if body.code != 0 {
        return Err(BiliError::from_api(body.code, body.message));
    }
    let d = body
        .data
        .ok_or_else(|| BiliError::Parse("audio url 响应缺 data".into()))?;
    if d.url.is_empty() {
        return Err(BiliError::from_api(4511001, "audio url empty"));
    }
    let backups = match d.backup_url {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(Value::String(s)) if !s.is_empty() => s.split(',').map(String::from).collect(),
        _ => Vec::new(),
    };
    Ok(AudioStream {
        url: d.url,
        backup_urls: backups,
        size: (d.size > 0).then_some(d.size),
        format: format_from_type_field(Some(&d.r#type)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_mapping() {
        assert_eq!(format_from_ext("flac"), AudioFormat::Flac);
        assert_eq!(format_from_ext("mp3"), AudioFormat::Mp3);
        assert_eq!(format_from_ext(""), AudioFormat::M4a);
        assert_eq!(format_from_ext("m4a"), AudioFormat::M4a);
        // 数字枚举：3 = FLAC/Hi-Res，其余 m4a
        assert_eq!(
            format_from_type_field(Some(&serde_json::json!(3))),
            AudioFormat::Flac
        );
        assert_eq!(
            format_from_type_field(Some(&serde_json::json!(1))),
            AudioFormat::M4a
        );
        assert_eq!(format_from_type_field(None), AudioFormat::M4a);
    }

    #[test]
    fn stream_urls_order() {
        let s = AudioStream {
            url: "https://cdn/a".into(),
            backup_urls: vec!["https://cdn/b".into(), "https://cdn/c".into()],
            size: Some(1),
            format: AudioFormat::M4a,
        };
        assert_eq!(s.urls().len(), 3);
        assert_eq!(s.urls()[0], "https://cdn/a");
    }

    #[test]
    fn backup_shapes() {
        let array = serde_json::json!(["https://cdn/b.mp3", "https://cdn/c.mp3"]);
        let from_array: Vec<String> = match Some(array) {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => vec![],
        };
        assert_eq!(from_array.len(), 2);
    }

    fn menu_info_fixture() -> Value {
        serde_json::json!({
            "menuId": 106240,
            "title": "测试歌单",
            "cover": "//i0.hdslb.com/bfs/music/menu.jpg",
            "uname": "UP甲",
            "intro": "歌单简介",
            "statistic": {"play": 100}
        })
    }

    fn menu_songs_fixture() -> Value {
        serde_json::json!({
            "curPage": 1,
            "pageCount": 2,
            "totalSize": 3,
            "pageSize": 2,
            "data": [
                {
                    "id": 912772,
                    "uid": 9,
                    "uname": "UP甲",
                    "title": "歌曲一",
                    "cover": "//i0.hdslb.com/bfs/music/1.jpg",
                    "duration": 180,
                    "type": 3
                },
                {
                    "id": 912773,
                    "uname": "UP甲",
                    "title": "歌曲二",
                    "cover": "",
                    "duration": 0
                },
                {"id": 912774, "title": "", "duration": 60}
            ]
        })
    }

    #[test]
    fn parse_menu_info_fixture() {
        let menu = parse_menu_info(&menu_info_fixture(), 106240);
        assert_eq!(menu.menu_id, 106240);
        assert_eq!(menu.title, "测试歌单");
        assert_eq!(
            menu.cover.as_deref(),
            Some("https://i0.hdslb.com/bfs/music/menu.jpg")
        );
        assert_eq!(menu.upper.as_deref(), Some("UP甲"));
        assert_eq!(menu.intro.as_deref(), Some("歌单简介"));
        assert!(menu.songs.is_empty());
    }

    #[test]
    fn parse_menu_songs_fixture() {
        let page = parse_menu_songs(&menu_songs_fixture(), 1);
        assert_eq!(page.songs.len(), 2, "空标题曲目跳过");
        assert_eq!(page.page, 1);
        assert_eq!(page.page_count, 2);
        assert_eq!(page.total, Some(3));
        assert_eq!(page.songs[0].song_id, 912772);
        assert_eq!(
            page.songs[0].cover.as_deref(),
            Some("https://i0.hdslb.com/bfs/music/1.jpg")
        );
        assert_eq!(page.songs[0].duration, Some(180));
        assert_eq!(page.songs[0].format, AudioFormat::Flac);
        assert_eq!(page.songs[1].duration, None);
    }

    #[test]
    fn menu_collection_output() {
        let mut menu = parse_menu_info(&menu_info_fixture(), 106240);
        menu.songs = parse_menu_songs(&menu_songs_fixture(), 1).songs;
        let collection = menu.to_collection();
        assert_eq!(collection.kind, SourceKind::AudioList);
        assert_eq!(collection.id, "106240");
        assert_eq!(collection.items.len(), 2);
        assert_eq!(collection.items[0].kind, SourceKind::Audio);
        assert_eq!(collection.items[0].bili_id, "912772");
        assert_eq!(collection.items[0].upper.as_deref(), Some("UP甲"));
        assert_eq!(
            collection.items[0].to_bili_id(),
            Some(BiliId::Audio {
                song_id: "912772".into()
            })
        );
    }

    #[test]
    fn single_song_entry_fixture() {
        let data = serde_json::json!({
            "id": 912772,
            "title": "单曲",
            "upper": {"username": "作者乙"},
            "cover": "https://i0.hdslb.com/bfs/music/song.jpg",
            "duration": 61,
            "type": "flac"
        });
        let entry = audio_entry(&data, 0);
        assert_eq!(entry.song_id, 912772);
        assert_eq!(entry.title, "单曲");
        assert_eq!(entry.upper, "作者乙");
        assert_eq!(entry.format, AudioFormat::Flac);
        assert_eq!(entry.duration, Some(61));
        assert_eq!(
            entry.cover.as_deref(),
            Some("https://i0.hdslb.com/bfs/music/song.jpg")
        );
    }
}
