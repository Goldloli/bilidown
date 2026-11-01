//! 应用数据目录（`~/Library/Application Support/bilidown/`）。

use std::path::{Path, PathBuf};

use crate::api::error::{BiliError, BiliResult};

/// 应用目录名（macOS）。
pub const APP_DIR_NAME: &str = "bilidown";
/// Windows 数据目录名：与 NSIS 默认安装目录（`%LOCALAPPDATA%\bilidown`）错开，
/// 避免程序文件与数据库/缓存混在同一目录、卸载时误删。
#[cfg(target_os = "windows")]
pub const APP_DIR_NAME_WINDOWS: &str = "bilidown-data";
/// 环境变量覆盖（测试/便携场景）。
pub const DATA_DIR_ENV: &str = "BILIDOWN_DATA_DIR";

/// 应用数据目录布局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// 标准位置：Windows 为 `%LOCALAPPDATA%\bilidown-data`（与安装目录分离、不放漫游目录），
    /// macOS 为 `~/Library/Application Support/bilidown`。
    pub fn standard() -> BiliResult<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::new(PathBuf::from(dir)));
        }
        #[cfg(target_os = "windows")]
        {
            let base = dirs::data_local_dir()
                .ok_or_else(|| BiliError::Disk("cannot resolve local app data directory".into()))?;
            return Ok(Self::new(base.join(APP_DIR_NAME_WINDOWS)));
        }
        #[cfg(not(target_os = "windows"))]
        {
            let base = dirs::data_dir().ok_or_else(|| {
                BiliError::Disk("cannot resolve application data directory".into())
            })?;
            Ok(Self::new(base.join(APP_DIR_NAME)))
        }
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.root.join("credentials.json")
    }

    pub fn database_file(&self) -> PathBuf {
        self.root.join("queue.sqlite")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.root.join("covers")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// 任务临时目录根（下载/合并中间文件；任务层按 task-id 再隔离）。
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// 创建目录并收紧权限（目录 0700）。
    pub fn ensure(&self) -> BiliResult<()> {
        for dir in [
            self.root.clone(),
            self.covers_dir(),
            self.logs_dir(),
            self.tmp_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(())
    }
}

/// v1 配置目录（`~/.bilidown`），仅用于一次性迁移。
pub fn legacy_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".bilidown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_under_root() {
        let p = AppPaths::new("/tmp/bilidown-test");
        assert_eq!(
            p.settings_file(),
            PathBuf::from("/tmp/bilidown-test/settings.json")
        );
        assert_eq!(
            p.credentials_file(),
            PathBuf::from("/tmp/bilidown-test/credentials.json")
        );
        assert_eq!(
            p.database_file(),
            PathBuf::from("/tmp/bilidown-test/queue.sqlite")
        );
        assert_eq!(p.tmp_dir(), PathBuf::from("/tmp/bilidown-test/tmp"));
    }

    #[test]
    fn ensure_creates_dirs_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("app"));
        paths.ensure().unwrap();
        assert!(paths.covers_dir().is_dir());
        assert!(paths.logs_dir().is_dir());
        assert!(paths.tmp_dir().is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(paths.root())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }
}
