//! 平台能力探测：窗口材质与系统通知在 Windows 上的可用性。
//!
//! 这些能力通过 `AppInfoDTO` 暴露给前端，用于：
//! - 材质不可用时把磨砂玻璃开关置灰并回退不透明；
//! - 通知不可用时不宣称系统通知，改用应用内提示。

/// 当前窗口策略是否支持原生材质，与用户偏好（开/关）无关。
/// Windows 使用不透明 HWND/WebView 保证真实圆角，不启用整窗 Acrylic。
pub fn glass_supported() -> bool {
    cfg!(target_os = "macos")
}

pub fn glass_unavailable_reason() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("当前窗口模式使用不透明背景，以保证圆角与边缘显示正常。")
    } else if !glass_supported() {
        Some("当前窗口模式不支持原生背景模糊，已使用不透明背景。")
    } else {
        None
    }
}

/// 「在文件夹中显示」动作的平台文案（Windows 资源管理器 / macOS Finder）。
pub const REVEAL_LABEL: &str = if cfg!(target_os = "windows") {
    "在资源管理器中显示"
} else {
    "在 Finder 中显示"
};

/// 系统通知是否可靠可用。
///
/// Windows 的 toast 依赖安装器写入的应用用户模型 ID（AUMID，来自开始菜单快捷方式）；
/// 开发运行或免安装直接运行 exe 时会显示为 PowerShell 且不可点击，因此仅在安装版
/// （可执行文件位于 `%LOCALAPPDATA%` 下）返回 true。macOS 始终可用。
pub fn notifications_supported() -> bool {
    #[cfg(target_os = "windows")]
    {
        if let (Ok(exe), Some(local)) = (std::env::current_exe(), dirs::data_local_dir()) {
            return exe.starts_with(local);
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}
