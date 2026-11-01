//! 原子、无覆盖地发布最终主产物。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use tempfile::Builder;
use uuid::Uuid;

use crate::api::error::{BiliError, BiliResult};

const STAGING_PREFIX: &str = ".bilidown-publish-";
const STAGING_SUFFIX: &str = ".tmp";
const STALE_STAGING_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameStatus {
    Published,
    AlreadyExists,
    CrossDevice,
    Unsupported,
}

#[derive(Debug, Default, Clone, Copy)]
struct PublishHooks {
    force_cross_device: bool,
    force_unsupported: bool,
    copy_limit: Option<u64>,
}

/// 将已完成的临时主产物发布到 `dir/file_name` 或首个可用去重名称。
pub fn publish_no_overwrite(src: &Path, dir: &Path, file_name: &str) -> BiliResult<PathBuf> {
    publish_with_hooks(src, dir, file_name, PublishHooks::default())
}

fn publish_with_hooks(
    src: &Path,
    dir: &Path,
    file_name: &str,
    hooks: PublishHooks,
) -> BiliResult<PathBuf> {
    if !matches!(
        Path::new(file_name)
            .components()
            .collect::<Vec<_>>()
            .as_slice(),
        [Component::Normal(_)]
    ) {
        return Err(BiliError::Disk(format!("非法输出文件名: {file_name}")));
    }
    fs::create_dir_all(dir).map_err(|error| {
        BiliError::Disk(format!("创建输出目录 {} 失败: {error}", dir.display()))
    })?;
    let _ = cleanup_stale_publish_files(dir);
    sync_file(src)?;

    if hooks.force_unsupported {
        return Err(unsupported_error(dir));
    }
    if !hooks.force_cross_device {
        for n in 0..10_000u32 {
            let candidate = candidate_path(dir, file_name, n)?;
            match rename_no_replace(src, &candidate)
                .map_err(|error| BiliError::Disk(format!("发布产物失败: {error}")))?
            {
                RenameStatus::Published => {
                    sync_directory(dir);
                    return Ok(candidate);
                }
                RenameStatus::AlreadyExists => continue,
                RenameStatus::CrossDevice => {
                    return publish_cross_device(src, dir, file_name, hooks);
                }
                RenameStatus::Unsupported => return Err(unsupported_error(dir)),
            }
        }
    } else {
        return publish_cross_device(src, dir, file_name, hooks);
    }
    publish_random_fallback(src, dir, file_name)
}

fn publish_cross_device(
    src: &Path,
    dir: &Path,
    file_name: &str,
    hooks: PublishHooks,
) -> BiliResult<PathBuf> {
    let mut staged = Builder::new()
        .prefix(STAGING_PREFIX)
        .suffix(STAGING_SUFFIX)
        .tempfile_in(dir)
        .map_err(|error| {
            BiliError::Disk(format!(
                "在目标目录 {} 创建发布暂存文件失败: {error}",
                dir.display()
            ))
        })?;

    let mut input = File::open(src).map_err(|error| {
        BiliError::Disk(format!("打开待发布文件 {} 失败: {error}", src.display()))
    })?;
    copy_contents(&mut input, staged.as_file_mut(), hooks.copy_limit)?;
    staged
        .as_file_mut()
        .flush()
        .map_err(|error| BiliError::Disk(format!("刷新发布暂存文件失败: {error}")))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| BiliError::Disk(format!("同步发布暂存文件失败: {error}")))?;
    let staged_path = staged.into_temp_path();

    for n in 0..10_000u32 {
        let candidate = candidate_path(dir, file_name, n)?;
        match rename_no_replace(staged_path.as_ref(), &candidate)
            .map_err(|error| BiliError::Disk(format!("发布产物失败: {error}")))?
        {
            RenameStatus::Published => {
                let _ = fs::remove_file(src);
                sync_directory(dir);
                return Ok(candidate);
            }
            RenameStatus::AlreadyExists => continue,
            RenameStatus::CrossDevice | RenameStatus::Unsupported => {
                return Err(unsupported_error(dir));
            }
        }
    }
    let candidate = dir.join(format!(
        "{}-{}{}",
        split_ext(file_name).0,
        Uuid::new_v4(),
        ext_suffix(split_ext(file_name).1)
    ));
    super::filename::check_path_length(&candidate).map_err(BiliError::Disk)?;
    match rename_no_replace(staged_path.as_ref(), &candidate)
        .map_err(|error| BiliError::Disk(format!("发布产物失败: {error}")))?
    {
        RenameStatus::Published => {
            let _ = fs::remove_file(src);
            sync_directory(dir);
            Ok(candidate)
        }
        _ => Err(unsupported_error(dir)),
    }
}

fn copy_contents(reader: &mut File, writer: &mut File, limit: Option<u64>) -> BiliResult<()> {
    match limit {
        Some(limit) => {
            let copied = io::copy(&mut reader.take(limit), writer)
                .map_err(|error| BiliError::Disk(format!("复制发布暂存文件失败: {error}")))?;
            if copied < limit {
                return Err(BiliError::Disk("复制发布暂存文件未完成".into()));
            }
            Err(BiliError::Disk("测试注入：复制发布暂存文件中断".into()))
        }
        None => io::copy(reader, writer)
            .map(|_| ())
            .map_err(|error| BiliError::Disk(format!("复制发布暂存文件失败: {error}"))),
    }
}

fn candidate_path(dir: &Path, file_name: &str, n: u32) -> BiliResult<PathBuf> {
    let (stem, ext) = split_ext(file_name);
    let name = if n == 0 {
        file_name.to_string()
    } else if ext.is_empty() {
        format!("{stem} ({n})")
    } else {
        format!("{stem} ({n}).{ext}")
    };
    let path = dir.join(name);
    super::filename::check_path_length(&path).map_err(BiliError::Disk)?;
    Ok(path)
}

fn publish_random_fallback(src: &Path, dir: &Path, file_name: &str) -> BiliResult<PathBuf> {
    let (stem, ext) = split_ext(file_name);
    let path = dir.join(format!("{stem}-{}{}", Uuid::new_v4(), ext_suffix(ext)));
    super::filename::check_path_length(&path).map_err(BiliError::Disk)?;
    match rename_no_replace(src, &path)
        .map_err(|error| BiliError::Disk(format!("发布产物失败: {error}")))?
    {
        RenameStatus::Published => {
            sync_directory(dir);
            Ok(path)
        }
        RenameStatus::AlreadyExists => publish_random_fallback(src, dir, file_name),
        _ => Err(unsupported_error(dir)),
    }
}

fn split_ext(file_name: &str) -> (&str, &str) {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (file_name, ""),
    }
}

fn ext_suffix(ext: &str) -> String {
    if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    }
}

fn sync_file(path: &Path) -> BiliResult<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            BiliError::Disk(format!("同步待发布文件 {} 失败: {error}", path.display()))
        })
}

#[cfg(unix)]
fn sync_directory(dir: &Path) {
    let _ = File::open(dir).and_then(|file| file.sync_all());
}

#[cfg(not(unix))]
fn sync_directory(_dir: &Path) {}

fn unsupported_error(dir: &Path) -> BiliError {
    BiliError::Disk(format!(
        "输出目录 {} 不支持安全的原子无覆盖发布，未写入最终文件",
        dir.display()
    ))
}

/// 清理目标目录中超过 `max_age` 的发布隐藏暂存文件。
pub fn cleanup_stale_staging(dir: &Path, max_age: Duration) -> BiliResult<usize> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(BiliError::Disk(format!(
                "读取发布暂存目录 {} 失败: {error}",
                dir.display()
            )));
        }
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(STAGING_PREFIX) || !name.ends_with(STAGING_SUFFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn cleanup_stale_publish_files(dir: &Path) -> BiliResult<usize> {
    cleanup_stale_staging(dir, STALE_STAGING_AGE)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_no_replace(from: &Path, to: &Path) -> Result<RenameStatus, String> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    use rustix::io::Errno;

    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(RenameStatus::Published),
        Err(error) if error == Errno::EXIST => Ok(RenameStatus::AlreadyExists),
        Err(error) if error == Errno::XDEV => Ok(RenameStatus::CrossDevice),
        Err(error)
            if error == Errno::NOSYS
                || error == Errno::OPNOTSUPP
                || error == Errno::NOTSUP
                || error == Errno::INVAL =>
        {
            Ok(RenameStatus::Unsupported)
        }
        Err(error) => Err(format!("{} -> {}: {error}", from.display(), to.display())),
    }
}

#[cfg(target_os = "windows")]
fn rename_no_replace(from: &Path, to: &Path) -> Result<RenameStatus, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_NOT_SAME_DEVICE, ERROR_NOT_SUPPORTED,
    };
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let from = wide(from);
    let to = wide(to);
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved != 0 {
        return Ok(RenameStatus::Published);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == ERROR_ALREADY_EXISTS as i32 || code == ERROR_FILE_EXISTS as i32 => {
            Ok(RenameStatus::AlreadyExists)
        }
        Some(code) if code == ERROR_NOT_SAME_DEVICE as i32 => Ok(RenameStatus::CrossDevice),
        Some(code) if code == ERROR_NOT_SUPPORTED as i32 => Ok(RenameStatus::Unsupported),
        _ => Err(error.to_string()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn rename_no_replace(_from: &Path, _to: &Path) -> Result<RenameStatus, String> {
    Ok(RenameStatus::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::FileTimes;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn concurrent_publishers_never_share_or_overwrite_name() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles = ["first", "second"].map(|bytes| {
            let root = root.path().to_path_buf();
            let out = out.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let src = root.join(format!("{bytes}.m4s"));
                write(&src, bytes.as_bytes());
                barrier.wait();
                publish_no_overwrite(&src, &out, "同名.mp4").unwrap()
            })
        });

        let paths = handles.map(|handle| handle.join().unwrap());
        assert_ne!(paths[0], paths[1]);
        assert_eq!(fs::read(&paths[0]).unwrap(), b"first");
        assert_eq!(fs::read(&paths[1]).unwrap(), b"second");
    }

    #[test]
    fn existing_target_is_preserved_and_suffix_is_used() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let existing = out.join("视频.mp4");
        write(&existing, b"keep");
        let src = root.path().join("new.part");
        write(&src, b"new");

        let published = publish_no_overwrite(&src, &out, "视频.mp4").unwrap();

        assert_eq!(published.file_name().unwrap(), "视频 (1).mp4");
        assert_eq!(fs::read(existing).unwrap(), b"keep");
        assert_eq!(fs::read(published).unwrap(), b"new");
    }

    #[test]
    fn forced_cross_device_copy_publishes_only_complete_file() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let src = root.path().join("source.part");
        write(&src, b"complete");

        let published = publish_with_hooks(
            &src,
            &out,
            "视频.mp4",
            PublishHooks {
                force_cross_device: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(fs::read(published).unwrap(), b"complete");
        assert!(!src.exists());
        assert_eq!(
            fs::read_dir(&out)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(STAGING_PREFIX))
                .count(),
            0
        );
    }

    #[test]
    fn interrupted_copy_leaves_no_final_or_hidden_staging_file() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let src = root.path().join("source.part");
        write(&src, b"0123456789");

        let result = publish_with_hooks(
            &src,
            &out,
            "视频.mp4",
            PublishHooks {
                force_cross_device: true,
                copy_limit: Some(3),
                ..Default::default()
            },
        );

        assert!(result.is_err());
        assert!(!out.join("视频.mp4").exists());
        assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    }

    #[test]
    fn unsupported_publish_preserves_existing_target() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let existing = out.join("视频.mp4");
        write(&existing, b"keep");
        let src = root.path().join("source.part");
        write(&src, b"new");

        let result = publish_with_hooks(
            &src,
            &out,
            "视频.mp4",
            PublishHooks {
                force_unsupported: true,
                ..Default::default()
            },
        );

        assert!(result.is_err());
        assert_eq!(fs::read(existing).unwrap(), b"keep");
    }

    #[test]
    fn cleanup_removes_only_stale_publish_staging_files() {
        let root = tempfile::tempdir().unwrap();
        let stale = root.path().join(".bilidown-publish-old.tmp");
        let fresh = root.path().join(".bilidown-publish-new.tmp");
        let user_file = root.path().join("用户文件.mp4");
        write(&stale, b"old");
        write(&fresh, b"new");
        write(&user_file, b"user");
        let stale_file = OpenOptions::new().write(true).open(&stale).unwrap();
        stale_file
            .set_times(
                FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60)),
            )
            .unwrap();

        let removed = cleanup_stale_staging(root.path(), Duration::from_secs(60 * 60)).unwrap();

        assert_eq!(removed, 1);
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(user_file.exists());
    }
}
