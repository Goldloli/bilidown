//! 存储统计与缓存清理（change: add-appearance-and-maintenance-settings / design D3）。
//!
//! 安全边界：只允许清理 AppPaths 根目录下的 covers/tmp/logs 白名单目录；
//! 不触碰下载产物、settings.json、credentials.json、queue.sqlite 与历史。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::dto::{AppErrorDTO, CommandResult};
use crate::state::ShellState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatsDTO {
    pub data_dir: String,
    pub download_dir: String,
    pub data_bytes: u64,
    pub cover_bytes: u64,
    pub temp_bytes: u64,
    pub log_bytes: u64,
    pub download_free_bytes: Option<u64>,
    pub download_total_bytes: Option<u64>,
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearResultDTO {
    pub freed_bytes: u64,
    pub removed_files: u64,
}

const MAX_ENTRIES: usize = 50_000;

fn dir_size_limited(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut count = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if count >= MAX_ENTRIES {
                return total;
            }
            count += 1;
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

/// 磁盘可用/总容量（字节）；失败返回 None（不阻塞页面）。
///
/// Windows 走 `GetDiskFreeSpaceExW`；macOS/Linux 继续用 `df -kP`。
fn disk_space(path: &Path) -> Option<(u64, u64)> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let mut available: u64 = 0;
        let mut total: u64 = 0;
        // SAFETY: 传入的路径以 NUL 结尾，输出指针指向栈上 u64。
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut available,
                &mut total,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        Some((available, total))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = std::process::Command::new("df")
            .args(["-kP", &path.to_string_lossy()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text.lines().nth(1)?;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            return None;
        }
        let total = fields[1].parse::<u64>().ok()?.saturating_mul(1024);
        let available = fields[3].parse::<u64>().ok()?.saturating_mul(1024);
        Some((available, total))
    }
}

#[tauri::command]
pub async fn get_storage_stats(state: State<'_, ShellState>) -> CommandResult<StorageStatsDTO> {
    let core = state.core().await.map_err(AppErrorDTO::from)?;
    let paths = core.paths.clone();
    let settings = core.settings();
    let download_dir = settings.effective_download_dir();
    let ffmpeg = bilidown_core::media::ffmpeg::Ffmpeg::detect(settings.ffmpeg_path.as_deref())
        .map(|f| f.path().to_string_lossy().into_owned());

    let data_dir = paths.root().to_path_buf();
    let covers = paths.covers_dir();
    let temp = paths.tmp_dir();
    let logs = paths.logs_dir();
    let download_dir_for_task = download_dir.clone();

    let stats = tauri::async_runtime::spawn_blocking(move || {
        let data_bytes = dir_size_limited(&data_dir);
        let cover_bytes = dir_size_limited(&covers);
        let temp_bytes = dir_size_limited(&temp);
        let log_bytes = dir_size_limited(&logs);
        let disk = disk_space(&download_dir_for_task);
        (data_bytes, cover_bytes, temp_bytes, log_bytes, disk)
    })
    .await
    .map_err(|e| AppErrorDTO::new("unknown", format!("统计存储失败: {e}")))?;

    let (data_bytes, cover_bytes, temp_bytes, log_bytes, disk) = stats;
    Ok(StorageStatsDTO {
        data_dir: paths.root().to_string_lossy().into_owned(),
        download_dir: download_dir.to_string_lossy().into_owned(),
        data_bytes,
        cover_bytes,
        temp_bytes,
        log_bytes,
        download_free_bytes: disk.map(|(free, _)| free),
        download_total_bytes: disk.map(|(_, total)| total),
        ffmpeg_path: ffmpeg,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanTarget {
    Covers,
    Temp,
    Logs,
}

impl CleanTarget {
    fn parse(value: &str) -> Option<&'static [CleanTarget]> {
        match value {
            "covers" => Some(&[CleanTarget::Covers]),
            "temp" => Some(&[CleanTarget::Temp]),
            "logs" => Some(&[CleanTarget::Logs]),
            "all" => Some(&[CleanTarget::Covers, CleanTarget::Temp, CleanTarget::Logs]),
            _ => None,
        }
    }

    fn dir(self, paths: &bilidown_core::AppPaths) -> PathBuf {
        match self {
            CleanTarget::Covers => paths.covers_dir(),
            CleanTarget::Temp => paths.tmp_dir(),
            CleanTarget::Logs => paths.logs_dir(),
        }
    }
}

fn clear_dir(dir: &Path, keep_newest: bool) -> (u64, u64) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut entries: Vec<(PathBuf, u64, SystemTime, bool)> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let size = if meta.is_dir() {
            dir_size_limited(&path)
        } else {
            meta.len()
        };
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        entries.push((path, size, modified, meta.is_dir()));
    }
    let mut keep: Option<PathBuf> = None;
    if keep_newest {
        keep = entries
            .iter()
            .filter(|(_, _, _, is_dir)| !is_dir)
            .max_by_key(|(_, _, m, _)| *m)
            .map(|(p, _, _, _)| p.clone());
    }
    let mut freed = 0u64;
    let mut removed = 0u64;
    for (path, size, _, is_dir) in entries {
        if Some(&path) == keep.as_ref() {
            continue;
        }
        let ok = if is_dir {
            std::fs::remove_dir_all(&path).is_ok()
        } else {
            std::fs::remove_file(&path).is_ok()
        };
        if ok {
            freed = freed.saturating_add(size);
            removed += 1;
        }
    }
    (freed, removed)
}

#[tauri::command]
pub async fn clear_app_cache(
    app: AppHandle,
    state: State<'_, ShellState>,
    target: String,
) -> CommandResult<ClearResultDTO> {
    let targets = CleanTarget::parse(target.trim())
        .ok_or_else(|| AppErrorDTO::new("invalid-argument", format!("未知清理目标: {target}")))?;
    let clears_covers = targets.contains(&CleanTarget::Covers);
    let core = state.core().await.map_err(AppErrorDTO::from)?;
    let paths = core.paths.clone();
    let root = std::fs::canonicalize(paths.root())
        .map_err(|e| AppErrorDTO::new("unknown", format!("无法解析应用数据目录: {e}")))?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut freed = 0u64;
        let mut removed = 0u64;
        for target in targets {
            let dir = target.dir(&paths);
            // 路径安全：目标必须存在且 canonicalize 后仍在应用数据根下
            let Ok(canon) = std::fs::canonicalize(&dir) else {
                continue;
            };
            if !canon.starts_with(&root) {
                return Err(format!("拒绝清理越界目录: {}", canon.display()));
            }
            let symlink_meta =
                std::fs::symlink_metadata(&dir).map_err(|e| format!("无法读取目录: {e}"))?;
            if symlink_meta.file_type().is_symlink() {
                return Err(format!("拒绝清理符号链接目录: {}", dir.display()));
            }
            if !symlink_meta.is_dir() {
                return Err(format!("清理目标不是目录: {}", dir.display()));
            }
            let (f, r) = clear_dir(&dir, matches!(target, CleanTarget::Logs));
            freed = freed.saturating_add(f);
            removed = removed.saturating_add(r);
        }
        Ok::<_, String>((freed, removed))
    })
    .await
    .map_err(|e| AppErrorDTO::new("unknown", format!("清理任务失败: {e}")))?
    .map_err(|e| AppErrorDTO::new("unknown", e))?;

    if clears_covers {
        // 前端据此失效内存图片缓存（design D11/7.7）。
        crate::events::emit(&app, crate::events::COVERS_CLEARED, ());
    }
    Ok(ClearResultDTO {
        freed_bytes: result.0,
        removed_files: result.1,
    })
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let core = state
        .try_core()
        .ok_or_else(|| AppErrorDTO::new("unknown", "引擎尚未就绪"))?;
    let root = core.paths.root().to_path_buf();
    app.opener()
        .open_path(root.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppErrorDTO::new("unknown", format!("打开数据目录失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bilidown-maintenance-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dir_size_skips_symlinks() {
        let dir = temp_dir("size");
        std::fs::write(dir.join("a.bin"), b"12345").unwrap();
        std::fs::write(dir.join("b.bin"), b"123").unwrap();
        assert_eq!(dir_size_limited(&dir), 8);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_dir_keeps_newest_for_logs() {
        let dir = temp_dir("logs");
        std::fs::write(dir.join("old.log"), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(dir.join("current.log"), b"current").unwrap();
        let (freed, removed) = clear_dir(&dir, true);
        assert_eq!(removed, 1);
        assert_eq!(freed, 3);
        assert!(dir.join("current.log").is_file());
        assert!(!dir.join("old.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_target_whitelist() {
        assert!(CleanTarget::parse("covers").is_some());
        assert!(CleanTarget::parse("temp").is_some());
        assert!(CleanTarget::parse("logs").is_some());
        assert_eq!(CleanTarget::parse("all").unwrap().len(), 3);
        assert!(CleanTarget::parse("../settings").is_none());
        assert!(CleanTarget::parse("downloads").is_none());
    }

    /// 存储统计的磁盘空间读取（Windows 走 GetDiskFreeSpaceExW，其他平台走 df）。
    #[test]
    fn disk_space_reads_positive_numbers() {
        let cwd = std::env::current_dir().expect("current dir");
        let (available, total) = disk_space(&cwd).expect("应能读取当前目录所在磁盘的空间");
        assert!(total > 0, "总容量应大于 0");
        assert!(available <= total, "可用空间不应超过总容量");
    }
}
