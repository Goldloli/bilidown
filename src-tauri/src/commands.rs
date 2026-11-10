//! Tauri commands（tasks 4.2 / 4.5）：前端 `invoke` 的唯一入口。
//!
//! 返回类型为 `web/src/lib/contract.ts` 的 camelCase DTO；错误统一为 `AppErrorDTO`。

use bilidown_core::api::account::{self, AccountState};
use bilidown_core::api::types::{BiliId, Content, PlayUrl, TaskSpec};
use bilidown_core::api::{audio, parse, video};
use bilidown_core::media::ffmpeg::Ffmpeg;
use bilidown_core::storage::HistoryQuery;
use bilidown_core::{BiliError, HistoryRecord, QueueSnapshot};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

use crate::dto::*;
use crate::state::{self, Core, ShellState};
#[cfg(debug_assertions)]
use crate::{dock, events};
use crate::{menu, window};

// ---------------------------------------------------------------------------
// 内部助手
// ---------------------------------------------------------------------------

async fn core_of(state: &State<'_, ShellState>) -> CommandResult<Arc<Core>> {
    state.core().await.map_err(AppErrorDTO::from)
}

fn snapshot_of(core: &Core) -> QueueSnapshot {
    core.snapshot()
}

/// 按内容类型解析 Content（video/bangumi/cheese 走 video::resolve_content；
/// audio 用各自模块映射到统一 Content）。
async fn resolve_content(core: &Core, id: &BiliId) -> bilidown_core::BiliResult<Content> {
    let client = core.client();
    match id {
        BiliId::Audio { song_id } => {
            let song = audio::resolve_song(&client, song_id).await?;
            Ok(Content {
                id: id.clone(),
                title: song.title.clone(),
                cover_url: None,
                upper: song.upper,
                entries: vec![bilidown_core::api::types::Entry {
                    cid: song.song_id,
                    title: song.title,
                    index: 0,
                    ep_id: None,
                    season_id: None,
                    aid: None,
                    duration_secs: None,
                    qualities: Vec::new(),
                }],
                duration_secs: None,
                pub_time: None,
            })
        }
        BiliId::AudioList { menu_id } => audio::resolve_menu(&client, menu_id).await,
        _ => video::resolve_content(&client, id).await,
    }
}

async fn play_for(core: &Core, id: &BiliId, content: &Content) -> Option<PlayUrl> {
    let entry = content.entries.first()?;
    let logged_in = core.account().logged_in;
    video::play_url(&core.client(), id, entry, logged_in)
        .await
        .ok()
}

fn history_query(filter: Option<HistoryFilterDTO>) -> HistoryQuery {
    let filter = filter.unwrap_or_default();
    let status = filter.status.as_deref().map(|status| match status {
        "completed" => "done".to_string(),
        other => other.to_string(),
    });
    HistoryQuery {
        status,
        keyword: filter.keyword.filter(|keyword| !keyword.trim().is_empty()),
        limit: 500,
        offset: 0,
    }
}

fn history_entries(core: &Core, query: &HistoryQuery) -> CommandResult<Vec<HistoryEntryDTO>> {
    let rows: Vec<HistoryRecord> = core
        .storage
        .db()
        .list_history(query)
        .map_err(AppErrorDTO::from)?;
    // 一次历史查询内共享文件状态探测，并命中跨批次展示缓存（design D10/7.1、D9/7.2）。
    let mut probe = FileProbe::with_cache(std::sync::Arc::clone(&core.file_status));
    Ok(rows
        .into_iter()
        .map(|row| history_dto_with_probe(row, &mut probe))
        .collect())
}

// ---------------------------------------------------------------------------
// 应用
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: State<'_, ShellState>) -> AppInfoDTO {
    let info = app.package_info();
    let core = state.try_core();
    let (data_dir, log_dir) = match &core {
        Some(core) => (core.paths.root().to_path_buf(), core.paths.logs_dir()),
        None => (state.data_dir(), state.log_dir()),
    };
    let ffmpeg = core
        .as_ref()
        .and_then(|core| Ffmpeg::detect(core.settings().ffmpeg_path.as_deref()));
    AppInfoDTO {
        product_name: info.name.clone(),
        version: info.version.to_string(),
        tauri_version: tauri::VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        identifier: app.config().identifier.clone(),
        data_dir: data_dir.to_string_lossy().into_owned(),
        log_dir: log_dir.to_string_lossy().into_owned(),
        ffmpeg_found: ffmpeg.is_some(),
        ffmpeg_path: ffmpeg.map(|f| f.path().to_string_lossy().into_owned()),
        glass_supported: crate::platform::glass_supported(),
        glass_unavailable_reason: crate::platform::glass_unavailable_reason().map(str::to_string),
        notifications_supported: crate::platform::notifications_supported(),
        just_updated: std::env::args().any(|arg| arg == "--updated"),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
static PREFERENCES_OPEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn open_preferences(app: AppHandle) -> CommandResult<()> {
    // 入口可被连续触发，序列化首次初始化，避免另一次调用提前显示尚未定位的窗口。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let _open_guard = PREFERENCES_OPEN_LOCK.lock().await;
    if let Some(win) = app.get_webview_window("preferences") {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return crate::preferences_window::present(&win, false)
            .await
            .map_err(|err| AppErrorDTO::new("unknown", format!("设置窗口定位失败: {err}")));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = win.show();
            let _ = win.set_focus();
            return Ok(());
        }
    }
    let builder = tauri::WebviewWindowBuilder::new(
        &app,
        "preferences",
        tauri::WebviewUrl::App(PathBuf::from("index.html#/preferences")),
    )
    .title("设置")
    .inner_size(880.0, 600.0)
    .min_inner_size(720.0, 480.0);
    // macOS 保留 Liquid Glass；Windows 使用不透明的原生圆角窗口。
    #[cfg(target_os = "macos")]
    let builder = builder
        .transparent(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(18.0, 22.0))
        .effects(
            tauri::window::EffectsBuilder::new()
                .effect(tauri::window::Effect::LiquidGlassRegular)
                .effect(tauri::window::Effect::UnderWindowBackground)
                .state(tauri::window::EffectState::Active)
                .build(),
        )
        .visible(false);
    #[cfg(target_os = "windows")]
    let builder = builder
        .decorations(false)
        .shadow(false)
        .transparent(false)
        .visible(false);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let builder = builder.decorations(false);
    // Windows 创建 WebView 必须从异步 command 派发，避免 UI 线程等待自身。
    let win = builder.build().map_err(|e| {
        tracing::warn!(target: "shell::window", error = %e, "打开偏好设置窗口失败");
        AppErrorDTO::new("unknown", format!("打开偏好设置窗口失败: {e}"))
    })?;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        if let Err(err) = crate::preferences_window::present(&win, true).await {
            // 不保留未完成初始化的隐藏窗口，下一次打开可以重新创建。
            let _ = win.destroy();
            return Err(AppErrorDTO::new(
                "unknown",
                format!("设置窗口初始化失败: {err}"),
            ));
        }
    }
    tracing::info!(target: "shell::window", "偏好设置窗口已创建");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = win.set_focus();
    Ok(())
}

/// 主动退出：先保存（可重试），保存失败时返回错误、不退出，由前端提供「重试保存 / 仍然退出」。
#[tauri::command]
pub async fn quit_app(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    if let Some(core) = state.try_core() {
        tracing::info!(target: "shell::app", "退出：保存任务状态");
        core.manager
            .prepare_shutdown()
            .await
            .map_err(AppErrorDTO::from)?;
    }
    state.confirm_quit();
    if let Some(core) = state.try_core() {
        let _ = core.manager.finalize_shutdown().await;
    }
    tracing::info!(target: "shell::app", "用户确认退出");
    app.exit(0);
    Ok(())
}

/// 「仍然退出」：用户明确放弃未保存状态后强制退出，不再要求保存成功。
#[tauri::command]
pub async fn force_quit_app(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    state.confirm_quit();
    if let Some(core) = state.try_core() {
        let _ = core.manager.finalize_shutdown().await;
    }
    tracing::warn!(target: "shell::app", "用户选择放弃未保存状态并退出");
    app.exit(0);
    Ok(())
}

/// 本地初始化失败后重试（design D5/5.4）：仅在 core 尚未就绪时重新拉起初始化。
#[tauri::command]
pub async fn retry_init(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    if state.try_core().is_some() {
        return Ok(());
    }
    state::spawn_init(app);
    Ok(())
}

/// 网络阶段重试（design D5/5.2/5.4）：本地已就绪时重新尝试设备激活与账号恢复。
#[tauri::command]
pub async fn retry_network(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    let core = core_of(&state).await?;
    state::spawn_network_phase(app, core);
    Ok(())
}

#[tauri::command]
pub fn get_diagnostics(state: State<'_, ShellState>) -> String {
    let info = state
        .try_core()
        .map(|core| {
            let snapshot = core.snapshot();
            let settings = core.settings();
            let account = core.account();
            format!(
                "queue: active={} queued={} paused={} done={} failed={}\nsettings: max_concurrent={} quality={:?} codec={:?} ffmpeg={:?}\naccount: logged_in={} ({}), expired={}\n",
                snapshot.active_count,
                snapshot.queued_count,
                snapshot.paused_count,
                snapshot.done_count,
                snapshot.failed_count,
                settings.max_concurrent,
                settings.default_quality,
                settings.default_codec,
                settings.ffmpeg_path,
                account.logged_in,
                account.user.uname,
                account.expired,
            )
        })
        .unwrap_or_else(|| "core: 未就绪\n".to_string());
    format!(
        "bilidown {}\nTauri {}\nos: {} {}\ndata: {}\nlogs: {}\n{}",
        env!("CARGO_PKG_VERSION"),
        tauri::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        state.data_dir().display(),
        state.log_dir().display(),
        info
    )
}

// ---------------------------------------------------------------------------
// 解析 / 队列
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn parse_input(
    state: State<'_, ShellState>,
    input: String,
) -> CommandResult<ParseResultDTO> {
    let core = core_of(&state).await?;
    let raw = input.trim().to_string();
    if raw.is_empty() {
        return Err(AppErrorDTO::new("parse", "输入为空"));
    }
    let parsed = parse::parse_input(&core.client(), &raw)
        .await
        .map_err(AppErrorDTO::from)?;
    let content = resolve_content(&core, &parsed.id)
        .await
        .map_err(AppErrorDTO::from)?;
    let play = if matches!(
        parsed.id,
        BiliId::Video { .. } | BiliId::Bangumi { .. } | BiliId::Cheese { .. }
    ) {
        play_for(&core, &parsed.id, &content).await
    } else {
        None
    };
    Ok(parse_result_dto(
        &raw,
        &parsed,
        &content,
        parsed.page_hint,
        play.as_ref(),
    ))
}

#[tauri::command]
pub async fn add_tasks(
    state: State<'_, ShellState>,
    input: String,
    options: AddOptionsDTO,
) -> CommandResult<AddTasksResultDTO> {
    let core = core_of(&state).await?;
    if input.trim().is_empty() {
        return Err(AppErrorDTO::new("parse", "没有可入队的链接"));
    }
    validate_add_options(&options)
        .map_err(|message| AppErrorDTO::new("invalid-argument", message))?;
    let (parsed, rejected) = parse::parse_many(core.client().http(), &input).await;
    if parsed.is_empty() {
        return Ok(AddTasksResultDTO {
            task_ids: Vec::new(),
            rejected: rejected
                .into_iter()
                .map(|(line, reason)| RejectedInputDTO { line, reason })
                .collect(),
        });
    }
    let single_input = parsed.len() == 1;
    let specs: Vec<TaskSpec> = parsed
        .iter()
        .map(|item| {
            let mut spec = TaskSpec::new(item.id.clone());
            spec.page_hint = item.page_hint;
            apply_add_options(&mut spec, &options, single_input);
            spec
        })
        .collect();
    let task_ids = core
        .manager
        .enqueue(specs)
        .await
        .map_err(AppErrorDTO::from)?;
    tracing::info!(
        target: "shell::queue",
        tasks = task_ids.len(),
        rejected = rejected.len(),
        "任务入队"
    );
    Ok(AddTasksResultDTO {
        task_ids,
        rejected: rejected
            .into_iter()
            .map(|(line, reason)| RejectedInputDTO { line, reason })
            .collect(),
    })
}

#[tauri::command]
pub async fn list_queue(state: State<'_, ShellState>) -> CommandResult<QueueSnapshotDTO> {
    let core = core_of(&state).await?;
    Ok(snapshot_dto(
        &snapshot_of(&core),
        &core.account(),
        &core.settings(),
        Some(&core.file_status),
    ))
}

#[tauri::command]
pub async fn pause_task(state: State<'_, ShellState>, task_id: String) -> CommandResult<()> {
    core_of(&state)
        .await?
        .manager
        .pause(&task_id)
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn resume_task(state: State<'_, ShellState>, task_id: String) -> CommandResult<()> {
    core_of(&state)
        .await?
        .manager
        .resume(&task_id)
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn cancel_task(state: State<'_, ShellState>, task_id: String) -> CommandResult<()> {
    core_of(&state)
        .await?
        .manager
        .cancel(&task_id)
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn retry_task(state: State<'_, ShellState>, task_id: String) -> CommandResult<()> {
    core_of(&state)
        .await?
        .manager
        .retry(&task_id)
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn remove_task(state: State<'_, ShellState>, task_id: String) -> CommandResult<()> {
    core_of(&state)
        .await?
        .manager
        .remove(&task_id)
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn clear_finished(
    state: State<'_, ShellState>,
) -> CommandResult<bilidown_core::task::ClearOutcome> {
    core_of(&state)
        .await?
        .manager
        .clear_finished()
        .await
        .map_err(AppErrorDTO::from)
}

/// 清空「未激活」任务：actor 内单批确定目标并统一补调度（design D2/3.5）。
#[tauri::command]
pub async fn clear_inactive(
    state: State<'_, ShellState>,
) -> CommandResult<bilidown_core::task::ClearOutcome> {
    core_of(&state)
        .await?
        .manager
        .clear_inactive()
        .await
        .map_err(AppErrorDTO::from)
}

/// 重试所有待保存动作（完成事务/历史/任务记录），返回可读错误（design D8/3.8）。
#[tauri::command]
pub async fn retry_persistence(state: State<'_, ShellState>) -> CommandResult<()> {
    core_of(&state)
        .await?
        .manager
        .retry_persistence()
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn start_all(state: State<'_, ShellState>) -> CommandResult<usize> {
    core_of(&state)
        .await?
        .manager
        .resume_all()
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn pause_all(state: State<'_, ShellState>) -> CommandResult<usize> {
    core_of(&state)
        .await?
        .manager
        .pause_all()
        .await
        .map_err(AppErrorDTO::from)
}

#[tauri::command]
pub async fn retry_all(state: State<'_, ShellState>) -> CommandResult<usize> {
    core_of(&state)
        .await?
        .manager
        .retry_all_failed()
        .await
        .map_err(AppErrorDTO::from)
}

// ---------------------------------------------------------------------------
// 账户 / 登录
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_account(state: State<'_, ShellState>) -> CommandResult<Option<AccountInfoDTO>> {
    Ok(account_dto(&core_of(&state).await?.account()))
}

#[tauri::command]
pub async fn login_start(state: State<'_, ShellState>) -> CommandResult<LoginStartDTO> {
    let core = core_of(&state).await?;
    let flow = bilidown_core::api::login::QrLogin::start(&core.client())
        .await
        .map_err(AppErrorDTO::from)?;
    core.set_login_flow(Some(flow.clone()));
    tracing::info!(target: "shell::login", "二维码已生成");
    Ok(login_start_dto(&flow))
}

#[tauri::command]
pub async fn login_poll(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> CommandResult<LoginPollDTO> {
    use bilidown_core::api::account::LoginOutcome;

    let core = core_of(&state).await?;
    let Some(flow) = core.login_flow() else {
        return Ok(LoginPollDTO {
            status: "error".into(),
            message: "尚未开始登录，请先调用 login_start".into(),
            account: None,
        });
    };
    // 轮询偶发网络抖动/代理超时：最多 3 次尝试（间隔 400ms），保留 BiliError 分类。
    let mut flow = flow;
    let mut outcome = None;
    let mut last_error: Option<BiliError> = None;
    for attempt in 0..3u32 {
        match account::login_poll(&core.client(), &core.storage, &mut flow).await {
            Ok(result) => {
                outcome = Some(result);
                break;
            }
            Err(err) => {
                let retryable = err.is_retryable();
                tracing::warn!(
                    target: "shell::login",
                    attempt,
                    retryable,
                    error = %err.describe(),
                    "登录轮询失败"
                );
                last_error = Some(err);
                if !retryable || attempt == 2 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }
    }
    let Some(outcome) = outcome else {
        core.set_login_flow(Some(flow.clone()));
        return Err(AppErrorDTO::from(
            last_error.unwrap_or_else(|| BiliError::Other("登录轮询失败".into())),
        ));
    };
    match &outcome {
        LoginOutcome::Success(account) => {
            core.set_account(account.clone());
            core.set_login_flow(None);
            state::publish_account(&app, &core);
            tracing::info!(target: "shell::login", "登录成功并已持久化凭据");
        }
        _ => core.set_login_flow(Some(flow.clone())),
    }
    Ok(login_poll_dto(outcome))
}

#[tauri::command]
pub async fn login_logout(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    let core = core_of(&state).await?;
    account::logout(&core.client(), &core.storage).map_err(AppErrorDTO::from)?;
    core.set_account(AccountState::anonymous());
    core.set_login_flow(None);
    state::publish_account(&app, &core);
    tracing::info!(target: "shell::login", "已退出登录");
    Ok(())
}

#[tauri::command]
pub async fn refresh_account(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> CommandResult<Option<AccountInfoDTO>> {
    let core = core_of(&state).await?;
    let account = account::restore(&core.client(), &core.storage).await;
    let _ = account::persist_device(&core.client(), &core.storage).await;
    core.set_account(account.clone());
    state::publish_account(&app, &core);
    Ok(account_dto(&account))
}

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_settings(state: State<'_, ShellState>) -> CommandResult<SettingsDTO> {
    let core = core_of(&state).await?;
    let mut settings = settings_dto(&core.settings());
    settings.revision = Some(core.settings_revision());
    Ok(settings)
}

#[tauri::command]
pub async fn set_settings(
    app: AppHandle,
    state: State<'_, ShellState>,
    settings: SettingsDTO,
) -> CommandResult<SettingsDTO> {
    let core = core_of(&state).await?;
    // 兼容入口：完整替换仍走同一提交协调器（design D3）。
    let previous = core.settings();
    let applied = settings_from_dto(&settings, &previous);
    let revision = core
        .update_settings(|_| Ok(applied.clone()))
        .await
        .map_err(AppErrorDTO::from)?;
    state::publish_settings(&app, &core);
    tracing::info!(
        target: "shell::settings",
        revision,
        max_concurrent = applied.max_concurrent,
        "设置已保存并热更新"
    );
    let mut dto = settings_dto(&applied);
    dto.revision = Some(revision);
    Ok(dto)
}

/// 增量提交设置（design D3）：省略字段保持不变、null 清空可空字段；返回新修订号。
#[tauri::command]
pub async fn patch_settings(
    app: AppHandle,
    state: State<'_, ShellState>,
    patch: SettingsPatchDTO,
) -> CommandResult<SettingsDTO> {
    let core = core_of(&state).await?;
    let revision = core
        .update_settings(|current| Ok(apply_settings_patch(&patch, current)))
        .await
        .map_err(AppErrorDTO::from)?;
    state::publish_settings(&app, &core);
    let mut dto = settings_dto(&core.settings());
    dto.revision = Some(revision);
    Ok(dto)
}

// ---------------------------------------------------------------------------
// 历史
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_history(
    state: State<'_, ShellState>,
    filter: Option<HistoryFilterDTO>,
) -> CommandResult<Vec<HistoryEntryDTO>> {
    let core = core_of(&state).await?;
    history_entries(&core, &history_query(filter))
}

#[tauri::command]
pub async fn remove_history(
    app: AppHandle,
    state: State<'_, ShellState>,
    id: String,
) -> CommandResult<()> {
    let core = core_of(&state).await?;
    core.storage
        .db()
        .delete_history(&id)
        .map_err(AppErrorDTO::from)?;
    state::publish_history(&app);
    Ok(())
}

#[tauri::command]
pub async fn clear_history(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<usize> {
    let core = core_of(&state).await?;
    let removed = core
        .storage
        .db()
        .clear_history()
        .map_err(AppErrorDTO::from)?;
    state::publish_history(&app);
    Ok(removed)
}

// ---------------------------------------------------------------------------
// 系统集成
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn pick_directory(app: AppHandle) -> CommandResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    match rx.await {
        Ok(Some(folder)) => match folder.into_path() {
            Ok(path) => Ok(Some(path.to_string_lossy().into_owned())),
            Err(_) => Ok(None),
        },
        Ok(None) => Ok(None),
        Err(_) => Ok(None),
    }
}

/// 文件缺失的可读错误：前端据此提示重新下载/重新解析（design D8/3.3）。
fn file_missing_error(path: &Path) -> AppErrorDTO {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    AppErrorDTO {
        kind: "file-missing".into(),
        message: format!("文件不存在或已被移动：{name}"),
        actions: vec!["retry-download".into()],
    }
}

fn invalidate_file_status(core: Option<&Core>, path: &str) {
    if let Some(core) = core {
        core.file_status.invalidate(path);
    }
}

#[tauri::command]
pub async fn reveal_output(
    app: AppHandle,
    state: State<'_, ShellState>,
    task_id: String,
) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let core = core_of(&state).await?;
    // 多产物任务优先定位第一个主产物；旧记录回退 output_path（design D7/3.3）。
    let path = {
        let snapshot = core.snapshot();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| AppErrorDTO::new("unknown", format!("任务不存在：{task_id}")))?;
        task.artifacts
            .first()
            .map(|artifact| artifact.path.clone())
            .or_else(|| task.output_path.clone())
            .ok_or_else(|| AppErrorDTO::new("unknown", format!("任务 {task_id} 还没有输出文件")))?
    };
    // 文件操作前做权威校验，不信任展示缓存（design D9/7.2）。
    if !PathBuf::from(&path).is_file() {
        invalidate_file_status(Some(&core), &path);
        crate::events::emit(&app, crate::events::FILES_INVALIDATED, ());
        return Err(file_missing_error(&PathBuf::from(&path)));
    }
    app.opener()
        .reveal_item_in_dir(PathBuf::from(&path))
        .map_err(|e| {
            invalidate_file_status(Some(&core), &path);
            AppErrorDTO::new(
                "unknown",
                format!("{}失败: {e}", crate::platform::REVEAL_LABEL),
            )
        })
}

#[tauri::command]
pub async fn reveal_path(
    app: AppHandle,
    state: State<'_, ShellState>,
    path: String,
) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let core = state.try_core();
    let target = PathBuf::from(&path);
    if !target.exists() {
        invalidate_file_status(core.as_deref(), &path);
        return Err(file_missing_error(&target));
    }
    app.opener()
        .reveal_item_in_dir(&target)
        .map_err(|e| {
            invalidate_file_status(core.as_deref(), &path);
            AppErrorDTO::new(
                "unknown",
                format!("{}失败: {e}", crate::platform::REVEAL_LABEL),
            )
        })
}

#[tauri::command]
pub async fn open_file(
    app: AppHandle,
    state: State<'_, ShellState>,
    path: String,
) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let core = state.try_core();
    if !PathBuf::from(&path).is_file() {
        invalidate_file_status(core.as_deref(), &path);
        return Err(file_missing_error(&PathBuf::from(&path)));
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| AppErrorDTO::new("unknown", format!("打开文件失败: {e}")))
}

#[tauri::command]
pub async fn show_logs(app: AppHandle, state: State<'_, ShellState>) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.log_dir();
    let _ = std::fs::create_dir_all(&dir);
    app.opener()
        .reveal_item_in_dir(&dir)
        .map_err(|e| AppErrorDTO::new("unknown", format!("打开日志目录失败: {e}")))
}

#[tauri::command]
pub fn get_menu_state(app: AppHandle) -> MenuStateDTO {
    menu::enabled_map(&app)
}

#[tauri::command]
pub fn request_quit_check(app: AppHandle) -> bool {
    window::request_quit_check(&app)
}

// ---------------------------------------------------------------------------
// EXTENSION（web/src/lib/contract.ts 标注 EXTENSION 的命令）
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> CommandResult<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .write_text(text)
        .map_err(|e| AppErrorDTO::new("unknown", format!("写剪贴板失败: {e}")))
}

#[tauri::command]
pub fn read_clipboard_text(app: AppHandle) -> CommandResult<String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .read_text()
        .map_err(|e| AppErrorDTO::new("unknown", format!("读剪贴板失败: {e}")))
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| AppErrorDTO::new("unknown", format!("打开链接失败: {e}")))
}

#[tauri::command]
pub async fn detect_ffmpeg(state: State<'_, ShellState>) -> CommandResult<FfmpegDetectionDTO> {
    let core = core_of(&state).await?;
    let settings = core.settings();
    Ok(crate::dto::detect_ffmpeg(settings.ffmpeg_path.as_deref()))
}

#[tauri::command]
pub async fn test_proxy(proxy: ProxyDTO) -> CommandResult<TestProxyResultDTO> {
    let url = match proxy.mode.as_str() {
        "manual" => {
            let host = proxy.host.clone().unwrap_or_default();
            if host.trim().is_empty() {
                return Ok(TestProxyResultDTO {
                    ok: false,
                    message: "代理地址为空".into(),
                });
            }
            let port = proxy.port.unwrap_or(7890);
            let auth = match (proxy.user.as_deref(), proxy.pass.as_deref()) {
                (Some(user), Some(pass)) if !user.is_empty() => format!("{user}:{pass}@"),
                (Some(user), None) if !user.is_empty() => format!("{user}@"),
                _ => String::new(),
            };
            format!("http://{auth}{host}:{port}")
        }
        _ => String::new(),
    };
    let proxy_arg = (!url.is_empty()).then(|| url.clone());
    let client = bilidown_core::api::client::build_http_client(proxy_arg.as_deref())
        .map_err(AppErrorDTO::from)?;
    let started = std::time::Instant::now();
    match client
        .get("https://api.bilibili.com/x/frontend/finger/spi")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )
        .send()
        .await
    {
        Ok(resp) => {
            let elapsed = started.elapsed().as_millis();
            Ok(TestProxyResultDTO {
                ok: resp.status().is_success(),
                message: format!("HTTP {}（{} ms）", resp.status(), elapsed),
            })
        }
        Err(err) => Ok(TestProxyResultDTO {
            ok: false,
            message: err.to_string(),
        }),
    }
}

/// 抓取封面并返回 data URL（B 站图床防盗链：带 Referer/UA + 设置代理；
/// 缓存到 `<appData>/covers/`，失败返回空串，前端回退占位图）。
#[tauri::command]
pub async fn fetch_cover(state: State<'_, ShellState>, url: String) -> CommandResult<String> {
    let core = core_of(&state).await?;
    let proxy = core.settings().proxy;
    Ok(crate::covers::fetch(proxy.as_deref(), &core.paths.covers_dir(), &url).await)
}

// ---------------------------------------------------------------------------
// 应用内更新（逻辑在 update.rs；这里统一包成 AppErrorDTO）
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_app_update_state(
    state: State<'_, crate::update::AppUpdateState>,
) -> CommandResult<crate::update::AppUpdateSnapshot> {
    crate::update::get_app_update_state(state)
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

/// 关于页静态版本信息（EXTENSION；也可用 get_app_info.version）。
#[tauri::command]
pub fn get_app_version(app: AppHandle) -> crate::update::AppVersionInfo {
    crate::update::get_app_version(app)
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    state: State<'_, crate::update::AppUpdateState>,
) -> CommandResult<crate::update::AppUpdateCheck> {
    crate::update::check_app_update(app, state)
        .await
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

#[tauri::command]
pub fn start_app_update_download(
    app: AppHandle,
    state: State<'_, crate::update::AppUpdateState>,
    version: String,
) -> CommandResult<crate::update::AppUpdateSnapshot> {
    crate::update::start_app_update_download(app, state, version)
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

#[tauri::command]
pub fn cancel_app_update_download(
    app: AppHandle,
    state: State<'_, crate::update::AppUpdateState>,
) -> CommandResult<crate::update::AppUpdateSnapshot> {
    crate::update::cancel_app_update_download(app, state)
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

#[tauri::command]
pub fn open_downloaded_update(
    app: AppHandle,
    state: State<'_, crate::update::AppUpdateState>,
) -> CommandResult<()> {
    crate::update::open_downloaded_update(app, state)
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

#[tauri::command]
pub fn open_update_file_location(
    app: AppHandle,
    state: State<'_, crate::update::AppUpdateState>,
) -> CommandResult<()> {
    crate::update::open_update_file_location(app, state)
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

#[tauri::command]
pub fn open_about_link(app: AppHandle, link: String) -> CommandResult<()> {
    crate::update::open_about_link(app, link)
        .map_err(|message| AppErrorDTO::new("unknown", message))
}

/// UGC 合集：`parse → resolve_content` 后调用；`null` 表示普通单视频（保持原行为）。
#[tauri::command]
pub async fn resolve_ugc_season(
    state: State<'_, ShellState>,
    bvid: String,
) -> CommandResult<Option<UgcSeasonDTO>> {
    let core = core_of(&state).await?;
    let bvid = bvid.trim();
    if bvid.is_empty() {
        return Err(AppErrorDTO {
            kind: "parse".into(),
            message: "bvid 为空".into(),
            actions: vec!["retry".into()],
        });
    }
    let season = bilidown_core::api::video::resolve_ugc_season(&core.client(), bvid).await?;
    Ok(season.as_ref().map(ugc_season_dto))
}

// ---------------------------------------------------------------------------
// dev 辅助命令（仅 debug 构建注册；release 包不包含）
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_report(report: String) {
    tracing::info!(target: "shell::dev", "{report}");
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_menu_state(app: AppHandle) -> MenuStateDTO {
    menu::enabled_map(&app)
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_menu_dispatch(app: AppHandle, id: String) {
    menu::dispatch(&app, &id);
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_notify(app: AppHandle, title: String, body: String) -> CommandResult<()> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| AppErrorDTO::new("unknown", format!("发送通知失败: {e}")))
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_set_badge(app: AppHandle, count: Option<i64>) {
    let count = count.and_then(|c| u32::try_from(c).ok());
    dock::set_badge(&app, count);
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_get_badge(app: AppHandle) -> CommandResult<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(dock::get_badge());
    })
    .map_err(|e| AppErrorDTO::new("unknown", format!("主线程调用失败: {e}")))?;
    Ok(rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap_or(None))
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_emit_notice(app: AppHandle, level: String, message: String) {
    events::notice(&app, &level, message);
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_sample_output(state: State<'_, ShellState>) -> CommandResult<String> {
    let dir = state.data_dir().join("tmp");
    std::fs::create_dir_all(&dir).map_err(AppErrorDTO::from)?;
    let path = dir.join("dev-sample.txt");
    std::fs::write(&path, b"bilidown shell dev sample output\n").map_err(AppErrorDTO::from)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_stat_file(path: String) -> serde_json::Value {
    match std::fs::metadata(&path) {
        Ok(meta) => serde_json::json!({ "exists": meta.is_file(), "size": meta.len() }),
        Err(_) => serde_json::json!({ "exists": false, "size": 0 }),
    }
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_close_window(app: AppHandle) {
    window::close_main(&app);
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_show_window(app: AppHandle) {
    window::show_main(&app);
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_window_visible(app: AppHandle) -> bool {
    window::main_visible(&app)
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_request_quit(app: AppHandle) -> bool {
    let prevented = window::request_quit_check(&app);
    if !prevented {
        events::notice(&app, "info", "当前无活跃任务，可退出");
    }
    prevented
}
