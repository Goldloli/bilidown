//! 登录凭据：`credentials.json`（0600 + 原子写）。
//!
//! 包含登录 cookie、基础风控凭据（buvid/_uuid/bili_ticket）与持久化刷新口令
//! `refresh_token`（cookie 自动续期用，design D4）。日志与错误信息不得包含
//! cookie/token 值（user-login spec「不泄漏」）。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::api::client::{DeviceCredentials, StoredCookies};

use super::fsutil::{MODE_SECRET, read_json, write_json_atomic};

/// 需要持久化的关键登录 cookie。
pub const LOGIN_COOKIE_KEYS: [&str; 10] = [
    "SESSDATA",
    "bili_jct",
    "DedeUserID",
    "DedeUserID__ckMd5",
    "sid",
    "buvid3",
    "buvid4",
    "b_nut",
    "bili_ticket",
    "bili_ticket_expires",
];

/// 持久化凭据文件形状。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(default)]
    pub cookies: Vec<(String, String)>,
    #[serde(default)]
    pub device: DeviceCredentials,
    /// 持久化刷新口令（登录响应返回；cookie 自动续期用）。
    /// 旧版本凭据文件没有该字段，`serde(default)` 保证反序列化兼容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// 最近保存时间（Unix 秒）
    #[serde(default)]
    pub updated_at: Option<i64>,
}

impl Credentials {
    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (String, String)>,
        device: DeviceCredentials,
    ) -> Self {
        Self {
            cookies: pairs.into_iter().collect(),
            device,
            refresh_token: None,
            updated_at: None,
        }
    }

    /// 只保留需要持久化的登录 cookie（其余会话级 cookie 不落盘）。
    pub fn filtered(cookies: &[(String, String)], device: DeviceCredentials) -> Self {
        let keep: Vec<(String, String)> = cookies
            .iter()
            .filter(|(k, v)| !v.is_empty() && LOGIN_COOKIE_KEYS.contains(&k.as_str()))
            .cloned()
            .collect();
        Self {
            cookies: keep,
            device,
            refresh_token: None,
            updated_at: None,
        }
    }

    pub fn has_login(&self) -> bool {
        self.stored().has_login()
    }

    pub fn stored(&self) -> StoredCookies {
        StoredCookies {
            cookies: self.cookies.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty() && self.device.is_empty() && self.refresh_token.is_none()
    }

    /// 从 cookie 中提取基础凭据（buvid3/buvid4/_uuid/b_nut）。
    pub fn device_from_cookies(&self) -> DeviceCredentials {
        let get = |name: &str| {
            self.cookies
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };
        DeviceCredentials {
            buvid3: get("buvid3"),
            buvid4: get("buvid4"),
            uuid: get("_uuid"),
            b_nut: get("b_nut"),
        }
    }
}

/// 读取凭据：不存在返回默认值；损坏时备份并返回默认值（不阻塞启动）。
pub fn load(path: &Path) -> Credentials {
    match read_json::<Credentials>(path) {
        Ok(Some(c)) => c,
        Ok(None) => Credentials::default(),
        Err(e) => {
            tracing::warn!(error = %e, "credentials file corrupt, backing up");
            let backup = super::fsutil::backup_corrupt(path);
            tracing::warn!(backup = ?backup, "credentials backed up");
            Credentials::default()
        }
    }
}

/// 保存凭据（0600）。
pub fn save(path: &Path, creds: &Credentials) -> crate::api::error::BiliResult<()> {
    let mut creds = creds.clone();
    creds.updated_at = Some(now_secs());
    write_json_atomic(path, &creds, MODE_SECRET)
}

/// 清除凭据文件（登出）。
pub fn clear(path: &Path) -> crate::api::error::BiliResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Credentials {
        Credentials::from_pairs(
            vec![
                ("SESSDATA".to_string(), "s3cr3t".to_string()),
                ("bili_jct".to_string(), "csrf".to_string()),
                ("buvid3".to_string(), "b3".to_string()),
            ],
            DeviceCredentials::default(),
        )
    }

    #[test]
    fn roundtrip_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let creds = sample();
        save(&path, &creds).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.cookies, creds.cookies);
        assert!(loaded.has_login());
        assert!(loaded.updated_at.is_some());
        #[cfg(unix)]
        assert_eq!(super::super::fsutil::file_mode(&path), Some(0o600));
    }

    #[test]
    fn roundtrip_with_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let mut creds = sample();
        creds.refresh_token = Some("rt-secret".to_string());
        save(&path, &creds).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.refresh_token.as_deref(), Some("rt-secret"));
        assert_eq!(loaded.cookies, creds.cookies);
    }

    #[test]
    fn legacy_credentials_without_refresh_token_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        // 旧版本凭据文件：没有 refresh_token 字段
        std::fs::write(
            &path,
            r#"{"cookies":[["SESSDATA","s"],["bili_jct","c"]],"device":{"buvid3":"b3"},"updated_at":1700000000}"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert!(loaded.has_login());
        assert!(loaded.refresh_token.is_none(), "旧凭据反序列化应兼容");
        assert_eq!(loaded.device.buvid3.as_deref(), Some("b3"));
    }

    #[test]
    fn filtered_drops_token_but_keeps_risk_cookies() {
        let cookies = vec![
            ("SESSDATA".to_string(), "s".to_string()),
            ("bili_ticket".to_string(), "tk".to_string()),
            ("bili_ticket_expires".to_string(), "1700000000".to_string()),
            ("buvid3".to_string(), "b3".to_string()),
            ("random_session".to_string(), "x".to_string()),
        ];
        let mut c = Credentials::filtered(&cookies, DeviceCredentials::default());
        assert_eq!(
            c.cookies.len(),
            4,
            "buvid3 + bili_ticket* 应持久化，随机 cookie 不落盘"
        );
        assert!(
            c.refresh_token.is_none(),
            "filtered 不携带 token，由账号层显式写入"
        );
        c.refresh_token = Some("rt".to_string());
        assert!(!c.is_empty());
    }

    #[test]
    fn filters_session_cookies() {
        let cookies = vec![
            ("SESSDATA".to_string(), "a".to_string()),
            ("random_session".to_string(), "b".to_string()),
            ("bili_jct".to_string(), "c".to_string()),
        ];
        let c = Credentials::filtered(&cookies, DeviceCredentials::default());
        assert_eq!(c.cookies.len(), 2);
        assert!(!c.cookies.iter().any(|(k, _)| k == "random_session"));
    }

    #[test]
    fn corrupt_file_backs_up_and_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        std::fs::write(&path, b"{broken").unwrap();
        let loaded = load(&path);
        assert!(loaded.is_empty());
        assert!(!path.exists());
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn device_from_cookies() {
        let creds = Credentials::from_pairs(
            vec![
                ("buvid3".to_string(), "b3".to_string()),
                ("_uuid".to_string(), "u".to_string()),
            ],
            DeviceCredentials::default(),
        );
        let d = creds.device_from_cookies();
        assert_eq!(d.buvid3.as_deref(), Some("b3"));
        assert_eq!(d.uuid.as_deref(), Some("u"));
        assert!(d.buvid4.is_none());
    }

    #[test]
    fn clear_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        clear(&path).unwrap();
        save(&path, &sample()).unwrap();
        clear(&path).unwrap();
        assert!(!path.exists());
        clear(&path).unwrap();
    }
}
