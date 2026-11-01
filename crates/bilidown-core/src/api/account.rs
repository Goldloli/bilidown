//! 账号：登录态恢复/校验、登录落盘、cookie 自动续期、退出登录（design D4/D7）。
//!
//! 账号状态以 [`AccountState`] 结构化输出；登录成功后凭据原子写入应用数据目录
//! （0600），此后所有请求复用同一个 [`BiliClient`]（cookie 原地替换，无需重建）。
//! 启动与「账号刷新」都调用 [`restore`]，内部按需执行一次 cookie 自动续期
//! （[`refresh_session`]）；续期失败不阻塞下载主流程，只影响账号状态提示。

use serde::{Deserialize, Serialize};

use super::client::BiliClient;
use super::error::{BiliError, BiliResult};
use super::login::{self, LoginPoll, LoginState, QrLogin, RefreshError, RefreshOutcome};
use super::types::UserInfo;
use crate::storage::{Credentials, Storage};

/// 账号状态（序列化给前端；不含任何 cookie 值）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountState {
    pub logged_in: bool,
    pub user: UserInfo,
    /// 凭据失效，需要重新扫码
    pub expired: bool,
    /// cookie 已接近过期、续期失败/未执行（前端可提示「重新登录以延长有效期」）
    pub cookie_refresh_required: bool,
    pub last_error: Option<BiliError>,
}

impl AccountState {
    pub fn anonymous() -> Self {
        Self::default()
    }
}

/// 登录轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub enum LoginOutcome {
    Pending(LoginState),
    Success(AccountState),
    Ended(LoginState),
}

/// 会话自动续期结果（壳层据此决定是否提示「请重新扫码」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SessionRefresh {
    /// cookie 仍有效，无需续期
    NotNeeded,
    /// 续期成功，cookie + refresh_token 已 0600 原子写回
    Refreshed,
    /// 无法续期（refresh_token 缺失/被拒），应引导重新扫码
    ReloginRequired { reason: RefreshError },
    /// 暂时性失败（网络/风控/服务端异常），保持登录态，下次启动可再试
    Failed { reason: RefreshError },
}

impl SessionRefresh {
    /// 是否需要引导用户重新扫码。
    pub fn needs_relogin(&self) -> bool {
        matches!(self, SessionRefresh::ReloginRequired { .. })
    }
}

/// 应用持久化的凭据到 client（启动时调用）。
///
/// 用合并而不是整体替换：壳启动顺序是 `activate_buvid()` → `restore()`，
/// 合并可保留启动时刚获取的匿名风控 cookie（bili_ticket/buvid）。
pub fn apply_credentials(client: &BiliClient, storage: &Storage) {
    let creds = storage.load_credentials();
    client.merge_cookies(&creds.stored());
    client.apply_device(&creds.device);
}

/// 把 client 当前基础凭据（buvid/_uuid/bili_ticket）持久化。
///
/// 匿名用户也要落盘风控 cookie（`bili_ticket` 等），下次启动可免重复申请；
/// 登录用户的登录 cookie 由 [`restore`] 单独同步，这里只补充设备/风控项。
pub async fn persist_device(client: &BiliClient, storage: &Storage) -> BiliResult<()> {
    let mut creds = storage.load_credentials();
    let device = client.device();
    let filtered = Credentials::filtered(&client.cookies().cookies, device.clone());
    if device.is_empty() && filtered.cookies.is_empty() {
        return Ok(());
    }
    creds.device = device;
    if !filtered.cookies.is_empty() {
        creds.cookies = filtered.cookies;
    }
    storage.save_credentials(&creds)
}

/// 启动恢复：应用凭据 → nav 校验 → 按需自动续期一次 → 返回账号状态。
pub async fn restore(client: &BiliClient, storage: &Storage) -> AccountState {
    apply_credentials(client, storage);
    let mut state = AccountState::anonymous();
    if !client.is_logged_in() {
        return state;
    }
    match client.nav_user_info().await {
        Ok(user) if user.is_login => {
            state.logged_in = true;
            state.user = user;
            // nav 可能刷新 cookie；顺带落盘，并检测/执行一次自动续期
            persist_login_cookies(client, storage);
            match refresh_session(client, storage).await {
                SessionRefresh::NotNeeded | SessionRefresh::Refreshed => {}
                SessionRefresh::ReloginRequired { reason } => {
                    state.cookie_refresh_required = true;
                    state.expired = true;
                    tracing::info!(step = reason.step(), "cookie refresh requires relogin");
                }
                SessionRefresh::Failed { reason } => {
                    state.cookie_refresh_required = true;
                    tracing::warn!(step = reason.step(), "cookie refresh deferred");
                }
            }
        }
        Ok(_) => {
            state.expired = true;
            tracing::info!("stored credentials expired");
        }
        Err(e) => {
            // 网络故障不能判定为登录失效：保持已登录展示，提示错误
            state.logged_in = true;
            state.last_error = Some(e);
        }
    }
    state
}

/// 轮询扫码登录；成功时持久化凭据（含 refresh_token）并补齐风控凭据。
pub async fn login_poll(
    client: &BiliClient,
    storage: &Storage,
    flow: &mut QrLogin,
) -> BiliResult<LoginOutcome> {
    match flow.poll(client).await? {
        LoginPoll::Pending(state) => Ok(LoginOutcome::Pending(state)),
        LoginPoll::Ended(state) => Ok(LoginOutcome::Ended(state)),
        LoginPoll::Success(success) => {
            // 凭据落盘：登录 cookie + 设备凭据 + refresh_token
            let mut creds = Credentials::filtered(&success.cookies.cookies, client.device());
            let device = client.device();
            creds.device = if device.is_empty() {
                creds.device_from_cookies()
            } else {
                device
            };
            creds.refresh_token = success.refresh_token;
            storage.save_credentials(&creds)?;
            // 登录后补齐 bili_ticket/_uuid/buvid（失败仅日志，不阻塞登录）
            client.activate_buvid().await;
            persist_login_cookies(client, storage);
            // nav 校验（同时刷新昵称/头像/大会员）
            let state = match client.nav_user_info().await {
                Ok(user) => AccountState {
                    logged_in: user.is_login,
                    user,
                    ..Default::default()
                },
                Err(e) => AccountState {
                    logged_in: true,
                    last_error: Some(e),
                    ..Default::default()
                },
            };
            tracing::info!(logged_in = state.logged_in, "login persisted");
            Ok(LoginOutcome::Success(state))
        }
    }
}

/// 退出登录：清 cookie + 删凭据文件（已下载文件与历史不受影响）。
pub fn logout(client: &BiliClient, storage: &Storage) -> BiliResult<()> {
    client.clear_cookies();
    storage.clear_credentials()?;
    Ok(())
}

/// 检测并在需要时自动续期一次（应用启动 / 账号刷新时调用）。
///
/// - 未登录 → [`SessionRefresh::NotNeeded`]，不发任何网络请求；
/// - 续期成功 → 更新内存 cookie，并把新 cookie + 新 refresh_token 以 0600 原子写回；
/// - 失败按 [`RefreshError::needs_relogin`] 区分「重新扫码」与「暂时性失败」。
pub async fn refresh_session(client: &BiliClient, storage: &Storage) -> SessionRefresh {
    if !client.is_logged_in() {
        return SessionRefresh::NotNeeded;
    }
    let stored_token = storage.load_credentials().refresh_token;
    match login::refresh_cookie(client, stored_token.as_deref()).await {
        Ok(RefreshOutcome::NotNeeded) => SessionRefresh::NotNeeded,
        Ok(RefreshOutcome::Refreshed { refresh_token }) => {
            persist_refreshed(client, storage, &refresh_token);
            tracing::info!("cookie auto refresh succeeded");
            SessionRefresh::Refreshed
        }
        Err(reason) => {
            let needs_relogin = reason.needs_relogin();
            tracing::warn!(
                step = reason.step(),
                needs_relogin,
                "cookie auto refresh failed"
            );
            if needs_relogin {
                SessionRefresh::ReloginRequired { reason }
            } else {
                SessionRefresh::Failed { reason }
            }
        }
    }
}

/// 续期成功：新 cookie + 新 refresh_token 以 0600 原子写回（保留设备凭据）。
fn persist_refreshed(client: &BiliClient, storage: &Storage, refresh_token: &str) {
    let mut creds = storage.load_credentials();
    let filtered = Credentials::filtered(&client.cookies().cookies, client.device());
    creds.cookies = filtered.cookies;
    let device = client.device();
    if !device.is_empty() {
        creds.device = device;
    }
    creds.refresh_token = Some(refresh_token.to_string());
    if let Err(e) = storage.save_credentials(&creds) {
        tracing::warn!(error = %e, "persist refreshed credentials failed");
    }
}

/// 把 client 中登录相关 cookie 同步回凭据文件（保留设备凭据与 refresh_token）。
fn persist_login_cookies(client: &BiliClient, storage: &Storage) {
    let mut creds = storage.load_credentials();
    let filtered = Credentials::filtered(&client.cookies().cookies, client.device());
    creds.cookies = filtered.cookies;
    let device = client.device();
    if !device.is_empty() {
        creds.device = device;
    }
    if let Err(e) = storage.save_credentials(&creds) {
        tracing::warn!(error = %e, "persist login cookies failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::{Endpoints, StoredCookies, plain_client};
    use crate::storage::AppPaths;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn setup() -> (tempfile::TempDir, Storage, BiliClient) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Storage::open_with_legacy(paths, None).unwrap();
        let client = BiliClient::new(None, StoredCookies::default()).unwrap();
        (dir, storage, client)
    }

    fn setup_with_server(server: &MockServer) -> (tempfile::TempDir, Storage, BiliClient) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Storage::open_with_legacy(paths, None).unwrap();
        let endpoints = Endpoints {
            api: server.uri(),
            passport: server.uri(),
            www: server.uri(),
        };
        let client =
            BiliClient::with_endpoints(plain_client(), StoredCookies::default(), endpoints);
        (dir, storage, client)
    }

    fn logged_in_creds(refresh_token: Option<&str>) -> Credentials {
        let mut creds = Credentials::from_pairs(
            vec![
                ("SESSDATA".to_string(), "old-sess".to_string()),
                ("bili_jct".to_string(), "old-csrf".to_string()),
            ],
            Default::default(),
        );
        creds.refresh_token = refresh_token.map(str::to_string);
        creds
    }

    async fn mount_nav(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/x/web-interface/nav"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "message": "0",
                "data": {
                    "isLogin": true,
                    "mid": 1,
                    "uname": "tester",
                    "face": "https://i0.hdslb.com/bfs/face/x.jpg",
                    "vipStatus": 0,
                    "vipType": 0,
                    "level": 6,
                    "wbi_img": {"img_url": "https://i0.hdslb.com/bfs/wbi/abc.png", "sub_url": "https://i0.hdslb.com/bfs/wbi/def.png"}
                }
            })))
            .mount(server)
            .await;
    }

    async fn mount_refresh_flow(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/cookie/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "message": "0",
                "data": {"refresh": true, "timestamp": 1_700_000_000_000_i64}
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::path_regex(r"^/correspond/1/[0-9a-f]+$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("<div id=\"1-name\">CSRF-X</div>"),
            )
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/x/passport-login/web/cookie/refresh"))
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
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/x/passport-login/web/cookie/confirm/refresh"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code": 0, "message": "0"})),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn anonymous_restore_does_not_hit_network() {
        let (_dir, storage, client) = setup();
        let state = restore(&client, &storage).await;
        assert!(!state.logged_in);
        assert!(!state.expired);
        assert!(state.user.uname.is_empty());
    }

    #[tokio::test]
    async fn persist_device_roundtrip() {
        let (_dir, storage, client) = setup();
        client.ensure_uuid();
        persist_device(&client, &storage).await.unwrap();
        let creds = storage.load_credentials();
        assert_eq!(
            creds.device.uuid.as_deref(),
            client.device().uuid.as_deref()
        );
        assert!(!creds.has_login());
    }

    #[tokio::test]
    async fn persist_device_saves_anonymous_risk_cookies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x/frontend/finger/spi"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"b_3": "B3", "b_4": "B4"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"ticket": "TK", "created_at": 1_700_000_000_i64, "ttl": 259_200_i64}
            })))
            .mount(&server)
            .await;

        let (_dir, storage, client) = setup_with_server(&server);
        client.activate_buvid().await;
        persist_device(&client, &storage).await.unwrap();
        let creds = storage.load_credentials();
        assert!(creds.device.uuid.is_some(), "_uuid 应持久化");
        assert_eq!(creds.device.buvid3.as_deref(), Some("B3"));
        assert_eq!(creds.stored().get("bili_ticket"), Some("TK"));
        assert!(!creds.has_login(), "匿名启动不应产生登录态");
    }

    #[test]
    fn apply_credentials_keeps_in_memory_risk_cookies() {
        let (_dir, storage, client) = setup();
        // 模拟壳启动顺序：先 activate_buvid 拿到 ticket，再 restore 应用文件凭据
        client.set_cookies(StoredCookies::from_pairs(vec![(
            "bili_ticket".to_string(),
            "IN-MEM".to_string(),
        )]));
        apply_credentials(&client, &storage);
        assert_eq!(
            client.cookies().get("bili_ticket"),
            Some("IN-MEM"),
            "restore 不应丢弃启动时刚获取的风控 cookie"
        );
    }

    #[test]
    fn logout_clears_cookie_and_file() {
        let (_dir, storage, client) = setup();
        let creds = Credentials::from_pairs(
            vec![("SESSDATA".to_string(), "s".to_string())],
            Default::default(),
        );
        storage.save_credentials(&creds).unwrap();
        apply_credentials(&client, &storage);
        assert!(client.is_logged_in());
        logout(&client, &storage).unwrap();
        assert!(!client.is_logged_in());
        assert!(storage.load_credentials().is_empty());
    }

    #[tokio::test]
    async fn refresh_session_not_logged_in_is_not_needed() {
        let (_dir, storage, client) = setup();
        let out = refresh_session(&client, &storage).await;
        assert_eq!(out, SessionRefresh::NotNeeded);
        assert!(!out.needs_relogin());
    }

    #[tokio::test]
    async fn restore_logged_in_triggers_refresh_and_persists() {
        let server = MockServer::start().await;
        mount_nav(&server).await;
        mount_refresh_flow(&server).await;

        let (_dir, storage, client) = setup_with_server(&server);
        storage
            .save_credentials(&logged_in_creds(Some("old-token")))
            .unwrap();

        let state = restore(&client, &storage).await;
        assert!(state.logged_in);
        assert!(!state.expired, "续期成功不应标记失效");
        assert!(!state.cookie_refresh_required);
        assert_eq!(state.user.uname, "tester");
        // 内存 cookie 已更新
        assert_eq!(client.cookies().get("SESSDATA"), Some("new-sess"));
        // 新 refresh_token 与 cookie 已 0600 原子写回
        let creds = storage.load_credentials();
        assert_eq!(creds.refresh_token.as_deref(), Some("new-token"));
        assert_eq!(creds.stored().get("SESSDATA"), Some("new-sess"));
        assert_eq!(creds.stored().get("bili_jct"), Some("new-csrf"));
        #[cfg(unix)]
        assert_eq!(
            crate::storage::fsutil::file_mode(&storage.paths().credentials_file()),
            Some(0o600)
        );
    }

    #[tokio::test]
    async fn restore_without_refresh_token_requires_relogin() {
        let server = MockServer::start().await;
        mount_nav(&server).await;
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/cookie/info"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "message": "0",
                "data": {"refresh": true, "timestamp": 1_700_000_000_000_i64}
            })))
            .mount(&server)
            .await;

        let (_dir, storage, client) = setup_with_server(&server);
        storage.save_credentials(&logged_in_creds(None)).unwrap();

        let state = restore(&client, &storage).await;
        assert!(state.logged_in);
        assert!(state.expired, "refresh_token 缺失应引导重新扫码");
        assert!(state.cookie_refresh_required);
        // 未产生新 token
        assert!(storage.load_credentials().refresh_token.is_none());
    }

    #[tokio::test]
    async fn restore_transient_refresh_failure_keeps_login() {
        let server = MockServer::start().await;
        mount_nav(&server).await;
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/cookie/info"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let (_dir, storage, client) = setup_with_server(&server);
        storage
            .save_credentials(&logged_in_creds(Some("old-token")))
            .unwrap();

        let state = restore(&client, &storage).await;
        assert!(state.logged_in);
        assert!(!state.expired, "网络失败不应判死登录态");
        assert!(state.cookie_refresh_required);
        assert_eq!(
            storage.load_credentials().refresh_token.as_deref(),
            Some("old-token")
        );
    }

    #[tokio::test]
    async fn login_poll_persists_refresh_token_and_risk_cookies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x/passport-login/web/qrcode/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"url": "https://passport.bilibili.com/h5-app/passport/login/scan?k=1", "qrcode_key": "KEY"}
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
        // 登录成功后初始化的风控凭据
        Mock::given(method("GET"))
            .and(path("/x/frontend/finger/spi"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"b_3": "B3", "b_4": "B4"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": 0,
                "data": {"ticket": "TK", "created_at": 1_700_000_000_i64, "ttl": 259_200_i64}
            })))
            .mount(&server)
            .await;
        mount_nav(&server).await;

        let (_dir, storage, client) = setup_with_server(&server);
        let mut flow = QrLogin::start(&client).await.unwrap();
        let outcome = login_poll(&client, &storage, &mut flow).await.unwrap();
        match outcome {
            LoginOutcome::Success(state) => assert!(state.logged_in),
            other => panic!("unexpected login outcome: {other:?}"),
        }
        let creds = storage.load_credentials();
        assert_eq!(creds.refresh_token.as_deref(), Some("RT-123"));
        assert_eq!(creds.stored().get("SESSDATA"), Some("s"));
        // 登录后补齐的 bili_ticket 与 _uuid 一并持久化
        assert_eq!(creds.stored().get("bili_ticket"), Some("TK"));
        assert!(creds.device.uuid.is_some());
        assert_eq!(creds.device.buvid3.as_deref(), Some("B3"));
    }

    #[test]
    fn session_refresh_serde() {
        let out = SessionRefresh::ReloginRequired {
            reason: RefreshError::MissingRefreshToken,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["status"], "relogin_required");
        let back: SessionRefresh = serde_json::from_value(json).unwrap();
        assert_eq!(back, out);
        assert!(out.needs_relogin());
        assert!(!SessionRefresh::Refreshed.needs_relogin());
    }
}
