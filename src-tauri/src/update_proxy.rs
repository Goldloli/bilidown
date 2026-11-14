//! 只服务 updater 的代理路由。
//!
//! B 站、媒体下载和其它网络请求必须继续由 core 的 client 工厂处理；这里不得被复用到
//! 那些链路。系统模式显式读取操作系统代理，而不是启用 reqwest 的隐式全局代理。

use std::time::Duration;

use bilidown_core::{Settings, UpdateProxyMode};
use proxy_watch::pac::{PacPolicy, PacResolver};
use proxy_watch::{ProxyConfig, ProxyMode, ProxyStep, Url as ProxyUrl};
use reqwest::header::HeaderMap;
use reqwest::{Response, Url};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SYSTEM_PROXY_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyRoute {
    url: Option<String>,
    label: String,
}

enum SystemProxyResolver {
    Plain(PacResolver),
    Native(PacResolver),
}

pub struct UpdateProxySession {
    mode: UpdateProxyMode,
    kind: UpdateProxyKind,
}

enum UpdateProxyKind {
    Direct,
    Manual(ProxyRoute),
    System(Box<SystemProxyState>),
}

struct SystemProxyState {
    config: ProxyConfig,
    resolver: SystemProxyResolver,
}

impl UpdateProxySession {
    pub async fn from_settings(settings: &Settings) -> Result<Self, String> {
        let mode = settings.update_proxy_mode;
        let kind = match mode {
            UpdateProxyMode::Direct => UpdateProxyKind::Direct,
            UpdateProxyMode::Manual => {
                let raw = settings
                    .update_proxy
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "更新手动代理未配置地址。".to_string())?;
                reqwest::Proxy::all(raw)
                    .map_err(|error| format!("更新手动代理配置无效: {error}"))?;
                UpdateProxyKind::Manual(ProxyRoute {
                    url: Some(raw.to_string()),
                    label: redact_proxy_url(raw),
                })
            }
            UpdateProxyMode::System => {
                let loaded = tokio::time::timeout(
                    SYSTEM_PROXY_READ_TIMEOUT,
                    tokio::task::spawn_blocking(load_system_proxy),
                )
                .await
                .map_err(|_| "读取系统代理超过 5 秒。".to_string())?
                .map_err(|error| format!("系统代理读取任务失败: {error}"))??;
                UpdateProxyKind::System(Box::new(SystemProxyState {
                    config: loaded.0,
                    resolver: loaded.1,
                }))
            }
        };
        Ok(Self { mode, kind })
    }

    #[must_use]
    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            UpdateProxyMode::System => "跟随系统代理",
            UpdateProxyMode::Direct => "更新直连",
            UpdateProxyMode::Manual => "更新手动代理",
        }
    }

    fn routes(&self, target: &Url) -> Result<Vec<ProxyRoute>, String> {
        match &self.kind {
            UpdateProxyKind::Direct => Ok(vec![ProxyRoute {
                url: None,
                label: "直连".into(),
            }]),
            UpdateProxyKind::Manual(route) => Ok(vec![route.clone()]),
            UpdateProxyKind::System(state) => {
                let parsed = ProxyUrl::parse(target.as_str())
                    .map_err(|error| format!("更新目标 URL 非法: {error}"))?;
                let steps = match &state.resolver {
                    SystemProxyResolver::Plain(resolver)
                    | SystemProxyResolver::Native(resolver) => resolver
                        .resolve_config(&state.config, &parsed, None)
                        .map_err(|error| format!("系统代理无法解析更新目标: {error}")),
                }?;
                let routes = routes_from_steps(steps)?;
                if routes.is_empty() {
                    return Err("系统代理没有返回可用路由。".into());
                }
                Ok(routes)
            }
        }
    }
}

fn load_system_proxy() -> Result<(ProxyConfig, SystemProxyResolver), String> {
    let config = proxy_watch::read().map_err(|error| format!("读取系统代理失败: {error}"))?;
    let resolver = if matches!(
        config.effective,
        ProxyMode::Pac { .. } | ProxyMode::WpadAutoDetect
    ) {
        SystemProxyResolver::Native(PacResolver::new(PacPolicy::new()).with_native(
            native_pac_resolver().map_err(|error| format!("初始化系统 PAC 解析失败: {error}"))?,
        ))
    } else {
        SystemProxyResolver::Plain(PacResolver::new(PacPolicy::new()))
    };
    Ok((config, resolver))
}

#[cfg(target_os = "windows")]
fn native_pac_resolver() -> Result<proxy_watch::pac::NativePacResolver, proxy_watch::Error> {
    proxy_watch::pac::NativePacResolver::new().map(|resolver| resolver.with_wpad(true))
}

#[cfg(target_os = "macos")]
fn native_pac_resolver() -> Result<proxy_watch::pac::NativePacResolver, proxy_watch::Error> {
    Ok(proxy_watch::pac::NativePacResolver::new().with_wpad(true))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn native_pac_resolver() -> Result<proxy_watch::pac::NativePacResolver, proxy_watch::Error> {
    Err(proxy_watch::Error::Unsupported)
}

fn routes_from_steps(steps: Vec<ProxyStep>) -> Result<Vec<ProxyRoute>, String> {
    steps
        .into_iter()
        .map(|step| {
            if step.is_direct() {
                return Ok(ProxyRoute {
                    url: None,
                    label: "系统直连".into(),
                });
            }
            let endpoint = step
                .endpoint()
                .ok_or_else(|| "系统代理返回了无地址路由。".to_string())?;
            let url = step
                .to_url()
                .ok_or_else(|| "系统代理地址无法转换为 HTTP client URL。".to_string())?;
            Ok(ProxyRoute {
                url: Some(url.to_string()),
                label: endpoint.to_string(),
            })
        })
        .collect()
}

fn redact_proxy_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return "<invalid proxy url>".into();
    };
    if url.set_username("").is_err() || url.set_password(None).is_err() {
        return format!("<{} proxy>", url.scheme());
    }
    url.to_string()
}

pub struct UpdateTransport {
    session: UpdateProxySession,
    user_agent: &'static str,
}

impl UpdateTransport {
    pub async fn from_settings(
        settings: &Settings,
        user_agent: &'static str,
    ) -> Result<Self, String> {
        Ok(Self {
            session: UpdateProxySession::from_settings(settings).await?,
            user_agent,
        })
    }

    #[must_use]
    pub fn mode_label(&self) -> &'static str {
        self.session.mode_label()
    }

    pub async fn get(
        &self,
        target: &str,
        headers: HeaderMap,
        request_timeout: Option<Duration>,
    ) -> Result<Response, String> {
        let mut current = Url::parse(target).map_err(|error| format!("更新 URL 非法: {error}"))?;
        for redirect_count in 0..=MAX_REDIRECTS {
            validate_trusted_update_url(&current)?;
            let response = self
                .send_one(&current, headers.clone(), request_timeout)
                .await?;
            if !is_redirect(response.status()) {
                return Ok(response);
            }
            if redirect_count == MAX_REDIRECTS {
                return Err("更新请求重定向次数过多。".into());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "更新请求返回重定向但缺少 Location。".to_string())?;
            current = next_redirect_url(&current, location)?;
        }
        Err("更新请求重定向失败。".into())
    }

    async fn send_one(
        &self,
        target: &Url,
        headers: HeaderMap,
        request_timeout: Option<Duration>,
    ) -> Result<Response, String> {
        let routes = self.session.routes(target)?;
        let mut last_error = None;
        for route in routes {
            let mut builder = reqwest::Client::builder()
                .user_agent(self.user_agent)
                .connect_timeout(CONNECT_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy();
            if let Some(url) = route.url.as_deref() {
                let proxy = reqwest::Proxy::all(url)
                    .map_err(|error| format!("更新代理配置无效: {error}"))?;
                builder = builder.proxy(proxy);
            }
            let client = builder
                .build()
                .map_err(|error| format!("创建更新网络客户端失败: {error}"))?;
            let request = client.get(target.clone()).headers(headers.clone());
            let result = match request_timeout {
                Some(timeout) => match tokio::time::timeout(timeout, request.send()).await {
                    Ok(result) => result.map_err(|error| classify_request_error(&error)),
                    Err(_) => Err("更新请求超时".to_string()),
                },
                None => request
                    .send()
                    .await
                    .map_err(|error| classify_request_error(&error)),
            };
            match result {
                Ok(response) => return Ok(response),
                Err(error) => {
                    last_error = Some(format!(
                        "{}（{}）: {}",
                        self.session.mode_label(),
                        route.label,
                        error
                    ));
                    tracing::debug!(
                        target: "shell::update",
                        mode = self.session.mode_label(),
                        route = %route.label,
                        error = %error,
                        "更新代理路由失败"
                    );
                }
            }
        }
        Err(
            last_error.unwrap_or_else(|| format!("{}没有可用路由", self.session.mode_label()))
                + "，未自动改走直连",
        )
    }
}

fn is_redirect(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::MOVED_PERMANENTLY
            | reqwest::StatusCode::FOUND
            | reqwest::StatusCode::SEE_OTHER
            | reqwest::StatusCode::TEMPORARY_REDIRECT
            | reqwest::StatusCode::PERMANENT_REDIRECT
    )
}

fn next_redirect_url(current: &Url, location: &str) -> Result<Url, String> {
    let next = current
        .join(location)
        .map_err(|error| format!("更新重定向 URL 非法: {error}"))?;
    validate_trusted_update_url(&next)?;
    Ok(next)
}

fn validate_trusted_update_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("更新请求必须使用 https。".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "更新 URL 缺少主机名。".to_string())?;
    if !crate::update::is_trusted_host(host) {
        return Err("更新请求被重定向到非可信域名，已拒绝。".into());
    }
    Ok(())
}

fn classify_request_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "更新网络连接超时".into()
    } else if error.is_connect() {
        "更新网络无法连接代理或 GitHub".into()
    } else if error.is_body() {
        "更新网络响应读取失败".into()
    } else {
        "更新网络请求失败".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_watch::{ProxyConfigSource, ProxyEndpoint};

    #[tokio::test]
    async fn direct_and_manual_sessions_do_not_expand_to_direct_fallback() {
        let direct = UpdateProxySession::from_settings(&Settings {
            update_proxy_mode: UpdateProxyMode::Direct,
            ..Default::default()
        })
        .await
        .unwrap();
        let routes = direct
            .routes(&Url::parse("https://github.com/").unwrap())
            .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].label, "直连");

        let manual = UpdateProxySession::from_settings(&Settings {
            update_proxy_mode: UpdateProxyMode::Manual,
            update_proxy: Some("http://127.0.0.1:7897".into()),
            ..Default::default()
        })
        .await
        .unwrap();
        let routes = manual
            .routes(&Url::parse("https://github.com/").unwrap())
            .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url.as_deref(), Some("http://127.0.0.1:7897"));

        let mut changed = Settings {
            update_proxy_mode: UpdateProxyMode::Direct,
            ..Default::default()
        };
        let captured = UpdateProxySession::from_settings(&changed).await.unwrap();
        changed.update_proxy_mode = UpdateProxyMode::Manual;
        changed.update_proxy = Some("http://127.0.0.1:7897".into());
        let routes = captured
            .routes(&Url::parse("https://github.com/").unwrap())
            .unwrap();
        assert_eq!(routes[0].url, None, "进行中操作必须保持开始时策略");
    }

    #[test]
    fn route_conversion_keeps_direct_and_redacts_credentials() {
        let endpoint =
            ProxyEndpoint::parse("socks5h://user:secret@proxy.example:1080", 80).unwrap();
        let routes =
            routes_from_steps(vec![ProxyStep::Direct, ProxyStep::Socks5(endpoint)]).unwrap();
        assert_eq!(routes[0].url, None);
        assert_eq!(
            routes[1].url.as_deref(),
            Some("socks5h://user:secret@proxy.example:1080")
        );
        assert!(!routes[1].label.contains("user"));
        assert!(!routes[1].label.contains("secret"));
    }

    #[test]
    fn manual_label_redacts_auth() {
        let label = redact_proxy_url("http://alice:password@127.0.0.1:7897");
        assert!(!label.contains("alice"));
        assert!(!label.contains("password"));
        assert!(label.contains("127.0.0.1:7897"));
    }

    #[test]
    fn invalid_system_url_is_rejected_before_request() {
        let error =
            validate_trusted_update_url(&Url::parse("https://example.com/file.dmg").unwrap())
                .unwrap_err();
        assert!(error.contains("非可信域名"));
    }

    #[test]
    fn redirects_are_validated_before_the_next_request() {
        let current = Url::parse("https://github.com/Goldloli/bilidown/releases").unwrap();
        let trusted = next_redirect_url(
            &current,
            "https://release-assets.githubusercontent.com/file",
        )
        .unwrap();
        assert_eq!(
            trusted.host_str(),
            Some("release-assets.githubusercontent.com")
        );
        let error = next_redirect_url(&current, "https://example.com/steal").unwrap_err();
        assert!(error.contains("非可信域名"));
    }

    #[test]
    fn pac_resolution_error_does_not_fall_back_to_direct() {
        let config = ProxyConfig::from_source(
            ProxyConfigSource::Registry,
            ProxyMode::pac(ProxyUrl::parse("https://proxy.example/pac").unwrap()),
        );
        let session = UpdateProxySession {
            mode: UpdateProxyMode::System,
            kind: UpdateProxyKind::System(Box::new(SystemProxyState {
                config,
                resolver: SystemProxyResolver::Plain(PacResolver::new(PacPolicy::new())),
            })),
        };
        let error = session
            .routes(&Url::parse("https://github.com/").unwrap())
            .unwrap_err();
        assert!(error.contains("系统代理无法解析"));
        assert!(!error.contains("直连"));
    }

    #[tokio::test]
    async fn unreachable_manual_proxy_never_falls_back_to_direct() {
        let transport = UpdateTransport::from_settings(
            &Settings {
                update_proxy_mode: UpdateProxyMode::Manual,
                update_proxy: Some("http://127.0.0.1:1".into()),
                ..Default::default()
            },
            "bilidown-proxy-test",
        )
        .await
        .unwrap();
        let error = transport
            .get(
                "https://api.github.com/repos/Goldloli/bilidown",
                HeaderMap::new(),
                Some(Duration::from_millis(500)),
            )
            .await
            .unwrap_err();
        assert!(error.contains("更新手动代理"), "actual error: {error}");
        assert!(error.contains("未自动改走直连"), "actual error: {error}");
    }

    #[test]
    fn system_proxy_reads_os_configuration_and_resolves_github() {
        let (config, resolver) = load_system_proxy().unwrap();
        let session = UpdateProxySession {
            mode: UpdateProxyMode::System,
            kind: UpdateProxyKind::System(Box::new(SystemProxyState { config, resolver })),
        };
        let routes = session
            .routes(&Url::parse("https://api.github.com/repos/Goldloli/bilidown").unwrap())
            .unwrap();
        assert!(!routes.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires a real system proxy and GitHub network access"]
    async fn system_proxy_checks_api_and_reads_release_range() {
        let settings = Settings {
            update_proxy_mode: UpdateProxyMode::System,
            ..Default::default()
        };
        let transport = UpdateTransport::from_settings(&settings, "bilidown-proxy-test")
            .await
            .unwrap();
        let api = transport
            .get(
                "https://api.github.com/repos/Goldloli/bilidown",
                HeaderMap::new(),
                Some(Duration::from_secs(30)),
            )
            .await
            .unwrap();
        assert!(api.status().is_success());

        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RANGE,
            reqwest::header::HeaderValue::from_static("bytes=0-1023"),
        );
        let response = transport
            .get(
                "https://github.com/Goldloli/bilidown/releases/download/v2.5.0/bilidown_2.5.0_aarch64.dmg",
                headers,
                Some(Duration::from_secs(30)),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        let bytes = response.bytes().await.unwrap();
        assert_eq!(bytes.len(), 1024);
    }
}
