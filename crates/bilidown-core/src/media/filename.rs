//! 文件名与落盘规则（见 tasks 3.9）。
//!
//! 规则：
//! - `标题[清晰度]`（视频）/ `标题[码率/格式]`（音频）；
//! - 过滤非法字符与控制字符、Windows 保留名、首尾空格/点；
//! - 限制文件名长度（按 UTF-8 字节，保持字符边界）；
//! - 目标名已存在时追加 ` (1)`、` (2)`，不覆盖已有文件。

use std::path::{Path, PathBuf};

/// 文件名组件最大字节数（单组件 ≤255 字节，预留扩展名与去重后缀）。
///
/// Windows 还受 260 字符的 `MAX_PATH` 限制，因此组件上限进一步收紧，给下载目录与
/// 临时目录留出余量；配合 [`check_path_length`] 在落盘前给出可读错误。
#[cfg(target_os = "windows")]
pub const MAX_COMPONENT_BYTES: usize = 120;
#[cfg(not(target_os = "windows"))]
pub const MAX_COMPONENT_BYTES: usize = 200;

/// 最终路径长度上限（Windows 为 `MAX_PATH` 留出余量；其他平台基本不受限）。
pub fn path_len_limit() -> usize {
    #[cfg(target_os = "windows")]
    {
        250
    }
    #[cfg(not(target_os = "windows"))]
    {
        4096
    }
}

/// 校验最终落盘路径长度；Windows 超长路径无法 `CreateFile`，提前给出可读错误。
pub fn check_path_length(path: &Path) -> Result<(), String> {
    let limit = path_len_limit();
    let len = path.as_os_str().len();
    if len > limit {
        return Err(format!(
            "输出路径过长（{len} 字节，上限 {limit}）：请缩短下载目录或视频标题"
        ));
    }
    Ok(())
}

/// Windows 保留设备名（不区分大小写，可带扩展名）。
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 净化单个文件名组件（不含扩展名逻辑）。
pub fn sanitize_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => out.push(' '),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    // 折叠空白（含全角空格不处理，只处理 ASCII 空白）
    let mut collapsed = String::with_capacity(out.len());
    let mut last_ws = false;
    for c in out.chars() {
        if c.is_whitespace() {
            if !last_ws {
                collapsed.push(' ');
            }
            last_ws = true;
        } else {
            collapsed.push(c);
            last_ws = false;
        }
    }
    // Windows 不允许组件以空格或点结尾
    let trimmed = collapsed.trim().trim_end_matches(['.', ' ']).trim();
    let truncated = truncate_bytes(trimmed, MAX_COMPONENT_BYTES);
    let cleaned = truncated
        .trim_matches(|c| c == '.' || c == ' ')
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return "untitled".into();
    }
    if is_windows_reserved(&cleaned) {
        return format!("_{cleaned}");
    }
    cleaned
}

fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    WINDOWS_RESERVED
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
}

/// 兼容旧调用：净化文件名组件（等同 [`sanitize_component`]）。
pub fn sanitize_filename(name: &str) -> String {
    sanitize_component(name)
}

/// 按 UTF-8 字节截断且保持字符边界（不切坏多字节字符）。
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 清晰度技术标识（写入文件名，非本地化文案）：`1080P` / `4K` / `HDR` …
pub fn quality_tag(q: crate::api::types::Quality) -> String {
    match q.0 {
        16 => "360P".into(),
        32 => "480P".into(),
        64 => "720P".into(),
        74 => "720P60".into(),
        80 => "1080P".into(),
        112 => "1080P+".into(),
        116 => "1080P60".into(),
        120 => "4K".into(),
        125 => "HDR".into(),
        126 => "Dolby".into(),
        127 => "8K".into(),
        n => format!("Q{n}"),
    }
}

/// 视频/合并产物文件名（不含目录）：`标题[清晰度].mp4`；
/// 多条目时追加 ` - 分P标题`：`标题 - 第一话[1080P].mp4`。
pub fn build_video_name(
    title: &str,
    entry_title: Option<&str>,
    quality: crate::api::types::Quality,
    container: &str,
) -> String {
    let mut stem = sanitize_component(title);
    if let Some(e) = entry_title {
        let e = sanitize_component(e);
        if !e.is_empty() && e != stem {
            stem = format!("{stem} - {e}");
            // 组合后再截断一次，保证不超过组件上限
            stem = truncate_bytes(&stem, MAX_COMPONENT_BYTES).to_string();
            stem = stem.trim_end_matches(['.', ' ']).to_string();
        }
    }
    format!("{stem}[{}].{container}", quality_tag(quality))
}

/// 音频产物文件名（不含目录）：`标题[192k].m4a` / `标题[FLAC].flac` / `标题[mp3].mp3`。
pub fn build_audio_name(
    title: &str,
    entry_title: Option<&str>,
    quality: Option<crate::api::types::AudioQuality>,
    format: crate::api::types::AudioFormat,
    to_mp3: bool,
) -> String {
    let stem = sanitize_component(title);
    let stem = match entry_title {
        Some(e) if sanitize_component(e) != stem => {
            let combined = format!("{stem} - {}", sanitize_component(e));
            truncate_bytes(&combined, MAX_COMPONENT_BYTES)
                .trim_end_matches(['.', ' '])
                .to_string()
        }
        _ => stem,
    };
    if to_mp3 {
        return format!("{stem}[mp3].mp3");
    }
    let tag = match format {
        crate::api::types::AudioFormat::Flac => "FLAC".to_string(),
        crate::api::types::AudioFormat::Eac3 => "Dolby".to_string(),
        crate::api::types::AudioFormat::Mp3 => "mp3".to_string(),
        crate::api::types::AudioFormat::M4a => quality
            .map(|q| q.tag().to_string())
            .unwrap_or_else(|| "m4a".to_string()),
    };
    format!("{stem}[{tag}].{}", format.extension())
}

/// 在 `dir` 下为 `file_name` 找一个不存在的名字，冲突时在扩展名前追加 ` (n)`。
///
/// `file_name` 应已由 [`build_video_name`] 等生成（含扩展名）。
pub fn dedupe_in_dir(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_ext(file_name);
    for n in 1..10_000u32 {
        let name = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", uuid::Uuid::new_v4()))
}

fn split_ext(file_name: &str) -> (&str, &str) {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (file_name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{AudioFormat, AudioQuality, Quality};

    #[test]
    fn strips_illegal_and_control_chars() {
        assert_eq!(
            sanitize_component("a/b\\c:d*e?f\"g<h>i|j"),
            "a b c d e f g h i j"
        );
        assert_eq!(sanitize_component("bad\u{0}\u{1}name"), "bad name");
        assert_eq!(sanitize_component("换行\n标题\r"), "换行 标题");
    }

    #[test]
    fn trims_spaces_and_dots_for_windows() {
        assert_eq!(sanitize_component("  ..视频【1080P】.. "), "视频【1080P】");
        assert_eq!(sanitize_component("name. . . "), "name");
        assert_eq!(sanitize_component(""), "untitled");
        assert_eq!(sanitize_component("   "), "untitled");
    }

    #[test]
    fn windows_reserved_names_get_prefixed() {
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("con.txt"), "_con.txt");
        assert_eq!(sanitize_component("LPT1"), "_LPT1");
        assert_eq!(sanitize_component("console"), "console");
    }

    #[test]
    fn truncates_long_cjk_title_on_char_boundary() {
        let long = "视".repeat(300); // 3 bytes each
        let s = sanitize_component(&long);
        assert!(s.len() <= MAX_COMPONENT_BYTES);
        assert!(s.is_char_boundary(s.len()));
        assert!(s.chars().all(|c| c == '视'));
    }

    #[test]
    fn video_name_shape() {
        assert_eq!(
            build_video_name("某视频", None, Quality::P1080, "mp4"),
            "某视频[1080P].mp4"
        );
        assert_eq!(
            build_video_name("番剧", Some("第一话"), Quality::P4K, "mkv"),
            "番剧 - 第一话[4K].mkv"
        );
        // entry 与标题相同不重复拼接
        assert_eq!(
            build_video_name("标题", Some("标题"), Quality::P720, "mp4"),
            "标题[720P].mp4"
        );
    }

    #[test]
    fn audio_name_shape() {
        assert_eq!(
            build_audio_name(
                "歌名",
                None,
                Some(AudioQuality::K192),
                AudioFormat::M4a,
                false
            ),
            "歌名[192k].m4a"
        );
        assert_eq!(
            build_audio_name(
                "无损",
                None,
                Some(AudioQuality::FLAC),
                AudioFormat::Flac,
                false
            ),
            "无损[FLAC].flac"
        );
        assert_eq!(
            build_audio_name(
                "转码",
                None,
                Some(AudioQuality::K128),
                AudioFormat::M4a,
                true
            ),
            "转码[mp3].mp3"
        );
    }

    #[test]
    fn path_length_check_rejects_overlong_paths() {
        let short = PathBuf::from("C:\\tmp\\video.mp4");
        assert!(check_path_length(&short).is_ok());
        #[cfg(target_os = "windows")]
        {
            let long = PathBuf::from(format!("C:\\{}", "a".repeat(300)));
            assert!(check_path_length(&long).is_err());
        }
    }

    #[test]
    fn dedupe_appends_counter() {
        let dir = tempfile::tempdir().unwrap();
        let p0 = dir.path().join("视频[1080P].mp4");
        std::fs::write(&p0, b"x").unwrap();
        let p1 = dedupe_in_dir(dir.path(), "视频[1080P].mp4");
        assert_eq!(
            p1.file_name().unwrap().to_str().unwrap(),
            "视频[1080P] (1).mp4"
        );
        std::fs::write(&p1, b"x").unwrap();
        let p2 = dedupe_in_dir(dir.path(), "视频[1080P].mp4");
        assert_eq!(
            p2.file_name().unwrap().to_str().unwrap(),
            "视频[1080P] (2).mp4"
        );
        // 无扩展名
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir2.path().join("README"), b"x").unwrap();
        let q = dedupe_in_dir(dir2.path(), "README");
        assert_eq!(q.file_name().unwrap().to_str().unwrap(), "README (1)");
    }
}
