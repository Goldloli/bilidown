//! Windows 顶层窗口的应用绘制外阴影（change: add-windows-window-shadows-and-hide-glass）。
//!
//! 宿主窗口继续保持 `transparent=false`、`shadow=false` 和不透明圆角；这里创建一个
//! owned、逐像素 alpha、输入穿透且不激活的原生辅助窗口，只在宿主圆角外缘绘制柔和阴影。

#![cfg(target_os = "windows")]

use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::slice;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, HTTRANSPARENT, IsWindow,
    IsWindowVisible, MA_NOACTIVATE, RegisterClassExW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WM_DPICHANGED, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCDESTROY,
    WM_NCHITTEST, WM_SHOWWINDOW, WM_SIZE, WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::windows_chrome::{Outline, read_outline};

const SHADOW_CLASS_NAME: [u16; 20] = [
    b'B' as u16,
    b'i' as u16,
    b'l' as u16,
    b'i' as u16,
    b'd' as u16,
    b'o' as u16,
    b'w' as u16,
    b'n' as u16,
    b'S' as u16,
    b'h' as u16,
    b'a' as u16,
    b'd' as u16,
    b'o' as u16,
    b'w' as u16,
    b'L' as u16,
    b'a' as u16,
    b'y' as u16,
    b'e' as u16,
    b'r' as u16,
    0,
];
#[cfg(test)]
const STATIC_CLASS_NAME: [u16; 7] = [
    b'S' as u16,
    b'T' as u16,
    b'A' as u16,
    b'T' as u16,
    b'I' as u16,
    b'C' as u16,
    0,
];
const SUBCLASS_ID: usize = 0x4244_5348;

const MARGIN_DIP: f32 = 28.0;
const EDGE_FADE_DIP: f32 = 7.0;
const AMBIENT_ALPHA: f32 = 30.0;
const AMBIENT_SIGMA_DIP: f32 = 10.0;
const AMBIENT_OFFSET_DIP: f32 = 1.0;
const KEY_ALPHA: f32 = 24.0;
const KEY_SIGMA_DIP: f32 = 4.5;
const KEY_OFFSET_DIP: f32 = 2.0;
const MAX_ALPHA: f32 = 96.0;

static SHADOW_CLASS: OnceLock<Result<(), String>> = OnceLock::new();

/// 为一个可见的 Windows 顶层窗口安装阴影。宿主和辅助层的所有 Win32 操作都应在 UI 线程执行。
pub(crate) fn install(owner: HWND, label: &str) -> io::Result<()> {
    ensure_shadow_class()?;
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(win32_error("GetModuleHandleW"));
    }
    let layer = unsafe {
        CreateWindowExW(
            shadow_ex_style(),
            SHADOW_CLASS_NAME.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            owner,
            null_mut(),
            instance,
            null(),
        )
    };
    if layer.is_null() {
        return Err(win32_error("CreateWindowExW(shadow)"));
    }

    let state = Box::new(ShadowState {
        owner,
        layer,
        label: label.to_owned(),
        surface: None,
        surface_key: None,
        visible: false,
        syncing: false,
    });
    let state = Box::into_raw(state);
    let attached = unsafe {
        SetWindowSubclass(
            owner,
            Some(owner_subclass_proc),
            SUBCLASS_ID,
            state as usize,
        )
    };
    if attached == 0 {
        let mut state = unsafe { Box::from_raw(state) };
        state.destroy_layer();
        return Err(win32_error("SetWindowSubclass(shadow)"));
    }

    // 创建时宿主可能仍隐藏。同步失败只影响阴影，不影响后续 show/hide 与宿主操作。
    if let Err(err) = unsafe { (&mut *state).sync() } {
        tracing::warn!(
            target: "shell::window",
            window = label,
            error = %err,
            "Windows 外阴影首次同步失败"
        );
    }
    Ok(())
}

fn ensure_shadow_class() -> io::Result<()> {
    let result = SHADOW_CLASS.get_or_init(register_shadow_class);
    result
        .as_ref()
        .map(|_| ())
        .map_err(|message| io::Error::other(message.clone()))
}

fn register_shadow_class() -> Result<(), String> {
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(format!("GetModuleHandleW: {}", io::Error::last_os_error()));
    }
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(shadow_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: SHADOW_CLASS_NAME.as_ptr(),
        hIconSm: null_mut(),
    };
    if unsafe { RegisterClassExW(&window_class) } == 0 {
        return Err(format!("RegisterClassExW: {}", io::Error::last_os_error()));
    }
    Ok(())
}

unsafe extern "system" fn shadow_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LRESULT,
) -> LRESULT {
    match message {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => 1,
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe extern "system" fn owner_subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LRESULT,
    _subclass_id: usize,
    state: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        let mut shadow = if state == 0 {
            None
        } else {
            Some(unsafe { Box::from_raw(state as *mut ShadowState) })
        };
        if let Some(shadow) = shadow.as_mut() {
            shadow.shutdown();
        }
        unsafe {
            RemoveWindowSubclass(hwnd, Some(owner_subclass_proc), SUBCLASS_ID);
        }
        let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        drop(shadow);
        return result;
    }

    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if state != 0 && handles_owner_message(message) {
        let shadow = unsafe { &mut *(state as *mut ShadowState) };
        let result = if message == WM_SHOWWINDOW {
            if wparam == 0 {
                shadow.hide();
                Ok(())
            } else {
                // WM_SHOWWINDOW 在宿主可见位置更新前发送；此处不能只依赖 IsWindowVisible。
                shadow.sync_with_visibility(Some(true))
            }
        } else {
            shadow.sync()
        };
        if let Err(err) = result {
            tracing::error!(
                target: "shell::window",
                window = %shadow.label,
                message,
                error = %err,
                "同步 Windows 外阴影失败"
            );
        }
    }
    result
}

fn should_sync(message: u32) -> bool {
    matches!(message, WM_WINDOWPOSCHANGED | WM_SIZE | WM_DPICHANGED)
}

fn handles_owner_message(message: u32) -> bool {
    message == WM_SHOWWINDOW || should_sync(message)
}

fn shadow_ex_style() -> u32 {
    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RenderKey {
    width: i32,
    height: i32,
    dpi: u32,
}

impl From<Outline> for RenderKey {
    fn from(outline: Outline) -> Self {
        Self {
            width: outline.width,
            height: outline.height,
            dpi: outline.dpi,
        }
    }
}

struct ShadowState {
    owner: HWND,
    layer: HWND,
    label: String,
    surface: Option<LayerSurface>,
    surface_key: Option<RenderKey>,
    visible: bool,
    syncing: bool,
}

impl ShadowState {
    fn sync(&mut self) -> io::Result<()> {
        self.sync_with_visibility(None)
    }

    fn sync_with_visibility(&mut self, visibility_hint: Option<bool>) -> io::Result<()> {
        if self.syncing {
            return Ok(());
        }
        self.syncing = true;
        let result = self.sync_inner(visibility_hint);
        self.syncing = false;
        result
    }

    fn sync_inner(&mut self, visibility_hint: Option<bool>) -> io::Result<()> {
        let visible =
            visibility_hint.unwrap_or_else(|| unsafe { IsWindowVisible(self.owner) } != 0);
        if unsafe { IsWindow(self.owner) } == 0 || !visible {
            self.hide();
            return Ok(());
        }
        let Some(outline) = read_outline(self.owner)? else {
            self.hide();
            return Ok(());
        };
        if outline.square {
            self.hide();
            return Ok(());
        }

        let key = RenderKey::from(outline);
        let rasterized = self.surface_key != Some(key);
        if rasterized {
            let next = LayerSurface::new(outline)?;
            self.surface = Some(next);
            self.surface_key = Some(key);
        }

        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| io::Error::other("阴影位图未初始化"))?;
        let mut bounds = Default::default();
        if unsafe { GetWindowRect(self.owner, &mut bounds) } == 0 {
            return Err(win32_error("GetWindowRect(shadow)"));
        }
        let origin = POINT {
            x: bounds.left - surface.layout.margin,
            y: bounds.top - surface.layout.margin,
        };

        if rasterized {
            surface.present(self.layer, origin)?;
        } else {
            position_layer(self.layer, origin)?;
        }
        if !self.visible {
            unsafe {
                ShowWindow(self.layer, SW_SHOWNA);
            }
            self.visible = true;
        }
        Ok(())
    }

    fn hide(&mut self) {
        if unsafe { IsWindow(self.layer) } != 0 && unsafe { IsWindowVisible(self.layer) } != 0 {
            unsafe {
                ShowWindow(self.layer, SW_HIDE);
            }
        }
        self.visible = false;
    }

    fn destroy_layer(&mut self) {
        self.surface = None;
        self.surface_key = None;
        self.visible = false;
        if unsafe { IsWindow(self.layer) } != 0 {
            unsafe {
                DestroyWindow(self.layer);
            }
        }
        self.layer = null_mut();
    }

    fn shutdown(&mut self) {
        self.destroy_layer();
    }
}

fn position_layer(layer: HWND, origin: POINT) -> io::Result<()> {
    let positioned = unsafe {
        SetWindowPos(
            layer,
            null_mut(),
            origin.x,
            origin.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        )
    };
    if positioned == 0 {
        Err(win32_error("SetWindowPos(shadow)"))
    } else {
        Ok(())
    }
}

struct LayerSurface {
    hdc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    layout: ShadowLayout,
}

impl LayerSurface {
    fn new(outline: Outline) -> io::Result<Self> {
        let layout = ShadowLayout::new(outline)?;
        let hdc = unsafe { CreateCompatibleDC(null_mut()) };
        if hdc.is_null() {
            return Err(win32_error("CreateCompatibleDC(shadow)"));
        }
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: layout.width,
                biHeight: -layout.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: layout.byte_len as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        let bitmap = unsafe {
            CreateDIBSection(hdc, &bitmap_info, DIB_RGB_COLORS, &mut bits, null_mut(), 0)
        };
        if bitmap.is_null() || bits.is_null() {
            unsafe {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                DeleteDC(hdc);
            }
            return Err(win32_error("CreateDIBSection(shadow)"));
        }
        let old_bitmap = unsafe { SelectObject(hdc, bitmap) };
        if old_bitmap.is_null() || old_bitmap as isize == -1 {
            unsafe {
                DeleteObject(bitmap);
                DeleteDC(hdc);
            }
            return Err(win32_error("SelectObject(shadow)"));
        }

        let surface = Self {
            hdc,
            bitmap,
            old_bitmap,
            layout,
        };
        let pixels = unsafe { slice::from_raw_parts_mut(bits.cast::<u8>(), layout.byte_len) };
        rasterize_shadow(layout, pixels);
        Ok(surface)
    }

    fn present(&self, layer: HWND, origin: POINT) -> io::Result<()> {
        let source = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: self.layout.width,
            cy: self.layout.height,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let updated = unsafe {
            UpdateLayeredWindow(
                layer,
                null_mut(),
                &origin,
                &size,
                self.hdc,
                &source,
                0,
                &blend,
                ULW_ALPHA,
            )
        };
        if updated == 0 {
            return Err(win32_error("UpdateLayeredWindow(shadow)"));
        }
        Ok(())
    }
}

impl Drop for LayerSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old_bitmap);
            DeleteObject(self.bitmap);
            DeleteDC(self.hdc);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ShadowLayout {
    margin: i32,
    width: i32,
    height: i32,
    content_width: i32,
    content_height: i32,
    radius: f32,
    dpi: u32,
    byte_len: usize,
}

impl ShadowLayout {
    fn new(outline: Outline) -> io::Result<Self> {
        if outline.width <= 0 || outline.height <= 0 || outline.dpi == 0 {
            return Err(io::Error::other("窗口尺寸或 DPI 无效"));
        }
        let margin = scale_pixels(MARGIN_DIP, outline.dpi);
        let width = outline
            .width
            .checked_add(
                margin
                    .checked_mul(2)
                    .ok_or_else(|| io::Error::other("阴影横向边距溢出"))?,
            )
            .ok_or_else(|| io::Error::other("阴影宽度溢出"))?;
        let height = outline
            .height
            .checked_add(
                margin
                    .checked_mul(2)
                    .ok_or_else(|| io::Error::other("阴影纵向边距溢出"))?,
            )
            .ok_or_else(|| io::Error::other("阴影高度溢出"))?;
        let byte_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| io::Error::other("阴影位图尺寸溢出"))?;
        Ok(Self {
            margin,
            width,
            height,
            content_width: outline.width,
            content_height: outline.height,
            radius: outline.corner_diameter() as f32 / 2.0,
            dpi: outline.dpi,
            byte_len,
        })
    }
}

fn rasterize_shadow(layout: ShadowLayout, pixels: &mut [u8]) {
    debug_assert_eq!(pixels.len(), layout.byte_len);
    pixels.fill(0);
    let dpi_scale = layout.dpi as f32 / 96.0;
    let radius = layout.radius;
    let ambient_offset = AMBIENT_OFFSET_DIP * dpi_scale;
    let key_offset = KEY_OFFSET_DIP * dpi_scale;
    let ambient_sigma = (AMBIENT_SIGMA_DIP * dpi_scale).max(1.0);
    let key_sigma = (KEY_SIGMA_DIP * dpi_scale).max(1.0);
    let edge_fade = (EDGE_FADE_DIP * dpi_scale).max(1.0);
    let margin = layout.margin as f32;

    for y in 0..layout.height {
        let py = y as f32 + 0.5;
        for x in 0..layout.width {
            let px = x as f32 + 0.5;
            let content_distance = rounded_rect_distance(
                px,
                py,
                margin,
                margin,
                layout.content_width as f32,
                layout.content_height as f32,
                radius,
            );
            // 宿主 HWND 内部完全不参与绘制；只处理圆角外轮廓之外的阴影。
            if content_distance < 0.0 {
                continue;
            }

            let ambient_distance = rounded_rect_distance(
                px,
                py,
                margin,
                margin + ambient_offset,
                layout.content_width as f32,
                layout.content_height as f32,
                radius,
            )
            .max(0.0);
            let key_distance = rounded_rect_distance(
                px,
                py,
                margin,
                margin + key_offset,
                layout.content_width as f32,
                layout.content_height as f32,
                radius,
            )
            .max(0.0);
            let ambient = AMBIENT_ALPHA * (-0.5 * (ambient_distance / ambient_sigma).powi(2)).exp();
            let key = KEY_ALPHA * (-0.5 * (key_distance / key_sigma).powi(2)).exp();
            let edge_distance = px
                .min(py)
                .min(layout.width as f32 - px)
                .min(layout.height as f32 - py)
                .max(0.0);
            let edge = smoothstep(0.0, edge_fade, edge_distance);
            let alpha = ((ambient + key) * edge).clamp(0.0, MAX_ALPHA).round() as u8;
            if alpha == 0 {
                continue;
            }
            let offset = ((y as usize * layout.width as usize) + x as usize) * 4;
            // 纯黑阴影使用预乘 alpha，RGB 通道保持零。
            pixels[offset + 3] = alpha;
        }
    }
}

fn rounded_rect_distance(
    px: f32,
    py: f32,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    radius: f32,
) -> f32 {
    let half_width = width / 2.0;
    let half_height = height / 2.0;
    let radius = radius.min(half_width).min(half_height).max(0.0);
    let qx = (px - (left + half_width)).abs() - (half_width - radius);
    let qy = (py - (top + half_height)).abs() - (half_height - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn scale_pixels(dip: f32, dpi: u32) -> i32 {
    ((dip * dpi as f32 / 96.0).round() as i32).max(1)
}

fn win32_error(operation: &str) -> io::Error {
    let err = io::Error::last_os_error();
    io::Error::new(err.kind(), format!("{operation}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::{
        GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;

    fn outline(width: i32, height: i32, dpi: u32) -> Outline {
        Outline {
            width,
            height,
            dpi,
            square: false,
        }
    }

    fn rendered(outline: Outline) -> (ShadowLayout, Vec<u8>) {
        let layout = ShadowLayout::new(outline).unwrap();
        let mut pixels = vec![0; layout.byte_len];
        rasterize_shadow(layout, &mut pixels);
        (layout, pixels)
    }

    fn alpha_at(layout: ShadowLayout, pixels: &[u8], x: i32, y: i32) -> u8 {
        let offset = ((y as usize * layout.width as usize) + x as usize) * 4;
        pixels[offset + 3]
    }

    #[test]
    fn content_region_is_fully_transparent() {
        let window = outline(640, 480, 96);
        let (layout, pixels) = rendered(window);
        for (x, y) in [
            (
                layout.margin + window.width / 2,
                layout.margin + window.height / 2,
            ),
            (layout.margin + 1, layout.margin + window.height / 2),
            (
                layout.margin + window.width - 2,
                layout.margin + window.height / 2,
            ),
            (layout.margin + window.width / 2, layout.margin + 1),
            (
                layout.margin + window.width / 2,
                layout.margin + window.height - 2,
            ),
        ] {
            assert_eq!(alpha_at(layout, &pixels, x, y), 0);
        }
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]));
    }

    #[test]
    fn shadow_is_soft_outside_round_corners_and_fades_to_zero() {
        let window = outline(640, 480, 96);
        let (layout, pixels) = rendered(window);
        let corner = alpha_at(layout, &pixels, layout.margin - 2, layout.margin - 2);
        let edge = alpha_at(layout, &pixels, layout.margin + 320, layout.margin - 6);
        let outer = alpha_at(layout, &pixels, 0, 0);
        assert!(corner > 0);
        assert!(edge > 0);
        assert!(corner <= MAX_ALPHA as u8);
        assert_eq!(outer, 0);
    }

    #[test]
    fn layout_scales_margin_and_radius_with_dpi() {
        let at_96 = ShadowLayout::new(outline(640, 480, 96)).unwrap();
        let at_192 = ShadowLayout::new(outline(640, 480, 192)).unwrap();
        assert_eq!(at_96.margin, 28);
        assert_eq!(at_192.margin, 56);
        assert_eq!(at_96.radius, 10.0);
        assert_eq!(at_192.radius, 20.0);
    }

    #[test]
    fn render_key_ignores_pure_movement_and_changes_for_resize_or_dpi() {
        let base = outline(640, 480, 96);
        assert_eq!(RenderKey::from(base), RenderKey::from(base));
        assert_ne!(
            RenderKey::from(base),
            RenderKey::from(outline(641, 480, 96))
        );
        assert_ne!(
            RenderKey::from(base),
            RenderKey::from(outline(640, 480, 120))
        );
    }

    #[test]
    fn lifecycle_messages_cover_show_move_size_and_dpi() {
        for message in [WM_SHOWWINDOW, WM_WINDOWPOSCHANGED, WM_SIZE, WM_DPICHANGED] {
            assert!(handles_owner_message(message));
        }
        assert!(!handles_owner_message(WM_MOUSEACTIVATE));
    }

    #[test]
    fn helper_layer_is_layered_click_through_non_activating_and_hidden_from_switchers() {
        let style = shadow_ex_style();
        assert_ne!(style & WS_EX_LAYERED, 0);
        assert_ne!(style & WS_EX_TRANSPARENT, 0);
        assert_ne!(style & WS_EX_NOACTIVATE, 0);
        assert_ne!(style & WS_EX_TOOLWINDOW, 0);
        assert_eq!(
            unsafe { shadow_window_proc(null_mut(), WM_NCHITTEST, 0, 0) },
            HTTRANSPARENT as LRESULT
        );
        assert_eq!(
            unsafe { shadow_window_proc(null_mut(), WM_MOUSEACTIVATE, 0, 0) },
            MA_NOACTIVATE as LRESULT
        );
    }

    #[test]
    #[ignore = "process-wide GDI/USER counts require an isolated desktop test"]
    fn repeated_shadow_show_resize_hide_and_destroy_does_not_leak_gui_resources() {
        fn gui_counts() -> (u32, u32) {
            let process = unsafe { GetCurrentProcess() };
            unsafe {
                (
                    GetGuiResources(process, GR_GDIOBJECTS),
                    GetGuiResources(process, GR_USEROBJECTS),
                )
            }
        }

        let (gdi_before, user_before) = gui_counts();
        for cycle in 0..12 {
            let owner = unsafe {
                CreateWindowExW(
                    0,
                    STATIC_CLASS_NAME.as_ptr(),
                    null(),
                    WS_OVERLAPPEDWINDOW,
                    100,
                    100,
                    640,
                    480,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            assert!(!owner.is_null(), "test owner window creation failed");
            install(owner, "shadow-resource-test").expect("shadow install failed");

            for step in 0..3 {
                unsafe {
                    ShowWindow(owner, SW_SHOWNA);
                    SetWindowPos(
                        owner,
                        null_mut(),
                        100 + cycle * 8 + step * 4,
                        100 + cycle * 4 + step * 3,
                        640 + step * 12,
                        480 + step * 8,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    ShowWindow(owner, SW_HIDE);
                }
            }
            unsafe {
                DestroyWindow(owner);
            }
        }

        let (gdi_after, user_after) = gui_counts();
        assert!(
            gdi_after <= gdi_before + 4,
            "GDI objects grew from {gdi_before} to {gdi_after}"
        );
        assert!(
            user_after <= user_before + 4,
            "USER objects grew from {user_before} to {user_after}"
        );
    }
}
