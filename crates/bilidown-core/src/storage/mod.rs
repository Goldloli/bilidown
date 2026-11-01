//! 持久化层（tasks 3.1/3.2、design D4）：
//!
//! - 应用数据目录 [`AppPaths`]：`~/Library/Application Support/bilidown/`；
//! - `settings.json`：人类可读、原子写；
//! - `credentials.json`：cookie + 设备凭据，0600 + 原子写；
//! - `queue.sqlite`：SQLite（WAL）任务与历史，`PRAGMA user_version` 增量迁移；
//! - v1 `~/.bilidown/` 一次性迁移（保留旧文件）。

pub mod credentials;
pub mod db;
pub mod fsutil;
pub mod migrate;
pub mod paths;
pub mod settings;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::api::client::{DeviceCredentials, StoredCookies};
use crate::api::error::{BiliError, BiliResult};

pub use credentials::{Credentials, LOGIN_COOKIE_KEYS};
pub use db::{Db, HistoryQuery, HistoryRecord, SCHEMA_VERSION, TaskRecord};
pub use migrate::MigrationReport;
pub use paths::AppPaths;
pub use settings::{MotionPreference, Settings, ThemePreference, ValidationError};

/// 组合存储句柄：路径 + 数据库。设置/凭据按需读写文件（频率低）。
pub struct Storage {
    paths: AppPaths,
    db: Arc<Db>,
    migration: MigrationReport,
}

impl Storage {
    /// 打开存储：创建目录 → 迁移 v1（如有）→ 打开/迁移 SQLite。
    pub fn open(paths: AppPaths) -> BiliResult<Self> {
        Self::open_with_legacy(paths, paths::legacy_config_dir().as_deref())
    }

    /// 显式指定 v1 目录（测试/便携场景；None = 不迁移）。
    pub fn open_with_legacy(paths: AppPaths, legacy: Option<&Path>) -> BiliResult<Self> {
        paths.ensure()?;
        let migration = match legacy.filter(|d| d.is_dir()) {
            Some(dir) => migrate::migrate_from(&paths, dir),
            None => MigrationReport::default(),
        };
        if migration.anything_migrated() {
            tracing::info!(from = ?migration.from_dir, notes = ?migration.notes, "migrated v1 data");
        }
        let db = Arc::new(Db::open(&paths.database_file())?);
        Ok(Self {
            paths,
            db,
            migration,
        })
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    pub fn migration_report(&self) -> &MigrationReport {
        &self.migration
    }

    /// 读取设置：损坏时备份并返回默认值（不阻塞启动）。
    pub fn load_settings(&self) -> Settings {
        match fsutil::read_json::<Settings>(&self.paths.settings_file()) {
            Ok(Some(s)) => s,
            Ok(None) => Settings::default(),
            Err(e) => {
                tracing::warn!(error = %e, "settings corrupt, backing up and using defaults");
                fsutil::backup_corrupt(&self.paths.settings_file());
                Settings::default()
            }
        }
    }

    /// 保存设置（原子写）。调用方应先 [`Settings::validate`]。
    pub fn save_settings(&self, settings: &Settings) -> BiliResult<()> {
        settings
            .validate()
            .map_err(|errors| BiliError::Other(format!("invalid settings: {:?}", errors)))?;
        fsutil::write_json_atomic(&self.paths.settings_file(), settings, fsutil::MODE_FILE)
    }

    pub fn load_credentials(&self) -> Credentials {
        credentials::load(&self.paths.credentials_file())
    }

    /// 保存凭据（0600 + 原子写）。
    pub fn save_credentials(&self, creds: &Credentials) -> BiliResult<()> {
        credentials::save(&self.paths.credentials_file(), creds)
    }

    pub fn clear_credentials(&self) -> BiliResult<()> {
        credentials::clear(&self.paths.credentials_file())
    }

    /// 应用缓存临时目录（任务按 task-id 再隔离）。
    pub fn tmp_dir(&self) -> PathBuf {
        self.paths.tmp_dir()
    }

    /// 当前登录 cookie（供 tasks/account 组装）。
    pub fn stored_cookies(&self) -> StoredCookies {
        self.load_credentials().stored()
    }

    /// 读取设备凭据。
    pub fn device_credentials(&self) -> DeviceCredentials {
        self.load_credentials().device
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Storage::open_with_legacy(paths, None).unwrap();
        (dir, storage)
    }

    #[test]
    fn open_creates_layout_and_db() {
        let (_dir, storage) = storage();
        assert!(storage.paths().database_file().exists() || storage.paths().root().exists());
        assert_eq!(storage.db().user_version().unwrap(), SCHEMA_VERSION);
        assert!(storage.paths().logs_dir().is_dir());
        assert!(storage.tmp_dir().is_dir());
    }

    #[test]
    fn settings_roundtrip_and_corrupt_recovery() {
        let (_dir, storage) = storage();
        assert_eq!(storage.load_settings(), Settings::default());
        let s = Settings {
            max_concurrent: 5,
            theme: ThemePreference::Dark,
            ..Default::default()
        };
        storage.save_settings(&s).unwrap();
        assert_eq!(storage.load_settings(), s);
        // 无效设置拒绝保存
        let bad = Settings {
            max_concurrent: 99,
            ..Default::default()
        };
        assert!(storage.save_settings(&bad).is_err());
        // 损坏文件 → 备份 + 默认值
        std::fs::write(storage.paths().settings_file(), b"{broken").unwrap();
        assert_eq!(storage.load_settings(), Settings::default());
        let backups: Vec<_> = std::fs::read_dir(storage.paths().root())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn credentials_roundtrip_via_storage() {
        let (_dir, storage) = storage();
        assert!(storage.load_credentials().is_empty());
        let creds = Credentials::from_pairs(
            vec![("SESSDATA".to_string(), "s".to_string())],
            DeviceCredentials {
                buvid3: Some("b3".into()),
                ..Default::default()
            },
        );
        storage.save_credentials(&creds).unwrap();
        let loaded = storage.load_credentials();
        assert!(loaded.has_login());
        assert_eq!(loaded.device.buvid3.as_deref(), Some("b3"));
        assert_eq!(storage.stored_cookies().get("SESSDATA"), Some("s"));
        storage.clear_credentials().unwrap();
        assert!(storage.load_credentials().is_empty());
    }
}
