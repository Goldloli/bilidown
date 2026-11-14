//! 应用设置：`settings.json`（人类可读、原子写）。
//!
//! 注意：core 不依赖 UI；`ThemePreference` 只是**数据**（system/light/dark），
//! 由前端渲染与跟随系统。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::api::types::Codec;

/// 主题偏好（数据；渲染由前端负责）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// 设置校验错误（前端按 `field` 做行内报错）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: &'static str,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// 界面动态效果偏好（design D2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionPreference {
    /// 跟随系统「减弱动态效果」
    #[default]
    System,
    /// 强制减弱
    Reduce,
    /// 保持完整动效
    Normal,
}

/// 软件更新链路使用的代理策略；媒体代理不读取此设置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateProxyMode {
    /// 跟随操作系统当前 HTTP/HTTPS/SOCKS、PAC/WPAD 与绕过规则。
    #[default]
    System,
    /// 更新请求显式直连。
    Direct,
    /// 仅使用更新专用手动代理。
    Manual,
}

/// 应用设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 下载目录（None = 系统「下载」目录）
    pub download_dir: Option<String>,
    /// 同时下载任务数（1–8，默认 3）
    pub max_concurrent: usize,
    /// 默认清晰度（qn；None = 最高可用）
    pub default_quality: Option<u32>,
    /// 默认视频编码（None = 优先 AVC）
    pub default_codec: Option<Codec>,
    /// 音频任务默认转 mp3
    pub audio_to_mp3: bool,
    /// 合并时写入标题等元数据
    pub write_metadata: bool,
    /// ffmpeg 显式路径（None = 自动探测）
    pub ffmpeg_path: Option<String>,
    /// HTTP 代理（显式设置才启用）
    pub proxy: Option<String>,
    /// 软件更新代理策略（默认跟随系统；媒体请求不读取）
    pub update_proxy_mode: UpdateProxyMode,
    /// 软件更新手动代理 URL；切换 system/direct 时保留
    pub update_proxy: Option<String>,
    /// 主题偏好
    pub theme: ThemePreference,
    /// 强调色（`#RRGGBB`；None = 默认蓝色 #3B82F6）
    pub accent_color: Option<String>,
    /// 动态效果偏好
    pub motion: MotionPreference,
    /// 完成/失败通知
    pub notify_on_done: bool,
    /// 监听剪贴板（仅提示，不自动下载）
    pub watch_clipboard: bool,
    /// 历史记录上限（默认 500）
    pub history_limit: usize,
    /// PCDN 过滤（默认开）
    pub pcdn_filter: bool,
    /// ffmpeg 缺失时允许「仅保存流文件」降级
    pub allow_streams_only: bool,
    /// 视频/番剧/课程完成后下载字幕 SRT（默认关，design D2）
    pub download_subtitle: bool,
    /// 视频/番剧/课程完成后下载封面 jpg（默认关，design D2）
    pub download_cover: bool,
    /// 启动时自动检查应用更新（默认开；不自动安装，design D5）
    pub check_updates_on_startup: bool,
    /// 指定档位不可用时是否自动回退到实际可用档位（默认开）
    pub quality_fallback: bool,
    /// 窗口磨砂玻璃背景（默认开；仅 macOS 原生窗口生效）
    pub window_glass: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: None,
            max_concurrent: 3,
            default_quality: None,
            default_codec: None,
            audio_to_mp3: false,
            write_metadata: true,
            ffmpeg_path: None,
            proxy: None,
            update_proxy_mode: UpdateProxyMode::System,
            update_proxy: None,
            theme: ThemePreference::System,
            accent_color: None,
            motion: MotionPreference::System,
            notify_on_done: true,
            watch_clipboard: false,
            history_limit: 500,
            pcdn_filter: true,
            allow_streams_only: false,
            download_subtitle: false,
            download_cover: false,
            check_updates_on_startup: true,
            quality_fallback: true,
            window_glass: true,
        }
    }
}

impl Settings {
    /// 有效下载目录。
    pub fn effective_download_dir(&self) -> PathBuf {
        self.download_dir
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::download_dir()
                    .or_else(dirs::home_dir)
                    .unwrap_or_else(|| PathBuf::from("."))
            })
    }

    /// 校验；返回全部错误（前端逐字段显示）。
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        if !(1..=8).contains(&self.max_concurrent) {
            errors.push(ValidationError {
                field: "max_concurrent",
                message: "must be between 1 and 8".into(),
            });
        }
        if let Some(q) = self.default_quality
            && q != 0
            && !(16..=127).contains(&q)
        {
            errors.push(ValidationError {
                field: "default_quality",
                message: "unknown quality value".into(),
            });
        }
        if let Some(p) = self
            .ffmpeg_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !std::path::Path::new(p).is_file()
        {
            errors.push(ValidationError {
                field: "ffmpeg_path",
                message: format!("file not found: {p}"),
            });
        }
        if let Some(accent) = self.accent_color.as_deref() {
            let valid = accent.len() == 7
                && accent.starts_with('#')
                && accent[1..].chars().all(|c| c.is_ascii_hexdigit());
            if !valid {
                errors.push(ValidationError {
                    field: "accent_color",
                    message: "must be #RRGGBB".into(),
                });
            }
        }
        if let Some(proxy) = self
            .proxy
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && reqwest::Proxy::all(proxy).is_err()
        {
            errors.push(ValidationError {
                field: "proxy",
                message: format!("invalid proxy url: {proxy}"),
            });
        }
        match self.update_proxy_mode {
            UpdateProxyMode::Manual => {
                let invalid = self
                    .update_proxy
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none_or(|proxy| reqwest::Proxy::all(proxy).is_err());
                if invalid {
                    errors.push(ValidationError {
                        field: "update_proxy",
                        message: "manual update proxy requires a valid proxy URL".into(),
                    });
                }
            }
            UpdateProxyMode::System | UpdateProxyMode::Direct => {}
        }
        if let Some(dir) = self
            .download_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !std::path::Path::new(dir).is_absolute()
        {
            errors.push(ValidationError {
                field: "download_dir",
                message: "must be an absolute path".into(),
            });
        }
        if !(10..=10_000).contains(&self.history_limit) {
            errors.push(ValidationError {
                field: "history_limit",
                message: "must be between 10 and 10000".into(),
            });
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// 应用设置中的编码默认值。
    pub fn default_codec_or_avc(&self) -> Codec {
        self.default_codec.unwrap_or(Codec::Avc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        let s = Settings::default();
        assert!(s.validate().is_ok());
        assert_eq!(s.max_concurrent, 3);
        assert_eq!(s.history_limit, 500);
        assert!(s.pcdn_filter);
    }

    #[test]
    fn rejects_invalid_fields() {
        let cases = [
            Settings {
                max_concurrent: 0,
                ..Default::default()
            },
            Settings {
                max_concurrent: 9,
                ..Default::default()
            },
            Settings {
                default_quality: Some(15),
                ..Default::default()
            },
            Settings {
                ffmpeg_path: Some("/nonexistent/ffmpeg".into()),
                ..Default::default()
            },
            Settings {
                proxy: Some("not a proxy".into()),
                ..Default::default()
            },
            Settings {
                update_proxy_mode: UpdateProxyMode::Manual,
                update_proxy: Some("not a proxy".into()),
                ..Default::default()
            },
            Settings {
                download_dir: Some("relative/path".into()),
                ..Default::default()
            },
            Settings {
                history_limit: 1,
                ..Default::default()
            },
        ];
        for s in cases {
            assert!(s.validate().is_err(), "应拒绝: {s:?}");
        }
        assert!(
            Settings {
                max_concurrent: 8,
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        assert!(
            Settings {
                default_quality: Some(120),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        assert!(
            Settings {
                default_quality: Some(0),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compat() {
        let json = serde_json::json!({
            "max_concurrent": 2,
            "future_field": {"x": 1}
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.max_concurrent, 2);
        assert_eq!(s.theme, ThemePreference::System);
    }

    #[test]
    fn effective_download_dir_falls_back() {
        let s = Settings {
            download_dir: Some("/tmp/downloads".into()),
            ..Default::default()
        };
        assert_eq!(s.effective_download_dir(), PathBuf::from("/tmp/downloads"));
        let s = Settings {
            download_dir: Some("   ".into()),
            ..Default::default()
        };
        assert!(s.effective_download_dir().is_absolute());
    }

    #[test]
    fn subtitle_cover_toggles_default_off() {
        let s = Settings::default();
        assert!(!s.download_subtitle);
        assert!(!s.download_cover);
        assert!(s.check_updates_on_startup);
        assert!(s.quality_fallback);
        assert!(s.window_glass);
        assert_eq!(s.accent_color, None);
        assert_eq!(s.motion, MotionPreference::System);
        assert_eq!(s.update_proxy_mode, UpdateProxyMode::System);
        assert_eq!(s.update_proxy, None);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn legacy_json_without_toggles_deserializes() {
        // 旧 settings.json（无 download_subtitle/download_cover 字段）必须兼容
        let legacy = serde_json::json!({
            "max_concurrent": 2,
            "audio_to_mp3": true,
            "theme": "dark",
            "proxy": "http://127.0.0.1:7897"
        });
        let s: Settings = serde_json::from_value(legacy).unwrap();
        assert_eq!(s.max_concurrent, 2);
        assert!(!s.download_subtitle, "旧配置缺字段应回退 false");
        assert!(!s.download_cover);
        assert_eq!(s.update_proxy_mode, UpdateProxyMode::System);
        assert_eq!(s.update_proxy, None);
        assert_eq!(s.proxy.as_deref(), Some("http://127.0.0.1:7897"));
        // 显式开启往返
        let s = Settings {
            download_subtitle: true,
            download_cover: true,
            update_proxy_mode: UpdateProxyMode::Manual,
            update_proxy: Some("http://127.0.0.1:7897".into()),
            ..Default::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(back.download_subtitle && back.download_cover);
        assert_eq!(back.update_proxy_mode, UpdateProxyMode::Manual);
        assert_eq!(back.update_proxy.as_deref(), Some("http://127.0.0.1:7897"));
    }

    #[test]
    fn accent_color_validates_hex() {
        let ok = Settings {
            accent_color: Some("#3B82F6".into()),
            ..Default::default()
        };
        assert!(ok.validate().is_ok());
        let none = Settings {
            accent_color: None,
            ..Default::default()
        };
        assert!(none.validate().is_ok());
        for bad in ["#12345", "3B82F6", "#GGGGGG", "#1234567"] {
            let s = Settings {
                accent_color: Some(bad.into()),
                ..Default::default()
            };
            assert!(s.validate().is_err(), "应拒绝 accent={bad}");
        }
    }

    #[test]
    fn legacy_json_without_accent_motion_deserializes() {
        let legacy = serde_json::json!({"max_concurrent": 3, "theme": "light"});
        let s: Settings = serde_json::from_value(legacy).unwrap();
        assert_eq!(s.accent_color, None);
        assert_eq!(s.motion, MotionPreference::System);
        assert!(s.quality_fallback);
        assert!(s.window_glass);
    }

    #[test]
    fn serde_roundtrip() {
        let s = Settings {
            theme: ThemePreference::Dark,
            default_codec: Some(Codec::Hevc),
            max_concurrent: 5,
            accent_color: Some("#10B981".into()),
            motion: MotionPreference::Reduce,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
