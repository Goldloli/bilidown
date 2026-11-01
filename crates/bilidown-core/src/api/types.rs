//! 领域类型：内容标识、内容模型、清晰度/编码/音质、DASH 流与任务参数。
//!
//! 本模块**不包含任何 UI 文案/本地化字符串**：`Quality`/`Codec`/`AudioQuality`
//! 等只承载数据，中文标签由前端映射（design D2）。

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 内容大类（序列化给前端做图标/分组）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Video,
    Bangumi,
    Cheese,
    Audio,
    AudioList,
}

/// 解析出的内容标识（五类内容）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BiliId {
    /// 普通视频（bvid 或 av 号）
    Video { bvid: String },
    /// 番剧/影视：ep 或 ss/md
    Bangumi { id: String },
    /// 课程（cheese）ep
    Cheese { id: String },
    /// 音频单曲
    Audio { song_id: String },
    /// 歌单
    AudioList { menu_id: String },
}

impl BiliId {
    pub fn kind(&self) -> ContentKind {
        match self {
            BiliId::Video { .. } => ContentKind::Video,
            BiliId::Bangumi { .. } => ContentKind::Bangumi,
            BiliId::Cheese { .. } => ContentKind::Cheese,
            BiliId::Audio { .. } => ContentKind::Audio,
            BiliId::AudioList { .. } => ContentKind::AudioList,
        }
    }

    /// 对应的人类可读来源页 URL（历史记录「复制链接」用）。
    pub fn page_url(&self) -> String {
        match self {
            BiliId::Video { bvid } => format!("https://www.bilibili.com/video/{bvid}"),
            BiliId::Bangumi { id } => format!("https://www.bilibili.com/bangumi/play/{id}"),
            BiliId::Cheese { id } => format!("https://www.bilibili.com/cheese/play/{id}"),
            BiliId::Audio { song_id } => {
                format!("https://www.bilibili.com/audio/au{song_id}")
            }
            BiliId::AudioList { menu_id } => {
                format!("https://www.bilibili.com/audio/am{menu_id}")
            }
        }
    }
}

impl fmt::Display for BiliId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BiliId::Video { bvid } => write!(f, "{bvid}"),
            BiliId::Bangumi { id } => write!(f, "{id}"),
            BiliId::Cheese { id } => write!(f, "{id}"),
            BiliId::Audio { song_id } => write!(f, "au{song_id}"),
            BiliId::AudioList { menu_id } => write!(f, "am{menu_id}"),
        }
    }
}

/// 清晰度档位（qn 值）。前端映射为 360P/480P/1080P… 标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Quality(pub u32);

impl Quality {
    pub const AUTO: Quality = Quality(0);
    pub const P360: Quality = Quality(16);
    pub const P480: Quality = Quality(32);
    pub const P720: Quality = Quality(64);
    pub const P720_60: Quality = Quality(74);
    pub const P1080: Quality = Quality(80);
    pub const P1080_PLUS: Quality = Quality(112);
    pub const P1080_60: Quality = Quality(116);
    pub const P4K: Quality = Quality(120);
    pub const HDR: Quality = Quality(125);
    pub const DOLBY: Quality = Quality(126);
    pub const P8K: Quality = Quality(127);

    pub fn qn(self) -> u32 {
        self.0
    }

    /// 该档位在 B 站语义上通常需要大会员（仅用于展示辅助，可用性以 API 返回为准）。
    pub fn is_vip_tier(self) -> bool {
        matches!(self.0, 112 | 116 | 120 | 125 | 126 | 127)
    }

    pub fn is_known(self) -> bool {
        matches!(
            self.0,
            16 | 32 | 64 | 74 | 80 | 112 | 116 | 120 | 125 | 126 | 127
        )
    }
}

/// 视频编码（`codecid`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    /// H.264 / AVC，兼容性最好（默认优先）
    #[default]
    Avc,
    /// H.265 / HEVC
    Hevc,
    /// AV1
    Av1,
    /// 未识别编码
    Unknown,
}

impl Codec {
    pub fn from_codecid(codecid: i64) -> Self {
        match codecid {
            7 => Codec::Avc,
            12 => Codec::Hevc,
            13 => Codec::Av1,
            _ => Codec::Unknown,
        }
    }

    pub fn codecid(self) -> i64 {
        match self {
            Codec::Avc => 7,
            Codec::Hevc => 12,
            Codec::Av1 => 13,
            Codec::Unknown => 0,
        }
    }

    /// 技术标识（非本地化文案，用于日志/诊断）。
    pub fn as_str(self) -> &'static str {
        match self {
            Codec::Avc => "avc",
            Codec::Hevc => "hevc",
            Codec::Av1 => "av1",
            Codec::Unknown => "unknown",
        }
    }
}

/// 音频码率/音质档位（DASH `audio[].id`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AudioQuality(pub u32);

impl AudioQuality {
    pub const K64: AudioQuality = AudioQuality(30216);
    pub const K128: AudioQuality = AudioQuality(30228);
    pub const K132: AudioQuality = AudioQuality(30232);
    pub const K192: AudioQuality = AudioQuality(30280);
    pub const DOLBY: AudioQuality = AudioQuality(30250);
    pub const FLAC: AudioQuality = AudioQuality(30251);
    pub const HI_RES: AudioQuality = AudioQuality(30252);

    pub fn id(self) -> u32 {
        self.0
    }

    /// 技术标识（文件名/日志用，非本地化文案）。
    pub fn tag(self) -> &'static str {
        match self.0 {
            30216 => "64k",
            30228 => "128k",
            30232 => "132k",
            30280 => "192k",
            30250 => "dolby",
            30251 => "flac",
            30252 => "hires",
            _ => "audio",
        }
    }
}

/// 音频容器/编码格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    /// AAC in m4a（默认）
    #[default]
    M4a,
    /// FLAC（无损/Hi-Res 必须 mkv 或 m4a 兼容容器）
    Flac,
    /// E-AC-3（杜比全景声，mp4 容器）
    Eac3,
    /// 原始 MP3（音频区旧接口）
    Mp3,
}

impl AudioFormat {
    pub fn from_quality(q: AudioQuality) -> Self {
        match q.0 {
            30250 => AudioFormat::Eac3,
            30251 | 30252 => AudioFormat::Flac,
            _ => AudioFormat::M4a,
        }
    }

    /// 该音频格式在「视频+音频合并」时应使用的容器扩展名。
    pub fn merged_container(self) -> &'static str {
        match self {
            AudioFormat::Flac => "mkv",
            _ => "mp4",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Flac => "flac",
            AudioFormat::Eac3 | AudioFormat::M4a => "m4a",
            AudioFormat::Mp3 => "mp3",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AudioFormat::M4a => "m4a",
            AudioFormat::Flac => "flac",
            AudioFormat::Eac3 => "eac3",
            AudioFormat::Mp3 => "mp3",
        }
    }
}

/// 一条媒体流（DASH 视频/音频，或 durl 兜底单流）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaStream {
    /// 档位 id：视频为 qn，音频为 abr。
    pub id: u32,
    /// 视频编码；音频流为 [`Codec::Unknown`]。
    pub codec: Codec,
    pub base_url: String,
    pub backup_urls: Vec<String>,
    pub bandwidth: u64,
    pub mime: String,
    /// 原始 codecs 字符串（如 `avc1.640032`）。
    pub codecs: String,
    pub size: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    /// 视频流优先使用的主机名（PCDN 过滤后写回，便于诊断）。
    pub host: Option<String>,
}

impl MediaStream {
    /// 主 URL + 备用 URL（PCDN 过滤前）。
    pub fn urls(&self) -> Vec<String> {
        std::iter::once(self.base_url.clone())
            .chain(self.backup_urls.iter().cloned())
            .collect()
    }
}

/// 某清晰度下的一组视频流（同档位可能有 AVC/HEVC/AV1 多编码）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoQuality {
    pub quality: Quality,
    pub streams: Vec<MediaStream>,
}

impl VideoQuality {
    /// 按偏好选编码：优先请求编码 → AVC（兼容性）→ HEVC → AV1 → 第一个。
    pub fn pick_stream(&self, wanted: Option<Codec>) -> Option<&MediaStream> {
        if let Some(c) = wanted
            && let Some(s) = self.streams.iter().find(|s| s.codec == c)
        {
            return Some(s);
        }
        for c in [Codec::Avc, Codec::Hevc, Codec::Av1] {
            if let Some(s) = self.streams.iter().find(|s| s.codec == c) {
                return Some(s);
            }
        }
        self.streams.first()
    }

    pub fn codecs(&self) -> Vec<Codec> {
        let mut out: Vec<Codec> = Vec::new();
        for s in &self.streams {
            if !out.contains(&s.codec) {
                out.push(s.codec);
            }
        }
        out
    }
}

/// 一条音频流。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    pub quality: AudioQuality,
    pub format: AudioFormat,
    pub stream: MediaStream,
}

/// 档位不可用/受限原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierUnavailable {
    /// 接口未返回（番剧试看、未上线等）
    NotProvided,
    /// 需要登录
    LoginRequired,
    /// 需要大会员
    VipRequired,
    /// 需要付费
    PaidRequired,
    /// 区域限制
    Region,
}

/// 清晰度档位可用性（含置灰数据）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityTier {
    pub quality: Quality,
    /// 接口给出的档位名（可能为空）。
    pub name: Option<String>,
    /// `None` = 可用；`Some(reason)` = 当前账号不可用。
    pub unavailable: Option<TierUnavailable>,
}

/// 一次 playurl 解析结果（单条目）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayUrl {
    /// 是否为 DASH 响应（否则为 durl/flv 兜底单流）。
    pub is_dash: bool,
    /// 实际可用视频档位，按清晰度降序。
    pub qualities: Vec<VideoQuality>,
    /// 实际可用音频流，按码率降序。
    pub audios: Vec<AudioTrack>,
    /// 全部档位及其可用性（含不可用档位，供置灰展示）。
    pub quality_tiers: Vec<QualityTier>,
    /// durl/flv 兜底：音视频一体的单流。
    pub single_stream: Option<MediaStream>,
    /// 接口宣告的可选档位（qn）。
    pub accept_quality: Vec<Quality>,
}

impl PlayUrl {
    pub fn quality(&self, q: Quality) -> Option<&VideoQuality> {
        self.qualities.iter().find(|v| v.quality == q)
    }

    /// 可用的最高档位。
    pub fn best_quality(&self) -> Option<&VideoQuality> {
        self.qualities.first()
    }

    pub fn best_audio(&self) -> Option<&AudioTrack> {
        self.audios.first()
    }

    /// 指定音质；不可用时返回最高可用。
    pub fn audio_by_quality(&self, wanted: Option<AudioQuality>) -> Option<&AudioTrack> {
        match wanted {
            Some(q) => self
                .audios
                .iter()
                .find(|a| a.quality == q)
                .or_else(|| self.audios.first()),
            None => self.audios.first(),
        }
    }

    /// 该内容是否至少有一种可下载形态。
    pub fn has_downloadable(&self) -> bool {
        !self.qualities.is_empty() || self.single_stream.is_some()
    }
}

/// 一个内容条目（分 P / 剧集 / 曲目）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub cid: u64,
    pub title: String,
    pub index: usize,
    /// PGC/PUGV 剧集的 ep id（如 "ep425850"），UGC 无。
    pub ep_id: Option<String>,
    /// PGC/PUGV 所属 season_id（playurl 需要）。
    #[serde(default)]
    pub season_id: Option<u64>,
    /// 课程/PGC 的 avid（课程 playurl 需要）。
    #[serde(default)]
    pub aid: Option<u64>,
    /// 时长（秒），用于合并进度/展示。
    pub duration_secs: Option<u64>,
    /// 该条目可用的视频清晰度（解析 playurl 后由外部填充；通常为空）。
    pub qualities: Vec<Quality>,
}

/// 解析后的内容统一模型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub id: BiliId,
    pub title: String,
    pub cover_url: Option<String>,
    pub upper: String,
    pub entries: Vec<Entry>,
    /// 内容总时长（秒）；番剧/视频为分 P 合计。
    pub duration_secs: Option<u64>,
    /// 发布时间（Unix 秒）。
    pub pub_time: Option<i64>,
}

/// 音频条目（单曲信息，歌单展开后复用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioEntry {
    pub song_id: u64,
    pub title: String,
    pub upper: String,
    /// 原始音频格式（接口返回 type 字段，缺省 m4a）。
    pub format: AudioFormat,
    /// 封面 URL（歌单列表展示用；接口未提供时为空）。
    #[serde(default)]
    pub cover: Option<String>,
    /// 时长（秒）。
    #[serde(default)]
    pub duration: Option<u64>,
}

/// 登录用户信息。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserInfo {
    pub is_login: bool,
    pub mid: u64,
    pub uname: String,
    pub avatar_url: Option<String>,
    pub vip_status: bool,
    pub vip_type: i64,
    pub level: u64,
}

/// 下载任务参数（入队时确定；重试使用同一份）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: BiliId,
    /// 用户选择的条目下标（从 0；None = 按默认语义）
    pub selected_entries: Option<Vec<usize>>,
    /// 链接携带的分 P 提示（`?p=N` 转成 0 基下标）；None = 无提示
    pub page_hint: Option<usize>,
    /// 指定清晰度；None = 按默认策略（最高可用）
    pub quality: Option<Quality>,
    /// 指定视频编码；None = 优先 AVC
    pub codec: Option<Codec>,
    /// 指定音质；None = 最高可用
    pub audio_quality: Option<AudioQuality>,
    /// 音频转 mp3（设置/任务参数透传）
    pub to_mp3: bool,
    /// 输出目录覆盖；None = 使用全局设置
    pub output_dir: Option<PathBuf>,
    /// 用户备注（可选）
    pub note: Option<String>,
}

impl TaskSpec {
    pub fn new(id: BiliId) -> Self {
        Self {
            id,
            selected_entries: None,
            page_hint: None,
            quality: None,
            codec: None,
            audio_quality: None,
            to_mp3: false,
            output_dir: None,
            note: None,
        }
    }
}

/// 任务进度/阶段（持久化到 SQLite 的 progress JSON）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStage {
    #[default]
    Idle,
    Resolving,
    Downloading,
    Merging,
    PostProcess,
    Done,
}

/// 任务进度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    #[serde(default)]
    pub stage: DownloadStage,
    /// 已完成条目数（批量任务恢复时从该条继续）。
    #[serde(default)]
    pub entries_done: u32,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.downloaded as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}
