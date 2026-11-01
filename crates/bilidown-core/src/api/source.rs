//! UGC 合集/歌单共用的轻量列表模型。
//!
//! 仅保留下载管线需要的条目描述；不包含任何链接识别/列表浏览逻辑。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::BiliId;

/// 列表分页请求之间的节流间隔（保留给歌单等分页接口）。
pub const PAGE_THROTTLE: Duration = Duration::from_millis(400);

/// 列表条目的内容类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Video,
    Bangumi,
    Cheese,
    Audio,
    AudioList,
}

impl SourceKind {
    pub fn from_bili_id(id: &BiliId) -> Self {
        match id {
            BiliId::Video { .. } => SourceKind::Video,
            BiliId::Bangumi { .. } => SourceKind::Bangumi,
            BiliId::Cheese { .. } => SourceKind::Cheese,
            BiliId::Audio { .. } => SourceKind::Audio,
            BiliId::AudioList { .. } => SourceKind::AudioList,
        }
    }
}

/// 列表中的一条条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceItem {
    pub bili_id: String,
    pub kind: SourceKind,
    pub title: String,
    pub cover: Option<String>,
    pub duration: Option<u64>,
    pub upper: Option<String>,
}

impl SourceItem {
    pub fn new(kind: SourceKind, bili_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            bili_id: bili_id.into(),
            kind,
            title: title.into(),
            cover: None,
            duration: None,
            upper: None,
        }
    }

    pub fn with_cover(mut self, cover: Option<String>) -> Self {
        self.cover = cover;
        self
    }

    pub fn with_duration(mut self, duration: Option<u64>) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_upper(mut self, upper: Option<String>) -> Self {
        self.upper = upper;
        self
    }

    pub fn to_bili_id(&self) -> Option<BiliId> {
        match self.kind {
            SourceKind::Video => Some(BiliId::Video { bvid: self.bili_id.clone() }),
            SourceKind::Bangumi => Some(BiliId::Bangumi { id: self.bili_id.clone() }),
            SourceKind::Cheese => Some(BiliId::Cheese { id: self.bili_id.clone() }),
            SourceKind::Audio => Some(BiliId::Audio { song_id: self.bili_id.clone() }),
            SourceKind::AudioList => Some(BiliId::AudioList { menu_id: self.bili_id.clone() }),
        }
    }
}

/// 合集/列表元信息 + 条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceCollection {
    pub kind: SourceKind,
    pub id: String,
    pub title: String,
    pub cover: Option<String>,
    pub upper: Option<String>,
    pub description: Option<String>,
    pub items: Vec<SourceItem>,
}

impl SourceCollection {
    pub fn new(
        kind: SourceKind,
        id: impl Into<String>,
        title: impl Into<String>,
        items: Vec<SourceItem>,
    ) -> Self {
        Self {
            kind,
            id: id.into(),
            title: title.into(),
            cover: None,
            upper: None,
            description: None,
            items,
        }
    }
}

/// 补全协议相对 URL（B 站封面/图片多为 `//i0.hdslb.com/...`）。
pub(crate) fn https_url(url: &str) -> String {
    match url.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

/// 从 JSON 字段取 URL 并补全协议。
pub(crate) fn duration_secs(value: Option<&Value>) -> Option<u64> {
    let n = value?.as_u64()?;
    if n == 0 {
        None
    } else if n > 100_000 {
        Some(n / 1000)
    } else {
        Some(n)
    }
}

pub(crate) fn url_value(value: Option<&Value>) -> Option<String> {
    let url = value?.as_str()?.trim();
    (!url.is_empty()).then(|| https_url(url))
}
