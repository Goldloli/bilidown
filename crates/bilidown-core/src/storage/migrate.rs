//! v1 → v2 数据迁移（design D14 / desktop-shell spec）：
//! 首次启动读取 `~/.bilidown/settings.toml` 与 `~/.bilidown/cookies.json`，
//! 映射到新数据目录；**保留旧文件**，迁移结果写入日志。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::api::client::DeviceCredentials;

use super::credentials::{self, Credentials};
use super::fsutil::MODE_FILE;
use super::paths::AppPaths;
use super::settings::{Settings, ThemePreference};

/// v1 settings.toml 形状（只取可识别字段）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct V1Settings {
    theme: Option<String>,
    download_dir: Option<String>,
    max_concurrent: Option<usize>,
    default_quality: Option<u32>,
    audio_to_mp3: Option<bool>,
    write_metadata: Option<bool>,
    ffmpeg_path: Option<String>,
    proxy: Option<String>,
}

/// v1 cookies.json 形状。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct V1Cookies {
    cookies: Vec<(String, String)>,
}

/// 迁移结果（写入日志/诊断信息，不包含 cookie 值）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MigrationReport {
    pub from_dir: Option<PathBuf>,
    pub settings_migrated: bool,
    pub credentials_migrated: bool,
    pub notes: Vec<String>,
}

impl MigrationReport {
    pub fn anything_migrated(&self) -> bool {
        self.settings_migrated || self.credentials_migrated
    }
}

/// 从指定 v1 目录迁移到 v2 数据目录（幂等：已存在 v2 文件则跳过，不覆盖）。
pub fn migrate_from(paths: &AppPaths, legacy_dir: &Path) -> MigrationReport {
    let mut report = MigrationReport {
        from_dir: Some(legacy_dir.to_path_buf()),
        ..Default::default()
    };
    if !legacy_dir.is_dir() {
        return report;
    }
    // 1) settings.toml → settings.json
    let legacy_settings = legacy_dir.join("settings.toml");
    if !paths.settings_file().exists() && legacy_settings.is_file() {
        match std::fs::read_to_string(&legacy_settings)
            .map_err(|e| e.to_string())
            .and_then(|s| toml::from_str::<V1Settings>(&s).map_err(|e| e.to_string()))
        {
            Ok(v1) => {
                let settings = map_settings(&v1);
                match super::fsutil::write_json_atomic(&paths.settings_file(), &settings, MODE_FILE)
                {
                    Ok(()) => {
                        report.settings_migrated = true;
                        report.notes.push(format!(
                            "settings migrated (theme={:?}, max_concurrent={})",
                            settings.theme, settings.max_concurrent
                        ));
                    }
                    Err(e) => report.notes.push(format!("settings write failed: {e}")),
                }
            }
            Err(e) => report
                .notes
                .push(format!("settings.toml parse failed: {e}")),
        }
    }
    // 2) cookies.json → credentials.json
    let legacy_cookies = legacy_dir.join("cookies.json");
    if !paths.credentials_file().exists() && legacy_cookies.is_file() {
        match std::fs::read_to_string(&legacy_cookies)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<V1Cookies>(&s).map_err(|e| e.to_string()))
        {
            Ok(v1) => {
                let mut creds = Credentials::filtered(&v1.cookies, DeviceCredentials::default());
                creds.device = creds.device_from_cookies();
                match credentials::save(&paths.credentials_file(), &creds) {
                    Ok(()) => {
                        report.credentials_migrated = true;
                        // 只报告 cookie 数量/名字，不记录值
                        let names: Vec<&str> =
                            creds.cookies.iter().map(|(k, _)| k.as_str()).collect();
                        report.notes.push(format!(
                            "credentials migrated: {} cookies {:?}",
                            creds.cookies.len(),
                            names
                        ));
                    }
                    Err(e) => report.notes.push(format!("credentials write failed: {e}")),
                }
            }
            Err(e) => report.notes.push(format!("cookies.json parse failed: {e}")),
        }
    }
    report
}

fn map_settings(v1: &V1Settings) -> Settings {
    let defaults = Settings::default();
    Settings {
        theme: match v1.theme.as_deref() {
            Some("Dark") | Some("dark") => ThemePreference::Dark,
            Some("Light") | Some("light") => ThemePreference::Light,
            _ => ThemePreference::System,
        },
        download_dir: v1.download_dir.clone().filter(|d| !d.trim().is_empty()),
        max_concurrent: v1
            .max_concurrent
            .unwrap_or(defaults.max_concurrent)
            .clamp(1, 8),
        // v1 的 Some(0) 表示「自动」，映射为 None
        default_quality: v1.default_quality.filter(|q| *q != 0),
        audio_to_mp3: v1.audio_to_mp3.unwrap_or(defaults.audio_to_mp3),
        write_metadata: v1.write_metadata.unwrap_or(defaults.write_metadata),
        ffmpeg_path: v1.ffmpeg_path.clone().filter(|p| !p.trim().is_empty()),
        proxy: v1.proxy.clone().filter(|p| !p.trim().is_empty()),
        ..defaults
    }
}

/// 若 v1 目录存在则执行迁移（[`super::Storage::open`] 内部调用）。
pub fn migrate_if_present(paths: &AppPaths) -> MigrationReport {
    match super::paths::legacy_config_dir() {
        Some(dir) if dir.is_dir() => migrate_from(paths, &dir),
        _ => MigrationReport::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_v1(dir: &Path) {
        std::fs::write(
            dir.join("settings.toml"),
            r#"
theme = "Dark"
download_dir = "/tmp/v1-downloads"
max_concurrent = 2
default_quality = 80
audio_to_mp3 = true
write_metadata = false
ffmpeg_path = "/opt/homebrew/bin/ffmpeg"
proxy = "http://127.0.0.1:7890"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("cookies.json"),
            r#"{"cookies":[["SESSDATA","secret"],["bili_jct","csrf"],["random_session","x"],["buvid3","b3"]]}"#,
        )
        .unwrap();
    }

    #[test]
    fn migrates_settings_and_credentials_without_deleting_legacy() {
        let root = tempfile::tempdir().unwrap();
        let legacy = tempfile::tempdir().unwrap();
        write_v1(legacy.path());
        let paths = AppPaths::new(root.path().join("app"));
        paths.ensure().unwrap();
        let report = migrate_from(&paths, legacy.path());
        assert!(report.settings_migrated);
        assert!(report.credentials_migrated);
        // 旧文件保留
        assert!(legacy.path().join("settings.toml").exists());
        assert!(legacy.path().join("cookies.json").exists());
        // 新设置
        let s: Settings = super::super::fsutil::read_json(&paths.settings_file())
            .unwrap()
            .unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.max_concurrent, 2);
        assert_eq!(s.default_quality, Some(80));
        assert!(s.audio_to_mp3);
        assert!(!s.write_metadata);
        assert_eq!(s.download_dir.as_deref(), Some("/tmp/v1-downloads"));
        // 新凭据：只保留登录相关 cookie，设备凭据抽出
        let creds = credentials::load(&paths.credentials_file());
        assert_eq!(creds.cookies.len(), 3);
        assert!(!creds.cookies.iter().any(|(k, _)| k == "random_session"));
        assert_eq!(creds.device.buvid3.as_deref(), Some("b3"));
        // 0600
        #[cfg(unix)]
        assert_eq!(
            super::super::fsutil::file_mode(&paths.credentials_file()),
            Some(0o600)
        );
    }

    #[test]
    fn does_not_overwrite_existing_v2_files() {
        let root = tempfile::tempdir().unwrap();
        let legacy = tempfile::tempdir().unwrap();
        write_v1(legacy.path());
        let paths = AppPaths::new(root.path().join("app"));
        paths.ensure().unwrap();
        let existing = Settings {
            max_concurrent: 6,
            ..Default::default()
        };
        super::super::fsutil::write_json_atomic(&paths.settings_file(), &existing, MODE_FILE)
            .unwrap();
        let report = migrate_from(&paths, legacy.path());
        assert!(!report.settings_migrated);
        assert!(report.credentials_migrated);
        let s: Settings = super::super::fsutil::read_json(&paths.settings_file())
            .unwrap()
            .unwrap();
        assert_eq!(s.max_concurrent, 6);
    }

    #[test]
    fn missing_legacy_dir_is_noop() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(root.path().join("app"));
        paths.ensure().unwrap();
        let report = migrate_from(&paths, &root.path().join("nope"));
        assert!(!report.anything_migrated());
    }

    #[test]
    fn clamps_invalid_v1_concurrency() {
        let v1 = V1Settings {
            max_concurrent: Some(9),
            default_quality: Some(0),
            ..Default::default()
        };
        let s = map_settings(&v1);
        assert_eq!(s.max_concurrent, 8);
        assert_eq!(s.default_quality, None);
    }
}
