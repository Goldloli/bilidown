//! 原子写与容错读取（settings/credentials 共用）。
//!
//! 原子写：同目录临时文件 → 写满 + fsync → 设置权限 → rename → fsync 目录。
//! 崩溃时最多留下一个临时文件，不会产生半写坏的正式文件。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::api::error::{BiliError, BiliResult};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 写入模式：普通文件 0644。
pub const MODE_FILE: u32 = 0o644;
/// 凭据文件 0600。
pub const MODE_SECRET: u32 = 0o600;

/// 原子写文件。
pub fn write_atomic(path: &Path, data: &[u8], mode: u32) -> BiliResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| BiliError::Disk(format!("invalid path: {}", path.display())))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.tmp-{}-{n}", std::process::id()));
    let result = (|| -> BiliResult<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        std::fs::rename(&tmp, path)?;
        // fsync 目录，确保 rename 落盘（macOS/Linux）
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// 原子写 JSON。
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T, mode: u32) -> BiliResult<()> {
    let data = serde_json::to_vec_pretty(value)
        .map_err(|e| BiliError::Parse(format!("serialize {}: {e}", path.display())))?;
    write_atomic(path, &data, mode)
}

/// 读 JSON：文件不存在返回 `Ok(None)`；内容损坏返回 Parse 错误（由调用方决定降级）。
pub fn read_json<T: DeserializeOwned>(path: &Path) -> BiliResult<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let value = serde_json::from_slice(&bytes)
                .map_err(|e| BiliError::Parse(format!("parse {}: {e}", path.display())))?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(BiliError::from(e)),
    }
}

/// 把损坏文件改名保留（`<name>.corrupt-<unix秒>`），返回备份路径。
pub fn backup_corrupt(path: &Path) -> Option<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path.file_name()?.to_str()?;
    let backup = path.with_file_name(format!("{name}.corrupt-{ts}"));
    std::fs::rename(path, &backup).ok()?;
    Some(backup)
}

/// 文件权限（Unix 位；非 Unix 返回 None）。
pub fn file_mode(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .ok()
            .map(|m| m.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_roundtrip_and_no_temp_left() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write_atomic(&path, b"{\"a\":1}", MODE_FILE).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":1}");
        write_atomic(&path, b"{\"a\":2}", MODE_FILE).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":2}");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件");
    }

    #[cfg(unix)]
    #[test]
    fn secret_file_mode_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        write_atomic(&path, b"{}", MODE_SECRET).unwrap();
        assert_eq!(file_mode(&path), Some(0o600));
    }

    #[test]
    fn read_json_missing_is_none_and_corrupt_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        assert!(read_json::<serde_json::Value>(&path).unwrap().is_none());
        std::fs::write(&path, b"{not json").unwrap();
        assert!(read_json::<serde_json::Value>(&path).is_err());
        let backup = backup_corrupt(&path).expect("应生成备份");
        assert!(backup.exists());
        assert!(!path.exists());
        assert!(read_json::<serde_json::Value>(&path).unwrap().is_none());
    }

    #[test]
    fn write_json_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct X {
            a: u32,
        }
        let path = dir.path().join("x.json");
        write_json_atomic(&path, &X { a: 7 }, MODE_SECRET).unwrap();
        let back: Option<X> = read_json(&path).unwrap();
        assert_eq!(back, Some(X { a: 7 }));
    }
}
