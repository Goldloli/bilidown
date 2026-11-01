//! 结构化错误：core 只输出**分类 + 技术细节**，展示文案与建议操作由前端映射
//! （design D2：core 不携带 UI 文案）。
//!
//! 序列化形状（对前端稳定契约）：
//!
//! ```json
//! {"kind": "risk_control"}
//! {"kind": "api", "detail": {"code": -404, "message": "啥都木有"}}
//! {"kind": "network", "detail": "connection refused"}
//! ```

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 错误分类。前端据此渲染文案、图标与建议操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// B 站接口返回非 0（未归入更具体分类）
    Api,
    /// 风控：-352 / -509 / HTTP 412
    RiskControl,
    /// 未登录 / 凭据失效
    Auth,
    /// 权限不足（付费/私享/审核）
    Permission,
    /// 地区限制
    Region,
    /// 需要大会员
    VipRequired,
    /// 网络错误（连接/传输）
    Network,
    /// 网络超时
    Timeout,
    /// CDN 拒绝（403/404，已尝试备用 URL）
    Cdn,
    /// ffmpeg 缺失或执行失败
    Ffmpeg,
    /// 磁盘/文件系统
    Disk,
    /// 响应解析失败
    Parse,
    /// 输入无法识别
    InvalidInput,
    /// 用户取消
    Cancelled,
    /// 其他
    Other,
}

impl ErrorKind {
    /// 是否属于「稍后重试可能成功」的错误（前端可展示重试入口，但不自动轰炸）。
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::RiskControl | ErrorKind::Network | ErrorKind::Timeout | ErrorKind::Cdn
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Api => "api",
            ErrorKind::RiskControl => "risk_control",
            ErrorKind::Auth => "auth",
            ErrorKind::Permission => "permission",
            ErrorKind::Region => "region",
            ErrorKind::VipRequired => "vip_required",
            ErrorKind::Network => "network",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cdn => "cdn",
            ErrorKind::Ffmpeg => "ffmpeg",
            ErrorKind::Disk => "disk",
            ErrorKind::Parse => "parse",
            ErrorKind::InvalidInput => "invalid_input",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Other => "other",
        }
    }
}

/// 统一错误类型。内部保留可诊断的技术细节（API message / stderr / 系统错误），
/// 不做本地化；前端按 [`ErrorKind`] 渲染用户文案。
#[derive(Debug, Clone, PartialEq, Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum BiliError {
    /// 接口返回非 0。
    #[error("api error {code}: {message}")]
    Api { code: i64, message: String },
    /// 触发风控（-352 / -509 / 412）。
    #[error("risk control")]
    RiskControl,
    /// 未登录或凭据失效（-101）。
    #[error("not logged in")]
    NotLoggedIn,
    /// 权限不足。
    #[error("permission denied: {message}")]
    Permission { message: String },
    /// 地区限制。
    #[error("region restricted: {message}")]
    Region { message: String },
    /// 需要大会员。
    #[error("vip required: {message}")]
    VipRequired { message: String },
    /// 网络层错误。
    #[error("network: {0}")]
    Network(String),
    /// 网络超时。
    #[error("timeout: {0}")]
    Timeout(String),
    /// 流媒体 CDN 拒绝（403/404，已尝试备用 URL）。
    #[error("cdn denied")]
    CdnDenied,
    /// ffmpeg 缺失或执行失败。
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    /// 磁盘/文件系统错误。
    #[error("disk: {0}")]
    Disk(String),
    /// 响应解析失败。
    #[error("parse: {0}")]
    Parse(String),
    /// 输入无法识别。
    #[error("invalid input")]
    InvalidInput,
    /// 用户取消（暂停与真正取消由上层区分）。
    #[error("cancelled")]
    Cancelled,
    /// 其他 IO 错误。
    #[error("io: {0}")]
    Io(String),
    /// 其他。
    #[error("{0}")]
    Other(String),
}

impl BiliError {
    /// 错误分类（序列化字段 `kind` 的强类型版本）。
    pub fn kind(&self) -> ErrorKind {
        match self {
            BiliError::Api { .. } => ErrorKind::Api,
            BiliError::RiskControl => ErrorKind::RiskControl,
            BiliError::NotLoggedIn => ErrorKind::Auth,
            BiliError::Permission { .. } => ErrorKind::Permission,
            BiliError::Region { .. } => ErrorKind::Region,
            BiliError::VipRequired { .. } => ErrorKind::VipRequired,
            BiliError::Network(_) => ErrorKind::Network,
            BiliError::Timeout(_) => ErrorKind::Timeout,
            BiliError::CdnDenied => ErrorKind::Cdn,
            BiliError::Ffmpeg(_) => ErrorKind::Ffmpeg,
            BiliError::Disk(_) => ErrorKind::Disk,
            BiliError::Parse(_) => ErrorKind::Parse,
            BiliError::InvalidInput => ErrorKind::InvalidInput,
            BiliError::Cancelled => ErrorKind::Cancelled,
            BiliError::Io(_) => ErrorKind::Disk,
            BiliError::Other(_) => ErrorKind::Other,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, BiliError::Cancelled)
    }

    pub fn is_retryable(&self) -> bool {
        self.kind().retryable()
    }

    /// B 站接口 code/message 归一：可识别的类别转具体错误，其余保留 code。
    pub fn from_api(code: i64, message: impl Into<String>) -> Self {
        let message = message.into();
        match code {
            // 风控/请求频率
            -352 | -509 | 412 => BiliError::RiskControl,
            // 未登录
            -101 => BiliError::NotLoggedIn,
            // 地区限制（番剧/影视）
            -10403 => BiliError::Region { message },
            // 大会员专享
            87007 | 87008 => BiliError::VipRequired { message },
            // 付费/私享/审核/其他权限
            -403 | 403 => BiliError::Permission { message },
            _ => BiliError::Api { code, message },
        }
    }

    /// 构建网络错误；reqwest 的超时错误归入 [`BiliError::Timeout`]。
    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            return BiliError::Timeout(e.to_string());
        }
        if e.is_connect() {
            return BiliError::Network(format!("connect: {e}"));
        }
        if e.is_status() {
            let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
            return match status {
                403 | 404 => BiliError::CdnDenied,
                412 => BiliError::RiskControl,
                401 => BiliError::NotLoggedIn,
                _ => BiliError::Network(format!("HTTP {status}")),
            };
        }
        BiliError::Network(e.to_string())
    }

    pub fn describe(&self) -> String {
        match self {
            BiliError::Api { code, message } => format!("code {code}: {message}"),
            other => other.to_string(),
        }
    }
}

impl From<reqwest::Error> for BiliError {
    fn from(e: reqwest::Error) -> Self {
        BiliError::from_reqwest(e)
    }
}

impl From<serde_json::Error> for BiliError {
    fn from(e: serde_json::Error) -> Self {
        BiliError::Parse(e.to_string())
    }
}

impl From<std::io::Error> for BiliError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::AlreadyExists
            | std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::ReadOnlyFilesystem => BiliError::Disk(e.to_string()),
            _ => BiliError::Io(e.to_string()),
        }
    }
}

pub type BiliResult<T> = Result<T, BiliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_code_classification() {
        assert_eq!(
            BiliError::from_api(-352, "风控").kind(),
            ErrorKind::RiskControl
        );
        assert_eq!(
            BiliError::from_api(-509, "请求过于频繁").kind(),
            ErrorKind::RiskControl
        );
        assert_eq!(
            BiliError::from_api(-101, "账号未登录").kind(),
            ErrorKind::Auth
        );
        assert_eq!(
            BiliError::from_api(-10403, "地区限制").kind(),
            ErrorKind::Region
        );
        assert_eq!(
            BiliError::from_api(87007, "大会员专享").kind(),
            ErrorKind::VipRequired
        );
        assert_eq!(
            BiliError::from_api(-403, "权限不足").kind(),
            ErrorKind::Permission
        );
        match BiliError::from_api(-400, "请求错误") {
            BiliError::Api { code, message } => {
                assert_eq!(code, -400);
                assert_eq!(message, "请求错误");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn retryable_classification() {
        assert!(BiliError::RiskControl.is_retryable());
        assert!(BiliError::Timeout("t".into()).is_retryable());
        assert!(!BiliError::NotLoggedIn.is_retryable());
        assert!(!BiliError::InvalidInput.is_retryable());
        assert!(
            !BiliError::Permission {
                message: String::new()
            }
            .is_retryable()
        );
    }

    #[test]
    fn serde_shape_is_stable() {
        let unit = serde_json::to_value(BiliError::RiskControl).unwrap();
        assert_eq!(unit, serde_json::json!({"kind": "risk_control"}));
        let api = serde_json::to_value(BiliError::Api {
            code: -404,
            message: "啥都木有".into(),
        })
        .unwrap();
        assert_eq!(
            api,
            serde_json::json!({"kind": "api", "detail": {"code": -404, "message": "啥都木有"}})
        );
        let net = serde_json::to_value(BiliError::Network("refused".into())).unwrap();
        assert_eq!(
            net,
            serde_json::json!({"kind": "network", "detail": "refused"})
        );
        // 往返
        let back: BiliError = serde_json::from_value(api).unwrap();
        assert_eq!(back.kind(), ErrorKind::Api);
        let back: BiliError = serde_json::from_value(unit).unwrap();
        assert!(matches!(back, BiliError::RiskControl));
    }

    #[test]
    fn io_error_classification() {
        let disk = BiliError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        ));
        assert_eq!(disk.kind(), ErrorKind::Disk);
        let io = BiliError::from(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
        assert_eq!(io.kind(), ErrorKind::Disk);
    }
}
