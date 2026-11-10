//! 壳内状态与 `bilidown-core` 接线（tasks 4.5）。
//!
//! 启动顺序（core API.md §1，做了两处工程化调整，见 SHELL_NOTES.md）：
//! `AppPaths::standard → logging::init → Storage::open → BiliClient::new +
//! activate_buvid → account::restore → TaskManager::start`
//!
//! - 初始化在后台任务中执行：窗口先出现，命令通过 [`ShellState::core`] 等待就绪；
//! - core 的 `watch<QueueSnapshot>` / `broadcast<TaskEvent>` 由两个 bridge 任务转成
//!   Tauri 事件，并驱动菜单可用性、Dock 徽标、系统通知；
//! - 退出前必须 `manager.shutdown()`（见 `commands::quit_app`）。

use bilidown_core::api::account::{self, AccountState};
use bilidown_core::api::login::QrLogin;
use bilidown_core::{
    AppPaths, BiliClient, BiliError, BiliResult, HistoryQuery, QueueSnapshot, Settings, Storage,
    TaskEvent, TaskManager, TaskManagerConfig, TaskPhase,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use crate::{dock, dto, events, menu};

/// core 初始化状态（命令等待用；失败时立即返回错误而不是永远挂起）。
#[derive(Debug, Clone)]
enum InitState {
    Initializing,
    Ready,
    Failed(String),
}

/// 已初始化的 core 句柄。
pub struct Core {
    pub paths: AppPaths,
    pub storage: Arc<Storage>,
    client: RwLock<Arc<BiliClient>>,
    pub manager: TaskManager,
    settings: RwLock<Settings>,
    /// 设置提交串行化（design D3）：合并/校验/持久化/运行态替换在锁内完成。
    settings_commit: tokio::sync::Mutex<()>,
    /// 进程内单调递增的设置修订号（design D3）。
    settings_revision: AtomicU64,
    /// 网络准备代次（design D5/5.4）：旧结果不得覆盖新状态。
    network_generation: AtomicU64,
    account: RwLock<AccountState>,
    /// 进行中的扫码登录流程（`account::login_poll` 需要 &mut）。
    login: Mutex<Option<QrLogin>>,
}

impl Core {
    pub fn client(&self) -> Arc<BiliClient> {
        self.client
            .read()
            .map(|c| c.clone())
            .expect("core client lock poisoned")
    }

    pub fn set_client(&self, client: Arc<BiliClient>) {
        if let Ok(mut guard) = self.client.write() {
            *guard = client;
        }
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        self.manager.snapshot()
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn set_settings(&self, settings: Settings) {
        if let Ok(mut guard) = self.settings.write() {
            *guard = settings;
        }
    }

    /// 当前设置修订号（design D3）。
    pub fn settings_revision(&self) -> u64 {
        self.settings_revision.load(Ordering::SeqCst)
    }

    /// 开始一次新的网络准备，返回其代次（design D5/5.4）。
    pub fn begin_network_attempt(&self) -> u64 {
        self.network_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn network_generation(&self) -> u64 {
        self.network_generation.load(Ordering::SeqCst)
    }

    /// 串行提交设置：在锁内合并、校验、预构造资源、持久化，成功后才替换运行态。
    /// 持久化失败时运行态不变；返回新的修订号（design D3）。
    pub async fn update_settings<F>(&self, update: F) -> BiliResult<u64>
    where
        F: FnOnce(&Settings) -> BiliResult<Settings>,
    {
        let _guard = self.settings_commit.lock().await;
        let previous = self.settings();
        let next = update(&previous)?;
        next.validate().map_err(|errors| {
            BiliError::Other(
                errors
                    .iter()
                    .map(|error| format!("{}: {}", error.field, error.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        // 可能失败的本地资源先构造，失败时不做任何提交。
        let rebuilt_client = if previous.proxy != next.proxy {
            let creds = self.storage.load_credentials();
            let client = Arc::new(BiliClient::new(next.proxy.as_deref(), creds.stored())?);
            client.apply_device(&creds.device);
            Some(client)
        } else {
            None
        };
        // 持久化失败：不提交、不改运行态（design D3/D8）。
        self.storage.save_settings(&next)?;
        self.set_settings(next.clone());
        self.manager.set_settings(next).await?;
        if let Some(client) = rebuilt_client {
            self.manager.set_client(client.clone()).await?;
            self.set_client(client.clone());
            // 网络准备独立进行，不阻塞提交；失败只记录，后续请求可重试（design D3/5.2）。
            tauri::async_runtime::spawn(async move {
                client.activate_buvid().await;
            });
        }
        let revision = self.settings_revision.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::info!(revision, "设置已提交");
        Ok(revision)
    }

    pub fn account(&self) -> AccountState {
        self.account.read().map(|a| a.clone()).unwrap_or_default()
    }

    pub fn set_account(&self, account: AccountState) {
        if let Ok(mut guard) = self.account.write() {
            *guard = account;
        }
    }

    pub fn login_flow(&self) -> Option<QrLogin> {
        self.login.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn set_login_flow(&self, flow: Option<QrLogin>) {
        if let Ok(mut guard) = self.login.lock() {
            *guard = flow;
        }
    }

    /// 进行中（排队 + 解析/下载/合并）数量：Dock 徽标与退出保护使用。
    pub fn running_count(&self) -> u32 {
        let snapshot = self.snapshot();
        snapshot.active_count + snapshot.queued_count
    }

    pub fn menu_counts(&self) -> MenuCounts {
        MenuCounts::from_snapshot(&self.snapshot())
    }
}

/// 菜单可用性所需的队列计数（core 快照派生）。
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuCounts {
    pub queued: u32,
    pub active: u32,
    pub paused: u32,
    pub failed: u32,
    pub done: u32,
    pub cancelled: u32,
}

impl MenuCounts {
    pub fn from_snapshot(snapshot: &QueueSnapshot) -> Self {
        Self {
            queued: snapshot.queued_count,
            active: snapshot.active_count,
            paused: snapshot.paused_count,
            failed: snapshot.failed_count,
            done: snapshot.done_count,
            cancelled: snapshot
                .tasks
                .iter()
                .filter(|t| t.phase == TaskPhase::Cancelled)
                .count() as u32,
        }
    }

    pub fn can_start_all(&self) -> bool {
        self.queued + self.paused > 0
    }
    pub fn can_pause_all(&self) -> bool {
        self.active > 0
    }
    pub fn can_retry_all(&self) -> bool {
        self.failed > 0
    }
    /// core 的 clear_finished 会清掉 done/failed/cancelled
    pub fn can_clear_finished(&self) -> bool {
        self.done + self.failed + self.cancelled > 0
    }
    pub fn can_clear_inactive(&self) -> bool {
        self.queued + self.paused + self.cancelled > 0
    }
}

/// 由 Tauri `manage` 持有的全局壳状态。
pub struct ShellState {
    core: RwLock<Option<Arc<Core>>>,
    init: watch::Sender<InitState>,
    /// 初始化进行中标记：避免重复 spawn（design D5/5.4 重试）。
    init_running: AtomicBool,
    /// 退出保护：前端确认后置 true，避免 ExitRequested 再次拦截。
    quit_confirmed: AtomicBool,
}

impl ShellState {
    pub fn new() -> Self {
        let (init, _) = watch::channel(InitState::Initializing);
        Self {
            core: RwLock::new(None),
            init,
            init_running: AtomicBool::new(false),
            quit_confirmed: AtomicBool::new(false),
        }
    }

    pub fn try_core(&self) -> Option<Arc<Core>> {
        self.core.read().ok().and_then(|guard| guard.clone())
    }

    /// 等待 core 初始化完成（命令入口使用）。
    pub async fn core(&self) -> BiliResult<Arc<Core>> {
        if let Some(core) = self.try_core() {
            return Ok(core);
        }
        let mut rx = self.init.subscribe();
        loop {
            match rx.borrow().clone() {
                InitState::Ready => {
                    return self
                        .try_core()
                        .ok_or_else(|| BiliError::Other("core 已就绪但句柄缺失".into()));
                }
                InitState::Failed(reason) => {
                    return Err(BiliError::Other(format!("core 初始化失败: {reason}")));
                }
                InitState::Initializing => {}
            }
            if rx.changed().await.is_err() {
                return Err(BiliError::Other("core 初始化通道关闭".into()));
            }
        }
    }

    fn set_core(&self, core: Arc<Core>) {
        if let Ok(mut guard) = self.core.write() {
            *guard = Some(core);
        }
        let _ = self.init.send(InitState::Ready);
    }

    fn mark_failed(&self, reason: String) {
        let _ = self.init.send(InitState::Failed(reason));
    }

    /// 把初始化状态重置为进行中（前端「重试初始化」使用，design D5/5.4）。
    pub fn reset_init(&self) {
        let _ = self.init.send(InitState::Initializing);
    }

    pub fn confirm_quit(&self) {
        self.quit_confirmed.store(true, Ordering::SeqCst);
    }

    pub fn quit_confirmed(&self) -> bool {
        self.quit_confirmed.load(Ordering::SeqCst)
    }

    pub fn data_dir(&self) -> PathBuf {
        self.try_core()
            .map(|c| c.paths.root().to_path_buf())
            .unwrap_or_else(crate::logging::app_data_dir)
    }

    pub fn log_dir(&self) -> PathBuf {
        self.try_core()
            .map(|c| c.paths.logs_dir())
            .unwrap_or_else(crate::logging::logs_dir)
    }
}

impl Default for ShellState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 初始化
// ---------------------------------------------------------------------------

/// 启动后台初始化：先完成本地阶段并唤醒等待命令，再独立进行网络阶段（design D5/5.1/5.2）。
pub fn spawn_init(app: AppHandle) {
    {
        let state = app.state::<ShellState>();
        if state
            .init_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            tracing::info!(target: "shell::app", "初始化已在进行中，忽略重复请求");
            return;
        }
        state.reset_init();
    }
    tauri::async_runtime::spawn(async move {
        tracing::info!(target: "shell::app", "开始初始化 core（本地阶段）");
        match init_local_core().await {
            Ok(core) => {
                tracing::info!(
                    target: "shell::app",
                    data_dir = %core.paths.root().display(),
                    log_dir = %core.paths.logs_dir().display(),
                    "core 本地阶段就绪"
                );
                let state = app.state::<ShellState>();
                state.init_running.store(false, Ordering::SeqCst);
                state.set_core(core.clone());
                spawn_snapshot_bridge(&app, &core);
                spawn_event_bridge(&app, &core);
                spawn_clipboard_watch(&app);

                // 本地数据（队列/设置/历史/账号快照）先发布，界面无需等待网络。
                let snapshot = core.snapshot();
                let account = core.account();
                let settings = core.settings();
                events::emit(
                    &app,
                    events::QUEUE_SNAPSHOT,
                    dto::snapshot_dto(&snapshot, &account, &settings),
                );
                events::emit(&app, events::ACCOUNT_CHANGED, dto::account_dto(&account));
                let mut dto = dto::settings_dto(&settings);
                dto.revision = Some(core.settings_revision());
                events::emit(&app, events::SETTINGS_CHANGED, dto);
                publish_history(&app);
                menu::refresh(&app);
                dock::update(&app, core.running_count());
                events::emit(
                    &app,
                    events::CORE_STATUS,
                    dto::CoreStatusDTO {
                        stage: "local-ready".into(),
                        generation: core.network_generation(),
                        degraded: false,
                        error: None,
                    },
                );

                let restored = snapshot.tasks.iter().filter(|t| t.restored).count();
                let message = if restored > 0 {
                    format!("本地数据已就绪（从上次退出恢复 {restored} 个任务，已暂停）")
                } else {
                    "本地数据已就绪".to_string()
                };
                events::notice(&app, "info", message);

                // 封面缓存剪枝改为后台执行，不阻塞本地就绪（design D5）。
                let covers = core.paths.covers_dir();
                tauri::async_runtime::spawn_blocking(move || {
                    crate::covers::prune(&covers, 30, 500);
                });

                spawn_network_phase(app.clone(), core.clone());
            }
            Err(err) => {
                tracing::error!(target: "shell::app", error = %err.describe(), "core 本地阶段初始化失败");
                let state = app.state::<ShellState>();
                state.init_running.store(false, Ordering::SeqCst);
                state.mark_failed(err.describe());
                events::notice(&app, "error", format!("引擎初始化失败：{}", err.describe()));
            }
        }
    });
}

/// 网络阶段：5 秒预算内完成设备激活与账号恢复；超时降级但本地内容可继续使用（design D5/5.2）。
pub(crate) fn spawn_network_phase(app: AppHandle, core: Arc<Core>) {
    tauri::async_runtime::spawn(async move {
        let generation = core.begin_network_attempt();
        let client = core.client();
        let storage = Arc::clone(&core.storage);
        let prepared = tokio::time::timeout(Duration::from_secs(5), async move {
            client.activate_buvid().await;
            let account_state = account::restore(&client, &storage).await;
            let _ = account::persist_device(&client, &storage).await;
            account_state
        })
        .await;
        // 代次保护：旧结果不得覆盖较新的重试结果（design D5/5.4）。
        if core.network_generation() != generation {
            tracing::info!(target: "shell::app", generation, "网络准备结果过期，丢弃");
            return;
        }
        match prepared {
            Ok(account_state) => {
                core.set_account(account_state.clone());
                events::emit(&app, events::ACCOUNT_CHANGED, dto::account_dto(&account_state));
                events::emit(
                    &app,
                    events::CORE_STATUS,
                    dto::CoreStatusDTO {
                        stage: "network-ready".into(),
                        generation,
                        degraded: false,
                        error: None,
                    },
                );
                match account_state {
                    a if a.logged_in => {
                        events::notice(&app, "info", format!("已登录：{}", a.user.uname));
                    }
                    a if a.expired => {
                        events::notice(&app, "warning", "登录已失效，请重新扫码登录");
                    }
                    _ => {}
                }
            }
            Err(_) => {
                tracing::warn!(
                    target: "shell::app",
                    "网络准备未在 5 秒预算内完成，进入降级可用状态"
                );
                events::emit(
                    &app,
                    events::CORE_STATUS,
                    dto::CoreStatusDTO {
                        stage: "network-degraded".into(),
                        generation,
                        degraded: true,
                        error: Some("网络准备超时，本地内容可继续使用；稍后会自动重试".into()),
                    },
                );
                events::notice(
                    &app,
                    "warning",
                    "网络准备较慢，本地内容可继续使用，稍后自动重试",
                );
                let retry_app = app.clone();
                let retry_core = Arc::clone(&core);
                tauri::async_runtime::spawn(async move {
                    resume_network_phase(retry_app, retry_core).await;
                });
            }
        }
        // 凭据已在本地阶段装载；无论网络成功或降级都允许调度，避免队列永久停摆。
        if let Err(error) = core.manager.enable_scheduling().await {
            tracing::warn!(target: "shell::app", error = %error, "启用传输调度失败");
        }
        if core.settings().check_updates_on_startup {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::update::check_in_background(handle).await {
                    Ok(check) => tracing::info!(
                        target: "shell::update",
                        update_available = check.update_available,
                        version = check.release.as_ref().map(|release| release.version.clone()),
                        "启动时检查更新完成"
                    ),
                    Err(err) => tracing::warn!(
                        target: "shell::update",
                        error = %err,
                        "启动时检查更新失败"
                    ),
                }
            });
        }
    });
}

/// 降级后的后台重试：成功才应用账号状态，失败只记录（design D5/5.4）。
async fn resume_network_phase(app: AppHandle, core: Arc<Core>) {
    let generation = core.begin_network_attempt();
    let client = core.client();
    let storage = Arc::clone(&core.storage);
    let prepared = tokio::time::timeout(Duration::from_secs(15), async move {
        client.activate_buvid().await;
        let account_state = account::restore(&client, &storage).await;
        let _ = account::persist_device(&client, &storage).await;
        account_state
    })
    .await;
    if core.network_generation() != generation {
        return;
    }
    if let Ok(account_state) = prepared {
        core.set_account(account_state.clone());
        events::emit(&app, events::ACCOUNT_CHANGED, dto::account_dto(&account_state));
        events::emit(
            &app,
            events::CORE_STATUS,
            dto::CoreStatusDTO {
                stage: "network-ready".into(),
                generation,
                degraded: false,
                error: None,
            },
        );
    }
}

/// 本地阶段：只读路径/存储/凭据与设置并恢复队列，不等待任何外网请求（design D5/5.2）。
async fn init_local_core() -> BiliResult<Arc<Core>> {
    let paths = AppPaths::standard()?;
    // 日志尽早初始化（core 的 tracing subscriber，写 <data>/logs/）
    let _ = crate::logging::init_tracing(&paths);
    let storage = Arc::new(Storage::open(paths.clone())?);
    let settings = storage.load_settings();
    let creds = storage.load_credentials();
    tracing::info!(
        target: "shell::app",
        settings_file = %paths.settings_file().display(),
        migration = ?storage.migration_report(),
        "storage ready"
    );

    // 只做离线构造：cookie/device 来自本地存储，网络激活放到网络阶段。
    let client = Arc::new(BiliClient::new(settings.proxy.as_deref(), creds.stored())?);
    client.apply_device(&creds.device);

    let mut config = TaskManagerConfig::new(client.clone(), storage.clone(), settings.clone());
    // 启动分阶段：先把队列恢复为 Queued 但暂不调度，网络准备结束后再启用。
    config.startup_defer_schedule = true;
    let manager = TaskManager::start(config).await?;

    Ok(Arc::new(Core {
        paths: storage.paths().clone(),
        storage,
        client: RwLock::new(client),
        manager,
        settings: RwLock::new(settings),
        settings_commit: tokio::sync::Mutex::new(()),
        settings_revision: AtomicU64::new(0),
        network_generation: AtomicU64::new(0),
        account: RwLock::new(AccountState::default()),
        login: Mutex::new(None),
    }))
}

/// watch<QueueSnapshot> → `queue://snapshot` + 菜单可用性 + Dock 徽标。
fn spawn_snapshot_bridge(app: &AppHandle, core: &Arc<Core>) {
    let app = app.clone();
    let mut rx = core.manager.subscribe_snapshot();
    tauri::async_runtime::spawn(async move {
        loop {
            if rx.changed().await.is_err() {
                break;
            }
            let snapshot = rx.borrow_and_update().clone();
            let account = app
                .state::<ShellState>()
                .try_core()
                .map(|core| core.account())
                .unwrap_or_default();
            let settings = app
                .state::<ShellState>()
                .try_core()
                .map(|core| core.settings())
                .unwrap_or_default();
            events::emit(
                &app,
                events::QUEUE_SNAPSHOT,
                dto::snapshot_dto(&snapshot, &account, &settings),
            );
            menu::refresh(&app);
            let running = snapshot.active_count + snapshot.queued_count;
            dock::update(&app, running);
            update_taskbar_progress(&app, &snapshot);
            crate::energy::set_active(running > 0);
        }
    });
}

/// broadcast<TaskEvent> → `queue://events` + 通知 / 历史刷新。
fn spawn_event_bridge(app: &AppHandle, core: &Arc<Core>) {
    let app = app.clone();
    let core = core.clone();
    let mut rx = core.manager.subscribe_events();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Some(dto_event) = dto::task_event_dto(&event) {
                        events::emit(&app, events::QUEUE_EVENTS, dto_event);
                    }
                    match &event {
                        TaskEvent::Done { view } => {
                            notify(&app, &core, true, &view.title, None);
                        }
                        TaskEvent::Failed { view, error } => {
                            notify(&app, &core, false, &view.title, Some(error.describe()));
                        }
                        TaskEvent::HistoryChanged => publish_history(&app),
                        _ => {}
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(target: "shell::events", skipped, "事件通道落后，已跳过");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn notify(app: &AppHandle, core: &Arc<Core>, success: bool, title: &str, error: Option<String>) {
    if !core.settings().notify_on_done {
        return;
    }
    let (heading, body) = if success {
        ("下载完成", title.to_string())
    } else {
        (
            "下载失败",
            format!("{title}\n{}", error.unwrap_or_default()),
        )
    };
    // Windows 开发/免安装运行没有 AUMID 快捷方式，系统通知不可靠（显示为 PowerShell）：
    // 按规格不宣称通知可用，改用应用内提示。
    if !crate::platform::notifications_supported() {
        events::notice(
            app,
            if success { "success" } else { "error" },
            format!("{heading}：{body}"),
        );
        tracing::info!(target: "shell::notify", success, title, "系统通知不可用，改用应用内提示");
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    match app
        .notification()
        .builder()
        .title(heading)
        .body(body)
        .show()
    {
        Ok(()) => tracing::info!(target: "shell::notify", success, title, "系统通知已发送"),
        Err(err) => tracing::warn!(target: "shell::notify", %err, "系统通知发送失败"),
    }
}

/// 任务栏进度条：Windows 用 Tauri 内置 `set_progress_bar` 展示整体下载进度。
fn update_taskbar_progress(app: &AppHandle, snapshot: &QueueSnapshot) {
    #[cfg(target_os = "windows")]
    {
        use tauri::window::{ProgressBarState, ProgressBarStatus};
        let Some(win) = app.get_webview_window("main") else {
            return;
        };
        let (done, total) = snapshot
            .tasks
            .iter()
            .filter(|task| matches!(task.phase, TaskPhase::Downloading | TaskPhase::Merging))
            .fold((0u64, 0u64), |(done, total), task| {
                (
                    done.saturating_add(task.progress.downloaded),
                    total.saturating_add(task.progress.total),
                )
            });
        let running = snapshot.active_count + snapshot.queued_count;
        let state = if total > 0 {
            ProgressBarState {
                status: Some(ProgressBarStatus::Normal),
                progress: Some((done.saturating_mul(100) / total).min(100)),
            }
        } else if running > 0 {
            ProgressBarState {
                status: Some(ProgressBarStatus::Indeterminate),
                progress: None,
            }
        } else {
            ProgressBarState {
                status: Some(ProgressBarStatus::None),
                progress: None,
            }
        };
        if let Err(err) = win.set_progress_bar(state) {
            tracing::debug!(target: "shell::window", error = %err, "任务栏进度更新失败");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, snapshot);
    }
}

// ---------------------------------------------------------------------------
// 公开：登录态 / 账号（命令与菜单共用）
// ---------------------------------------------------------------------------

/// 登录轮询成功后广播账号变化。
pub fn publish_account(app: &AppHandle, core: &Arc<Core>) {
    events::emit(
        app,
        events::ACCOUNT_CHANGED,
        dto::account_dto(&core.account()),
    );
    menu::refresh(app);
}

pub fn publish_settings(app: &AppHandle, core: &Arc<Core>) {
    let mut settings = dto::settings_dto(&core.settings());
    settings.revision = Some(core.settings_revision());
    events::emit(app, events::SETTINGS_CHANGED, settings);
}

/// 历史变化：payload 为完整 DTO 数组（web bootstrap 直接 setEntries）。
pub fn publish_history(app: &AppHandle) {
    let entries = app
        .state::<ShellState>()
        .try_core()
        .map(|core| {
            let query = HistoryQuery {
                status: None,
                keyword: None,
                limit: 500,
                offset: 0,
            };
            core.storage
                .db()
                .list_history(&query)
                .map(|rows| rows.into_iter().map(dto::history_dto).collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    events::emit(app, events::HISTORY_CHANGED, entries);
}

/// 监听剪贴板（偏好设置开启时）：检测到 B 站链接就发一条可操作提示，不自动下载。
fn spawn_clipboard_watch(app: &AppHandle) {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            let Ok(core) = app.state::<ShellState>().core().await else {
                continue;
            };
            if !core.settings().watch_clipboard {
                continue;
            }
            let Ok(text) = app.clipboard().read_text() else {
                continue;
            };
            let text = text.trim();
            if text.is_empty() || text.len() > 2000 || text == last {
                continue;
            }
            let lower = text.to_ascii_lowercase();
            if !(lower.contains("bilibili.com")
                || lower.contains("b23.tv")
                || lower.contains("bilibili.tv")
                || lower.contains("bili2233.cn"))
            {
                continue;
            }
            last = text.to_string();
            events::notice_coded(
                &app,
                "info",
                "clipboard-link",
                "剪贴板中检测到 B 站链接，可一键添加",
            );
        }
    });
}
