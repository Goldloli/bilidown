//! 扫码登录 + cookie 自动续期（design D4/D7）：
//!
//! - 二维码由 core 用 `qrcode` crate 输出模块矩阵，前端 canvas 绘制（不调用外部服务）；
//! - 轮询状态机 0/860/861/864；成功时从 Set-Cookie 提取登录 cookie + `refresh_token`；
//! - cookie 续期：`cookie/info` 检测 → RSA-OAEP(SHA-256) 生成 correspondPath（小写 hex）
//!   → correspond 页面解析 `#1-name` 得 refresh_csrf → `cookie/refresh` 更新 cookie 与
//!   refresh_token → `confirm/refresh`；失败按步骤分类返回，由账号层决定
//!   「引导重新扫码」还是「下次启动再试」。

use rsa::pkcs8::DecodePublicKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::{BiliClient, StoredCookies};
use super::error::{BiliError, BiliResult, ErrorKind};
use super::types::UserInfo;

/// 轮询状态（B 站 code：0 成功 / 860 已扫待确认 / 861 未扫 / 864 过期）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    Idle,
    /// 二维码已出，等待扫码
    Waiting,
    /// 已扫码，等待手机确认
    Scanned,
    /// 二维码过期（前端可一键刷新）
    Expired,
    /// 登录成功
    Success,
    Failed,
}

/// 二维码模块矩阵（row-major，true = 深色模块）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: Vec<bool>,
}

impl QrMatrix {
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.modules
            .get(y * self.size + x)
            .copied()
            .unwrap_or(false)
    }
}

/// 生成二维码矩阵（不依赖任何外部服务）。
pub fn qr_matrix(text: &str) -> BiliResult<QrMatrix> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M)
        .map_err(|e| BiliError::Parse(format!("qrcode encode: {e}")))?;
    let width = code.width();
    let colors = code.to_colors();
    let modules = colors.iter().map(|c| *c == qrcode::Color::Dark).collect();
    Ok(QrMatrix {
        size: width,
        modules,
    })
}

/// 旧 egui 应用兼容别名（8.1 删除旧 UI 后移除）。
pub type LoginFlow = QrLogin;

/// 一次登录流程。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrLogin {
    pub qrcode_key: String,
    /// 待编码进二维码的 URL
    pub qr_url: String,
    pub state: LoginState,
    /// 二维码矩阵（前端直接绘制）
    pub matrix: QrMatrix,
}

/// 扫码成功：登录 cookie（已过滤为需要持久化的关键项）+ 持久化刷新口令。
#[derive(Debug, Clone, PartialEq)]
pub struct LoginSuccess {
    pub cookies: StoredCookies,
    /// 登录响应返回的 `refresh_token`（B 站可能不返回，此时为 `None`）
    pub refresh_token: Option<String>,
}

/// 轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub enum LoginPoll {
    /// 状态未变或中间态
    Pending(LoginState),
    /// 成功：客户端 cookie 已更新，返回应持久化的 cookie 与 refresh_token
    Success(LoginSuccess),
    /// 过期/失败
    Ended(LoginState),
}

impl QrLogin {
    /// 申请二维码。
    pub async fn start(client: &BiliClient) -> BiliResult<Self> {
        #[derive(Deserialize)]
        struct Resp {
            code: i64,
            message: Option<String>,
            data: Option<QrData>,
        }
        #[derive(Deserialize)]
        struct QrData {
            url: String,
            qrcode_key: String,
        }
        let url = client
            .endpoints()
            .passport_url("/x/passport-login/web/qrcode/generate");
        let resp = client.get_response(&url).await?;
        let body: Resp = resp.json().await?;
        if body.code != 0 {
            return Err(BiliError::from_api(
                body.code,
                body.message.unwrap_or_default(),
            ));
        }
        let d = body
            .data
            .ok_or_else(|| BiliError::Parse("二维码响应缺 data".into()))?;
        let matrix = qr_matrix(&d.url)?;
        Ok(Self {
            qrcode_key: d.qrcode_key,
            qr_url: d.url,
            state: LoginState::Waiting,
            matrix,
        })
    }

    /// 轮询一次；成功后返回应持久化的登录 cookie 与 refresh_token。
    pub async fn poll(&mut self, client: &BiliClient) -> BiliResult<LoginPoll> {
        #[derive(Deserialize)]
        struct Resp {
            code: i64,
            #[serde(default)]
            message: String,
            data: Option<PollData>,
        }
        #[derive(Deserialize)]
        struct PollData {
            code: i64,
            #[serde(default)]
            message: String,
            /// 登录成功时返回的持久化刷新口令（旧协议可能缺省）
            #[serde(default)]
            refresh_token: Option<String>,
        }
        let url = format!(
            "{}?qrcode_key={}",
            client
                .endpoints()
                .passport_url("/x/passport-login/web/qrcode/poll"),
            self.qrcode_key
        );
        let resp = client.get_response(&url).await?;
        let body: Resp = resp.json().await?;
        if body.code != 0 {
            self.state = LoginState::Failed;
            return Err(BiliError::from_api(body.code, body.message));
        }
        let (code, message, refresh_token) = match body.data {
            Some(d) => (d.code, d.message, d.refresh_token),
            None => (0, String::new(), None),
        };
        apply_poll_code(&mut self.state, code, &message);
        match self.state {
            LoginState::Success => {
                let cookies = filter_login_cookies(client.cookies());
                if cookies.is_empty() {
                    return Err(BiliError::Parse("登录成功但未收到 cookie".into()));
                }
                let refresh_token = refresh_token.filter(|token| !token.is_empty());
                if refresh_token.is_none() {
                    tracing::warn!("登录响应未返回 refresh_token，cookie 自动续期不可用");
                }
                Ok(LoginPoll::Success(LoginSuccess {
                    cookies,
                    refresh_token,
                }))
            }
            LoginState::Expired | LoginState::Failed => Ok(LoginPoll::Ended(self.state.clone())),
            _ => Ok(LoginPoll::Pending(self.state.clone())),
        }
    }

    /// 重新申请（过期后一键刷新）。
    pub async fn refresh(&mut self, client: &BiliClient) -> BiliResult<()> {
        let fresh = Self::start(client).await?;
        *self = fresh;
        Ok(())
    }
}

/// 纯函数：轮询 code → 状态机流转（单测友好）。
///
/// B 站 2026 起 poll 的 `data.code` 改为五位码，同时保留旧三位码兼容：
/// `86101` 未扫码 / `86090` 已扫待确认 / `86038` 已失效；旧协议为 861/860/864。
fn apply_poll_code(state: &mut LoginState, code: i64, message: &str) {
    *state = match code {
        0 => LoginState::Success,
        860 | 86090 => LoginState::Scanned,
        861 | 86101 | -4 => LoginState::Waiting,
        864 | 86038 => LoginState::Expired,
        -1 | -2 => LoginState::Failed,
        _ => {
            if !message.is_empty() {
                tracing::debug!(code, message, "login poll unexpected code");
            }
            LoginState::Failed
        }
    };
}

/// 过滤需要持久化的登录 cookie（保留顺序与去重）。
pub fn filter_login_cookies(stored: StoredCookies) -> StoredCookies {
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in stored.cookies {
        if v.is_empty() || !LOGIN_COOKIE_KEYS.contains(&k.as_str()) {
            continue;
        }
        if out.iter().any(|(name, _)| name == &k) {
            continue;
        }
        out.push((k, v));
    }
    StoredCookies { cookies: out }
}

/// 需要持久化的关键登录 cookie 名。
pub const LOGIN_COOKIE_KEYS: [&str; 10] = [
    "SESSDATA",
    "bili_jct",
    "DedeUserID",
    "DedeUserID__ckMd5",
    "sid",
    "buvid3",
    "buvid4",
    "b_nut",
    "bili_ticket",
    "bili_ticket_expires",
];

/// cookie 续期状态（`/x/passport-login/web/cookie/info`，host 为 `passport.bilibili.com`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookieRefreshInfo {
    /// 是否需要续期
    pub refresh: bool,
    /// 当前毫秒时间戳（用于 correspondPath 流程）
    pub timestamp: i64,
}

/// B 站 correspondPath 公钥（RSA 1024-bit）。
///
/// 来源：bilibili-API-collect `docs/login/cookie_refresh.md`。算法固定为
/// **RSA-OAEP(SHA-256)**，密文以小写 base16（hex）编码。
pub const BILI_CORRESPOND_PUB_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDLgd2OAkcGVtoE3ThUREbio0Eg
Uc/prcajMKXvkCKFCWhJYJcLkcM2DKKcSeFpD/j6Boy538YXnR6VhcuUJOhH2x71
nzPjfdTcqMz7djHum0qSZA0AyCBDABUqCrfNgCiJ00Ra7GmRj+YCK1NJEuewlb40
JNrRuoEUXpabUzGB8QIDAQAB
-----END PUBLIC KEY-----";

/// 解析内置的 correspondPath 公钥（PEM/SPKI）。
pub fn correspond_public_key() -> BiliResult<rsa::RsaPublicKey> {
    rsa::RsaPublicKey::from_public_key_pem(BILI_CORRESPOND_PUB_KEY)
        .map_err(|e| BiliError::Other(format!("correspondPath 公钥解析失败: {e}")))
}

/// 用给定公钥加密 `refresh_{timestamp_ms}`，返回小写 hex 的 correspondPath。
pub fn encrypt_correspond_path(key: &rsa::RsaPublicKey, timestamp_ms: i64) -> BiliResult<String> {
    let mut rng = rand::thread_rng();
    let message = format!("refresh_{timestamp_ms}");
    let cipher = key
        .encrypt(
            &mut rng,
            rsa::Oaep::new::<sha2::Sha256>(),
            message.as_bytes(),
        )
        .map_err(|e| BiliError::Other(format!("correspondPath 加密失败: {e}")))?;
    Ok(super::client::hex_lower(&cipher))
}

/// 用内置公钥生成 correspondPath（`timestamp_ms` 取 [`CookieRefreshInfo::timestamp`]）。
pub fn correspond_path(timestamp_ms: i64) -> BiliResult<String> {
    encrypt_correspond_path(&correspond_public_key()?, timestamp_ms)
}

/// 从 correspond 页面 HTML 中解析 `#1-name` 的 refresh_csrf。
///
/// 页面形如 `<div id="1-name">b0cc...</div>`；解析失败（404/结构变化）返回 `None`。
pub fn parse_refresh_csrf(html: &str) -> Option<String> {
    for marker in ["id=\"1-name\"", "id='1-name'"] {
        let Some(pos) = html.find(marker) else {
            continue;
        };
        let after = &html[pos + marker.len()..];
        let Some(tag_end) = after.find('>') else {
            continue;
        };
        let rest = &after[tag_end + 1..];
        let Some(value_end) = rest.find('<') else {
            continue;
        };
        let value = rest[..value_end].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// 查询 cookie 是否需要续期（需已登录的 bili_jct）。
pub async fn cookie_refresh_info(client: &BiliClient) -> BiliResult<CookieRefreshInfo> {
    let csrf = client.csrf().ok_or(BiliError::NotLoggedIn)?;
    let url = client
        .endpoints()
        .passport_url("/x/passport-login/web/cookie/info");
    let data = client.get_json(&url, &[("csrf".to_string(), csrf)]).await?;
    Ok(CookieRefreshInfo {
        refresh: data
            .get("refresh")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        timestamp: data.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 生成 correspondPath → 拉取 correspond 页面 → 解析 refresh_csrf。
pub async fn fetch_refresh_csrf(client: &BiliClient, timestamp_ms: i64) -> BiliResult<String> {
    let path = correspond_path(timestamp_ms)?;
    let url = client.endpoints().www_url(&format!("/correspond/1/{path}"));
    let html = client.get_text(&url).await?;
    parse_refresh_csrf(&html)
        .ok_or_else(|| BiliError::Parse("correspond 页面缺少 #1-name refresh_csrf".into()))
}

/// 刷新成功：客户端 cookie 已原地更新，`refresh_token` 为新的持久化口令。
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshedCookies {
    pub cookies: StoredCookies,
    pub refresh_token: String,
}

/// 续期第一步：`POST /x/passport-login/web/cookie/refresh`
/// （`csrf` / `refresh_csrf` / `source=main_web` / `refresh_token`）。
///
/// 成功后 B 站通过 Set-Cookie 下发新的 `SESSDATA`/`bili_jct` 等 cookie
/// （已由 client 吸收），响应 `data.refresh_token` 为**新**口令。
pub async fn cookie_refresh(
    client: &BiliClient,
    refresh_csrf: &str,
    refresh_token: &str,
) -> BiliResult<RefreshedCookies> {
    let csrf = client.csrf().ok_or(BiliError::NotLoggedIn)?;
    let url = client
        .endpoints()
        .passport_url("/x/passport-login/web/cookie/refresh");
    let data = client
        .post_form_json(
            &url,
            &[
                ("csrf".to_string(), csrf),
                ("refresh_csrf".to_string(), refresh_csrf.to_string()),
                ("source".to_string(), "main_web".to_string()),
                ("refresh_token".to_string(), refresh_token.to_string()),
            ],
        )
        .await?;
    let new_token = data
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| BiliError::Parse("cookie/refresh 响应缺少 refresh_token".into()))?;
    Ok(RefreshedCookies {
        cookies: client.cookies(),
        refresh_token: new_token.to_string(),
    })
}

/// 续期第二步：`POST /x/passport-login/web/confirm/refresh`
///
/// 使用**新** cookie 中的 `bili_jct` 与**旧** refresh_token 让旧会话失效。
pub async fn cookie_refresh_confirm(
    client: &BiliClient,
    refresh_token_old: &str,
) -> BiliResult<()> {
    let csrf = client.csrf().ok_or(BiliError::NotLoggedIn)?;
    let url = client
        .endpoints()
        .passport_url("/x/passport-login/web/cookie/confirm/refresh");
    client
        .post_form_json(
            &url,
            &[
                ("csrf".to_string(), csrf),
                ("refresh_token".to_string(), refresh_token_old.to_string()),
            ],
        )
        .await?;
    Ok(())
}

/// 自动续期失败原因（按协议步骤分类，便于上层区分「重新扫码」与「稍后再试」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum RefreshError {
    /// 未登录 / 登录 cookie 缺失：不发起网络请求
    #[error("not logged in")]
    NotLoggedIn,
    /// 本地没有 refresh_token（老版本凭据或登录响应未返回）
    #[error("missing refresh_token")]
    MissingRefreshToken,
    /// `cookie/info` 失败
    #[error("cookie info failed: {0}")]
    Info(BiliError),
    /// correspondPath 生成失败（公钥/加密异常）
    #[error("correspondPath failed: {0}")]
    CorrespondPath(String),
    /// correspond 页面获取/解析失败（path 过期、页面结构变化）
    #[error("correspond page failed: {0}")]
    CorrespondPage(BiliError),
    /// `cookie/refresh` 被 B 站拒绝（86095 token 不匹配 / -111 csrf / -101 未登录 等）
    #[error("refresh rejected: {code} {message}")]
    RefreshRejected { code: i64, message: String },
    /// `cookie/refresh` 传输层失败
    #[error("refresh request failed: {0}")]
    Refresh(BiliError),
}

impl RefreshError {
    /// 是否必须重新扫码（refresh_token 缺失/被拒/登录态已失效）。
    pub fn needs_relogin(&self) -> bool {
        match self {
            RefreshError::NotLoggedIn | RefreshError::MissingRefreshToken => true,
            RefreshError::Info(e) | RefreshError::Refresh(e) | RefreshError::CorrespondPage(e) => {
                matches!(e.kind(), ErrorKind::Auth)
            }
            // 公钥/加密异常属实现侧问题，不引导用户重新扫码
            RefreshError::CorrespondPath(_) => false,
            // -101 未登录 / -111 csrf 失效 / 86095 token 与 cookie 不匹配 → 只能重新扫码
            RefreshError::RefreshRejected { code, .. } => matches!(*code, -101 | -111 | 86095),
        }
    }

    /// 协议步骤名（用于日志，不含 cookie/token 值）。
    pub fn step(&self) -> &'static str {
        match self {
            RefreshError::NotLoggedIn => "not_logged_in",
            RefreshError::MissingRefreshToken => "missing_refresh_token",
            RefreshError::Info(_) => "cookie_info",
            RefreshError::CorrespondPath(_) => "correspond_path",
            RefreshError::CorrespondPage(_) => "correspond_page",
            RefreshError::RefreshRejected { .. } => "refresh_rejected",
            RefreshError::Refresh(_) => "refresh_request",
        }
    }
}

/// 自动续期的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// cookie 尚未到需要续期的时间点
    NotNeeded,
    /// 续期成功；`refresh_token` 为新的持久化口令
    Refreshed { refresh_token: String },
}

/// 执行一次完整续期
/// （`cookie/info` → correspondPath → correspond 页面 → `cookie/refresh` → `confirm/refresh`）。
///
/// - 未登录或 refresh_token 缺失：直接返回可判别错误，不发网络请求；
/// - `info.refresh == false`：返回 [`RefreshOutcome::NotNeeded`]；
/// - confirm 失败只影响旧 token 的失效时间，不否定本次续期（仅记 warn）。
pub async fn refresh_cookie(
    client: &BiliClient,
    stored_refresh_token: Option<&str>,
) -> Result<RefreshOutcome, RefreshError> {
    if !client.is_logged_in() {
        return Err(RefreshError::NotLoggedIn);
    }
    let info = cookie_refresh_info(client)
        .await
        .map_err(RefreshError::Info)?;
    if !info.refresh {
        return Ok(RefreshOutcome::NotNeeded);
    }
    let old_token = stored_refresh_token
        .filter(|token| !token.is_empty())
        .ok_or(RefreshError::MissingRefreshToken)?;
    let refresh_csrf = fetch_refresh_csrf(client, info.timestamp)
        .await
        .map_err(RefreshError::CorrespondPage)?;
    let refreshed = match cookie_refresh(client, &refresh_csrf, old_token).await {
        Ok(refreshed) => refreshed,
        Err(BiliError::Api { code, message }) => {
            return Err(RefreshError::RefreshRejected { code, message });
        }
        Err(e) => return Err(RefreshError::Refresh(e)),
    };
    if let Err(e) = cookie_refresh_confirm(client, old_token).await {
        tracing::warn!(error = %e, "cookie refresh confirm failed");
    }
    Ok(RefreshOutcome::Refreshed {
        refresh_token: refreshed.refresh_token,
    })
}

/// 登录成功后校验并拉取用户信息（nav）。
pub async fn validate_login(client: &BiliClient) -> BiliResult<UserInfo> {
    let user = client.nav_user_info().await?;
    if user.is_login {
        Ok(user)
    } else {
        Err(BiliError::NotLoggedIn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk() -> QrLogin {
        QrLogin {
            qrcode_key: "k".into(),
            qr_url: "https://x".into(),
            state: LoginState::Waiting,
            matrix: QrMatrix {
                size: 1,
                modules: vec![true],
            },
        }
    }

    #[test]
    fn qr_matrix_is_square_and_has_modules() {
        let m = qr_matrix("https://passport.bilibili.com/h5-app/passport/login/scan?x=1").unwrap();
        assert!(m.size >= 21, "QR 至少 21 模块");
        assert_eq!(m.modules.len(), m.size * m.size);
        assert!(m.modules.iter().any(|d| *d), "应有深色模块");
        assert!(m.is_dark(0, 0), "左上角应有定位图案");
    }

    #[test]
    fn poll_state_machine() {
        let mut state = LoginState::Idle;
        for code in [861, 861, -4] {
            apply_poll_code(&mut state, code, "");
            assert_eq!(state, LoginState::Waiting, "code {code}");
        }
        apply_poll_code(&mut state, 860, "");
        assert_eq!(state, LoginState::Scanned);
        apply_poll_code(&mut state, 0, "");
        assert_eq!(state, LoginState::Success);
        apply_poll_code(&mut state, 864, "");
        assert_eq!(state, LoginState::Expired);
        apply_poll_code(&mut state, -1, "");
        assert_eq!(state, LoginState::Failed);
        apply_poll_code(&mut state, 999, "?");
        assert_eq!(state, LoginState::Failed);
    }

    #[test]
    fn login_cookie_filtering() {
        let stored = StoredCookies::from_pairs(vec![
            ("SESSDATA".to_string(), "s".to_string()),
            ("bili_jct".to_string(), "c".to_string()),
            ("random".to_string(), "r".to_string()),
            ("SESSDATA".to_string(), "dup".to_string()),
        ]);
        let filtered = filter_login_cookies(stored);
        assert_eq!(filtered.cookies.len(), 2);
        assert_eq!(filtered.get("SESSDATA"), Some("s"));
    }

    #[test]
    fn login_cookie_keys_cover_essentials() {
        assert!(LOGIN_COOKIE_KEYS.contains(&"SESSDATA"));
        assert!(LOGIN_COOKIE_KEYS.contains(&"bili_jct"));
        assert!(LOGIN_COOKIE_KEYS.contains(&"DedeUserID"));
        // 风控凭据也要持久化（bili_ticket 由 client 初始化）
        assert!(LOGIN_COOKIE_KEYS.contains(&"bili_ticket"));
        assert!(LOGIN_COOKIE_KEYS.contains(&"bili_ticket_expires"));
    }

    #[test]
    fn correspond_path_is_lowercase_hex_of_1024_bit_cipher() {
        let path = correspond_path(1_700_000_000_000).unwrap();
        // 1024-bit RSA → 128 字节密文 → 256 个小写 hex 字符
        assert_eq!(path.len(), 256, "cipher hex 长度应为 256");
        assert!(
            path.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn correspond_path_oaep_roundtrip() {
        // 用临时生成的 RSA 私钥验证「OAEP(SHA-256) + 小写 hex」语义正确
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 1024).unwrap();
        let public = rsa::RsaPublicKey::from(&private);
        let path = encrypt_correspond_path(&public, 1_700_000_000_000).unwrap();
        let cipher = hex_bytes(&path);
        let plain = private
            .decrypt(rsa::Oaep::new::<sha2::Sha256>(), &cipher)
            .unwrap();
        assert_eq!(String::from_utf8(plain).unwrap(), "refresh_1700000000000");
        // 内置 B 站公钥可解析
        assert!(correspond_public_key().is_ok());
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2));
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn parse_refresh_csrf_from_correspond_html() {
        let html = "<body><div id=\"1-name\">b0cc8411ded2f9db2cff2edb3123acac</div><div id=\"token-iframe-app\"></div></body>";
        assert_eq!(
            parse_refresh_csrf(html).as_deref(),
            Some("b0cc8411ded2f9db2cff2edb3123acac")
        );
        // 单引号 / 属性顺序兼容
        let html = "<div class=\"x\" id='1-name'>  token-value  </div>";
        assert_eq!(parse_refresh_csrf(html).as_deref(), Some("token-value"));
        // 404 页 / 结构变化 → None
        assert_eq!(
            parse_refresh_csrf("<html><body>404 Not Found</body></html>"),
            None
        );
        assert_eq!(parse_refresh_csrf("<div id=\"1-name\"></div>"), None);
    }

    #[test]
    fn refresh_requires_login() {
        // 无 cookie 时不应发出请求，直接分类错误
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let c = BiliClient::new(None, StoredCookies::default()).unwrap();
            assert!(matches!(
                cookie_refresh_info(&c).await,
                Err(BiliError::NotLoggedIn)
            ));
            assert!(matches!(
                cookie_refresh(&c, "a", "b").await,
                Err(BiliError::NotLoggedIn)
            ));
            assert!(matches!(
                refresh_cookie(&c, Some("token")).await,
                Err(RefreshError::NotLoggedIn)
            ));
            let _ = mk();
        });
    }

    #[test]
    fn refresh_error_classification() {
        assert!(RefreshError::NotLoggedIn.needs_relogin());
        assert!(RefreshError::MissingRefreshToken.needs_relogin());
        assert!(!RefreshError::Info(BiliError::Network("x".into())).needs_relogin());
        assert!(RefreshError::Info(BiliError::NotLoggedIn).needs_relogin());
        assert!(!RefreshError::CorrespondPath("bad key".into()).needs_relogin());
        assert!(
            !RefreshError::CorrespondPage(BiliError::Permission {
                message: "404".into()
            })
            .needs_relogin()
        );
        assert!(
            RefreshError::RefreshRejected {
                code: 86095,
                message: "mismatch".into()
            }
            .needs_relogin()
        );
        assert!(
            RefreshError::RefreshRejected {
                code: -111,
                message: "csrf".into()
            }
            .needs_relogin()
        );
        assert!(
            !RefreshError::RefreshRejected {
                code: -352,
                message: "risk".into()
            }
            .needs_relogin()
        );
        assert!(!RefreshError::Refresh(BiliError::Timeout("t".into())).needs_relogin());
        // 可序列化（壳层可映射为用户提示）
        let json = serde_json::to_value(RefreshError::MissingRefreshToken).unwrap();
        assert_eq!(json, serde_json::json!("MissingRefreshToken"));
        let back: RefreshError = serde_json::from_value(json).unwrap();
        assert_eq!(back, RefreshError::MissingRefreshToken);
    }

    // ---- wiremock HTTP 状态机（不发起真实网络请求）----

    use wiremock::matchers::{body_string_contains, method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(uri: &str, cookies: StoredCookies) -> BiliClient {
        let endpoints = super::super::client::Endpoints {
            api: uri.to_string(),
            passport: uri.to_string(),
            www: uri.to_string(),
        };
        BiliClient::with_endpoints(super::super::client::plain_client(), cookies, endpoints)
    }

    fn logged_in_cookies() -> StoredCookies {
        StoredCookies::from_pairs(vec![
            ("SESSDATA".to_string(), "old-sess".to_string()),
            ("bili_jct".to_string(), "old-csrf".to_string()),
        ])
    }

    async fn mount_info(server: &MockServer, refresh: bool) {
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/cookie/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "message": "0",
                "data": {"refresh": refresh, "timestamp": 1_700_000_000_000_i64}
            })))
            .mount(server)
            .await;
    }

    async fn mount_correspond(server: &MockServer, html: &str) {
        Mock::given(method("GET"))
            .and(path_regex(r"^/correspond/1/[0-9a-f]+$"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html.to_string()))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn refresh_not_needed_when_info_says_valid() {
        let server = MockServer::start().await;
        mount_info(&server, false).await;
        let client = test_client(&server.uri(), logged_in_cookies());
        let out = refresh_cookie(&client, Some("old-token")).await.unwrap();
        assert_eq!(out, RefreshOutcome::NotNeeded);
        // 只请求过 cookie/info
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn refresh_full_flow_updates_cookies_and_token() {
        let server = MockServer::start().await;
        mount_info(&server, true).await;
        mount_correspond(
            &server,
            "<body><div id=\"1-name\">REFRESH-CSRF</div><div id=\"token-iframe-app\"></div></body>",
        )
        .await;
        // 第一步：refresh 用旧 csrf / refresh_csrf / refresh_token / source=main_web
        Mock::given(method("POST"))
            .and(path("/x/passport-login/web/cookie/refresh"))
            .and(body_string_contains("refresh_csrf=REFRESH-CSRF"))
            .and(body_string_contains("refresh_token=old-token"))
            .and(body_string_contains("source=main_web"))
            .and(body_string_contains("csrf=old-csrf"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("set-cookie", "SESSDATA=new-sess; Path=/; HttpOnly")
                    .append_header("set-cookie", "bili_jct=new-csrf; Path=/")
                    .set_body_json(serde_json::json!({
                        "code": 0,
                        "message": "0",
                        "data": {"status": 0, "message": "", "refresh_token": "new-token"}
                    })),
            )
            .mount(&server)
            .await;
        // 第二步：confirm 用新 csrf + 旧 refresh_token
        Mock::given(method("POST"))
            .and(path("/x/passport-login/web/cookie/confirm/refresh"))
            .and(body_string_contains("csrf=new-csrf"))
            .and(body_string_contains("refresh_token=old-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code": 0, "message": "0"})),
            )
            .mount(&server)
            .await;

        let client = test_client(&server.uri(), logged_in_cookies());
        let out = refresh_cookie(&client, Some("old-token")).await.unwrap();
        assert_eq!(
            out,
            RefreshOutcome::Refreshed {
                refresh_token: "new-token".into()
            }
        );
        // 客户端 cookie 已原地更新
        assert_eq!(client.cookies().get("SESSDATA"), Some("new-sess"));
        assert_eq!(client.csrf().as_deref(), Some("new-csrf"));
    }

    #[tokio::test]
    async fn refresh_missing_token_does_not_hit_correspond() {
        let server = MockServer::start().await;
        mount_info(&server, true).await;
        let client = test_client(&server.uri(), logged_in_cookies());
        let err = refresh_cookie(&client, None).await.unwrap_err();
        assert_eq!(err, RefreshError::MissingRefreshToken);
        assert!(err.needs_relogin());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests.len(),
            1,
            "缺 refresh_token 时不应请求 correspond 页面"
        );
    }

    #[tokio::test]
    async fn refresh_rejected_maps_to_relogin() {
        let server = MockServer::start().await;
        mount_info(&server, true).await;
        mount_correspond(&server, "<div id=\"1-name\">CSRF</div>").await;
        Mock::given(method("POST"))
            .and(path("/x/passport-login/web/cookie/refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 86095,
                "message": "refresh_csrf error",
                "ttl": 1
            })))
            .mount(&server)
            .await;
        let client = test_client(&server.uri(), logged_in_cookies());
        let err = refresh_cookie(&client, Some("old-token"))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            RefreshError::RefreshRejected {
                code: 86095,
                message: "refresh_csrf error".into()
            }
        );
        assert!(err.needs_relogin());
        assert_eq!(err.step(), "refresh_rejected");
    }

    #[tokio::test]
    async fn refresh_bad_correspond_page_is_recoverable() {
        let server = MockServer::start().await;
        mount_info(&server, true).await;
        mount_correspond(&server, "<html><body>404 Not Found</body></html>").await;
        let client = test_client(&server.uri(), logged_in_cookies());
        let err = refresh_cookie(&client, Some("old-token"))
            .await
            .unwrap_err();
        assert_eq!(err.step(), "correspond_page");
        assert!(!err.needs_relogin(), "页面过期/结构变化不应直接判死登录态");
    }

    #[tokio::test]
    async fn refresh_info_server_error_is_transient() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/cookie/info"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let client = test_client(&server.uri(), logged_in_cookies());
        let err = refresh_cookie(&client, Some("old-token"))
            .await
            .unwrap_err();
        assert_eq!(err.step(), "cookie_info");
        assert!(!err.needs_relogin(), "5xx 属于暂时性失败");
    }

    #[tokio::test]
    async fn qr_poll_returns_refresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/qrcode/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "message": "0",
                "data": {
                    "url": "https://passport.bilibili.com/h5-app/passport/login/scan?k=1",
                    "qrcode_key": "KEY"
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/qrcode/poll"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("set-cookie", "SESSDATA=s; Path=/")
                    .append_header("set-cookie", "bili_jct=c; Path=/")
                    .append_header("set-cookie", "DedeUserID=42; Path=/")
                    .set_body_json(serde_json::json!({
                        "code": 0,
                        "message": "0",
                        "data": {
                            "code": 0,
                            "message": "",
                            "refresh_token": "RT-123",
                            "url": "https://x"
                        }
                    })),
            )
            .mount(&server)
            .await;

        let client = test_client(&server.uri(), StoredCookies::default());
        let mut flow = QrLogin::start(&client).await.unwrap();
        match flow.poll(&client).await.unwrap() {
            LoginPoll::Success(success) => {
                assert_eq!(success.cookies.get("SESSDATA"), Some("s"));
                assert_eq!(success.refresh_token.as_deref(), Some("RT-123"));
            }
            other => panic!("unexpected poll: {other:?}"),
        }
    }

    #[test]
    fn poll_code_new_and_legacy() {
        use super::{LoginState, apply_poll_code};

        let mut s = LoginState::Idle;
        apply_poll_code(&mut s, 86101, "未扫码");
        assert_eq!(s, LoginState::Waiting);
        apply_poll_code(&mut s, 861, "未扫码");
        assert_eq!(s, LoginState::Waiting);
        apply_poll_code(&mut s, 86090, "已扫码");
        assert_eq!(s, LoginState::Scanned);
        apply_poll_code(&mut s, 860, "已扫码");
        assert_eq!(s, LoginState::Scanned);
        apply_poll_code(&mut s, 86038, "二维码已失效");
        assert_eq!(s, LoginState::Expired);
        apply_poll_code(&mut s, 864, "二维码已失效");
        assert_eq!(s, LoginState::Expired);
        apply_poll_code(&mut s, 0, "");
        assert_eq!(s, LoginState::Success);
    }
}
