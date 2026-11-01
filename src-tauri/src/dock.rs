//! Dock 徽标（tasks 4.4）。
//!
//! Tauri 2.12 没有 `set_badge_count` API（已核对 docs.rs），因此走 macOS 专用模块：
//! `NSApplication.sharedApplication.dockTile.badgeLabel`（objc2-app-kit）。
//! 非 macOS 平台降级为 no-op。

use tauri::AppHandle;

/// 设置 Dock 徽标；`None` 或 0 时清除。
pub fn set_badge(app: &AppHandle, count: Option<u32>) {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let _ = app.clone().run_on_main_thread(move || {
            macos::set_dock_badge(count);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, count);
        tracing::debug!(target: "shell::dock", "Dock 徽标仅 macOS 支持，已忽略");
    }
}

/// 读取当前 Dock 徽标文本（dev 验证用；非 macOS 返回 None）。
#[cfg(debug_assertions)]
pub fn get_badge() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::get_dock_badge()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// 由队列统计推导徽标（排队 + 解析/下载/合并）。
pub fn update(app: &AppHandle, running: u32) {
    if running > 0 {
        set_badge(app, Some(running));
    } else {
        set_badge(app, None);
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    #[cfg(debug_assertions)]
    pub fn get_dock_badge() -> Option<String> {
        let mtm = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(mtm);
        let tile = app.dockTile();
        tile.badgeLabel().map(|label| label.to_string())
    }

    pub fn set_dock_badge(count: Option<u32>) {
        let Some(mtm) = MainThreadMarker::new() else {
            tracing::warn!(target: "shell::dock", "不在主线程，跳过 Dock 徽标设置");
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let tile: Retained<_> = app.dockTile();
        match count {
            Some(n) if n > 0 => {
                let label = NSString::from_str(&n.to_string());
                tile.setBadgeLabel(Some(&label));
                tile.display();
            }
            _ => {
                tile.setBadgeLabel(None);
                tile.display();
            }
        }
        tracing::debug!(target: "shell::dock", count = ?count, "Dock 徽标已更新");
    }
}
