//! HTTP 客户端：统一 UA/Referer/cookie、超时、代理、WBI key 缓存、API 错误归一。
//!
//! cookie 由本模块**自己维护**（`CookieStore`）而不是 reqwest jar，原因：
//! - 登录成功后需要导出/持久化/刷新 cookie（reqwest jar 不可枚举）；
//! - 凭据失效/重登时可在不重建 client 的情况下原地替换 cookie。
//!
//! 约束：cookie 明文只存在于内存与 0600 凭据文件；日志/错误只记录 cookie 名，
//! 不记录值（见 `tracing` 调用点）。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use reqwest::{Client, Proxy, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{BiliError, BiliResult};
use super::types::UserInfo;

pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const REFERER: &str = "https://www.bilibili.com";

/// API 请求连接超时。
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// API 请求读超时（同一次 socket 读）。
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// API 请求总超时（防御挂起）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// 下载连接超时。
pub const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 下载读超时（DASH 流允许较慢）。
pub const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);
/// WBI key 缓存时长（B 站每日轮换，10 分钟足够）。
const WBI_CACHE_TTL: Duration = Duration::from_secs(600);

/// `bili_ticket` 的 HMAC-SHA256 签名 key（B 站 Web 端固定值）。
pub const BILI_TICKET_HMAC_KEY: &str = "XgwSnGZ1p";
/// `bili_ticket` 距离过期不足该时长时重新申请（ticket 有效期约 3 天）。
const TICKET_REFRESH_MARGIN_SECS: i64 = 6 * 60 * 60;

/// B 站各域名基址（测试可注入 mock server；生产使用默认值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// `api.bilibili.com`：业务接口
    pub api: String,
    /// `passport.bilibili.com`：登录/续期接口
    pub passport: String,
    /// `www.bilibili.com`：网页/页面接口（correspond 续期页等）
    pub www: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            api: "https://api.bilibili.com".to_string(),
            passport: "https://passport.bilibili.com".to_string(),
            www: "https://www.bilibili.com".to_string(),
        }
    }
}

impl Endpoints {
    pub fn api_url(&self, path: &str) -> String {
        join_url(&self.api, path)
    }

    pub fn passport_url(&self, path: &str) -> String {
        join_url(&self.passport, path)
    }

    pub fn www_url(&self, path: &str) -> String {
        join_url(&self.www, path)
    }
}

fn join_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

/// 小写 hex 编码（B 站 correspondPath 与 bili_ticket hexsign 均为小写 Base16）。
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// 计算 `bili_ticket` 的 `hexsign`：`HMAC-SHA256(key = "XgwSnGZ1p", msg = "ts{timestamp}")`，
/// 输出小写 hex。纯函数，便于单测。
pub fn ticket_hexsign(timestamp_secs: i64) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(BILI_TICKET_HMAC_KEY.as_bytes())
        .expect("HMAC-SHA256 接受任意长度 key");
    mac.update(format!("ts{timestamp_secs}").as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

/// 持久化的登录凭据（credentials.json 的 cookies 部分）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredCookies {
    #[serde(default)]
    pub cookies: Vec<(String, String)>,
}

impl StoredCookies {
    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            cookies: pairs.into_iter().collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    pub fn has_login(&self) -> bool {
        self.get("SESSDATA").is_some_and(|v| !v.is_empty())
    }
}

/// 基础风控凭据（buvid/_uuid 等），由 core 维护并持久化。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceCredentials {
    #[serde(default)]
    pub buvid3: Option<String>,
    #[serde(default)]
    pub buvid4: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub b_nut: Option<String>,
}

impl DeviceCredentials {
    pub fn is_empty(&self) -> bool {
        self.buvid3.is_none() && self.buvid4.is_none() && self.uuid.is_none()
    }
}

/// 内存 cookie 存储（name → value）。
#[derive(Debug, Clone, Default)]
pub struct CookieStore {
    cookies: BTreeMap<String, String>,
}

impl CookieStore {
    pub fn from_stored(stored: &StoredCookies) -> Self {
        let mut cookies = BTreeMap::new();
        for (k, v) in &stored.cookies {
            if !k.is_empty() && !v.is_empty() {
                cookies.insert(k.clone(), v.clone());
            }
        }
        Self { cookies }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        if value.is_empty() {
            self.cookies.remove(&name);
        } else {
            self.cookies.insert(name, value);
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.cookies.remove(name);
    }

    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    pub fn is_logged_in(&self) -> bool {
        self.get("SESSDATA").is_some_and(|v| !v.is_empty())
    }

    pub fn to_stored(&self) -> StoredCookies {
        StoredCookies {
            cookies: self
                .cookies
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// 生成请求 Cookie 头（按名字排序，便于稳定复现）。
    pub fn header_value(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        Some(
            self.cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    /// 吸收响应头中的 Set-Cookie（只取 name=value，忽略属性）。
    pub fn absorb_set_cookie(&mut self, headers: &reqwest::header::HeaderMap) {
        for value in headers.get_all(reqwest::header::SET_COOKIE) {
            let Ok(s) = value.to_str() else { continue };
            let pair = s.split(';').next().unwrap_or_default();
            if let Some((name, val)) = pair.split_once('=') {
                let name = name.trim();
                let val = val.trim();
                if !name.is_empty() {
                    self.insert(name, val);
                }
            }
        }
    }
}

/// 缓存的 WBI 原始 key。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WbiKeys {
    pub img_key: String,
    pub sub_key: String,
}

struct CachedWbi {
    keys: WbiKeys,
    fetched_at: Instant,
}

/// 应用级 HTTP 客户端：所有 B 站请求都经过它。
pub struct BiliClient {
    http: Client,
    proxy: Option<String>,
    cookies: RwLock<CookieStore>,
    endpoints: Endpoints,
    wbi: tokio::sync::Mutex<Option<CachedWbi>>,
}

impl std::fmt::Debug for BiliClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BiliClient")
            .field("proxy", &self.proxy)
            .field("endpoints", &self.endpoints)
            .field("cookie_names", &self.cookie_names())
            .finish()
    }
}

impl BiliClient {
    /// 构建客户端；`proxy` 为可选 HTTP(S) 代理地址（显式设置才启用）。
    pub fn new(proxy: Option<&str>, cookies: StoredCookies) -> BiliResult<Self> {
        let http = build_http_client(proxy)?;
        Ok(Self {
            http,
            proxy: proxy.map(str::to_string),
            cookies: RwLock::new(CookieStore::from_stored(&cookies)),
            endpoints: Endpoints::default(),
            wbi: tokio::sync::Mutex::new(None),
        })
    }

    /// 用外部构造的 reqwest client 构建（测试/嵌入场景）。
    pub fn with_http(http: Client, cookies: StoredCookies) -> Self {
        Self::with_endpoints(http, cookies, Endpoints::default())
    }

    /// 用外部构造的 reqwest client + 自定义域名基址构建（单测 wiremock 定点用）。
    pub fn with_endpoints(http: Client, cookies: StoredCookies, endpoints: Endpoints) -> Self {
        Self {
            http,
            proxy: None,
            cookies: RwLock::new(CookieStore::from_stored(&cookies)),
            endpoints,
            wbi: tokio::sync::Mutex::new(None),
        }
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    pub fn proxy(&self) -> Option<&str> {
        self.proxy.as_deref()
    }

    pub fn cookies(&self) -> StoredCookies {
        self.cookies.read().expect("cookie lock").to_stored()
    }

    pub fn cookie_names(&self) -> Vec<String> {
        self.cookies
            .read()
            .expect("cookie lock")
            .to_stored()
            .cookies
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    }

    pub fn set_cookies(&self, stored: StoredCookies) {
        *self.cookies.write().expect("cookie lock") = CookieStore::from_stored(&stored);
    }

    /// 合并持久化 cookie：同名覆盖，但**不清除**内存中已获取的匿名风控 cookie
    /// （启动顺序为先 `activate_buvid` 后 `account::restore`，见 API.md）。
    pub fn merge_cookies(&self, stored: &StoredCookies) {
        let mut store = self.cookies.write().expect("cookie lock");
        for (name, value) in &stored.cookies {
            if !name.is_empty() && !value.is_empty() {
                store.insert(name.clone(), value.clone());
            }
        }
    }

    pub fn clear_cookies(&self) {
        self.cookies.write().expect("cookie lock").clear();
    }

    pub fn cookie_header(&self) -> Option<String> {
        self.cookies.read().expect("cookie lock").header_value()
    }

    pub fn is_logged_in(&self) -> bool {
        self.cookies.read().expect("cookie lock").is_logged_in()
    }

    pub fn csrf(&self) -> Option<String> {
        self.cookies
            .read()
            .expect("cookie lock")
            .get("bili_jct")
            .map(str::to_string)
    }

    pub fn buvid3(&self) -> Option<String> {
        self.cookies
            .read()
            .expect("cookie lock")
            .get("buvid3")
            .map(str::to_string)
    }

    /// 应用持久化的基础凭据。
    pub fn apply_device(&self, device: &DeviceCredentials) {
        let mut store = self.cookies.write().expect("cookie lock");
        if let Some(v) = &device.buvid3 {
            store.insert("buvid3", v.clone());
        }
        if let Some(v) = &device.buvid4 {
            store.insert("buvid4", v.clone());
        }
        if let Some(v) = &device.uuid {
            store.insert("_uuid", v.clone());
        }
        if let Some(v) = &device.b_nut {
            store.insert("b_nut", v.clone());
        }
    }

    /// 从 cookie 中提取基础凭据（持久化用）。
    pub fn device(&self) -> DeviceCredentials {
        let store = self.cookies.read().expect("cookie lock");
        DeviceCredentials {
            buvid3: store.get("buvid3").map(str::to_string),
            buvid4: store.get("buvid4").map(str::to_string),
            uuid: store.get("_uuid").map(str::to_string),
            b_nut: store.get("b_nut").map(str::to_string),
        }
    }

    /// 生成并写入 `_uuid`（B 站 web 端格式：大写 UUID + `infoc` 后缀）。
    pub fn ensure_uuid(&self) -> String {
        let mut store = self.cookies.write().expect("cookie lock");
        if let Some(v) = store.get("_uuid").filter(|v| !v.is_empty()) {
            return v.to_string();
        }
        let uuid = generate_uuid();
        store.insert("_uuid", uuid.clone());
        uuid
    }

    /// 初始化匿名环境凭据：`_uuid`、`buvid3`/`buvid4`、`bili_ticket`。
    ///
    /// - `buvid3` 已存在时不重复请求（登录用户沿用登录 cookie 里的值）；
    /// - `bili_ticket` 存在且未临近过期时跳过；失败只记日志，不阻塞启动/登录。
    pub async fn activate_buvid(&self) {
        self.ensure_uuid();
        if self.buvid3().is_none() {
            self.fetch_buvid().await;
        }
        self.activate_ticket().await;
    }

    /// GET `/x/frontend/finger/spi` 取 buvid3/buvid4（失败只记日志）。
    async fn fetch_buvid(&self) {
        #[derive(Deserialize)]
        struct Spi {
            code: i64,
            data: Option<SpiData>,
        }
        #[derive(Deserialize)]
        struct SpiData {
            #[serde(rename = "b_3")]
            b3: String,
            #[serde(rename = "b_4")]
            b4: String,
        }
        let url = self.endpoints.api_url("/x/frontend/finger/spi");
        match self
            .send(self.http.get(&url).header("Referer", REFERER))
            .await
        {
            Ok(resp) => match resp.json::<Spi>().await {
                Ok(spi) if spi.code == 0 => {
                    if let Some(d) = spi.data {
                        let now = unix_now();
                        let mut store = self.cookies.write().expect("cookie lock");
                        store.insert("buvid3", d.b3);
                        store.insert("buvid4", d.b4);
                        store.insert("b_nut", now.to_string());
                    }
                }
                Ok(spi) => tracing::warn!(code = spi.code, "buvid spi returned non-zero"),
                Err(e) => tracing::warn!(error = %e, "buvid spi parse failed"),
            },
            Err(e) => tracing::warn!(error = %e, "buvid spi request failed"),
        }
    }

    /// 申请并写入 `bili_ticket` / `bili_ticket_expires`（HMAC-SHA256 签名，失败只记日志）。
    ///
    /// 请求：`POST /bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket`
    /// `?key_id=ec02&hexsign=...&context[ts]={ts}&csrf={bili_jct}`；
    /// 未登录时 `csrf` 为空（B 站 Web 端同样匿名申请）。
    pub async fn activate_ticket(&self) {
        if self.ticket_is_fresh() {
            return;
        }
        let ts = unix_now() as i64;
        let hexsign = ticket_hexsign(ts);
        let csrf = self.csrf().unwrap_or_default();
        let query = [
            ("key_id".to_string(), "ec02".to_string()),
            ("hexsign".to_string(), hexsign),
            ("context[ts]".to_string(), ts.to_string()),
            ("csrf".to_string(), csrf),
        ];
        let url = self
            .endpoints
            .api_url("/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket");
        let data = match self.post_query_json(&url, &query).await {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!(error = %e, "bili_ticket request failed");
                return;
            }
        };
        let Some(ticket) = data
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            tracing::warn!("bili_ticket response missing ticket");
            return;
        };
        let created_at = data.get("created_at").and_then(Value::as_i64).unwrap_or(ts);
        let ttl = data.get("ttl").and_then(Value::as_i64).unwrap_or(0);
        let mut store = self.cookies.write().expect("cookie lock");
        store.insert("bili_ticket", ticket);
        if ttl > 0 {
            store.insert("bili_ticket_expires", (created_at + ttl).to_string());
        }
    }

    /// 已持久化的 `bili_ticket` 是否仍在有效期内（预留 6 小时余量）。
    pub fn ticket_is_fresh(&self) -> bool {
        let store = self.cookies.read().expect("cookie lock");
        if store.get("bili_ticket").is_none_or(str::is_empty) {
            return false;
        }
        let Some(expires) = store
            .get("bili_ticket_expires")
            .and_then(|v| v.parse::<i64>().ok())
        else {
            return false;
        };
        expires - unix_now() as i64 > TICKET_REFRESH_MARGIN_SECS
    }

    /// 发送请求：附加 cookie 头、吸收 Set-Cookie、把 412/403 等 HTTP 状态归一为分类错误。
    async fn send(&self, mut req: reqwest::RequestBuilder) -> BiliResult<Response> {
        if let Some(cookie) = self.cookie_header() {
            req = req.header(reqwest::header::COOKIE, cookie);
        }
        let resp = req.send().await.map_err(BiliError::from_reqwest)?;
        self.cookies
            .write()
            .expect("cookie lock")
            .absorb_set_cookie(resp.headers());
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        Err(match status.as_u16() {
            401 | 403 => BiliError::Permission {
                message: format!("HTTP {status}"),
            },
            404 => BiliError::Permission {
                message: format!("HTTP {status}"),
            },
            412 => BiliError::RiskControl,
            429 => BiliError::RiskControl,
            _ => BiliError::Network(format!("HTTP {status}")),
        })
    }

    /// GET 并返回原始响应（需要读 Set-Cookie 头的登录/续期流程用）。
    pub async fn get_response(&self, url: &str) -> BiliResult<Response> {
        tracing::debug!(url = trim_query(url), "bili get response");
        self.send(self.http.get(url).header("Referer", REFERER))
            .await
    }

    /// GET 一个 JSON 接口，返回 `data` 字段；code != 0 归一为 BiliError。
    pub async fn get_json(&self, url: &str, query: &[(String, String)]) -> BiliResult<Value> {
        tracing::debug!(url = trim_query(url), "bili api get");
        let mut req = self.http.get(url).header("Referer", REFERER);
        if !query.is_empty() {
            req = req.query(query);
        }
        let resp = self.send(req).await?;
        let body: Value = resp.json().await?;
        extract_data(body)
    }

    /// 直接返回原始 JSON body（不校验 code，如 nav）。
    pub async fn get_raw_json(&self, url: &str) -> BiliResult<Value> {
        tracing::debug!(url = trim_query(url), "bili api raw get");
        let resp = self
            .send(self.http.get(url).header("Referer", REFERER))
            .await?;
        Ok(resp.json().await?)
    }

    /// 获取文本（HTML 页面等）。
    pub async fn get_text(&self, url: &str) -> BiliResult<String> {
        tracing::debug!(url = trim_query(url), "bili page get");
        let resp = self
            .send(self.http.get(url).header("Referer", REFERER))
            .await?;
        Ok(resp.text().await?)
    }

    /// POST 表单并解析 JSON（cookie 刷新/登录确认等）。
    pub async fn post_form_json(&self, url: &str, form: &[(String, String)]) -> BiliResult<Value> {
        tracing::debug!(url = trim_query(url), "bili api post");
        let resp = self
            .send(self.http.post(url).header("Referer", REFERER).form(form))
            .await?;
        let body: Value = resp.json().await?;
        extract_data(body)
    }

    /// POST 携带 query 参数并解析 JSON（`GenWebTicket` 等查询参数式接口）。
    pub async fn post_query_json(
        &self,
        url: &str,
        query: &[(String, String)],
    ) -> BiliResult<Value> {
        tracing::debug!(url = trim_query(url), "bili api post query");
        let resp = self
            .send(self.http.post(url).header("Referer", REFERER).query(query))
            .await?;
        let body: Value = resp.json().await?;
        extract_data(body)
    }

    /// nav 接口的 data：匿名时 B 站返回 code=-101 但 data 照常（isLogin/wbi_img），
    /// 因此不做 code 校验，仅在响应损坏时报错。
    pub async fn nav_data(&self) -> BiliResult<Value> {
        let url = self.endpoints.api_url("/x/web-interface/nav");
        let body = self.get_raw_json(&url).await?;
        Ok(body.get("data").cloned().unwrap_or(Value::Null))
    }

    /// 查询登录态与用户信息（nav 接口）。
    pub async fn nav_user_info(&self) -> BiliResult<UserInfo> {
        let data = self.nav_data().await?;
        Ok(UserInfo {
            is_login: data
                .get("isLogin")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            mid: data.get("mid").and_then(Value::as_u64).unwrap_or(0),
            uname: data
                .get("uname")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            avatar_url: data
                .get("face")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from),
            vip_status: data
                .pointer("/vipStatus")
                .and_then(Value::as_i64)
                .map(|v| v == 1)
                .unwrap_or(false),
            vip_type: data
                .pointer("/vipType")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            level: data.get("level").and_then(Value::as_u64).unwrap_or(0),
        })
    }

    /// 取 WBI 原始 key（缓存 10 分钟；失败时向后端透传错误）。
    pub async fn wbi_keys(&self) -> BiliResult<WbiKeys> {
        {
            let cache = self.wbi.lock().await;
            if let Some(c) = cache.as_ref()
                && c.fetched_at.elapsed() < WBI_CACHE_TTL
            {
                return Ok(c.keys.clone());
            }
        }
        let nav = self.nav_data().await?;
        let keys = super::wbi::keys_from_nav(&nav)
            .ok_or_else(|| BiliError::Parse("nav 响应缺少 wbi_img".into()))?;
        let mut cache = self.wbi.lock().await;
        *cache = Some(CachedWbi {
            keys: keys.clone(),
            fetched_at: Instant::now(),
        });
        Ok(keys)
    }

    pub async fn invalidate_wbi(&self) {
        *self.wbi.lock().await = None;
    }
}

/// 统一的 HTTP client 工厂：默认**不走系统代理**。
///
/// 原因：reqwest 0.12 默认读取 macOS 系统代理，本机 Clash 系统代理对本进程的
/// CONNECT 会挂起（v1 实测）；B 站直连最优，代理仅在用户显式设置时启用。
pub fn plain_client() -> reqwest::Client {
    build_http_client(None).expect("http client")
}

/// 构造 API client（超时 + 可选代理）。
pub fn build_http_client(proxy: Option<&str>) -> BiliResult<Client> {
    let mut builder = Client::builder()
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .timeout(REQUEST_TIMEOUT);
    builder = apply_proxy(builder, proxy)?;
    builder.build().map_err(BiliError::from_reqwest)
}

/// 构造下载 client（连接 10s / 读空闲 60s，不设总超时）。
pub fn download_client(proxy: Option<&str>) -> BiliResult<Client> {
    let mut builder = Client::builder()
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT);
    builder = apply_proxy(builder, proxy)?;
    builder.build().map_err(BiliError::from_reqwest)
}

fn apply_proxy(
    mut builder: reqwest::ClientBuilder,
    proxy: Option<&str>,
) -> BiliResult<reqwest::ClientBuilder> {
    match proxy.map(str::trim).filter(|s| !s.is_empty()) {
        Some(addr) => {
            let proxy =
                Proxy::all(addr).map_err(|e| BiliError::Network(format!("代理配置无效: {e}")))?;
            builder = builder.proxy(proxy);
        }
        None => builder = builder.no_proxy(),
    }
    Ok(builder)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 生成 B 站 web 端风格的 `_uuid`（大写 UUID + infoc）。
fn generate_uuid() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut hex = |n: usize| -> String {
        (0..n)
            .map(|_| format!("{:X}", rng.gen_range(0u8..16)))
            .collect::<String>()
    };
    format!(
        "{}-{}-4{}-{}-{}{}",
        hex(8),
        hex(4),
        hex(3),
        hex(4),
        hex(12),
        "infoc"
    )
}

fn trim_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// 从 B 站标准响应 `{code, message, data}` 提取 data。
pub fn extract_data(body: Value) -> BiliResult<Value> {
    let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 0 {
        // 大多数接口用 `data`；PGC/PUGV 系列接口（season/playurl）用 `result`。
        Ok(body
            .get("data")
            .cloned()
            .or_else(|| body.get("result").cloned())
            .unwrap_or(Value::Null))
    } else {
        let message = body
            .get("message")
            .or_else(|| body.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or("unknown api error");
        Err(BiliError::from_api(code, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_default_and_join() {
        let e = Endpoints::default();
        assert_eq!(e.api_url("/x"), "https://api.bilibili.com/x");
        assert_eq!(
            e.passport_url("/x/passport-login/web/cookie/info"),
            "https://passport.bilibili.com/x/passport-login/web/cookie/info"
        );
        assert_eq!(
            e.www_url("/correspond/1/abc"),
            "https://www.bilibili.com/correspond/1/abc"
        );
        // 尾部斜杠不会产生双斜杠
        let custom = Endpoints {
            api: "http://127.0.0.1:1/".into(),
            passport: "http://127.0.0.1:1".into(),
            www: "http://127.0.0.1:1/".into(),
        };
        assert_eq!(custom.api_url("/x"), "http://127.0.0.1:1/x");
        assert_eq!(custom.www_url("/y"), "http://127.0.0.1:1/y");
    }

    #[test]
    fn ticket_hexsign_matches_reference() {
        // 参考值由 Python `hmac.new(b"XgwSnGZ1p", b"ts1700000000", sha256).hexdigest()` 生成
        assert_eq!(
            ticket_hexsign(1_700_000_000),
            "bb79f0d980ffbb51597aa1a3e8b55603025cc1322ac766f4c1a98852e6182514"
        );
        assert_eq!(
            ticket_hexsign(1_684_466_082),
            "c9a81d0950d424b999843355f8a0910302ab9581c3a26e934de1e57ca1fc22f2"
        );
        let sign = ticket_hexsign(1_700_000_000);
        assert_eq!(sign.len(), 64, "SHA-256 hexsign 固定 64 字符");
        assert!(
            sign.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn hex_lower_encodes_bytes() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex_lower(&[]), "");
    }

    fn test_endpoints(uri: &str) -> Endpoints {
        Endpoints {
            api: uri.to_string(),
            passport: uri.to_string(),
            www: uri.to_string(),
        }
    }

    #[tokio::test]
    async fn activate_buvid_also_fetches_ticket() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

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
                "data": {"ticket": "TICKET", "created_at": 1_700_000_000_i64, "ttl": 259_200_i64}
            })))
            .mount(&server)
            .await;

        let client = BiliClient::with_endpoints(
            plain_client(),
            StoredCookies::default(),
            test_endpoints(&server.uri()),
        );
        client.activate_buvid().await;

        let cookies = client.cookies();
        assert_eq!(cookies.get("buvid3"), Some("B3"));
        assert_eq!(cookies.get("buvid4"), Some("B4"));
        assert!(cookies.get("_uuid").is_some(), "应生成 _uuid");
        assert_eq!(cookies.get("bili_ticket"), Some("TICKET"));
        assert!(cookies.get("bili_ticket_expires").is_some());
    }

    #[tokio::test]
    async fn activate_ticket_skips_when_fresh() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // 不应发出任何 GenWebTicket 请求
        Mock::given(method("POST"))
            .and(path("/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let far_future = unix_now() as i64 + 86_400;
        let client = BiliClient::with_endpoints(
            plain_client(),
            StoredCookies::from_pairs(vec![
                ("bili_ticket".to_string(), "OLD".to_string()),
                ("bili_ticket_expires".to_string(), far_future.to_string()),
            ]),
            test_endpoints(&server.uri()),
        );
        assert!(client.ticket_is_fresh());
        client.activate_ticket().await;
        assert_eq!(client.cookies().get("bili_ticket"), Some("OLD"));
    }

    #[test]
    fn extract_result_envelope() {
        // PGC/PUGV 系列接口用 result 而不是 data
        let v = serde_json::json!({"code": 0, "message": "0", "result": {"season_id": 61, "title": "课程"}});
        let data = extract_data(v).unwrap();
        assert_eq!(data["season_id"], 61);
        assert_eq!(data["title"], "课程");
    }

    #[test]
    fn extract_data_ok() {
        let v = serde_json::json!({"code": 0, "message": "0", "data": {"a": 1}});
        assert_eq!(extract_data(v).unwrap()["a"], 1);
    }

    #[test]
    fn extract_data_api_err() {
        let v = serde_json::json!({"code": -404, "message": "啥都木有"});
        match extract_data(v) {
            Err(BiliError::Api { code, .. }) => assert_eq!(code, -404),
            other => panic!("unexpected {other:?}"),
        }
        let v = serde_json::json!({"code": -352, "message": "风控"});
        assert!(matches!(extract_data(v), Err(BiliError::RiskControl)));
        let v = serde_json::json!({"code": -101, "message": "未登录"});
        assert!(matches!(extract_data(v), Err(BiliError::NotLoggedIn)));
    }

    #[test]
    fn cookie_store_roundtrip_and_header() {
        let stored = StoredCookies::from_pairs(vec![
            ("SESSDATA".to_string(), "abc".to_string()),
            ("bili_jct".to_string(), "xyz".to_string()),
        ]);
        let store = CookieStore::from_stored(&stored);
        assert!(store.is_logged_in());
        assert_eq!(store.header_value().unwrap(), "SESSDATA=abc; bili_jct=xyz");
        let back = store.to_stored();
        assert_eq!(back.len(), 2);
        assert_eq!(store.get("SESSDATA"), Some("abc"));
    }

    #[test]
    fn absorb_set_cookie_ignores_attributes() {
        let mut store = CookieStore::default();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            "SESSDATA=abc%2C123; Path=/; Domain=.bilibili.com; HttpOnly"
                .parse()
                .unwrap(),
        );
        headers.append(
            reqwest::header::SET_COOKIE,
            "buvid3=xyz; Path=/".parse().unwrap(),
        );
        store.absorb_set_cookie(&headers);
        assert_eq!(store.get("SESSDATA"), Some("abc%2C123"));
        assert_eq!(store.get("buvid3"), Some("xyz"));
    }

    #[test]
    fn uuid_format() {
        let c = BiliClient::with_http(plain_client(), StoredCookies::default());
        let uuid = c.ensure_uuid();
        assert_eq!(uuid.len(), 41, "{uuid}");
        assert!(uuid.ends_with("infoc"));
        assert_eq!(c.ensure_uuid(), uuid, "已存在时幂等");
        assert_eq!(c.device().uuid.as_deref(), Some(uuid.as_str()));
    }

    #[test]
    fn invalid_proxy_rejected() {
        let err = BiliClient::new(Some("not a proxy"), StoredCookies::default()).unwrap_err();
        assert!(matches!(err, BiliError::Network(_)));
    }

    #[test]
    fn policy_prevents_cookie_leak_in_debug() {
        let stored = StoredCookies::from_pairs(vec![("SESSDATA".into(), "secret".into())]);
        let c = BiliClient::new(None, stored).unwrap();
        let dbg = format!("{c:?}");
        assert!(
            !dbg.contains("secret"),
            "Debug 输出不得包含 cookie 值: {dbg}"
        );
        assert!(dbg.contains("SESSDATA"));
    }

    #[tokio::test]
    async fn get_json_attaches_cookie_header() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .and(header("cookie", "SESSDATA=abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"code":0,"data":{"ok":true}})),
            )
            .mount(&server)
            .await;
        let stored = StoredCookies::from_pairs(vec![("SESSDATA".into(), "abc".into())]);
        let c = BiliClient::new(None, stored).unwrap();
        let data = c
            .get_json(&format!("{}/api", server.uri()), &[])
            .await
            .unwrap();
        assert_eq!(data["ok"], true);
    }
}
