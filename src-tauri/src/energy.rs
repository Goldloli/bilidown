//! 下载/合并期间的 App Nap 抑制（tasks 7.5）。
//!
//! macOS 的 App Nap 会在应用不可见时降低定时器与 IO 优先级；长时间后台下载时
//! 用 `NSProcessInfo.beginActivityWithOptions:reason:` 申请一个用户发起型 activity，
//! 任务全部结束后释放。activity token 由专用线程持有，避免 `Retained` 的跨线程
//! Send 约束；非 macOS 平台为 no-op。

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

static ACTIVE: Mutex<Option<Sender<()>>> = Mutex::new(None);

/// 有活跃任务时抑制 App Nap，空闲时释放（可重复调用）。
pub fn set_active(active: bool) {
    if active {
        let mut guard = ACTIVE.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let (tx, rx) = channel::<()>();
        std::thread::Builder::new()
            .name("bilidown-energy".into())
            .spawn(move || {
                #[cfg(target_os = "macos")]
                {
                    use objc2::rc::Retained;
                    use objc2::runtime::ProtocolObject;
                    use objc2_foundation::{
                        NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString,
                    };

                    let info: Retained<NSProcessInfo> = NSProcessInfo::processInfo();
                    let reason = NSString::from_str("bilidown 正在下载/合并");
                    let activity: Retained<ProtocolObject<dyn NSObjectProtocol>> = info
                        .beginActivityWithOptions_reason(
                            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                            &reason,
                        );
                    tracing::info!(target: "shell::energy", "已开启 App Nap 抑制（下载中）");
                    let _ = rx.recv();
                    unsafe { info.endActivity(&activity) };
                    tracing::info!(target: "shell::energy", "已释放 App Nap 抑制");
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = rx.recv();
                }
            })
            .ok();
        *guard = Some(tx);
    } else if let Some(tx) = ACTIVE.lock().unwrap().take() {
        let _ = tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无 Tauri 上下文下也应能安全开关（macOS 上真实调用 NSProcessInfo）。
    #[test]
    fn toggle_does_not_panic() {
        set_active(true);
        std::thread::sleep(std::time::Duration::from_millis(150));
        set_active(false);
        set_active(false);
    }
}
