//! 清晰度/编码/音质选择与回退（行为依据 tasks 3.7、video-download spec）。
//!
//! 回退规则：
//! - 请求档位不存在 → 取实际可用最高档（不报错，把实际值回写任务）；
//! - 请求编码不存在 → AVC → HEVC → AV1 → 第一个可用流；
//! - 请求音质不存在 → 取码率最高可用。

use crate::api::types::{
    AudioQuality, AudioTrack, Codec, MediaStream, PlayUrl, Quality, VideoQuality,
};

/// 用户选择（全部可空 = 自动）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamChoice {
    pub quality: Option<Quality>,
    pub codec: Option<Codec>,
    pub audio_quality: Option<AudioQuality>,
}

impl StreamChoice {
    pub fn auto() -> Self {
        Self::default()
    }
}

/// 回退后实际采用的流。
#[derive(Debug, Clone, PartialEq)]
pub struct StreamSelection {
    /// 实际采用的视频清晰度。
    pub quality: Quality,
    /// 实际采用的视频编码。
    pub codec: Codec,
    pub video: MediaStream,
    /// 实际采用的音频（durl 单流时为 None）。
    pub audio: Option<AudioTrack>,
    /// 实际采用的音频流（与 audio.stream 相同，便于统一处理）。
    pub audio_stream: Option<MediaStream>,
    /// 音视频一体（durl/flv 兜底）。
    pub single_stream: bool,
}

impl StreamSelection {
    pub fn audio_quality(&self) -> Option<AudioQuality> {
        self.audio.as_ref().map(|a| a.quality)
    }

    /// 选中的音轨对应的 PCDN 过滤候选 URL。
    pub fn video_urls(&self) -> Vec<String> {
        self.video.urls()
    }

    pub fn audio_urls(&self) -> Vec<String> {
        self.audio_stream
            .as_ref()
            .map(MediaStream::urls)
            .unwrap_or_default()
    }
}

/// 按用户偏好选择实际要下载的流；无可下载形态时返回 None。
///
/// `allow_fallback = false` 时，请求档位不存在会直接返回 None（由调用方报错），
/// 适用于用户在设置中关闭「自动回退」的场景。
pub fn select(
    play: &PlayUrl,
    choice: StreamChoice,
    allow_fallback: bool,
) -> Option<StreamSelection> {
    if let Some(single) = &play.single_stream {
        let quality = match choice.quality {
            Some(q) if play.accept_quality.contains(&q) => q,
            Some(_) if !allow_fallback => return None,
            _ => play
                .accept_quality
                .first()
                .copied()
                .unwrap_or(Quality::AUTO),
        };
        return Some(StreamSelection {
            quality,
            codec: single.codec,
            video: single.clone(),
            audio: None,
            audio_stream: None,
            single_stream: true,
        });
    }
    let vq = pick_quality(play, choice.quality, allow_fallback)?;
    let video = vq.pick_stream(choice.codec)?.clone();
    let audio = play.audio_by_quality(choice.audio_quality).cloned();
    Some(StreamSelection {
        quality: vq.quality,
        codec: video.codec,
        video,
        audio_stream: audio.as_ref().map(|a| a.stream.clone()),
        audio,
        single_stream: false,
    })
}

/// 选档位：指定档位可用则用之；否则按 `allow_fallback` 取最高可用或返回 None。
pub fn pick_quality(
    play: &PlayUrl,
    wanted: Option<Quality>,
    allow_fallback: bool,
) -> Option<&VideoQuality> {
    if let Some(w) = wanted
        && let Some(v) = play.quality(w)
    {
        return Some(v);
    }
    if allow_fallback {
        play.best_quality()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{AudioFormat, PlayUrl};

    fn stream(id: u32, codec: Codec, url: &str) -> MediaStream {
        MediaStream {
            id,
            codec,
            base_url: url.into(),
            backup_urls: vec![],
            bandwidth: 1000,
            mime: "video/mp4".into(),
            codecs: String::new(),
            size: None,
            width: None,
            height: None,
            frame_rate: None,
            host: None,
        }
    }

    fn vq(q: Quality, codecs: &[Codec]) -> VideoQuality {
        VideoQuality {
            quality: q,
            streams: codecs
                .iter()
                .map(|c| stream(q.0, *c, &format!("https://cdn/{}-{}.m4s", q.0, c.as_str())))
                .collect(),
        }
    }

    fn audio(q: AudioQuality) -> AudioTrack {
        AudioTrack {
            quality: q,
            format: AudioFormat::from_quality(q),
            stream: MediaStream {
                id: q.0,
                codec: Codec::Unknown,
                base_url: format!("https://cdn/a-{}.m4s", q.0),
                backup_urls: vec![],
                bandwidth: 320000,
                mime: "audio/mp4".into(),
                codecs: String::new(),
                size: None,
                width: None,
                height: None,
                frame_rate: None,
                host: None,
            },
        }
    }

    fn play() -> PlayUrl {
        PlayUrl {
            is_dash: true,
            qualities: vec![
                vq(Quality::P1080, &[Codec::Avc, Codec::Hevc]),
                vq(Quality::P480, &[Codec::Avc]),
            ],
            audios: vec![audio(AudioQuality::K192), audio(AudioQuality::K128)],
            quality_tiers: vec![],
            single_stream: None,
            accept_quality: vec![Quality::P1080, Quality::P480],
        }
    }

    #[test]
    fn auto_picks_highest_and_avc() {
        let sel = select(&play(), StreamChoice::auto(), true).unwrap();
        assert_eq!(sel.quality, Quality::P1080);
        assert_eq!(sel.codec, Codec::Avc);
        assert_eq!(sel.audio_quality(), Some(AudioQuality::K192));
    }

    #[test]
    fn auto_keeps_hdr_and_dolby_as_highest_available() {
        let mut play = play();
        play.qualities = vec![
            vq(Quality::DOLBY, &[Codec::Avc]),
            vq(Quality::HDR, &[Codec::Avc]),
            vq(Quality::P1080_PLUS, &[Codec::Avc]),
            vq(Quality::P1080, &[Codec::Avc]),
        ];
        play.accept_quality = vec![
            Quality::DOLBY,
            Quality::HDR,
            Quality::P1080_PLUS,
            Quality::P1080,
        ];

        let auto = select(&play, StreamChoice::auto(), true).unwrap();
        assert_eq!(auto.quality, Quality::DOLBY);

        let hdr = StreamChoice {
            quality: Some(Quality::HDR),
            ..Default::default()
        };
        assert_eq!(
            select(&play, hdr, true).unwrap().quality,
            Quality::HDR,
            "显式选择 HDR 时不得降级为 1080P"
        );
    }

    #[test]
    fn requested_quality_falls_back_to_highest() {
        let choice = StreamChoice {
            quality: Some(Quality::P4K),
            ..Default::default()
        };
        let sel = select(&play(), choice, true).unwrap();
        assert_eq!(sel.quality, Quality::P1080, "不可用档位应回退到实际最高");
    }

    #[test]
    fn requested_codec_falls_back_to_avc() {
        let choice = StreamChoice {
            codec: Some(Codec::Av1),
            ..Default::default()
        };
        let sel = select(&play(), choice, true).unwrap();
        assert_eq!(sel.codec, Codec::Avc);
        // HEVC 存在时优先请求
        let choice = StreamChoice {
            codec: Some(Codec::Hevc),
            ..Default::default()
        };
        assert_eq!(select(&play(), choice, true).unwrap().codec, Codec::Hevc);
    }

    #[test]
    fn requested_audio_falls_back_to_best() {
        let choice = StreamChoice {
            audio_quality: Some(AudioQuality::FLAC),
            ..Default::default()
        };
        let sel = select(&play(), choice, true).unwrap();
        assert_eq!(sel.audio_quality(), Some(AudioQuality::K192));
        let choice = StreamChoice {
            audio_quality: Some(AudioQuality::K128),
            ..Default::default()
        };
        assert_eq!(
            select(&play(), choice, true).unwrap().audio_quality(),
            Some(AudioQuality::K128)
        );
    }

    #[test]
    fn durl_single_stream() {
        let play = PlayUrl {
            is_dash: false,
            qualities: vec![],
            audios: vec![],
            quality_tiers: vec![],
            single_stream: Some(stream(32, Codec::Unknown, "https://cdn/flv.mp4")),
            accept_quality: vec![Quality::P480],
        };
        let sel = select(&play, StreamChoice::auto(), true).unwrap();
        assert!(sel.single_stream);
        assert_eq!(sel.quality, Quality::P480);
        assert_eq!(sel.video.base_url, "https://cdn/flv.mp4");
        assert!(select(&PlayUrl::default(), StreamChoice::auto(), true).is_none());
    }

    #[test]
    fn quality_not_available_but_vip_tier_listed() {
        // 4K 档在接口宣告中但未返回流（无大会员）→ 回退 1080P
        let mut p = play();
        p.quality_tiers.push(crate::api::types::QualityTier {
            quality: Quality::P4K,
            name: Some("4K 超清".into()),
            unavailable: Some(crate::api::types::TierUnavailable::VipRequired),
        });
        assert_eq!(
            pick_quality(&p, Some(Quality::P4K), true).unwrap().quality,
            Quality::P1080
        );
    }
}
