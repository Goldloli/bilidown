//! Windows 不透明无边框窗口的真实外轮廓（change: fix-windows-window-chrome-and-icons）。
//!
//! 创建窗口时须使用 transparent=false、shadow=false；在首次 show 之前调用 install。
//! Win11 优先使用 DWM 圆角和无色边框，旧系统及配置失败时使用 HWND 区域裁剪。

#![cfg(target_os = "windows")]

use std::io;
use std::mem::{forget, size_of};
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, TryLockError};
use tauri::{WebviewWindow, WindowEvent};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DWMWCP_ROUND, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, HRGN, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, SetWindowRgn,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed,
    WINDOWPLACEMENT, WS_EX_LAYERED,
};

pub(crate) const CORNER_RADIUS_DIP: i32 = 10;

#[derive(Clone, Copy, Debug)]
enum Strategy {
    Dwm,
    Region,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Outline {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) dpi: u32,
    pub(crate) square: bool,
}

struct ChromeState {
    strategy: Strategy,
    last: Option<Outline>,
}

/// 为一个不透明、无原生阴影的窗口安装圆角策略，主窗口和设置窗口均须调用一次。
///
/// 原生查询不经过 Tauri 的同步 dispatcher，避免在窗口事件回调里等待主线程自身。
/// region 属于 HWND，销毁窗口时由系统释放；事件闭包不持有窗口克隆或 region 句柄。
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    let hwnd = window.hwnd()?.0 as HWND;
    let label = window.label().to_owned();
    let strategy = match configure_dwm(hwnd, false) {
        Ok(()) => {
            // DWM 圆角与自定义 region 不能混用，避免旧区域阻止系统圆角。
            clear_region(hwnd)?;
            tracing::info!(target: "shell::window", window = %label, "已配置 DWM 圆角与无色窗口边框");
            Strategy::Dwm
        }
        Err(err) => {
            // Win10 不支持这些 DWM 属性；这是预期回退，不是可忽略的圆角失败。
            tracing::info!(target: "shell::window", window = %label, reason = %err, "使用原生窗口区域圆角");
            disable_dwm_rounding(hwnd);
            Strategy::Region
        }
    };
    let mut state = ChromeState {
        strategy,
        last: None,
    };
    state.update(hwnd, false)?;
    if let Err(err) = crate::windows_shadow::install(hwnd, &label) {
        // 阴影是增强层；初始化失败不能阻止宿主窗口继续可用，也不能回退到有白边的原生 shadow。
        tracing::error!(target: "shell::window", window = %label, error = %err, "初始化 Windows 外阴影失败");
    }

    let state = Arc::new(Mutex::new(state));
    // 原始 HWND 不实现 Send，存地址值而不捕获借用；仅窗口自身事件会访问它。
    let address = hwnd as isize;
    window.on_window_event(move |event| {
        let force = match event {
            WindowEvent::Resized(_)
            | WindowEvent::Moved(_)
            | WindowEvent::ScaleFactorChanged { .. } => false,
            WindowEvent::ThemeChanged(_) | WindowEvent::Focused(_) => true,
            _ => return,
        };
        // SetWindowRgn 会同步发出窗口位置消息；重入时外层调用已在更新同一轮廓。
        let mut state = match state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return,
            Err(TryLockError::Poisoned(_)) => {
                tracing::error!(target: "shell::window", window = %label, "窗口轮廓状态锁已损坏");
                return;
            }
        };
        if let Err(err) = state.update(address as HWND, force) {
            tracing::error!(target: "shell::window", window = %label, error = %err, "更新 Windows 窗口轮廓失败");
        }
    });
    Ok(())
}

impl ChromeState {
    fn update(&mut self, hwnd: HWND, force: bool) -> io::Result<()> {
        let Some(outline) = read_outline(hwnd)? else {
            return Ok(());
        };
        if !force && self.last == Some(outline) {
            return Ok(());
        }
        if matches!(self.strategy, Strategy::Dwm) {
            match configure_dwm(hwnd, outline.square) {
                Ok(()) => {
                    self.last = Some(outline);
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!(target: "shell::window", reason = %err, "DWM 轮廓配置失败，回退原生区域裁剪");
                    disable_dwm_rounding(hwnd);
                    self.strategy = Strategy::Region;
                }
            }
        }
        if outline.square {
            clear_region(hwnd)?;
        } else {
            OwnedRegion::rounded(outline)?.assign_to(hwnd)?;
        }
        self.last = Some(outline);
        Ok(())
    }
}

pub(crate) fn read_outline(hwnd: HWND) -> io::Result<Option<Outline>> {
    // 最小化时系统可能提供图标尺寸；保留当前 region，待还原事件再更新。
    if unsafe { IsIconic(hwnd) } != 0 {
        return Ok(None);
    }
    let mut bounds = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut bounds) } == 0 {
        return Err(win32_error("GetWindowRect"));
    }
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width <= 0 || height <= 0 {
        return Ok(None);
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        return Err(win32_error("GetDpiForWindow"));
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return Err(win32_error("GetMonitorInfoW"));
    }
    let mut placement = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
        return Err(win32_error("GetWindowPlacement"));
    }
    let tolerance = (dpi as i32 / 96).max(1) * 2;
    let fills_monitor = same_rect(&bounds, &info.rcMonitor, tolerance);
    let square = unsafe { IsZoomed(hwnd) } != 0
        || fills_monitor
        || is_tiled(
            &bounds,
            &info.rcWork,
            &placement.rcNormalPosition,
            tolerance,
        );
    Ok(Some(Outline {
        width,
        height,
        dpi,
        square,
    }))
}

impl Outline {
    pub(crate) fn corner_diameter(self) -> i32 {
        ((CORNER_RADIUS_DIP * 2 * self.dpi as i32 + 48) / 96)
            .min(self.width)
            .min(self.height)
    }
}

fn near(a: i32, b: i32, tolerance: i32) -> bool {
    (i64::from(a) - i64::from(b)).abs() <= i64::from(tolerance)
}

fn same_rect(a: &RECT, b: &RECT, tolerance: i32) -> bool {
    near(a.left, b.left, tolerance)
        && near(a.top, b.top, tolerance)
        && near(a.right, b.right, tolerance)
        && near(a.bottom, b.bottom, tolerance)
}

/// Win10 贴靠没有独立的 Tauri 事件：用工作区边缘和还原尺寸识别半屏/四分屏。
/// normal 的坐标可能是工作区坐标，因此只比较尺寸，不比较它的屏幕位置。
fn is_tiled(bounds: &RECT, work: &RECT, normal: &RECT, tolerance: i32) -> bool {
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    let restored_size = near(width, normal.right - normal.left, tolerance)
        && near(height, normal.bottom - normal.top, tolerance);
    if restored_size {
        return false;
    }
    let touches_horizontal_edge =
        near(bounds.left, work.left, tolerance) || near(bounds.right, work.right, tolerance);
    let touches_vertical_edge =
        near(bounds.top, work.top, tolerance) || near(bounds.bottom, work.bottom, tolerance);
    let within_work_area = bounds.left >= work.left - tolerance
        && bounds.top >= work.top - tolerance
        && bounds.right <= work.right + tolerance
        && bounds.bottom <= work.bottom + tolerance;
    touches_horizontal_edge && touches_vertical_edge && within_work_area
}

fn configure_dwm(hwnd: HWND, square: bool) -> io::Result<()> {
    if unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_LAYERED != 0 {
        return Err(io::Error::other("逐像素透明窗口不能采用 DWM 圆角"));
    }
    let preference = if square {
        DWMWCP_DONOTROUND
    } else {
        DWMWCP_ROUND
    };
    set_dwm(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &preference)?;
    set_dwm(hwnd, DWMWA_BORDER_COLOR as u32, &DWMWA_COLOR_NONE)?;
    // 检查配置实际可读回；这不能替代 Win11 真机画面的最终验收。
    let mut actual = 0i32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&mut actual as *mut i32).cast(),
            size_of::<i32>() as u32,
        )
    };
    if result < 0 || actual != preference {
        return Err(io::Error::other(format!(
            "DWM 圆角状态校验失败: HRESULT=0x{:08X}, preference={actual}",
            result as u32
        )));
    }
    Ok(())
}

fn set_dwm<T>(hwnd: HWND, attribute: u32, value: &T) -> io::Result<()> {
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (value as *const T).cast(),
            size_of::<T>() as u32,
        )
    };
    if result < 0 {
        Err(io::Error::other(format!(
            "DwmSetWindowAttribute({attribute}): HRESULT=0x{:08X}",
            result as u32
        )))
    } else {
        Ok(())
    }
}

fn disable_dwm_rounding(hwnd: HWND) {
    if let Err(err) = set_dwm(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE as u32,
        &DWMWCP_DONOTROUND,
    ) {
        tracing::debug!(target: "shell::window", reason = %err, "系统未提供 DWM 圆角开关，使用区域裁剪");
    }
}

fn clear_region(hwnd: HWND) -> io::Result<()> {
    if unsafe { SetWindowRgn(hwnd, null_mut(), 1) } == 0 {
        Err(win32_error("SetWindowRgn(NULL)"))
    } else {
        Ok(())
    }
}

/// region 在成功移交 HWND 前由本类型释放，失败和测试路径均不泄漏 GDI 句柄。
struct OwnedRegion(HRGN);

impl OwnedRegion {
    fn rounded(outline: Outline) -> io::Result<Self> {
        let diameter = outline.corner_diameter();
        // GDI 圆角 region 的右下边界少一个像素；+1 使 GetRgnBox 恰好覆盖 outer size。
        let region = unsafe {
            CreateRoundRectRgn(
                0,
                0,
                outline.width + 1,
                outline.height + 1,
                diameter,
                diameter,
            )
        };
        if region.is_null() {
            Err(win32_error("CreateRoundRectRgn"))
        } else {
            Ok(Self(region))
        }
    }

    fn assign_to(self, hwnd: HWND) -> io::Result<()> {
        if unsafe { SetWindowRgn(hwnd, self.0, 1) } == 0 {
            return Err(win32_error("SetWindowRgn"));
        }
        // 成功后系统拥有句柄，不得再 DeleteObject；窗口销毁或替换 region 时系统释放。
        forget(self);
        Ok(())
    }
}

impl Drop for OwnedRegion {
    fn drop(&mut self) {
        if unsafe { DeleteObject(self.0) } == 0 {
            tracing::error!(target: "shell::window", error = %io::Error::last_os_error(), "释放窗口 region 失败");
        }
    }
}

fn win32_error(operation: &str) -> io::Error {
    let err = io::Error::last_os_error();
    io::Error::new(err.kind(), format!("{operation}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::{GetRgnBox, PtInRegion};

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn native_region_covers_last_row_and_column_at_each_dpi() {
        for dpi in [96, 120, 144, 192] {
            let region = OwnedRegion::rounded(Outline {
                width: 1024,
                height: 680,
                dpi,
                square: false,
            })
            .unwrap();
            let mut bounds = RECT::default();
            assert_ne!(unsafe { GetRgnBox(region.0, &mut bounds) }, 0);
            assert_eq!(
                (bounds.left, bounds.top, bounds.right, bounds.bottom),
                (0, 0, 1024, 680)
            );
            assert_eq!(unsafe { PtInRegion(region.0, 0, 0) }, 0);
            assert_ne!(unsafe { PtInRegion(region.0, 1023, 340) }, 0);
            assert_ne!(unsafe { PtInRegion(region.0, 512, 679) }, 0);
        }
    }

    #[test]
    fn snap_halves_and_quarters_are_square_on_negative_monitor_coordinates() {
        let work = rect(-1920, 0, 0, 1040);
        let restored = rect(100, 100, 1124, 780);
        assert!(is_tiled(&rect(-1920, 0, -960, 1040), &work, &restored, 2));
        assert!(is_tiled(&rect(-960, 520, 0, 1040), &work, &restored, 2));
        assert!(!is_tiled(&rect(-1600, 100, -576, 780), &work, &restored, 2));
    }

    #[test]
    fn normal_window_at_screen_corner_keeps_rounding() {
        let bounds = rect(0, 0, 1024, 680);
        let work = rect(0, 0, 1920, 1040);
        assert!(!is_tiled(&bounds, &work, &bounds, 2));
    }
}
