//! macOS / Windows 设置窗口的状态恢复与定位。
//!
//! 不恢复历史坐标；首次显示前和再次主动打开时，均相对主窗口重新定位。

use std::io;
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewWindow};
#[cfg(target_os = "windows")]
use tauri_plugin_window_state::{StateFlags, WindowExt};

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Rect {
    fn valid(self) -> bool {
        self.width > 0 && self.height > 0
    }
}

#[derive(Clone, Copy, Debug)]
struct Placement {
    anchor: Rect,
    work: Rect,
}

impl Placement {
    fn position(self, size: PhysicalSize<u32>) -> PhysicalPosition<i32> {
        let width = i64::from(size.width);
        let height = i64::from(size.height);
        let left = i64::from(self.work.x);
        let top = i64::from(self.work.y);
        let right = left + i64::from(self.work.width);
        let bottom = top + i64::from(self.work.height);
        let centered_x = i64::from(self.anchor.x) + (i64::from(self.anchor.width) - width) / 2;
        let centered_y = i64::from(self.anchor.y) + (i64::from(self.anchor.height) - height) / 2;
        // 超宽时保住右上关闭按钮，超高时保住标题栏；不使用倒置的 clamp 区间。
        let x = if width > i64::from(self.work.width) {
            right - width
        } else {
            centered_x.clamp(left, right - width)
        };
        let y = if height > i64::from(self.work.height) {
            top
        } else {
            centered_y.clamp(top, bottom - height)
        };
        PhysicalPosition::new(
            x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        )
    }
}

/// 首次创建与复用均走此流程。调用方须序列化打开入口，首次创建须使用 visible=false。
pub async fn present(window: &WebviewWindow, newly_created: bool) -> tauri::Result<()> {
    on_ui(window, move |window| {
        if newly_created {
            // builder 返回前已排入插件 ready 回调；此任务在其后执行，且不跨线程持缓存锁。
            #[cfg(target_os = "windows")]
            window.hide_menu()?;
            #[cfg(target_os = "macos")]
            window.set_size(tauri::LogicalSize::new(880.0, 600.0))?;
            #[cfg(target_os = "windows")]
            if let Err(err) = window.restore_state(StateFlags::SIZE) {
                tracing::warn!(target: "shell::window", error = %err, "恢复设置普通尺寸失败，使用默认尺寸");
            }
            #[cfg(target_os = "windows")]
            crate::windows_chrome::install(window)?;
        }
        if window.is_minimized()? {
            window.unminimize()?;
        }
        if window.is_fullscreen()? {
            window.set_fullscreen(false)?;
        }
        if window.is_maximized()? {
            window.unmaximize()?;
        }
        // 先进入目标显示器。跨屏时 WM_DPICHANGED 会改变物理尺寸和位置。
        position_or_fallback(window);
        Ok(())
    })
    .await?;

    on_ui(window, |window| {
        // 独立的 UI 任务在移动之后重新读取实际尺寸，不能沿用移动前的 DPI/尺寸。
        position_or_fallback(window);
        window.show()?;
        window.set_focus()
    })
    .await
}

async fn on_ui<F>(window: &WebviewWindow, action: F) -> tauri::Result<()>
where
    F: FnOnce(&WebviewWindow) -> tauri::Result<()> + Send + 'static,
{
    let (send, receive) = tokio::sync::oneshot::channel();
    let owned = window.clone();
    window.run_on_main_thread(move || {
        let _ = send.send(action(&owned));
    })?;
    receive
        .await
        .map_err(|_| tauri::Error::FailedToReceiveMessage)?
}

fn position_or_fallback(window: &WebviewWindow) {
    if let Err(err) = position_on_main(window) {
        tracing::warn!(target: "shell::window", error = %err, "设置窗口相对定位失败，回退到显示器安全位置");
        if let Err(err) = window.center() {
            // 即使位置查询全部失效，调用方仍 show，避免留下找不回的隐藏设置窗口。
            tracing::warn!(target: "shell::window", error = %err, "设置窗口安全位置回退失败，将使用系统位置");
        }
    }
}

fn position_on_main(window: &WebviewWindow) -> tauri::Result<()> {
    let placement = placement(window)?;
    // 正常只需一次；移动触发 DPI 变化时，按新物理尺寸校正，避免小尺寸误差累积。
    for _ in 0..3 {
        let size = window.outer_size()?;
        if size.width == 0 || size.height == 0 {
            return Err(io::Error::other("设置窗口外框尺寸无效").into());
        }
        window.set_position(placement.position(size))?;
        if window.outer_size()? == size {
            break;
        }
    }
    Ok(())
}

fn placement(window: &WebviewWindow) -> tauri::Result<Placement> {
    let main = window.app_handle().get_webview_window("main");
    let monitor = main
        .as_ref()
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| io::Error::other("未找到主窗口所属显示器或主显示器"))?;
    let work_area = monitor.work_area();
    let work = Rect {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width,
        height: work_area.size.height,
    };
    if !work.valid() {
        return Err(io::Error::other("显示器工作区尺寸无效").into());
    }
    // 隐藏窗口仍可提供有效外框；最小化窗口的 -32000 坐标不能作为居中基准。
    let anchor = main
        .filter(|main| !main.is_minimized().unwrap_or(true))
        .and_then(|main| {
            let position = main.outer_position().ok()?;
            let size = main.outer_size().ok()?;
            let rect = Rect {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
            };
            rect.valid().then_some(rect)
        })
        .unwrap_or(work);
    Ok(Placement { anchor, work })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn locate(anchor: Rect, work: Rect, width: u32, height: u32) -> (i32, i32) {
        let position = Placement { anchor, work }.position(PhysicalSize::new(width, height));
        (position.x, position.y)
    }

    #[test]
    fn ordinary_window_centers_on_main_instead_of_screen() {
        assert_eq!(
            locate(rect(200, 100, 1024, 680), rect(0, 0, 1920, 1040), 880, 600),
            (272, 140)
        );
    }

    #[test]
    fn settings_larger_than_main_can_extend_beyond_main() {
        assert_eq!(
            locate(rect(300, 200, 760, 480), rect(0, 0, 1920, 1040), 880, 600),
            (240, 140)
        );
    }

    #[test]
    fn odd_size_differences_keep_centers_within_one_pixel() {
        for size in [(879, 599), (1025, 681)] {
            let (x, y) = locate(
                rect(200, 100, 1024, 680),
                rect(0, 0, 1920, 1040),
                size.0,
                size.1,
            );
            assert!((2 * x + size.0 as i32 - (2 * 200 + 1024)).abs() <= 1);
            assert!((2 * y + size.1 as i32 - (2 * 100 + 680)).abs() <= 1);
        }
    }

    #[test]
    fn negative_monitor_coordinates_remain_signed() {
        assert_eq!(
            locate(
                rect(-2400, -100, 1536, 1020),
                rect(-2560, -200, 2560, 1440),
                1320,
                900
            ),
            (-2292, -40)
        );
    }

    #[test]
    fn bottom_right_is_clamped_above_taskbar() {
        assert_eq!(
            locate(rect(1800, 950, 1024, 680), rect(0, 0, 1920, 1040), 880, 600),
            (1040, 440)
        );
    }

    #[test]
    fn top_left_respects_work_area_offsets() {
        assert_eq!(
            locate(
                rect(-100, -100, 1024, 680),
                rect(80, 60, 1840, 1020),
                880,
                600
            ),
            (80, 60)
        );
    }

    #[test]
    fn oversized_window_keeps_title_and_close_button_inside_work_area() {
        assert_eq!(
            locate(
                rect(200, 100, 1024, 680),
                rect(0, 40, 1920, 1000),
                2600,
                1400
            ),
            (-680, 40)
        );
    }

    #[test]
    fn unavailable_main_falls_back_to_work_area_center() {
        let work = rect(-1920, 40, 1920, 1000);
        assert_eq!(locate(work, work, 880, 600), (-1400, 240));
    }

    #[test]
    fn uses_actual_physical_dimensions_at_one_hundred_fifty_percent() {
        assert_eq!(
            locate(rect(100, 80, 1536, 1020), rect(0, 0, 2560, 1400), 1320, 900),
            (208, 140)
        );
    }

    #[test]
    fn reopening_recomputes_from_the_latest_main_window_position() {
        let work = rect(0, 0, 1920, 1080);
        let first = locate(rect(100, 100, 1000, 700), work, 880, 600);
        let reopened = locate(rect(500, 300, 1000, 700), work, 880, 600);

        assert_eq!(first, (160, 150));
        assert_eq!(reopened, (560, 350));
        assert_ne!(first, reopened);
    }

    #[test]
    fn uses_actual_physical_dimensions_at_two_hundred_percent() {
        assert_eq!(
            locate(
                rect(200, 100, 2048, 1360),
                rect(0, 0, 3840, 2160),
                1760,
                1200,
            ),
            (344, 180)
        );
    }
}
