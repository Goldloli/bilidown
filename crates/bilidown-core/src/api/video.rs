//! 视频类内容：UGC view / PGC 番剧 / PUGV 课程 的信息与 playurl 解析。
//!
//! playurl 解析要点（tasks 3.7）：
//! - 真实档位：只展示接口实际返回的 DASH 流，不虚构档位；
//! - 编码：同档位多种 codecid（AVC/HEVC/AV1）全部保留，供前端枚举与回退；
//! - 大会员档：`accept_quality` 宣告但 DASH 未返回的档位标记不可用原因；
//! - 音质：dash.audio + dolby.audio + flac.audio 合并并按音质排序；
//! - durl/FLV 兜底为音视频一体的单流。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::BiliClient;
use super::error::{BiliError, BiliResult};
use super::source::{self, SourceCollection, SourceItem, SourceKind};
use super::types::{
    AudioFormat, AudioQuality, AudioTrack, BiliId, Codec, Content, Entry, MediaStream, PlayUrl,
    Quality, QualityTier, TierUnavailable, VideoQuality,
};
use super::wbi;

/// 解析内容信息（标题/封面/条目）。视频/番剧/课程三类走这里。
pub async fn resolve_content(client: &BiliClient, id: &BiliId) -> BiliResult<Content> {
    match id {
        BiliId::Video { bvid } => resolve_ugc(client, bvid).await,
        BiliId::Bangumi { id } => resolve_bangumi(client, id).await,
        BiliId::Cheese { id } => resolve_cheese(client, id).await,
        BiliId::Audio { .. } | BiliId::AudioList { .. } => Err(
            BiliError::Other(format!("resolve via dedicated module: {}", id.kind_name())),
        ),
    }
}

fn duration_from_json(v: Option<&Value>) -> Option<u64> {
    let n = v?.as_u64()?;
    if n == 0 {
        None
    } else if n > 100_000 {
        // PGC 剧集时长单位是毫秒
        Some(n / 1000)
    } else {
        Some(n)
    }
}

/// UGC：`/x/web-interface/view`。
async fn resolve_ugc(client: &BiliClient, bvid: &str) -> BiliResult<Content> {
    let data = fetch_view(client, bvid).await?;
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cover = data.get("pic").and_then(Value::as_str).map(String::from);
    let upper = data
        .pointer("/owner/name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let pub_time = data.get("pubdate").and_then(Value::as_i64);
    // UGC 字幕接口需要 aid（player/wbi/v2?aid&cid）；分 P 共用稿件 aid
    let aid = data.get("aid").and_then(Value::as_u64);
    let pages = data
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut entries = Vec::new();
    if pages.len() > 1 {
        for (i, p) in pages.iter().enumerate() {
            entries.push(Entry {
                cid: p.get("cid").and_then(Value::as_u64).unwrap_or(0),
                title: p
                    .get("part")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("P?")
                    .to_string(),
                index: i,
                ep_id: None,
                season_id: None,
                aid,
                duration_secs: duration_from_json(p.get("duration")),
                qualities: Vec::new(),
            });
        }
    } else {
        entries.push(Entry {
            cid: data.get("cid").and_then(Value::as_u64).unwrap_or(0),
            title: data
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            index: 0,
            ep_id: None,
            season_id: None,
            aid,
            duration_secs: duration_from_json(data.get("duration")),
            qualities: Vec::new(),
        });
    }
    let duration_secs = sum_duration(&entries).or_else(|| duration_from_json(data.get("duration")));
    Ok(Content {
        id: BiliId::Video {
            bvid: data
                .get("bvid")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| bvid.to_string()),
        },
        title,
        cover_url: cover,
        upper,
        entries,
        duration_secs,
        pub_time,
    })
}

fn sum_duration(entries: &[Entry]) -> Option<u64> {
    if entries.iter().all(|e| e.duration_secs.is_some()) && !entries.is_empty() {
        Some(entries.iter().filter_map(|e| e.duration_secs).sum())
    } else {
        None
    }
}

/// 请求 UGC `view` 接口并解包 `data`（单视频内容与合集解析共用同一次响应形状）。
async fn fetch_view(client: &BiliClient, bvid: &str) -> BiliResult<Value> {
    let query = if bvid.starts_with("av") {
        vec![("aid".to_string(), bvid.trim_start_matches("av").to_string())]
    } else {
        vec![("bvid".to_string(), bvid.to_string())]
    };
    let data = client
        .get_json("https://api.bilibili.com/x/web-interface/view", &query)
        .await?;
    if data.is_null() {
        return Err(BiliError::Parse("view 响应缺少 data".into()));
    }
    Ok(data)
}

/// UGC 合集中的一个分节（`ugc_season.sections[]`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UgcSection {
    pub id: u64,
    pub title: String,
    /// 在合集 `sections` 中的下标。
    pub index: usize,
}

/// UGC 合集中的一个可下载条目（`sections[].episodes[]`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UgcEpisode {
    pub bvid: String,
    pub aid: Option<u64>,
    /// 该条目自身的 cid（合集内不同稿件的 cid 不同，入队时由管线按 bvid 重新解析）。
    pub cid: u64,
    pub title: String,
    pub cover: Option<String>,
    /// 时长（秒）。
    pub duration: Option<u64>,
    /// 所属分节下标/标题（无分节标题时为空串）。
    pub section_index: usize,
    pub section_title: String,
    /// 在合集全部条目中的下标（跨分节连续编号）。
    pub index: usize,
    /// 是否为发起解析的当前视频。
    pub is_current: bool,
}

/// UGC 合集元信息 + 可下载条目。
///
/// 由 [`resolve_ugc_season`] 返回；壳层据此弹「仅当前视频 / 整个合集」确认框，
/// 选择整个合集时把 [`Self::to_source_items`] 交给现有确认面板/任务队列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UgcSeason {
    pub season_id: u64,
    pub title: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub upper: String,
    pub sections: Vec<UgcSection>,
    pub episodes: Vec<UgcEpisode>,
    /// 发起解析的当前视频 bvid（view 返回的规范 bvid）。
    pub current_bvid: String,
}

impl UgcSeason {
    /// 当前视频在 [`Self::episodes`] 中的下标（未找到为 `None`）。
    pub fn current_index(&self) -> Option<usize> {
        self.episodes.iter().position(|ep| ep.is_current)
    }

    /// 展开为统一列表条目（content-sources 的 `SourceItem`）。
    pub fn to_source_items(&self) -> Vec<SourceItem> {
        let upper = (!self.upper.is_empty()).then(|| self.upper.clone());
        self.episodes
            .iter()
            .map(|ep| {
                SourceItem::new(SourceKind::Video, ep.bvid.clone(), ep.title.clone())
                    .with_cover(ep.cover.clone())
                    .with_duration(ep.duration)
                    .with_upper(upper.clone())
            })
            .collect()
    }

    /// 合集元信息 + 条目（统一输出，供壳层 DTO/UI 直接消费）。
    pub fn to_collection(&self) -> SourceCollection {
        let mut collection = SourceCollection::new(
            SourceKind::Video,
            self.season_id.to_string(),
            self.title.clone(),
            self.to_source_items(),
        );
        collection.cover = self.cover_url.clone();
        collection.upper = (!self.upper.is_empty()).then(|| self.upper.clone());
        collection.description = self.intro.clone();
        collection
    }
}

/// 解析视频所属的 UGC 合集（`ugc_season`）。
///
/// - 不是合集（无 `ugc_season`）：`Ok(None)`，壳层按现有单视频行为处理；
/// - 是合集但 `sections`/`episodes` 为空：`Ok(Some(...))` 且 `episodes` 为空，
///   壳层保留「仅当前视频」入口、不产生空任务；
/// - 现有 [`resolve_content`] 对视频的行为不变（仍只按 `pages` 构造单视频条目）。
pub async fn resolve_ugc_season(client: &BiliClient, bvid: &str) -> BiliResult<Option<UgcSeason>> {
    let data = fetch_view(client, bvid).await?;
    Ok(parse_ugc_season(&data))
}

/// 从 view 响应 `data` 解析 UGC 合集（纯函数，fixture 可测）。
pub fn parse_ugc_season(view: &Value) -> Option<UgcSeason> {
    let season = view.get("ugc_season").filter(|v| v.is_object())?;
    let title = season
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cover_url = source::url_value(season.get("cover"));
    let intro = season
        .get("intro")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let upper = view
        .pointer("/owner/name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let current_bvid = view
        .get("bvid")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut sections = Vec::new();
    let mut episodes: Vec<UgcEpisode> = Vec::new();
    let raw_sections = season
        .get("sections")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (si, section) in raw_sections.iter().enumerate() {
        let section_title = section
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        sections.push(UgcSection {
            id: section.get("id").and_then(Value::as_u64).unwrap_or(0),
            title: section_title.clone(),
            index: si,
        });
        let raw_episodes = section
            .get("episodes")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for ep in raw_episodes {
            let Some(bvid) = ep
                .get("bvid")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let title = ep
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| ep.pointer("/arc/title").and_then(Value::as_str))
                .unwrap_or("?")
                .to_string();
            let cover = source::url_value(ep.pointer("/arc/pic"));
            let duration = duration_from_json(ep.pointer("/arc/duration"))
                .or_else(|| duration_from_json(ep.pointer("/page/duration")));
            let cid = ep
                .get("cid")
                .and_then(Value::as_u64)
                .or_else(|| ep.pointer("/page/cid").and_then(Value::as_u64))
                .unwrap_or(0);
            let index = episodes.len();
            episodes.push(UgcEpisode {
                bvid: bvid.to_string(),
                aid: ep
                    .get("aid")
                    .and_then(Value::as_u64)
                    .or_else(|| ep.pointer("/arc/aid").and_then(Value::as_u64)),
                cid,
                title,
                cover,
                duration,
                section_index: si,
                section_title: section_title.clone(),
                index,
                is_current: bvid == current_bvid,
            });
        }
    }
    Some(UgcSeason {
        season_id: season.get("id").and_then(Value::as_u64).unwrap_or(0),
        title,
        cover_url,
        intro,
        upper,
        sections,
        episodes,
        current_bvid,
    })
}

/// PGC：ep/ss/md → season 信息 + 剧集列表。
///
/// 接口返回 `{code, result: {title, season_id, episodes[]}}`（result 由 client 层解包）。
async fn resolve_bangumi(client: &BiliClient, id: &str) -> BiliResult<Content> {
    let url = if let Some(n) = id.strip_prefix("md") {
        // md → media_id 换 season_id（/pgc/review/user）
        let media = client
            .get_json(
                "https://api.bilibili.com/pgc/review/user",
                &[("media_id".to_string(), n.to_string())],
            )
            .await?;
        let sid = media
            .pointer("/media/season_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| BiliError::Parse("md 未能换成 season_id".into()))?;
        format!("https://api.bilibili.com/pgc/view/web/season?season_id={sid}")
    } else {
        let (k, v) = if let Some(n) = id.strip_prefix("ep") {
            ("ep_id", n.to_string())
        } else if let Some(n) = id.strip_prefix("ss") {
            ("season_id", n.to_string())
        } else {
            return Err(BiliError::InvalidInput);
        };
        format!("https://api.bilibili.com/pgc/view/web/season?{k}={v}")
    };
    let data = client.get_json(&url, &[]).await?;
    if data.is_null() {
        return Err(BiliError::Parse("番剧 season 响应为空".into()));
    }
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cover = data.get("cover").and_then(Value::as_str).map(String::from);
    let upper = data
        .pointer("/up_info/uname")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let season_id = data.get("season_id").and_then(Value::as_u64);
    let mut entries = Vec::new();
    for ep in data
        .get("episodes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // status == 13 为未上线/预告
        if ep.get("status").and_then(Value::as_i64) == Some(13) {
            continue;
        }
        let long_title = ep
            .get("long_title")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| ep.get("title").and_then(Value::as_str))
            .unwrap_or("?");
        entries.push(Entry {
            cid: ep.get("cid").and_then(Value::as_u64).unwrap_or(0),
            title: long_title.to_string(),
            index: entries.len(),
            ep_id: ep
                .get("id")
                .and_then(Value::as_u64)
                .map(|n| format!("ep{n}")),
            season_id,
            aid: ep.get("aid").and_then(Value::as_u64),
            duration_secs: duration_from_json(ep.get("duration")),
            qualities: Vec::new(),
        });
    }
    if entries.is_empty() {
        return Err(BiliError::Other(
            "no downloadable episode in this season".into(),
        ));
    }
    let duration_secs = sum_duration(&entries);
    Ok(Content {
        id: BiliId::Bangumi { id: id.to_string() },
        title,
        cover_url: cover,
        upper,
        entries,
        duration_secs,
        pub_time: None,
    })
}

/// PUGV 课程。接口同时支持 `ep_id` 与 `season_id`。
async fn resolve_cheese(client: &BiliClient, id: &str) -> BiliResult<Content> {
    let (key, val) = if let Some(n) = id.strip_prefix("ss") {
        ("season_id", n.to_string())
    } else {
        ("ep_id", id.trim_start_matches("ep").to_string())
    };
    let data = client
        .get_json(
            "https://api.bilibili.com/pugv/view/web/season",
            &[(key.to_string(), val)],
        )
        .await?;
    if data.is_null() {
        return Err(BiliError::Parse("课程 season 响应为空".into()));
    }
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cover = data.get("cover").and_then(Value::as_str).map(String::from);
    let upper = data
        .pointer("/up_info/uname")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let season_id = data.get("season_id").and_then(Value::as_u64);
    let mut entries = Vec::new();
    for ep in data
        .get("episodes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        entries.push(Entry {
            cid: ep.get("cid").and_then(Value::as_u64).unwrap_or(0),
            title: ep
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string(),
            index: entries.len(),
            ep_id: ep
                .get("id")
                .and_then(Value::as_u64)
                .map(|n| format!("ep{n}")),
            season_id,
            aid: ep.get("aid").and_then(Value::as_u64),
            duration_secs: duration_from_json(ep.get("duration")),
            qualities: Vec::new(),
        });
    }
    if entries.is_empty() {
        return Err(BiliError::Other(
            "no downloadable episode in this course".into(),
        ));
    }
    let duration_secs = sum_duration(&entries);
    Ok(Content {
        id: BiliId::Cheese { id: id.to_string() },
        title,
        cover_url: cover,
        upper,
        entries,
        duration_secs,
        pub_time: None,
    })
}

/// 请求某条目的 playurl 并解析为 [`PlayUrl`]。
///
/// `logged_in` 决定直链能力（未登录最高 480P；登录后 1080P+，大会员 4K/HDR）。
pub async fn play_url(
    client: &BiliClient,
    id: &BiliId,
    entry: &Entry,
    logged_in: bool,
) -> BiliResult<PlayUrl> {
    let qn = if logged_in { "127" } else { "64" };
    let fnval = if logged_in { "4048" } else { "16" };
    let common = |v: &mut Vec<(String, String)>| {
        v.push(("qn".to_string(), qn.to_string()));
        v.push(("fnval".to_string(), fnval.to_string()));
        v.push(("fnver".to_string(), "0".to_string()));
        v.push(("fourk".to_string(), "1".to_string()));
    };
    let (url, params, use_wbi) = match id {
        BiliId::Video { bvid } => {
            let mut p = vec![
                ("bvid".to_string(), bvid.clone()),
                ("cid".to_string(), entry.cid.to_string()),
            ];
            common(&mut p);
            (
                "https://api.bilibili.com/x/player/wbi/playurl".to_string(),
                p,
                true,
            )
        }
        BiliId::Bangumi { .. } => {
            let ep_id = entry
                .ep_id
                .clone()
                .ok_or_else(|| BiliError::Parse("剧集缺少 ep_id".into()))?;
            let season_id = entry
                .season_id
                .ok_or_else(|| BiliError::Parse("剧集缺少 season_id".into()))?;
            // playurl 接口的 ep_id 是纯数字，不带 "ep" 前缀
            let ep_num = ep_id.trim_start_matches("ep").to_string();
            let mut p = vec![
                ("ep_id".to_string(), ep_num),
                ("season_id".to_string(), season_id.to_string()),
            ];
            common(&mut p);
            (
                "https://api.bilibili.com/pgc/player/web/v2/playurl".to_string(),
                p,
                true,
            )
        }
        BiliId::Cheese { .. } => {
            let ep_id = entry
                .ep_id
                .clone()
                .ok_or_else(|| BiliError::Parse("课程缺少 ep_id".into()))?;
            let season_id = entry
                .season_id
                .ok_or_else(|| BiliError::Parse("课程缺少 season_id".into()))?;
            let aid = entry
                .aid
                .ok_or_else(|| BiliError::Parse("课程缺少 aid".into()))?;
            let ep_num = ep_id.trim_start_matches("ep").to_string();
            let mut p = vec![
                ("ep_id".to_string(), ep_num),
                ("cid".to_string(), entry.cid.to_string()),
                ("avid".to_string(), aid.to_string()),
                ("season_id".to_string(), season_id.to_string()),
            ];
            common(&mut p);
            (
                "https://api.bilibili.com/pugv/player/web/playurl".to_string(),
                p,
                true,
            )
        }
        _ => return Err(BiliError::Other("not a video content".into())),
    };
    let data = if use_wbi {
        wbi::get_with_wbi(client, &url, params).await?
    } else {
        client.get_json(&url, &params).await?
    };
    // PGC v2 嵌套一层 result
    let data = match data.get("result").filter(|v| v.is_object()) {
        Some(r) => r.clone(),
        None => data,
    };
    parse_playurl(&data, logged_in)
}

/// 从 playurl 响应解析 DASH/兜底单流（纯函数，fixture 可测）。
pub fn parse_playurl(data: &Value, logged_in: bool) -> BiliResult<PlayUrl> {
    // PGC v2 playurl 可能把 dash 包在 video_info 里
    let data = data.get("video_info").unwrap_or(data);
    let accept_quality: Vec<Quality> = data
        .get("accept_quality")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_u64)
                .map(|n| Quality(n as u32))
                .collect()
        })
        .unwrap_or_default();
    let accept_description: Vec<String> = data
        .get("accept_description")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if let Some(dash) = data.get("dash").filter(|d| d.is_object()) {
        // 视频：按 qn 分组、保留多编码
        let videos = dash
            .get("video")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut quality_map: Vec<VideoQuality> = Vec::new();
        for v in &videos {
            let qn = v
                .get("id")
                .and_then(Value::as_u64)
                .map(|n| Quality(n as u32))
                .unwrap_or(Quality::AUTO);
            let stream = stream_from_json(v, false);
            match quality_map.iter_mut().find(|q| q.quality == qn) {
                Some(group) => {
                    if !group.streams.iter().any(|s| s.codec == stream.codec) {
                        group.streams.push(stream);
                    }
                }
                None => quality_map.push(VideoQuality {
                    quality: qn,
                    streams: vec![stream],
                }),
            }
        }
        quality_map.sort_by_key(|q| std::cmp::Reverse(q.quality));
        // 编码顺序：AVC 优先（兼容性）
        for group in &mut quality_map {
            group.streams.sort_by_key(|s| match s.codec {
                Codec::Avc => 0,
                Codec::Hevc => 1,
                Codec::Av1 => 2,
                Codec::Unknown => 3,
            });
        }

        // 音频：dash.audio + dolby.audio + flac.audio
        let mut raw_audios: Vec<&Value> = dash
            .get("audio")
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        if let Some(dolby) = dash.pointer("/dolby/audio").and_then(Value::as_array) {
            raw_audios.extend(dolby.iter());
        }
        if let Some(flac) = dash
            .get("flac")
            .filter(|v| !v.is_null())
            .and_then(|v| v.get("audio"))
            .filter(|v| !v.is_null())
        {
            raw_audios.push(flac);
        }
        let mut audios: Vec<AudioTrack> = Vec::new();
        for a in raw_audios {
            let id = a.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
            let quality = AudioQuality(id);
            if audios.iter().any(|t| t.quality == quality) {
                continue;
            }
            let stream = stream_from_json(a, true);
            audios.push(AudioTrack {
                quality,
                format: AudioFormat::from_quality(quality),
                stream,
            });
        }
        audios.sort_by_key(|a| std::cmp::Reverse(audio_rank(a.quality)));

        let actual: Vec<Quality> = quality_map.iter().map(|q| q.quality).collect();
        let quality_tiers = build_tiers(&accept_quality, &accept_description, &actual, logged_in);
        let mut out = PlayUrl {
            is_dash: true,
            qualities: quality_map,
            audios,
            quality_tiers,
            single_stream: None,
            accept_quality,
        };
        if out.qualities.is_empty() {
            // DASH 存在但视频流为空：可能是付费试看/权限不足
            return Err(BiliError::Permission {
                message: "playurl 未返回可用视频流".into(),
            });
        }
        out.qualities.shrink_to_fit();
        return Ok(out);
    }

    // durl（FLV/MP4 单流）兜底
    if let Some(durl) = data.get("durl").and_then(Value::as_array)
        && let Some(first) = durl.first()
    {
        let quality = Quality(data.get("quality").and_then(Value::as_u64).unwrap_or(32) as u32);
        let mime = first
            .get("url")
            .and_then(Value::as_str)
            .map(|u| {
                if u.contains(".flv") {
                    "video/x-flv"
                } else {
                    "video/mp4"
                }
            })
            .unwrap_or("video/mp4");
        let stream = MediaStream {
            id: quality.0,
            codec: Codec::Unknown,
            base_url: first
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            backup_urls: first
                .get("backup_url")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
            bandwidth: 0,
            mime: mime.to_string(),
            codecs: String::new(),
            size: first.get("size").and_then(Value::as_u64),
            width: None,
            height: None,
            frame_rate: None,
            host: None,
        };
        return Ok(PlayUrl {
            is_dash: false,
            qualities: Vec::new(),
            audios: Vec::new(),
            quality_tiers: build_tiers(&accept_quality, &accept_description, &[quality], logged_in),
            single_stream: Some(stream),
            accept_quality: if accept_quality.is_empty() {
                vec![quality]
            } else {
                accept_quality
            },
        });
    }
    Err(BiliError::Parse("playurl 响应中无 dash/durl".into()))
}

fn audio_rank(q: AudioQuality) -> u32 {
    match q.0 {
        30252 => 7,
        30251 => 6,
        30250 => 5,
        30280 => 4,
        30232 => 3,
        30228 => 2,
        30216 => 1,
        n => n % 1000,
    }
}

fn build_tiers(
    accept: &[Quality],
    descriptions: &[String],
    actual: &[Quality],
    logged_in: bool,
) -> Vec<QualityTier> {
    let mut tiers: Vec<QualityTier> = Vec::new();
    for (i, q) in accept.iter().enumerate() {
        let available = actual.contains(q);
        let unavailable = if available {
            None
        } else if q.is_vip_tier() && !logged_in {
            Some(TierUnavailable::LoginRequired)
        } else if q.is_vip_tier() {
            Some(TierUnavailable::VipRequired)
        } else {
            Some(TierUnavailable::NotProvided)
        };
        tiers.push(QualityTier {
            quality: *q,
            name: descriptions.get(i).cloned().filter(|s| !s.is_empty()),
            unavailable,
        });
    }
    // 实际返回但未被 accept_quality 覆盖的档位（接口异常兜底）
    for q in actual {
        if !tiers.iter().any(|t| t.quality == *q) {
            tiers.push(QualityTier {
                quality: *q,
                name: None,
                unavailable: None,
            });
        }
    }
    tiers.sort_by_key(|t| std::cmp::Reverse(t.quality));
    tiers
}

fn stream_from_json(v: &Value, audio: bool) -> MediaStream {
    let codecid = v.get("codecid").and_then(Value::as_i64).unwrap_or(0);
    let base_url = v
        .get("baseUrl")
        .or_else(|| v.get("base_url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let host = url_host(&base_url);
    MediaStream {
        id: v.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
        codec: if audio {
            Codec::Unknown
        } else {
            Codec::from_codecid(codecid)
        },
        base_url,
        backup_urls: v
            .get("backupUrl")
            .or_else(|| v.get("backup_url"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
        bandwidth: v.get("bandwidth").and_then(Value::as_u64).unwrap_or(0),
        mime: v
            .get("mimeType")
            .or_else(|| v.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or(if audio { "audio/mp4" } else { "video/mp4" })
            .to_string(),
        codecs: v
            .get("codecs")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        size: None,
        width: v.get("width").and_then(Value::as_u64).map(|n| n as u32),
        height: v.get("height").and_then(Value::as_u64).map(|n| n as u32),
        frame_rate: v.get("frameRate").and_then(Value::as_str).map(String::from),
        host,
    }
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    Some(rest.split('/').next()?.split(':').next()?.to_string())
}

/// 旧 egui 应用兼容：按条目枚举清晰度（v1 API 形状映射到新选择器）。
/// 新代码请用 [`play_url`] + [`crate::media::quality::select`]。
pub async fn qualities_for(
    client: &BiliClient,
    id: &BiliId,
    entry: &Entry,
    logged_in: bool,
) -> BiliResult<Vec<VideoQuality>> {
    Ok(play_url(client, id, entry, logged_in).await?.qualities)
}

impl BiliId {
    /// 内部诊断用的英文类别名（非 UI 文案）。
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            BiliId::Video { .. } => "video",
            BiliId::Bangumi { .. } => "bangumi",
            BiliId::Cheese { .. } => "cheese",
            BiliId::Audio { .. } => "audio",
            BiliId::AudioList { .. } => "audio_list",
        }
    }
}

/// 默认条目选择语义（batch-download spec）。
pub fn default_entries(id: &BiliId, content: &Content, page_hint: Option<usize>) -> Vec<usize> {
    if let Some(i) = page_hint
        && i < content.entries.len()
    {
        return vec![i];
    }
    match id {
        BiliId::Bangumi { id } | BiliId::Cheese { id } if id.starts_with("ep") => {
            if let Some(pos) = content
                .entries
                .iter()
                .position(|e| e.ep_id.as_deref() == Some(id.as_str()))
            {
                return vec![pos];
            }
            vec![0]
        }
        _ => (0..content.entries.len()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dash_fixture_with_codecs_and_audio() {
        let data = serde_json::json!({
            "accept_quality": [80, 64, 32, 16],
            "accept_description": ["高清 1080P", "高清 720P", "清晰 480P", "流畅 360P"],
            "dash": {
                "video": [
                    {"id": 80, "baseUrl": "https://cdn/v1080.m4s", "bandwidth": 2000, "codecid": 7, "codecs": "avc1.640032", "width": 1920, "height": 1080, "frameRate": "30"},
                    {"id": 80, "baseUrl": "https://cdn/v1080hev.m4s", "bandwidth": 1500, "codecid": 12, "codecs": "hev1.1.6.L120"},
                    {"id": 32, "baseUrl": "https://cdn/v480.m4s", "bandwidth": 600, "codecid": 7}
                ],
                "audio": [
                    {"id": 30280, "baseUrl": "https://cdn/a192.m4s", "bandwidth": 320000},
                    {"id": 30232, "baseUrl": "https://cdn/a64.m4s", "bandwidth": 100000}
                ]
            }
        });
        let play = parse_playurl(&data, false).unwrap();
        assert!(play.is_dash);
        assert_eq!(play.qualities.len(), 2);
        assert_eq!(play.qualities[0].quality, Quality::P1080);
        assert_eq!(play.qualities[0].codecs(), vec![Codec::Avc, Codec::Hevc]);
        assert_eq!(play.qualities[0].streams[0].height, Some(1080));
        assert_eq!(play.best_audio().unwrap().quality, AudioQuality::K192);
        assert_eq!(play.audios.len(), 2);
        // accept_quality 宣告 4 档，但 dash 只返回 1080P/480P：其余档标记不可用原因
        assert_eq!(play.quality_tiers.len(), 4);
        let tier = |q| play.quality_tiers.iter().find(|t| t.quality == q).unwrap();
        assert!(tier(Quality::P1080).unavailable.is_none());
        assert!(tier(Quality::P480).unavailable.is_none());
        assert_eq!(
            tier(Quality::P720).unavailable,
            Some(TierUnavailable::NotProvided)
        );
        assert_eq!(
            tier(Quality::P360).unavailable,
            Some(TierUnavailable::NotProvided)
        );
    }

    #[test]
    fn vip_tier_marked_when_missing() {
        let data = serde_json::json!({
            "accept_quality": [120, 80, 32],
            "accept_description": ["超清 4K", "高清 1080P", "清晰 480P"],
            "dash": {
                "video": [
                    {"id": 80, "baseUrl": "https://cdn/v1080.m4s", "bandwidth": 2000, "codecid": 7},
                    {"id": 32, "baseUrl": "https://cdn/v480.m4s", "bandwidth": 600, "codecid": 7}
                ],
                "audio": [{"id": 30280, "baseUrl": "https://cdn/a.m4s", "bandwidth": 320}]
            }
        });
        let tiers = parse_playurl(&data, true).unwrap().quality_tiers;
        let tier4k = tiers.iter().find(|t| t.quality == Quality::P4K).unwrap();
        assert_eq!(tier4k.unavailable, Some(TierUnavailable::VipRequired));
        assert_eq!(tier4k.name.as_deref(), Some("超清 4K"));
        let tiers_anon = parse_playurl(&data, false).unwrap().quality_tiers;
        assert_eq!(
            tiers_anon
                .iter()
                .find(|t| t.quality == Quality::P4K)
                .unwrap()
                .unavailable,
            Some(TierUnavailable::LoginRequired)
        );
    }

    #[test]
    fn flac_and_dolby_audio_merged_sorted() {
        let data = serde_json::json!({
            "accept_quality": [80],
            "dash": {
                "video": [{"id": 80, "baseUrl": "https://cdn/v.m4s", "codecid": 7}],
                "audio": [{"id": 30280, "baseUrl": "https://cdn/a192.m4s", "bandwidth": 320000}],
                "dolby": {"audio": [{"id": 30250, "baseUrl": "https://cdn/dolby.m4s", "bandwidth": 448000}]},
                "flac": {"audio": {"id": 30251, "baseUrl": "https://cdn/flac.m4s", "bandwidth": 900000}}
            }
        });
        let play = parse_playurl(&data, true).unwrap();
        assert_eq!(play.audios.len(), 3);
        assert_eq!(play.best_audio().unwrap().quality, AudioQuality::FLAC);
        assert_eq!(play.best_audio().unwrap().format, AudioFormat::Flac);
        assert!(play.audios.iter().any(|a| a.format == AudioFormat::Eac3));
    }

    #[test]
    fn durl_fallback_single_stream() {
        let data = serde_json::json!({
            "quality": 32,
            "accept_quality": [32, 16],
            "durl": [{"url": "https://cdn/flv.mp4", "size": 123, "backup_url": []}]
        });
        let play = parse_playurl(&data, false).unwrap();
        assert!(!play.is_dash);
        assert!(play.single_stream.is_some());
        assert_eq!(
            play.single_stream.as_ref().unwrap().base_url,
            "https://cdn/flv.mp4"
        );
        assert!(play.has_downloadable());
    }

    #[test]
    fn empty_dash_reports_permission() {
        let data = serde_json::json!({
            "accept_quality": [112],
            "dash": {"video": [], "audio": []}
        });
        let err = parse_playurl(&data, true).err().unwrap();
        assert_eq!(err.kind(), crate::api::error::ErrorKind::Permission);
    }

    #[test]
    fn parse_playurl_unwraps_video_info() {
        // PGC v2 playurl 把 dash 包在 video_info 里
        let data = serde_json::json!({
            "video_info": {
                "accept_quality": [80, 64],
                "dash": {
                    "video": [{"id": 80, "baseUrl": "https://cdn/v.m4s", "codecid": 7}],
                    "audio": [{"id": 30280, "baseUrl": "https://cdn/a.m4s"}]
                }
            }
        });
        let play = parse_playurl(&data, false).unwrap();
        assert!(play.is_dash);
        assert!(!play.qualities.is_empty());
    }

    #[test]
    fn audio_rank_order() {
        assert!(audio_rank(AudioQuality::HI_RES) > audio_rank(AudioQuality::FLAC));
        assert!(audio_rank(AudioQuality::FLAC) > audio_rank(AudioQuality::DOLBY));
        assert!(audio_rank(AudioQuality::DOLBY) > audio_rank(AudioQuality::K192));
    }

    #[test]
    fn default_entries_semantics() {
        let content = Content {
            id: BiliId::Video { bvid: "BV1".into() },
            title: "t".into(),
            cover_url: None,
            upper: String::new(),
            entries: (0..3)
                .map(|i| Entry {
                    cid: i as u64,
                    title: format!("P{i}"),
                    index: i,
                    ep_id: None,
                    season_id: None,
                    aid: None,
                    duration_secs: None,
                    qualities: vec![],
                })
                .collect(),
            duration_secs: None,
            pub_time: None,
        };
        assert_eq!(
            default_entries(&content.id.clone(), &content, None),
            vec![0, 1, 2]
        );
        assert_eq!(
            default_entries(&content.id.clone(), &content, Some(1)),
            vec![1]
        );
        assert_eq!(
            default_entries(&content.id.clone(), &content, Some(9)),
            vec![0, 1, 2]
        );
        // ep 链接默认只选该集
        let bangumi = BiliId::Bangumi { id: "ep2".into() };
        let mut content2 = content.clone();
        content2.entries[1].ep_id = Some("ep2".into());
        assert_eq!(default_entries(&bangumi, &content2, None), vec![1]);
    }

    fn season_view_fixture() -> Value {
        serde_json::json!({
            "bvid": "BV1second001",
            "aid": 111,
            "title": "第二集",
            "pic": "https://i0.hdslb.com/bfs/archive/current.jpg",
            "duration": 300,
            "pubdate": 1700000000,
            "owner": {"mid": 9, "name": "合集UP"},
            "pages": [{"cid": 3002, "page": 1, "part": "第二集", "duration": 300}],
            "ugc_season": {
                "id": 555000,
                "title": "测试合集",
                "cover": "https://i0.hdslb.com/bfs/archive/season.jpg",
                "intro": "合集简介",
                "sections": [
                    {
                        "id": 1001,
                        "title": "正片",
                        "episodes": [
                            {
                                "aid": 110,
                                "cid": 3001,
                                "bvid": "BV1first001",
                                "title": "第一集",
                                "arc": {"pic": "//i0.hdslb.com/bfs/archive/ep1.jpg", "duration": 120},
                                "page": {"cid": 3001, "duration": 120}
                            },
                            {
                                "aid": 111,
                                "cid": 3002,
                                "bvid": "BV1second001",
                                "title": "第二集",
                                "arc": {"pic": "https://i0.hdslb.com/bfs/archive/ep2.jpg", "duration": 300},
                                "page": {"cid": 3002, "duration": 300}
                            }
                        ]
                    },
                    {
                        "id": 1002,
                        "title": "花絮",
                        "episodes": [
                            {"aid": 112, "cid": 3003, "bvid": "BV1extra001", "title": "花絮一", "arc": {"pic": "", "duration": 60}}
                        ]
                    }
                ]
            }
        })
    }

    #[test]
    fn ugc_season_multi_sections_fixture() {
        let season = parse_ugc_season(&season_view_fixture()).expect("应识别到合集");
        assert_eq!(season.season_id, 555000);
        assert_eq!(season.title, "测试合集");
        assert_eq!(season.upper, "合集UP");
        assert_eq!(season.current_bvid, "BV1second001");
        assert_eq!(season.current_index(), Some(1));
        assert_eq!(season.sections.len(), 2);
        assert_eq!(season.sections[1].title, "花絮");
        assert_eq!(season.episodes.len(), 3);
        assert_eq!(season.episodes[0].section_title, "正片");
        assert_eq!(season.episodes[1].section_index, 0);
        assert_eq!(season.episodes[2].section_title, "花絮");
        assert_eq!(season.episodes[2].index, 2);
        assert!(season.episodes[1].is_current);
        assert!(!season.episodes[0].is_current);
        assert_eq!(season.episodes[0].cid, 3001);
        assert_eq!(season.episodes[0].aid, Some(110));
        assert_eq!(season.episodes[1].duration, Some(300));
        assert_eq!(
            season.episodes[0].cover.as_deref(),
            Some("https://i0.hdslb.com/bfs/archive/ep1.jpg")
        );
        // 统一列表输出
        let items = season.to_source_items();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].bili_id, "BV1first001");
        assert_eq!(items[0].kind, SourceKind::Video);
        assert_eq!(items[0].upper.as_deref(), Some("合集UP"));
        let collection = season.to_collection();
        assert_eq!(collection.id, "555000");
        assert_eq!(collection.title, "测试合集");
        assert_eq!(collection.items.len(), 3);
        assert_eq!(
            collection.cover.as_deref(),
            Some("https://i0.hdslb.com/bfs/archive/season.jpg")
        );
    }

    #[test]
    fn ugc_season_absent_returns_none() {
        let view = serde_json::json!({"bvid": "BV1only", "title": "单视频", "pages": []});
        assert!(parse_ugc_season(&view).is_none());
        // 空对象/非对象也不当作合集
        let view = serde_json::json!({"ugc_season": null});
        assert!(parse_ugc_season(&view).is_none());
    }

    #[test]
    fn ugc_season_empty_sections() {
        let view = serde_json::json!({
            "bvid": "BV1only",
            "owner": {"name": "UP"},
            "ugc_season": {"id": 1, "title": "空合集", "cover": "", "sections": []}
        });
        let season = parse_ugc_season(&view).expect("合集元信息仍应返回");
        assert!(season.episodes.is_empty());
        assert_eq!(season.current_index(), None);
        assert!(season.to_collection().items.is_empty());
    }
}
