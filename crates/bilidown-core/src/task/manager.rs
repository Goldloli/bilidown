//! TaskManager：actor 监督任务（design D2/D3/D4）。
//!
//! - 命令通道（mpsc）+ oneshot 应答：所有变更在单一任务内串行执行；
//! - watch 快照（≤10Hz 全量）+ broadcast 事件（立即推送）；
//! - UUID 寻址；Semaphore 控制并发；进度持久化节流（3s）+ 阶段变化即时落盘；
//! - 重启恢复：非终态任务启动时统一置为 Paused（Queued 保持 Queued）。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::api::client::{BiliClient, download_client};
use crate::api::error::{BiliError, BiliResult, ErrorKind};
use crate::api::types::{DownloadStage, Progress, TaskSpec};
use crate::media::{ffmpeg::Ffmpeg, output};
use crate::storage::{Db, HistoryRecord, Settings, Storage, TaskRecord};

use super::event::TaskEvent;
use super::pipeline::{DownloadPipeline, TaskReporter, TaskRunCtx, TaskRunner};
use super::state::{
    ClearOutcome, DownloadArtifact, QueueSnapshot, TASK_METADATA_VERSION, TaskId, TaskMeta,
    TaskMetadata, TaskOutcome, TaskPhase, TaskResume, TaskView, WorkerEvent,
    completed_entry_indices, now_ms,
};

/// 并发上限（设置允许 1–8）。
pub const MAX_CONCURRENT: usize = 8;
/// 快照推送间隔（≤10Hz）。
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(100);
/// 进度持久化节流。
const PROGRESS_FLUSH_INTERVAL: Duration = Duration::from_secs(3);
/// 事件广播容量。
const EVENT_CAPACITY: usize = 512;
/// 退出时等待 worker 的上限。
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);
/// 取消/移除后等待 worker 自行结束的上限；超时后终止并确认结束再清理（design D2）。
const CANCEL_CONVERGE: Duration = Duration::from_secs(3);
/// 任务临时目录清理失败后的重试间隔与次数上限（design D1）。
const CLEANUP_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const CLEANUP_MAX_ATTEMPTS: u32 = 3;
/// 待重试清理队列上限，避免异常文件系统下无限增长。
const PENDING_CLEANUP_CAP: usize = 32;

/// manager 构造配置。
pub struct TaskManagerConfig {
    pub client: Arc<BiliClient>,
    pub storage: Arc<Storage>,
    pub settings: Settings,
    pub ffmpeg: Option<Ffmpeg>,
    /// 测试注入的管线；None = 真实 [`DownloadPipeline`]。
    pub runner: Option<Arc<dyn TaskRunner>>,
    /// 测试注入的下载 client；None = 按设置代理构造。
    pub download_http: Option<reqwest::Client>,
    /// 测试注入：下一次产物 checkpoint 写库失败。
    pub artifact_store_fail_once: Option<Arc<AtomicBool>>,
    /// 测试注入：下一次批量入队写库失败（design D8/3.4 故障注入）。
    pub enqueue_store_fail_once: Option<Arc<AtomicBool>>,
    /// 启动分阶段（design D5/5.2）：true 时先恢复队列但不调度，等网络准备结束再
    /// `enable_scheduling()`，避免凭据/代理未就绪就发起传输。
    pub startup_defer_schedule: bool,
}

impl TaskManagerConfig {
    pub fn new(client: Arc<BiliClient>, storage: Arc<Storage>, settings: Settings) -> Self {
        let ffmpeg = Ffmpeg::detect(settings.ffmpeg_path.as_deref());
        Self {
            client,
            storage,
            settings,
            ffmpeg,
            runner: None,
            download_http: None,
            artifact_store_fail_once: None,
            enqueue_store_fail_once: None,
            startup_defer_schedule: false,
        }
    }
}

/// 对外句柄（可 Clone，命令通道多消费者安全）。
#[derive(Clone)]
pub struct TaskManager {
    cmd: mpsc::Sender<ManagerCommand>,
    snapshot: watch::Receiver<QueueSnapshot>,
    event_tx: broadcast::Sender<TaskEvent>,
    supervisor: Arc<tokio::task::JoinHandle<()>>,
}

impl TaskManager {
    /// 启动 manager：加载并恢复上次队列 → 启动监督任务。
    pub async fn start(config: TaskManagerConfig) -> BiliResult<Self> {
        let storage = Arc::clone(&config.storage);
        let db = Arc::clone(storage.db());
        let records = spawn_blocking_db(move || db.load_tasks()).await?;
        let mut tasks: Vec<TaskView> = records.iter().filter_map(view_from_record).collect();
        let mut recovered: Vec<TaskRecord> = Vec::new();
        for (idx, view) in tasks.iter_mut().enumerate() {
            if !view.artifacts.is_empty() {
                view.progress.entries_done = completed_entry_indices(view).len() as u32;
            }
            if !view.phase.is_terminal() {
                view.restored = true;
                if view.phase.is_active() {
                    view.phase = TaskPhase::Paused;
                    view.speed_bps = 0.0;
                    // 保留 progress.stage：恢复后合并中断的任务可直接重合并，下载中断按 .part 续传
                }
                view.touch();
                recovered.push(task_record(view, idx as i64));
            }
        }
        if !recovered.is_empty() {
            let db = Arc::clone(storage.db());
            spawn_blocking_db(move || {
                for rec in &recovered {
                    db.upsert_task(rec)?;
                }
                Ok(())
            })
            .await?;
        }
        let download_http = match config.download_http {
            Some(c) => c,
            None => download_client(config.settings.proxy.as_deref())?,
        };
        let cleanup_dir = config.settings.effective_download_dir();
        if let Ok(Err(error)) =
            tokio::task::spawn_blocking(move || output::cleanup_stale_publish_files(&cleanup_dir))
                .await
        {
            tracing::warn!(error = %error, "清理发布暂存文件失败");
        }
        let runner: Arc<dyn TaskRunner> =
            config.runner.unwrap_or_else(|| Arc::new(DownloadPipeline));
        let (snapshot_tx, snapshot_rx) = watch::channel(QueueSnapshot::default());
        let (event_tx, _) = broadcast::channel(EVENT_CAPACITY);
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (worker_tx, worker_rx) = mpsc::unbounded_channel();
        let settings = config.settings;
        let recovered_tasks: HashMap<TaskId, TaskView> =
            tasks.into_iter().map(|v| (v.id.clone(), v)).collect();
        let inner = Inner {
            client: config.client,
            storage,
            settings: settings.clone(),
            ffmpeg: config.ffmpeg,
            runner,
            download_http,
            tasks: recovered_tasks,
            order: Vec::new(),
            controls: HashMap::new(),
            workers: HashMap::new(),
            temp_dirs: HashMap::new(),
            tails: HashMap::new(),
            pending_cleanups: Vec::new(),
            last_cleanup_retry: Instant::now(),
            pending_completion: HashMap::new(),
            pending_history: HashMap::new(),
            flush_backoff: PROGRESS_FLUSH_INTERVAL,
            sem: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            reduction_permits: Vec::new(),
            dirty: HashSet::new(),
            auto_retried: HashSet::new(),
            last_progress_flush: Instant::now(),
            last_publish: Instant::now(),
            seq: 0,
            snapshot_tx,
            snapshot_dirty: false,
            event_tx: event_tx.clone(),
            worker_tx,
            artifact_store_fail_once: config.artifact_store_fail_once,
            enqueue_store_fail_once: config.enqueue_store_fail_once,
            schedule_enabled: !config.startup_defer_schedule,
        };
        let mut inner = inner;
        // 恢复顺序与已加载顺序一致
        inner.order = inner.tasks.keys().cloned().collect();
        inner.order.sort_by_key(|id| inner.tasks[id].created_at);
        inner.enforce_permits();
        inner.publish();
        inner.schedule();
        let supervisor = tokio::spawn(async move { inner.run(cmd_rx, worker_rx).await });
        Ok(Self {
            cmd: cmd_tx,
            snapshot: snapshot_rx,
            event_tx,
            supervisor: Arc::new(supervisor),
        })
    }

    pub fn subscribe_snapshot(&self) -> watch::Receiver<QueueSnapshot> {
        self.snapshot.clone()
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<TaskEvent> {
        self.event_tx.subscribe()
    }

    async fn call<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<BiliResult<T>>) -> ManagerCommand,
    ) -> BiliResult<T> {
        let (tx, rx) = oneshot::channel();
        self.cmd
            .send(make(tx))
            .await
            .map_err(|_| BiliError::Other("task manager stopped".into()))?;
        rx.await
            .map_err(|_| BiliError::Other("task manager dropped reply".into()))?
    }

    /// 入队一批任务，返回按序创建的 task id。
    pub async fn enqueue(&self, specs: Vec<TaskSpec>) -> BiliResult<Vec<TaskId>> {
        self.call(|reply| ManagerCommand::Enqueue { specs, reply })
            .await
    }

    pub async fn pause(&self, id: &str) -> BiliResult<()> {
        let id = id.to_string();
        self.call(move |reply| ManagerCommand::Pause { id, reply })
            .await
    }

    pub async fn resume(&self, id: &str) -> BiliResult<()> {
        let id = id.to_string();
        self.call(move |reply| ManagerCommand::Resume { id, reply })
            .await
    }

    pub async fn cancel(&self, id: &str) -> BiliResult<()> {
        let id = id.to_string();
        self.call(move |reply| ManagerCommand::Cancel { id, reply })
            .await
    }

    pub async fn retry(&self, id: &str) -> BiliResult<()> {
        let id = id.to_string();
        self.call(move |reply| ManagerCommand::Retry { id, reply })
            .await
    }

    pub async fn remove(&self, id: &str) -> BiliResult<()> {
        let id = id.to_string();
        self.call(move |reply| ManagerCommand::Remove { id, reply })
            .await
    }

    /// 清空已完成/失败/取消的任务（历史保留），返回移除数量与失败 id。
    pub async fn clear_finished(&self) -> BiliResult<ClearOutcome> {
        self.call(|reply| ManagerCommand::ClearFinished { reply })
            .await
    }

    /// 清空「未激活」任务（排队/暂停/已取消），保留运行/完成/失败与历史（design D2）。
    pub async fn clear_inactive(&self) -> BiliResult<ClearOutcome> {
        self.call(|reply| ManagerCommand::ClearInactive { reply })
            .await
    }

    pub async fn pause_all(&self) -> BiliResult<usize> {
        self.call(|reply| ManagerCommand::PauseAll { reply }).await
    }

    pub async fn resume_all(&self) -> BiliResult<usize> {
        self.call(|reply| ManagerCommand::ResumeAll { reply }).await
    }

    pub async fn retry_all_failed(&self) -> BiliResult<usize> {
        self.call(|reply| ManagerCommand::RetryAllFailed { reply })
            .await
    }

    /// 立即把脏进度落盘（退出前/测试用）。
    pub async fn flush(&self) -> BiliResult<()> {
        self.call(|reply| ManagerCommand::Flush { reply }).await
    }

    /// 应用设置（并发、下载目录、ffmpeg、代理等）。
    pub async fn set_settings(&self, settings: Settings) -> BiliResult<()> {
        self.call(move |reply| ManagerCommand::SetSettings { settings, reply })
            .await
    }

    /// 替换 B 站客户端（代理变更后由壳重建；cookie 变更无需调用）。
    pub async fn set_client(&self, client: Arc<BiliClient>) -> BiliResult<()> {
        self.call(move |reply| ManagerCommand::SetClient { client, reply })
            .await
    }

    /// 重试所有待保存动作（完成事务、历史与任务记录），失败时保留可重试状态（design D8/3.8）。
    pub async fn retry_persistence(&self) -> BiliResult<()> {
        self.call(|reply| ManagerCommand::RetryPersistence { reply })
            .await
    }

    /// 启用传输调度（启动分阶段：本地就绪后由网络准备结果触发，design D5/5.2）。
    pub async fn enable_scheduling(&self) -> BiliResult<()> {
        self.call(|reply| ManagerCommand::EnableScheduling { reply })
            .await
    }

    /// 退出准备：暂停进行中任务并落盘；失败返回错误且可再次调用重试（design D8/3.9）。
    pub async fn prepare_shutdown(&self) -> BiliResult<()> {
        self.call(|reply| ManagerCommand::PrepareShutdown { reply })
            .await
    }

    /// 最终 shutdown：停止 worker 并退出监督任务；仅在退出准备成功或用户明确放弃后调用。
    pub async fn finalize_shutdown(&self) -> BiliResult<()> {
        self.call(|reply| ManagerCommand::Shutdown { reply })
            .await?;
        let sup = self.supervisor.clone();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !sup.is_finished() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .ok();
        Ok(())
    }

    /// 便捷退出：先准备（保存），再最终退出；保存失败仍会退出并返回错误。
    pub async fn shutdown(&self) -> BiliResult<()> {
        let prepared = self.prepare_shutdown().await;
        let finalized = self.finalize_shutdown().await;
        prepared?;
        finalized
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        // 避免测试/壳退出后监督任务与 worker 泄漏
        self.supervisor.abort();
    }
}

enum ManagerCommand {
    Enqueue {
        specs: Vec<TaskSpec>,
        reply: oneshot::Sender<BiliResult<Vec<TaskId>>>,
    },
    Pause {
        id: TaskId,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    Resume {
        id: TaskId,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    Cancel {
        id: TaskId,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    Retry {
        id: TaskId,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    Remove {
        id: TaskId,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    ClearFinished {
        reply: oneshot::Sender<BiliResult<ClearOutcome>>,
    },
    /// 清空未激活（排队/暂停/已取消）任务；actor 内单批确定目标并统一补调度（design D2）。
    ClearInactive {
        reply: oneshot::Sender<BiliResult<ClearOutcome>>,
    },
    PauseAll {
        reply: oneshot::Sender<BiliResult<usize>>,
    },
    ResumeAll {
        reply: oneshot::Sender<BiliResult<usize>>,
    },
    RetryAllFailed {
        reply: oneshot::Sender<BiliResult<usize>>,
    },
    Flush {
        reply: oneshot::Sender<BiliResult<()>>,
    },
    SetSettings {
        settings: Settings,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    SetClient {
        client: Arc<BiliClient>,
        reply: oneshot::Sender<BiliResult<()>>,
    },
    RetryPersistence {
        reply: oneshot::Sender<BiliResult<()>>,
    },
    /// 启动分阶段：首次网络准备结束后启用传输调度（design D5/5.2）。
    EnableScheduling {
        reply: oneshot::Sender<BiliResult<()>>,
    },
    /// 退出准备：暂停并把最终状态落盘（可重试）；失败时不退出（design D8/3.9）。
    PrepareShutdown {
        reply: oneshot::Sender<BiliResult<()>>,
    },
    Shutdown {
        reply: oneshot::Sender<BiliResult<()>>,
    },
}

#[derive(Clone)]
struct WorkerControl {
    cancel: CancellationToken,
    pause: Arc<AtomicBool>,
}

enum LoopAction {
    Continue,
    Shutdown(oneshot::Sender<BiliResult<()>>),
}

struct Inner {
    client: Arc<BiliClient>,
    storage: Arc<Storage>,
    settings: Settings,
    ffmpeg: Option<Ffmpeg>,
    runner: Arc<dyn TaskRunner>,
    download_http: reqwest::Client,
    tasks: HashMap<TaskId, TaskView>,
    order: Vec<TaskId>,
    controls: HashMap<TaskId, WorkerControl>,
    workers: HashMap<TaskId, tokio::task::JoinHandle<()>>,
    /// 每个任务实际使用的临时目录（worker 启动时确认，供隔离清理；design D1）。
    temp_dirs: HashMap<TaskId, Vec<PathBuf>>,
    /// 已移除但 worker 尚未结束的收尾记录：任务临时目录（design D2）。
    tails: HashMap<TaskId, Vec<PathBuf>>,
    /// 清理失败、待重试的任务临时目录及已尝试次数。
    pending_cleanups: Vec<(Vec<PathBuf>, u32)>,
    last_cleanup_retry: Instant,
    /// 完成收尾写库失败、待重试的「任务记录 + 历史」事务（design D8）。
    pending_completion: HashMap<TaskId, HistoryRecord>,
    /// 失败/取消历史写入失败、待重试的记录（design D8）。
    pending_history: HashMap<TaskId, HistoryRecord>,
    /// 进度落盘失败后的有界退避间隔（design D8）。
    flush_backoff: Duration,
    sem: Arc<Semaphore>,
    reduction_permits: Vec<OwnedSemaphorePermit>,
    dirty: HashSet<TaskId>,
    /// 已对哪个任务做过一次「瞬时磁盘错误」自动重试（避免死循环）。
    auto_retried: HashSet<TaskId>,
    last_progress_flush: Instant,
    /// 最近一次快照发布时刻（≤10Hz 合并，design D10/7.4）。
    last_publish: Instant,
    seq: u64,
    snapshot_tx: watch::Sender<QueueSnapshot>,
    snapshot_dirty: bool,
    event_tx: broadcast::Sender<TaskEvent>,
    worker_tx: mpsc::UnboundedSender<WorkerEvent>,
    artifact_store_fail_once: Option<Arc<AtomicBool>>,
    enqueue_store_fail_once: Option<Arc<AtomicBool>>,
    /// 调度开关（启动分阶段期间为 false，design D5/5.2）。
    schedule_enabled: bool,
}

impl Inner {
    async fn run(
        mut self,
        mut cmd_rx: mpsc::Receiver<ManagerCommand>,
        mut worker_rx: mpsc::UnboundedReceiver<WorkerEvent>,
    ) {
        let mut pending_shutdown: Option<oneshot::Sender<BiliResult<()>>> = None;
        loop {
            let wake = self.next_deadline();
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    match self.handle_command(cmd).await {
                        LoopAction::Continue => {}
                        LoopAction::Shutdown(reply) => {
                            pending_shutdown = Some(reply);
                            break;
                        }
                    }
                }
                ev = worker_rx.recv() => {
                    let Some(ev) = ev else { break };
                    self.handle_worker_event(ev).await;
                }
                _ = wake_at(wake) => {
                    self.run_due().await;
                }
            }
        }
        self.shutdown_workers().await;
        if let Err(error) = self.flush_all().await {
            tracing::warn!(error = %error, "退出时最终落盘失败");
        }
        if let Some(reply) = pending_shutdown {
            let _ = reply.send(Ok(()));
        }
    }

    /// 发布快照（watch；序号递增）。
    fn publish(&mut self) {
        self.seq += 1;
        let mut tasks = Vec::with_capacity(self.order.len());
        for id in &self.order {
            if let Some(v) = self.tasks.get(id) {
                tasks.push(v.clone());
            }
        }
        let snap = QueueSnapshot::from_tasks(self.seq, tasks);
        self.snapshot_tx.send_replace(snap);
        self.snapshot_dirty = false;
        self.last_publish = Instant::now();
    }

    fn mark_snapshot_dirty(&mut self) {
        self.snapshot_dirty = true;
    }

    fn emit(&self, event: TaskEvent) {
        let _ = self.event_tx.send(event);
    }

    fn view(&self, id: &str) -> BiliResult<&TaskView> {
        self.tasks
            .get(id)
            .ok_or_else(|| BiliError::Other(format!("task not found: {id}")))
    }

    fn view_mut(&mut self, id: &str) -> BiliResult<&mut TaskView> {
        self.tasks
            .get_mut(id)
            .ok_or_else(|| BiliError::Other(format!("task not found: {id}")))
    }

    // ---- 调度 ----

    fn enforce_permits(&mut self) {
        // 通过「持有占位 permit」把信号量总量压到 max_concurrent
        let target =
            MAX_CONCURRENT.saturating_sub(self.settings.max_concurrent.clamp(1, MAX_CONCURRENT));
        while self.reduction_permits.len() < target {
            match self.sem.clone().try_acquire_owned() {
                Ok(p) => self.reduction_permits.push(p),
                Err(_) => break,
            }
        }
        while self.reduction_permits.len() > target {
            self.reduction_permits.pop();
        }
    }

    fn next_queued(&self) -> Option<TaskId> {
        self.order
            .iter()
            .find(|id| {
                !self.workers.contains_key(*id)
                    && self.tasks.get(*id).is_some_and(|t| t.phase.is_queued())
            })
            .cloned()
    }

    fn schedule(&mut self) {
        if !self.schedule_enabled {
            return;
        }
        loop {
            if self.workers.len() >= self.settings.max_concurrent.clamp(1, MAX_CONCURRENT) {
                break;
            }
            let Some(id) = self.next_queued() else { break };
            if !self.start_worker(id) {
                break;
            }
        }
    }

    fn start_worker(&mut self, id: TaskId) -> bool {
        let Ok(permit) = self.sem.clone().try_acquire_owned() else {
            return false;
        };
        let Some(view) = self.tasks.get(&id).cloned() else {
            return false;
        };
        let control = WorkerControl {
            cancel: CancellationToken::new(),
            pause: Arc::new(AtomicBool::new(false)),
        };
        self.controls.insert(id.clone(), control.clone());
        // 记录任务实际使用的临时目录：清理只针对这些路径（design D1）。
        let temp_dirs = task_temp_dirs(&self.storage, &view, &self.settings);
        self.temp_dirs.insert(id.clone(), temp_dirs);
        let (reporter, ctx) = {
            let reporter =
                TaskReporter::new(id.clone(), self.worker_tx.clone(), control.cancel.clone());
            let ctx = TaskRunCtx {
                id: id.clone(),
                spec: view.spec.clone(),
                resume: TaskResume {
                    completed_entries: completed_entry_indices(&view),
                    stage: view.progress.stage,
                },
                cancel: control.cancel.clone(),
                paused: control.pause.clone(),
                reporter: reporter.clone(),
                client: Arc::clone(&self.client),
                http: self.download_http.clone(),
                settings: self.settings.clone(),
                ffmpeg: self.ffmpeg.clone(),
            };
            (reporter, ctx)
        };
        let runner = Arc::clone(&self.runner);
        let tx = self.worker_tx.clone();
        let paused = control.pause.clone();
        let task_id = id.clone();
        let handle = tokio::spawn(async move {
            let result = runner.run(ctx).await;
            drop(permit);
            let _ = tx.send(WorkerEvent::Finished {
                id: task_id,
                result,
                paused: paused.load(Ordering::Relaxed),
            });
        });
        self.workers.insert(id, handle);
        let _ = reporter;
        true
    }

    // ---- 命令处理 ----

    async fn handle_command(&mut self, cmd: ManagerCommand) -> LoopAction {
        match cmd {
            ManagerCommand::Enqueue { specs, reply } => {
                // design D8/3.4：同一请求的记录在一个事务中写入；成功后才发布、调度，
                // 任一写入失败则整体回滚，不留下部分任务、不启动 worker、不返回成功数量。
                let views: Vec<TaskView> = specs.into_iter().map(TaskView::new).collect();
                let ids: Vec<TaskId> = views.iter().map(|view| view.id.clone()).collect();
                let base = self.order.len();
                let records: Vec<TaskRecord> = views
                    .iter()
                    .enumerate()
                    .map(|(index, view)| task_record(view, (base + index) as i64))
                    .collect();
                let injected_failure = self
                    .enqueue_store_fail_once
                    .as_ref()
                    .is_some_and(|fail| fail.swap(false, Ordering::SeqCst));
                let result = if injected_failure {
                    Err(BiliError::Disk("测试注入：批量入队写库失败".into()))
                } else {
                    let db = Arc::clone(self.storage.db());
                    spawn_blocking_db(move || db.insert_tasks(&records)).await
                };
                match result {
                    Ok(()) => {
                        for view in views {
                            self.order.push(view.id.clone());
                            self.tasks.insert(view.id.clone(), view.clone());
                            self.emit(TaskEvent::Created { view });
                        }
                        self.publish();
                        self.schedule();
                        let _ = reply.send(Ok(ids));
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            ManagerCommand::Pause { id, reply } => {
                let result = self.pause_task(&id).await;
                if result.is_ok() {
                    self.publish();
                }
                let _ = reply.send(result);
            }
            ManagerCommand::Resume { id, reply } => {
                let _ = reply.send(self.resume_task(&id));
                self.publish();
                self.schedule();
            }
            ManagerCommand::Cancel { id, reply } => {
                let result = self.cancel_task(&id).await;
                if result.is_ok() {
                    self.publish();
                }
                let _ = reply.send(result);
            }
            ManagerCommand::Retry { id, reply } => {
                let _ = reply.send(self.retry_task(&id).await);
                self.publish();
                self.schedule();
            }
            ManagerCommand::Remove { id, reply } => {
                let _ = reply.send(self.remove_task(&id).await);
            }
            ManagerCommand::ClearFinished { reply } => {
                let outcome = self.clear_finished().await;
                let _ = reply.send(Ok(outcome));
            }
            ManagerCommand::ClearInactive { reply } => {
                let outcome = self.clear_inactive().await;
                let _ = reply.send(Ok(outcome));
            }
            ManagerCommand::PauseAll { reply } => {
                let mut n = 0;
                let ids: Vec<TaskId> = self.order.clone();
                for id in ids {
                    if self.view(&id).map(|v| v.phase.can_pause()).unwrap_or(false)
                        && self.pause_task(&id).await.is_ok()
                    {
                        n += 1;
                    }
                }
                self.publish();
                let _ = reply.send(Ok(n));
            }
            ManagerCommand::ResumeAll { reply } => {
                let mut n = 0;
                let ids: Vec<TaskId> = self.order.clone();
                for id in ids {
                    if self
                        .view(&id)
                        .map(|v| v.phase.can_resume())
                        .unwrap_or(false)
                        && self.resume_task(&id).is_ok()
                    {
                        n += 1;
                    }
                }
                self.publish();
                self.schedule();
                let _ = reply.send(Ok(n));
            }
            ManagerCommand::RetryAllFailed { reply } => {
                let mut n = 0;
                let ids: Vec<TaskId> = self.order.clone();
                for id in ids {
                    if self
                        .view(&id)
                        .map(|v| v.phase == TaskPhase::Failed)
                        .unwrap_or(false)
                        && self.retry_task(&id).await.is_ok()
                    {
                        n += 1;
                    }
                }
                self.publish();
                self.schedule();
                let _ = reply.send(Ok(n));
            }
            ManagerCommand::Flush { reply } => {
                let result = self.flush_all().await;
                let _ = reply.send(result);
            }
            ManagerCommand::SetSettings { settings, reply } => {
                let result = self.apply_settings(settings);
                let _ = reply.send(result);
            }
            ManagerCommand::SetClient { client, reply } => {
                self.client = client;
                let _ = reply.send(Ok(()));
            }
            ManagerCommand::RetryPersistence { reply } => {
                let result = self.retry_persistence().await;
                let _ = reply.send(result);
            }
            ManagerCommand::EnableScheduling { reply } => {
                self.schedule_enabled = true;
                self.schedule();
                self.publish();
                let _ = reply.send(Ok(()));
            }
            ManagerCommand::PrepareShutdown { reply } => {
                // 暂停进行中任务并把最终状态落盘；失败时保留可重试状态、不退出（design D8/3.9）。
                for control in self.controls.values() {
                    control.pause.store(true, Ordering::Relaxed);
                    control.cancel.cancel();
                }
                for view in self.tasks.values_mut() {
                    if view.phase.is_active() {
                        view.phase = TaskPhase::Paused;
                        view.speed_bps = 0.0;
                        view.restored = true;
                        view.touch();
                    }
                }
                let result = self.flush_all().await;
                let _ = reply.send(result);
            }
            ManagerCommand::Shutdown { reply } => {
                for control in self.controls.values() {
                    control.pause.store(true, Ordering::Relaxed);
                    control.cancel.cancel();
                }
                return LoopAction::Shutdown(reply);
            }
        }
        LoopAction::Continue
    }

    async fn pause_task(&mut self, id: &str) -> BiliResult<()> {
        let phase = self.view(id)?.phase;
        if !phase.can_pause() {
            return Err(BiliError::Other(format!(
                "task cannot be paused in phase {}",
                phase.as_str()
            )));
        }
        if let Some(control) = self.controls.get(id) {
            control.pause.store(true, Ordering::Relaxed);
            control.cancel.cancel();
            // 运行中的任务：由 worker Finished 事件落 Paused；先按 Paused 展示
            if !self.workers.contains_key(id) {
                self.set_phase(id, TaskPhase::Paused).await?;
            }
        } else {
            self.set_phase(id, TaskPhase::Paused).await?;
        }
        Ok(())
    }

    fn resume_task(&mut self, id: &str) -> BiliResult<()> {
        let view = self.view_mut(id)?;
        if !view.phase.can_resume() {
            return Err(BiliError::Other(format!(
                "cannot resume in phase {}",
                view.phase.as_str()
            )));
        }
        view.phase = TaskPhase::Queued;
        view.error = None;
        view.speed_bps = 0.0;
        view.restored = false;
        view.touch();
        self.controls.remove(id);
        self.emit(TaskEvent::Resumed { id: id.to_string() });
        Ok(())
    }

    async fn cancel_task(&mut self, id: &str) -> BiliResult<()> {
        let phase = self.view(id)?.phase;
        if phase.is_terminal() {
            return Err(BiliError::Other("task already finished".into()));
        }
        if let Some(control) = self.controls.get(id) {
            control.pause.store(false, Ordering::Relaxed);
            control.cancel.cancel();
            self.set_phase(id, TaskPhase::Cancelled).await?;
            if self.workers.contains_key(id) {
                // 取消后最多等待 3 秒；超时由独立监督事件终止 worker 再收尾（design D2）。
                self.arm_cancel_convergence(id);
            } else {
                self.finish_cancelled(id).await;
            }
        } else {
            self.set_phase(id, TaskPhase::Cancelled).await?;
            self.finish_cancelled(id).await;
        }
        Ok(())
    }

    /// 取消/移除后启动独立收敛监督：3 秒内 worker 仍未结束则由 actor 终止并回收（design D2）。
    fn arm_cancel_convergence(&self, id: &str) {
        if !self.workers.contains_key(id) {
            return;
        }
        let tx = self.worker_tx.clone();
        let task_id = id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(CANCEL_CONVERGE).await;
            let _ = tx.send(WorkerEvent::Converge { id: task_id });
        });
    }

    async fn retry_task(&mut self, id: &str) -> BiliResult<()> {
        let view = self.view_mut(id)?;
        if !view.phase.can_retry() {
            return Err(BiliError::Other(format!(
                "cannot retry in phase {}",
                view.phase.as_str()
            )));
        }
        view.phase = TaskPhase::Queued;
        view.error = None;
        view.progress = Progress::default();
        view.speed_bps = 0.0;
        view.output_path = None;
        view.file_size = None;
        view.artifacts.clear();
        view.artifacts_legacy = false;
        view.restored = false;
        view.actual_quality = None;
        view.actual_codec = None;
        view.actual_audio = None;
        view.touch();
        self.auto_retried.remove(id);
        self.controls.remove(id);
        self.cleanup_temp(id).await;
        let _ = self.persist_view(id).await;
        self.emit(TaskEvent::Updated {
            view: self.tasks[id].clone(),
        });
        Ok(())
    }

    async fn remove_task(&mut self, id: &str) -> BiliResult<()> {
        self.remove_task_inner(id, true).await
    }

    async fn remove_task_inner(&mut self, id: &str, schedule: bool) -> BiliResult<()> {
        if !self.tasks.contains_key(id) {
            return Err(BiliError::Other(format!("task not found: {id}")));
        }
        // design D2：数据库删除成功后才从用户列表移除；失败保留任务行。
        let db = Arc::clone(self.storage.db());
        let task_id = id.to_string();
        spawn_blocking_db(move || db.delete_task(&task_id)).await?;
        // 先捕获任务实际使用的临时目录，之后的清理不再依赖全局设置或已移除的视图。
        let dirs = self.take_temp_dirs(id);
        if let Some(control) = self.controls.remove(id) {
            control.pause.store(false, Ordering::Relaxed);
            control.cancel.cancel();
        }
        let running = self.workers.contains_key(id);
        if running {
            // worker 仍在运行：保留收尾记录，确认 worker 结束后再清理由 Finished/Converge 处理。
            self.tails.insert(id.to_string(), dirs);
            self.arm_cancel_convergence(id);
        } else {
            self.cleanup_dirs_now(dirs).await;
        }
        self.tasks.remove(id);
        self.order.retain(|x| x != id);
        self.dirty.remove(id);
        self.emit(TaskEvent::Removed { id: id.to_string() });
        self.publish();
        if schedule {
            self.schedule();
        }
        Ok(())
    }

    async fn clear_finished(&mut self) -> ClearOutcome {
        let ids: Vec<TaskId> = self
            .order
            .iter()
            .filter(|id| {
                self.tasks
                    .get(*id)
                    .map(|v| v.phase.is_terminal())
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let mut outcome = ClearOutcome::default();
        for id in &ids {
            let db = Arc::clone(self.storage.db());
            let task_id = id.clone();
            // design D2/3.7：删除成功后才发布移除；失败保留记录并暴露诊断。
            if let Err(error) = spawn_blocking_db(move || db.delete_task(&task_id)).await {
                tracing::warn!(task = %id, error = %error, "删除已完成任务失败，保留队列记录");
                outcome.failed.push(id.clone());
                continue;
            }
            self.tasks.remove(id);
            self.controls.remove(id);
            self.dirty.remove(id);
            self.cleanup_temp(id).await;
            self.emit(TaskEvent::Removed { id: id.clone() });
            outcome.removed += 1;
        }
        self.order.retain(|id| self.tasks.contains_key(id));
        self.emit(TaskEvent::QueueChanged);
        self.publish();
        outcome
    }

    /// 清空未激活任务：同一 actor 内先确定目标，再逐条检查删除，最后统一补调度，
    /// 避免逐项删除时提前启动本批待移除任务（design D2/3.5）。
    async fn clear_inactive(&mut self) -> ClearOutcome {
        let ids: Vec<TaskId> = self
            .order
            .iter()
            .filter(|id| {
                self.tasks
                    .get(*id)
                    .map(|view| {
                        matches!(
                            view.phase,
                            TaskPhase::Queued | TaskPhase::Paused | TaskPhase::Cancelled
                        )
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let mut outcome = ClearOutcome::default();
        for id in &ids {
            match self.remove_task_inner(id, false).await {
                Ok(()) => outcome.removed += 1,
                Err(error) => {
                    tracing::warn!(task = %id, error = %error, "清理未激活任务失败，保留队列记录");
                    outcome.failed.push(id.clone());
                }
            }
        }
        if outcome.removed > 0 {
            self.emit(TaskEvent::QueueChanged);
        }
        self.publish();
        self.schedule();
        outcome
    }

    fn apply_settings(&mut self, settings: Settings) -> BiliResult<()> {
        settings
            .validate()
            .map_err(|errors| BiliError::Other(format!("invalid settings: {errors:?}")))?;
        self.settings = settings;
        self.ffmpeg = Ffmpeg::detect(self.settings.ffmpeg_path.as_deref());
        if let Ok(http) = download_client(self.settings.proxy.as_deref()) {
            self.download_http = http;
        }
        self.enforce_permits();
        self.schedule();
        self.emit(TaskEvent::QueueChanged);
        self.publish();
        Ok(())
    }

    // ---- worker 事件 ----

    async fn handle_worker_event(&mut self, ev: WorkerEvent) {
        match ev {
            WorkerEvent::Meta { id, meta } => {
                if let Some(view) = self.tasks.get_mut(&id) {
                    apply_meta(view, meta);
                    let view = view.clone();
                    self.emit(TaskEvent::Updated { view });
                    self.dirty.insert(id.clone());
                    let _ = self.persist_view(&id).await;
                    self.mark_snapshot_dirty();
                }
            }
            WorkerEvent::Phase { id, phase } => {
                self.set_phase_from_worker(&id, phase).await;
                self.publish();
            }
            WorkerEvent::Progress {
                id,
                progress,
                speed_bps,
            } => {
                if let Some(view) = self.tasks.get_mut(&id) {
                    view.progress = progress;
                    view.speed_bps = speed_bps;
                    view.touch();
                    self.dirty.insert(id);
                    self.snapshot_dirty = true;
                }
            }
            WorkerEvent::Actual { id, actual } => {
                if let Some(view) = self.tasks.get_mut(&id) {
                    view.actual_quality = actual.quality;
                    view.actual_codec = actual.codec;
                    view.actual_audio = actual.audio;
                    view.touch();
                    let view = view.clone();
                    self.emit(TaskEvent::Updated { view });
                    self.dirty.insert(id);
                    self.mark_snapshot_dirty();
                }
            }
            WorkerEvent::Output { id, artifact, ack } => {
                // 已移除任务不再写入任务记录：立即以取消错误结束 checkpoint，避免 worker 悬挂（design D2）。
                let result = if self.tasks.contains_key(&id) {
                    self.record_artifact(&id, artifact).await
                } else {
                    Err(BiliError::Cancelled)
                };
                let _ = ack.send(result);
            }
            WorkerEvent::Finished { id, result, paused } => {
                self.workers.remove(&id);
                self.controls.remove(&id);
                self.enforce_permits();
                if !self.tasks.contains_key(&id) {
                    // 任务已移除且 worker 已确认结束：清理收尾记录并继续调度（design D2）。
                    let dirs = self.tails.remove(&id).unwrap_or_default();
                    self.temp_dirs.remove(&id);
                    if !dirs.is_empty() {
                        self.cleanup_dirs_now(dirs).await;
                    }
                    self.schedule();
                    self.publish();
                    return;
                }
                self.handle_finished(&id, result, paused).await;
                self.schedule();
                self.publish();
            }
            WorkerEvent::Converge { id } => {
                // 3 秒仍未结束：终止 worker，确认结束后再清理（design D2）。
                let Some(handle) = self.workers.remove(&id) else {
                    return;
                };
                handle.abort();
                let _ = tokio::time::timeout(Duration::from_secs(1), handle).await;
                self.controls.remove(&id);
                self.enforce_permits();
                let tail = self.tails.remove(&id);
                self.temp_dirs.remove(&id);
                if let Some(dirs) = tail {
                    // 任务已移除：只清理其收尾记录中的临时目录，已发布文件保留。
                    self.cleanup_dirs_now(dirs).await;
                } else if self.tasks.contains_key(&id) {
                    // 用户取消但 worker 未自行结束：在此完成取消收尾。
                    self.dirty.remove(&id);
                    {
                        let view = self.tasks.get_mut(&id).expect("task exists");
                        view.phase = TaskPhase::Cancelled;
                        view.speed_bps = 0.0;
                        view.progress.stage = DownloadStage::Idle;
                        view.touch();
                    }
                    self.cleanup_temp(&id).await;
                    let _ = self.persist_view(&id).await;
                    let _ = self.write_history(&id, "cancelled").await;
                    self.emit(TaskEvent::Cancelled { id: id.clone() });
                }
                self.schedule();
                self.publish();
            }
        }
    }

    async fn handle_finished(&mut self, id: &str, result: BiliResult<TaskOutcome>, paused: bool) {
        let phase_before = self.view(id).map(|v| v.phase).unwrap_or(TaskPhase::Queued);
        let cancelled_by_user = phase_before == TaskPhase::Cancelled && !paused;
        match result {
            Ok(outcome) => {
                {
                    let view = self.tasks.get_mut(id).expect("task exists");
                    view.phase = TaskPhase::Done;
                    view.speed_bps = 0.0;
                    view.error = None;
                    view.progress.stage = DownloadStage::Done;
                    if !outcome.artifacts.is_empty() {
                        view.merge_artifacts(outcome.artifacts);
                    } else if let Some(path) = outcome.output_path {
                        view.merge_artifacts([DownloadArtifact::media(
                            path,
                            outcome.file_size,
                            None,
                            None,
                        )]);
                    }
                    view.refresh_artifact_projection();
                    view.progress.entries_done = completed_entry_indices(view).len() as u32;
                    // 可选后处理附加信息（字幕/封面）随 Done 事件进入快照/DTO
                    view.extras = outcome.extras;
                    if view.progress.total > 0 {
                        view.progress.downloaded = view.progress.total;
                    }
                    view.touch();
                }
                self.dirty.remove(id);
                self.auto_retried.remove(id);
                self.complete_task(id).await;
                let view = self.tasks[id].clone();
                self.emit(TaskEvent::Done { view });
            }
            Err(e) if e.is_cancelled() && paused => {
                {
                    let view = self.tasks.get_mut(id).expect("task exists");
                    view.phase = TaskPhase::Paused;
                    view.speed_bps = 0.0;
                    // 保留 progress.stage/entries_done 供继续时判断合并或续传
                    view.touch();
                }
                // 暂停保留临时文件供续传
                let _ = self.persist_view(id).await;
                self.emit(TaskEvent::Paused { id: id.to_string() });
            }
            Err(e) if e.is_cancelled() || cancelled_by_user => {
                {
                    let view = self.tasks.get_mut(id).expect("task exists");
                    view.phase = TaskPhase::Cancelled;
                    view.speed_bps = 0.0;
                    view.progress.stage = DownloadStage::Idle;
                    view.touch();
                }
                self.cleanup_temp(id).await;
                let _ = self.persist_view(id).await;
                let _ = self.write_history(id, "cancelled").await;
                self.emit(TaskEvent::Cancelled { id: id.to_string() });
            }
            Err(e) if is_transient_disk(&e) && !self.auto_retried.contains(id) => {
                // 下载完成与 rename 之间的瞬时文件丢失（外部清理/索引/杀软）在多台机器
                // 上表现为「第一次失败、第二次成功」：自动清理临时文件并重跑一次。
                self.auto_retried.insert(id.to_string());
                tracing::warn!(
                    target: "bilidown_core::task",
                    task = %id,
                    error = %e.describe(),
                    "瞬时磁盘错误，自动重试一次"
                );
                self.cleanup_temp(id).await;
                {
                    let view = self.tasks.get_mut(id).expect("task exists");
                    view.phase = TaskPhase::Queued;
                    view.error = None;
                    view.progress = Progress::default();
                    view.speed_bps = 0.0;
                    view.restored = false;
                    view.touch();
                }
                let _ = self.persist_view(id).await;
                self.emit(TaskEvent::Updated {
                    view: self.tasks[id].clone(),
                });
            }
            Err(e) => {
                {
                    let view = self.tasks.get_mut(id).expect("task exists");
                    view.phase = TaskPhase::Failed;
                    view.speed_bps = 0.0;
                    view.error = Some(e.clone());
                    view.touch();
                }
                tracing::warn!(
                    target: "bilidown_core::task",
                    task = %id,
                    kind = ?e.kind(),
                    error = %e.describe(),
                    out_dir = %self.settings.effective_download_dir().display(),
                    "任务失败（保留临时文件供重试续传）"
                );
                // 失败保留临时文件，重试时可续传
                let _ = self.persist_view(id).await;
                let _ = self.write_history(id, "failed").await;
                let view = self.tasks[id].clone();
                self.emit(TaskEvent::Failed { view, error: e });
            }
        }
    }

    async fn finish_cancelled(&mut self, id: &str) {
        self.cleanup_temp(id).await;
        let _ = self.persist_view(id).await;
        let _ = self.write_history(id, "cancelled").await;
        self.emit(TaskEvent::Cancelled { id: id.to_string() });
        self.publish();
    }

    async fn set_phase(&mut self, id: &str, phase: TaskPhase) -> BiliResult<()> {
        {
            let view = self.view_mut(id)?;
            view.phase = phase;
            if phase == TaskPhase::Paused {
                view.speed_bps = 0.0;
            }
            view.touch();
        }
        self.persist_view(id).await?;
        let view = self.tasks[id].clone();
        self.emit(TaskEvent::PhaseChanged {
            id: id.to_string(),
            phase,
            view,
        });
        self.publish();
        Ok(())
    }

    async fn set_phase_from_worker(&mut self, id: &str, phase: TaskPhase) {
        let Some(view) = self.tasks.get_mut(id) else {
            return;
        };
        // 已被用户标记终态时不被 worker 的中间阶段覆盖
        if view.phase == TaskPhase::Cancelled || view.phase == TaskPhase::Paused {
            return;
        }
        if view.phase == phase {
            return;
        }
        view.phase = phase;
        view.touch();
        let view = view.clone();
        self.emit(TaskEvent::PhaseChanged {
            id: id.to_string(),
            phase,
            view,
        });
        let _ = self.persist_view(id).await;
    }

    // ---- 持久化 ----

    async fn record_artifact(&mut self, id: &str, artifact: DownloadArtifact) -> BiliResult<()> {
        if self
            .artifact_store_fail_once
            .as_ref()
            .is_some_and(|fail| fail.swap(false, Ordering::SeqCst))
        {
            return Err(BiliError::Disk(
                "文件已保存但记录失败：测试注入的产物记录写库失败".into(),
            ));
        }
        let mut updated = self.view(id)?.clone();
        updated.upsert_artifact(artifact);
        updated.progress.entries_done = completed_entry_indices(&updated).len() as u32;
        updated.touch();
        let idx = self.order.iter().position(|x| x == id).unwrap_or(0) as i64;
        self.persist_record_checked(task_record(&updated, idx))
            .await
            .map_err(|error| {
                BiliError::Disk(format!("文件已保存但记录失败：{}", error.describe()))
            })?;
        self.tasks.insert(id.to_string(), updated.clone());
        self.dirty.insert(id.to_string());
        self.emit(TaskEvent::Updated { view: updated });
        self.mark_snapshot_dirty();
        Ok(())
    }

    fn record_index(&self, id: &str) -> i64 {
        self.order.iter().position(|x| x == id).unwrap_or(0) as i64
    }

    fn mark_persist_error(&mut self, id: &str, error: &BiliError) {
        if let Some(view) = self.tasks.get_mut(id) {
            view.persist_error = Some(format!("任务记录保存失败：{}", error.describe()));
            view.touch();
        }
        self.dirty.insert(id.to_string());
        self.mark_snapshot_dirty();
    }

    fn clear_persist_error(&mut self, id: &str) {
        if let Some(view) = self.tasks.get_mut(id) {
            view.persist_error = None;
        }
    }

    /// 关键状态落盘：失败时保留内存状态与待保存标记，并把错误返回给调用方（design D8）。
    async fn persist_view(&mut self, id: &str) -> BiliResult<()> {
        let Some(view) = self.tasks.get(id).cloned() else {
            return Ok(());
        };
        let record = task_record(&view, self.record_index(id));
        let db = Arc::clone(self.storage.db());
        match spawn_blocking_db(move || db.upsert_task(&record)).await {
            Ok(()) => {
                self.dirty.remove(id);
                self.clear_persist_error(id);
                Ok(())
            }
            Err(error) => {
                tracing::warn!(task = %id, error = %error, "persist task failed");
                self.mark_persist_error(id, &error);
                Err(error)
            }
        }
    }

    async fn persist_record_checked(&self, record: TaskRecord) -> BiliResult<()> {
        let db = Arc::clone(self.storage.db());
        spawn_blocking_db(move || db.upsert_task(&record)).await
    }

    /// 下一次需要唤醒的时间点（design D10/7.4）：无待发布/待落盘/待重试时返回 None，
    /// 空闲队列不再运行固定 interval 轮询。
    fn next_deadline(&self) -> Option<Instant> {
        let mut deadline: Option<Instant> = None;
        if self.snapshot_dirty {
            deadline = Some(self.last_publish + SNAPSHOT_INTERVAL);
        }
        if !self.dirty.is_empty() {
            let candidate = self.last_progress_flush + self.flush_backoff;
            deadline = Some(deadline.map_or(candidate, |current| current.min(candidate)));
        }
        if !self.pending_cleanups.is_empty() {
            let candidate = self.last_cleanup_retry + CLEANUP_RETRY_INTERVAL;
            deadline = Some(deadline.map_or(candidate, |current| current.min(candidate)));
        }
        deadline
    }

    /// 执行到期的发布/落盘/清理重试；无到期项时立即返回（design D10/7.4）。
    async fn run_due(&mut self) {
        if self.snapshot_dirty && self.last_publish.elapsed() >= SNAPSHOT_INTERVAL {
            self.publish();
        }
        if !self.dirty.is_empty()
            && self.last_progress_flush.elapsed() >= self.flush_backoff
            && let Err(error) = self.flush_dirty().await
        {
            tracing::warn!(error = %error, "flush progress failed");
        }
        if !self.pending_cleanups.is_empty() {
            self.retry_pending_cleanups().await;
        }
    }

    /// 批量落盘脏任务；仅成功保存后清除 dirty，失败保留并做有界退避（design D8/3.6）。
    async fn flush_dirty(&mut self) -> BiliResult<()> {
        self.dirty.retain(|id| self.tasks.contains_key(id));
        if self.dirty.is_empty() {
            return Ok(());
        }
        let ids: Vec<TaskId> = self.dirty.iter().cloned().collect();
        let mut records = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(view) = self.tasks.get(id) {
                records.push(task_record(view, self.record_index(id)));
            }
        }
        let db = Arc::clone(self.storage.db());
        match spawn_blocking_db(move || db.insert_tasks(&records)).await {
            Ok(()) => {
                for id in &ids {
                    self.dirty.remove(id);
                    self.clear_persist_error(id);
                }
                self.flush_backoff = PROGRESS_FLUSH_INTERVAL;
                self.last_progress_flush = Instant::now();
                Ok(())
            }
            Err(error) => {
                self.flush_backoff = (self.flush_backoff * 2).min(Duration::from_secs(30));
                self.last_progress_flush = Instant::now();
                for id in &ids {
                    self.mark_persist_error(id, &error);
                }
                Err(error)
            }
        }
    }

    async fn flush_all(&mut self) -> BiliResult<()> {
        self.dirty = self.tasks.keys().cloned().collect();
        self.flush_dirty().await
    }

    /// 完成任务：最终视图 + 历史 + 裁剪在同一事务提交；失败时保留可重试状态（design D8/3.7）。
    async fn complete_task(&mut self, id: &str) {
        self.auto_retried.remove(id);
        self.cleanup_temp(id).await;
        let Some(view) = self.tasks.get(id).cloned() else {
            return;
        };
        let history = history_record(&view, "done");
        let record = task_record(&view, self.record_index(id));
        let db = Arc::clone(self.storage.db());
        let keep = self.settings.history_limit;
        let history_for_retry = history.clone();
        match spawn_blocking_db(move || db.finish_task(&record, &history, keep)).await {
            Ok(()) => {
                self.dirty.remove(id);
                self.clear_persist_error(id);
                self.pending_completion.remove(id);
                self.emit(TaskEvent::HistoryChanged);
            }
            Err(error) => {
                tracing::warn!(task = %id, error = %error, "完成任务事务写入失败，保留重试状态");
                self.pending_completion
                    .insert(id.to_string(), history_for_retry);
                self.mark_persist_error(id, &error);
            }
        }
    }

    /// 写入失败/取消历史；失败时保留待重试记录（design D8）。
    async fn write_history(&mut self, id: &str, status: &str) -> BiliResult<()> {
        let Some(view) = self.tasks.get(id) else {
            return Ok(());
        };
        let record = history_record(view, status);
        let db = Arc::clone(self.storage.db());
        let limit = self.settings.history_limit;
        let record_for_retry = record.clone();
        match spawn_blocking_db(move || {
            db.insert_history(&record)?;
            db.prune_history(limit)?;
            Ok(())
        })
        .await
        {
            Ok(()) => {
                self.pending_history.remove(id);
                self.emit(TaskEvent::HistoryChanged);
                Ok(())
            }
            Err(error) => {
                tracing::warn!(task = %id, error = %error, "write history failed");
                self.pending_history
                    .insert(id.to_string(), record_for_retry);
                self.mark_persist_error(id, &error);
                Err(error)
            }
        }
    }

    /// 重试所有待保存动作：完成事务、历史写入与脏任务记录（design D8/3.8）。
    async fn retry_persistence(&mut self) -> BiliResult<()> {
        let mut first_error: Option<BiliError> = None;
        // 1) 完成事务（任务视图 + 历史 + 裁剪原子提交）
        let completions: Vec<(TaskId, HistoryRecord)> = self.pending_completion.drain().collect();
        for (id, history) in completions {
            let Some(view) = self.tasks.get(&id).cloned() else {
                continue;
            };
            let record = task_record(&view, self.record_index(&id));
            let db = Arc::clone(self.storage.db());
            let keep = self.settings.history_limit;
            let history_for_retry = history.clone();
            match spawn_blocking_db(move || db.finish_task(&record, &history, keep)).await {
                Ok(()) => {
                    self.dirty.remove(&id);
                    self.clear_persist_error(&id);
                    self.emit(TaskEvent::HistoryChanged);
                }
                Err(error) => {
                    self.pending_completion
                        .insert(id.clone(), history_for_retry);
                    self.mark_persist_error(&id, &error);
                    tracing::warn!(task = %id, error = %error, "重试完成事务仍失败");
                    first_error.get_or_insert(error);
                }
            }
        }
        // 2) 失败/取消历史
        let histories: Vec<(TaskId, HistoryRecord)> = self.pending_history.drain().collect();
        for (id, history) in histories {
            let db = Arc::clone(self.storage.db());
            let keep = self.settings.history_limit;
            let history_for_retry = history.clone();
            match spawn_blocking_db(move || {
                db.insert_history(&history)?;
                db.prune_history(keep)?;
                Ok(())
            })
            .await
            {
                Ok(()) => self.emit(TaskEvent::HistoryChanged),
                Err(error) => {
                    self.pending_history.insert(id.clone(), history_for_retry);
                    self.mark_persist_error(&id, &error);
                    first_error.get_or_insert(error);
                }
            }
        }
        // 3) 脏任务记录
        if let Err(error) = self.flush_dirty().await {
            first_error.get_or_insert(error);
        }
        self.publish();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// 取出任务启动时确认的临时目录；没有记录时按当前视图推算（旧数据/恢复任务）。
    fn take_temp_dirs(&mut self, id: &str) -> Vec<PathBuf> {
        self.temp_dirs
            .remove(id)
            .unwrap_or_else(|| self.compute_temp_dirs(id))
    }

    fn compute_temp_dirs(&self, id: &str) -> Vec<PathBuf> {
        let mut dirs = vec![self.storage.tmp_dir().join(id)];
        if let Some(view) = self.tasks.get(id) {
            dirs.push(
                view.spec
                    .output_dir
                    .clone()
                    .unwrap_or_else(|| self.settings.effective_download_dir())
                    .join(".bilidown-tmp")
                    .join(id),
            );
        }
        dirs
    }

    async fn cleanup_temp(&mut self, id: &str) {
        let dirs = self.take_temp_dirs(id);
        self.cleanup_dirs_now(dirs).await;
    }

    /// 只清理传入的任务专属目录；失败记录并排队重试，绝不扩大清理范围（design D1）。
    async fn cleanup_dirs_now(&mut self, dirs: Vec<PathBuf>) {
        if dirs.is_empty() {
            return;
        }
        let failures = cleanup_task_dirs(&dirs).await;
        if failures.is_empty() {
            return;
        }
        for (path, error) in &failures {
            tracing::warn!(
                target: "bilidown_core::task",
                path = %path.display(),
                error = %error,
                "清理任务临时目录失败，稍后重试"
            );
        }
        if dirs.len() >= PENDING_CLEANUP_CAP {
            tracing::warn!(count = dirs.len(), "待重试清理目录过多，丢弃本次重试记录");
            return;
        }
        if let Some((_, attempts)) = self
            .pending_cleanups
            .iter_mut()
            .find(|(pending, _)| pending == &dirs)
        {
            *attempts += 1;
        } else if self.pending_cleanups.len() < PENDING_CLEANUP_CAP {
            self.pending_cleanups.push((dirs, 1));
        }
    }

    /// 按间隔重试失败的任务临时目录清理；不阻塞调度（design D1）。
    async fn retry_pending_cleanups(&mut self) {
        if self.pending_cleanups.is_empty()
            || self.last_cleanup_retry.elapsed() < CLEANUP_RETRY_INTERVAL
        {
            return;
        }
        self.last_cleanup_retry = Instant::now();
        let pending = std::mem::take(&mut self.pending_cleanups);
        for (dirs, attempts) in pending {
            let failures = cleanup_task_dirs(&dirs).await;
            if failures.is_empty() {
                continue;
            }
            if attempts >= CLEANUP_MAX_ATTEMPTS {
                for (path, error) in &failures {
                    tracing::warn!(
                        target: "bilidown_core::task",
                        path = %path.display(),
                        error = %error,
                        "清理任务临时目录最终失败，保留诊断"
                    );
                }
                continue;
            }
            self.pending_cleanups.push((dirs, attempts + 1));
        }
    }

    // ---- 退出 ----

    async fn shutdown_workers(&mut self) {
        let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        for (id, control) in self.controls.iter() {
            control.pause.store(true, Ordering::Relaxed);
            control.cancel.cancel();
            let _ = id;
        }
        for (_, handle) in self.workers.drain() {
            handles.push(handle);
        }
        for handle in handles {
            let _ = tokio::time::timeout(SHUTDOWN_WAIT, handle).await;
        }
        let now = now_ms();
        for view in self.tasks.values_mut() {
            if view.phase.is_active() {
                view.phase = TaskPhase::Paused;
                view.restored = true;
                view.speed_bps = 0.0;
                view.updated_at = now;
            }
        }
        self.controls.clear();
    }
}

/// 任务拥有的临时目录：应用缓存下的任务目录 + 任务实际输出目录下的同卷临时目录。
fn task_temp_dirs(storage: &Storage, view: &TaskView, settings: &Settings) -> Vec<PathBuf> {
    let output_dir = view
        .spec
        .output_dir
        .clone()
        .unwrap_or_else(|| settings.effective_download_dir());
    vec![
        storage.tmp_dir().join(&view.id),
        output_dir.join(".bilidown-tmp").join(&view.id),
    ]
}

/// 只递归删除传入的任务专属目录；共享父目录 `.bilidown-tmp` 仅在其为空时非递归移除。
/// 不跟随目标路径上的符号链接扩大清理范围，也不删除任何已发布产物（design D1）。
async fn cleanup_task_dirs(dirs: &[PathBuf]) -> Vec<(PathBuf, std::io::Error)> {
    let dirs = dirs.to_vec();
    match tokio::task::spawn_blocking(move || {
        let mut failures = Vec::new();
        for dir in &dirs {
            match std::fs::symlink_metadata(dir) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    if let Err(error) = std::fs::remove_file(dir) {
                        failures.push((dir.clone(), error));
                    }
                }
                Ok(meta) if meta.is_dir() => {
                    if let Err(error) = std::fs::remove_dir_all(dir) {
                        failures.push((dir.clone(), error));
                    }
                }
                Ok(_) => {
                    if let Err(error) = std::fs::remove_file(dir) {
                        failures.push((dir.clone(), error));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => failures.push((dir.clone(), error)),
            }
        }
        // 共享父目录只在为空时移除；非空表示其他任务仍在使用，视为正常保留。
        for dir in &dirs {
            let Some(parent) = dir.parent() else { continue };
            if !parent
                .file_name()
                .is_some_and(|name| name == ".bilidown-tmp")
            {
                continue;
            }
            match std::fs::remove_dir(parent) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound
                            | std::io::ErrorKind::DirectoryNotEmpty
                            | std::io::ErrorKind::PermissionDenied
                    ) => {}
                Err(error) => failures.push((parent.to_path_buf(), error)),
            }
        }
        failures
    })
    .await
    {
        Ok(failures) => failures,
        Err(error) => vec![(
            PathBuf::from("<cleanup>"),
            std::io::Error::other(error.to_string()),
        )],
    }
}

/// 等待到下一个 deadline；None 表示没有任何待办，永久挂起直到其他事件唤醒。
async fn wake_at(deadline: Option<Instant>) {
    match deadline {
        Some(at) => {
            let now = Instant::now();
            if at > now {
                tokio::time::sleep(at - now).await;
            }
        }
        None => std::future::pending::<()>().await,
    }
}

fn apply_meta(view: &mut TaskView, meta: TaskMeta) {
    view.title = meta.title;
    view.upper = meta.upper;
    view.cover_url = meta.cover_url;
    view.page_url = meta.page_url;
    view.content_kind = meta.content_kind;
    view.entries = meta.entries;
    view.selected_entries = meta.selected_entries;
    view.touch();
}

fn kind_str(kind: crate::api::types::ContentKind) -> &'static str {
    use crate::api::types::ContentKind as K;
    match kind {
        K::Video => "video",
        K::Bangumi => "bangumi",
        K::Cheese => "cheese",
        K::Audio => "audio",
        K::AudioList => "audio_list",
    }
}

fn task_record(view: &TaskView, sort_index: i64) -> TaskRecord {
    TaskRecord {
        id: view.id.clone(),
        status: view.phase.as_str().to_string(),
        phase: view.phase.as_str().to_string(),
        source_kind: kind_str(view.content_kind).to_string(),
        source_id: view.spec.id.to_string(),
        title: view.title.clone(),
        cover_url: view.cover_url.clone(),
        page_url: Some(view.page_url.clone()),
        params: serde_json::to_string(&view.spec).unwrap_or_else(|_| "{}".into()),
        progress: serde_json::to_string(&view.progress).unwrap_or_else(|_| "{}".into()),
        output_path: view.output_path.clone(),
        artifacts: serde_json::to_string(&view.artifacts).unwrap_or_else(|_| "[]".into()),
        metadata: serde_json::to_string(&view.metadata()).unwrap_or_else(|_| "{}".into()),
        error: view
            .error
            .as_ref()
            .and_then(|e| serde_json::to_string(e).ok()),
        restored: view.restored,
        sort_index,
        created_at: view.created_at,
        updated_at: view.updated_at,
    }
}

fn view_from_record(rec: &TaskRecord) -> Option<TaskView> {
    let spec: TaskSpec = serde_json::from_str(&rec.params).ok()?;
    let progress: Progress = serde_json::from_str(&rec.progress).unwrap_or_default();
    let error = rec
        .error
        .as_ref()
        .and_then(|s| serde_json::from_str::<BiliError>(s).ok());
    let mut view = TaskView::new(spec);
    view.id = rec.id.clone();
    view.phase = TaskPhase::from_status(&rec.status);
    view.title = if rec.title.is_empty() {
        view.title
    } else {
        rec.title.clone()
    };
    view.cover_url = rec.cover_url.clone();
    view.page_url = rec
        .page_url
        .clone()
        .unwrap_or_else(|| view.spec.id.page_url());
    view.content_kind = view.spec.id.kind();
    view.progress = progress;
    view.output_path = rec.output_path.clone();
    view.artifacts = serde_json::from_str(&rec.artifacts).unwrap_or_default();
    if view.artifacts.is_empty()
        && let Some(path) = rec.output_path.clone()
    {
        let size = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        view.artifacts.push(DownloadArtifact::media(
            PathBuf::from(path),
            size,
            None,
            None,
        ));
        view.artifacts_legacy = true;
    }
    // D7：元信息独立解析；损坏或未知版本只丢失元信息，不丢弃任务本体或伪造值。
    match serde_json::from_str::<TaskMetadata>(&rec.metadata) {
        Ok(metadata) if metadata.version <= TASK_METADATA_VERSION => view.apply_metadata(metadata),
        Ok(metadata) => tracing::warn!(
            task = %rec.id,
            version = metadata.version,
            "未知任务元信息版本，保留基础记录"
        ),
        Err(error) => tracing::warn!(
            task = %rec.id,
            %error,
            "任务元信息损坏，按未知值恢复"
        ),
    }
    view.error = error;
    view.restored = rec.restored;
    view.created_at = rec.created_at;
    view.updated_at = rec.updated_at;
    Some(view)
}

fn history_record(view: &TaskView, status: &str) -> HistoryRecord {
    let quality = view
        .actual_quality
        .map(crate::media::filename::quality_tag)
        .or_else(|| view.actual_audio.map(|a| a.tag().to_string()));
    HistoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: Some(view.id.clone()),
        title: view.title.clone(),
        cover_url: view.cover_url.clone(),
        source_kind: kind_str(view.content_kind).to_string(),
        source_id: view.spec.id.to_string(),
        page_url: Some(view.page_url.clone()),
        quality,
        output_path: view.output_path.clone(),
        file_size: view.file_size.map(|n| n as i64),
        artifacts: serde_json::to_string(&view.artifacts).unwrap_or_else(|_| "[]".into()),
        status: status.to_string(),
        error: view
            .error
            .as_ref()
            .and_then(|e| serde_json::to_string(e).ok()),
        finished_at: now_ms(),
    }
}

async fn spawn_blocking_db<T, F>(f: F) -> BiliResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> BiliResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| BiliError::Other(format!("db worker join error: {e}")))?
}

// 让未使用字段在旧 API 兼容期不报警（Db 类型在签名中出现，保持引用）
#[allow(dead_code)]
fn _db_type_marker(_db: &Db) {}

/// 瞬时磁盘错误（文件在 rename/copy 时消失）可安全自动重试一次。
fn is_transient_disk(e: &BiliError) -> bool {
    if e.kind() != ErrorKind::Disk {
        return false;
    }
    let msg = e.describe();
    msg.contains("No such file or directory") || msg.contains("os error 2")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{BiliId, DownloadStage};
    use crate::storage::AppPaths;
    use crate::task::state::TaskMeta;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    struct MockBehavior {
        delay_ms: u64,
        fail_times: AtomicUsize,
        disk_fail_times: AtomicUsize,
        wait_for_cancel: bool,
        attempts: AtomicUsize,
        artifact_count: usize,
        progress_start_delay_ms: u64,
        finish_delay_ms: u64,
    }

    struct MockRunner {
        behavior: Arc<MockBehavior>,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl TaskRunner for MockRunner {
        async fn run(&self, ctx: TaskRunCtx) -> BiliResult<TaskOutcome> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            let attempt = self.behavior.attempts.fetch_add(1, Ordering::SeqCst);
            ctx.reporter.phase(TaskPhase::Resolving);
            ctx.reporter.meta(TaskMeta {
                title: format!("mock {}", ctx.id),
                upper: "up".into(),
                cover_url: None,
                page_url: "https://example.com".into(),
                content_kind: crate::api::types::ContentKind::Video,
                entries: vec!["正片".into()],
                selected_entries: vec![0],
                duration_secs: Some(10),
            });
            ctx.reporter.phase(TaskPhase::Downloading);
            if self.behavior.progress_start_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.behavior.progress_start_delay_ms))
                    .await;
            }
            for i in 1..=3 {
                ctx.reporter.progress(
                    Progress {
                        downloaded: i * 30,
                        total: 90,
                        stage: DownloadStage::Downloading,
                        entries_done: 0,
                    },
                    300.0,
                );
                if self.behavior.wait_for_cancel {
                    tokio::select! {
                        _ = ctx.cancel.cancelled() => {
                            self.active.fetch_sub(1, Ordering::SeqCst);
                            return Err(BiliError::Cancelled);
                        }
                        _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                    }
                } else if self.behavior.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(self.behavior.delay_ms)).await;
                }
            }
            if self.behavior.wait_for_cancel {
                tokio::select! {
                    _ = ctx.cancel.cancelled() => {
                        self.active.fetch_sub(1, Ordering::SeqCst);
                        return Err(BiliError::Cancelled);
                    }
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                }
            }
            if self.behavior.finish_delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.behavior.finish_delay_ms)).await;
            }
            if attempt < self.behavior.disk_fail_times.load(Ordering::SeqCst) {
                self.active.fetch_sub(1, Ordering::SeqCst);
                return Err(BiliError::Disk(
                    "rename /tmp/a -> /tmp/b 失败: No such file or directory (os error 2)".into(),
                ));
            }
            if attempt < self.behavior.fail_times.load(Ordering::SeqCst) {
                self.active.fetch_sub(1, Ordering::SeqCst);
                return Err(BiliError::Network("mock failure".into()));
            }
            let mut artifacts = Vec::new();
            if self.behavior.artifact_count > 0 {
                let dir = ctx.settings.effective_download_dir();
                tokio::fs::create_dir_all(&dir).await?;
                for index in 0..self.behavior.artifact_count {
                    let path = dir.join(format!("{}-{attempt}-{index}.mp4", ctx.id));
                    let bytes = format!("artifact-{attempt}-{index}").into_bytes();
                    tokio::fs::write(&path, &bytes).await?;
                    let artifact = DownloadArtifact::media(
                        path,
                        bytes.len() as u64,
                        Some(index),
                        Some(format!("P{}", index + 1)),
                    );
                    ctx.reporter.output(artifact.clone()).await?;
                    artifacts.push(artifact);
                }
            }
            ctx.reporter.phase(TaskPhase::Merging);
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(TaskOutcome {
                output_path: Some(PathBuf::from("/tmp/mock.mp4")),
                file_size: 90,
                artifacts,
                ..Default::default()
            })
        }
    }

    struct Harness {
        _dir: tempfile::TempDir,
        settings: Settings,
        storage: Arc<Storage>,
        manager: TaskManager,
        max_active: Arc<AtomicUsize>,
    }

    async fn harness(settings: Settings, behavior: MockBehavior) -> Harness {
        harness_full(settings, behavior, None, None).await
    }

    async fn harness_with_artifact_failure(
        settings: Settings,
        behavior: MockBehavior,
        artifact_store_fail_once: Option<Arc<AtomicBool>>,
    ) -> Harness {
        harness_full(settings, behavior, artifact_store_fail_once, None).await
    }

    async fn harness_full(
        mut settings: Settings,
        behavior: MockBehavior,
        artifact_store_fail_once: Option<Arc<AtomicBool>>,
        enqueue_store_fail_once: Option<Arc<AtomicBool>>,
    ) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        settings.download_dir = Some(dir.path().join("downloads").to_string_lossy().into_owned());
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Arc::new(Storage::open_with_legacy(paths, None).unwrap());
        let client = Arc::new(BiliClient::new(None, Default::default()).unwrap());
        let behavior = Arc::new(behavior);
        let mock = Arc::new(MockRunner {
            behavior: Arc::clone(&behavior),
            active: Arc::new(AtomicUsize::new(0)),
            max_active: Arc::new(AtomicUsize::new(0)),
        });
        let max_active = Arc::clone(&mock.max_active);
        let settings_for_harness = settings.clone();
        let config = TaskManagerConfig {
            client,
            storage: Arc::clone(&storage),
            settings,
            ffmpeg: None,
            runner: Some(mock),
            download_http: Some(reqwest::Client::new()),
            artifact_store_fail_once,
            enqueue_store_fail_once,
            startup_defer_schedule: false,
        };
        let manager = TaskManager::start(config).await.unwrap();
        Harness {
            _dir: dir,
            settings: settings_for_harness.clone(),
            storage,
            manager,
            max_active,
        }
    }

    fn spec(bvid: &str) -> TaskSpec {
        TaskSpec::new(BiliId::Video { bvid: bvid.into() })
    }

    async fn wait_for<F: FnMut(&QueueSnapshot) -> bool>(
        manager: &TaskManager,
        mut f: F,
    ) -> QueueSnapshot {
        for _ in 0..400 {
            let snap = manager.snapshot();
            if f(&snap) {
                return snap;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "timeout waiting for snapshot condition; last = {:?}",
            manager.snapshot()
        );
    }

    fn behavior_default() -> MockBehavior {
        MockBehavior {
            delay_ms: 0,
            fail_times: AtomicUsize::new(0),
            disk_fail_times: AtomicUsize::new(0),
            wait_for_cancel: false,
            attempts: AtomicUsize::new(0),
            artifact_count: 0,
            progress_start_delay_ms: 0,
            finish_delay_ms: 0,
        }
    }

    #[tokio::test]
    async fn state_flow_reaches_done_and_writes_history() {
        let h = harness(Settings::default(), behavior_default()).await;
        let ids = h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        assert_eq!(ids.len(), 1);
        let snap = wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        let view = &snap.tasks[0];
        assert_eq!(view.phase, TaskPhase::Done);
        assert_eq!(view.file_size, Some(90));
        assert!(
            view.output_path
                .as_deref()
                .unwrap_or_default()
                .ends_with("mock.mp4")
        );
        h.manager.flush().await.unwrap();
        let history = h.storage.db().list_history(&Default::default()).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "done");
        // 队列行持久化
        assert_eq!(h.storage.db().load_tasks().unwrap().len(), 1);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn transient_disk_failure_auto_retries_once() {
        let mut b = behavior_default();
        b.disk_fail_times = AtomicUsize::new(1);
        let h = harness(Settings::default(), b).await;
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        assert_eq!(
            snap.tasks[0].phase,
            TaskPhase::Done,
            "瞬时 disk 错误应自动重试成功"
        );
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrency_respects_settings() {
        let settings = Settings {
            max_concurrent: 2,
            ..Default::default()
        };
        let mut b = behavior_default();
        b.delay_ms = 40;
        let h = harness(settings, b).await;
        let specs: Vec<TaskSpec> = (0..5).map(|i| spec(&format!("BV{i}"))).collect();
        h.manager.enqueue(specs).await.unwrap();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        assert_eq!(snap.tasks.len(), 5);
        assert!(
            h.max_active.load(Ordering::SeqCst) <= 2,
            "并发超限: {}",
            h.max_active.load(Ordering::SeqCst)
        );
        assert_eq!(h.max_active.load(Ordering::SeqCst), 2, "应达到并发上限");
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn pause_then_resume() {
        let mut b = behavior_default();
        b.wait_for_cancel = true;
        let h = harness(Settings::default(), b).await;
        let ids = h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let id = ids[0].clone();
        wait_for(&h.manager, |s| s.tasks[0].phase == TaskPhase::Downloading).await;
        h.manager.pause(&id).await.unwrap();
        let snap = wait_for(&h.manager, |s| s.tasks[0].phase == TaskPhase::Paused).await;
        assert!(snap.tasks[0].no_speed());
        // 数据已持久化
        h.manager.flush().await.unwrap();
        assert_eq!(h.storage.db().load_tasks().unwrap()[0].status, "paused");
        // 继续：mock 的 wait_for_cancel 仍会一直等取消 → 用新行为不合适；这里只验证 Resume 变回 Queued/Resolving
        h.manager.resume(&id).await.unwrap();
        wait_for(&h.manager, |s| s.tasks[0].phase != TaskPhase::Paused).await;
        h.manager.cancel(&id).await.unwrap();
        wait_for(&h.manager, |s| s.tasks[0].phase == TaskPhase::Cancelled).await;
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn idle_queue_has_no_periodic_wakeups() {
        let h = harness(Settings::default(), behavior_default()).await;
        let mut snapshots = h.manager.subscribe_snapshot();
        snapshots.borrow_and_update();
        // 空闲且无待办时不得安排周期唤醒（design D10/7.4）。
        assert!(
            tokio::time::timeout(Duration::from_millis(1200), snapshots.changed())
                .await
                .is_err(),
            "空闲队列不应周期性发布快照"
        );
        // 事件到达后立即恢复响应（无固定 interval 延迟）。
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn idle_queue_does_not_republish_snapshot() {
        let h = harness(Settings::default(), behavior_default()).await;
        let mut snapshots = h.manager.subscribe_snapshot();
        snapshots.borrow_and_update();
        assert!(
            tokio::time::timeout(Duration::from_millis(350), snapshots.changed())
                .await
                .is_err(),
            "空闲队列不应产生新快照修订"
        );
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn burst_progress_is_coalesced_to_one_tick() {
        let mut behavior = behavior_default();
        behavior.progress_start_delay_ms = 80;
        behavior.finish_delay_ms = 500;
        let h = harness(Settings::default(), behavior).await;
        h.manager.enqueue(vec![spec("BV-coalesce")]).await.unwrap();
        wait_for(&h.manager, |snapshot| {
            snapshot.tasks[0].phase == TaskPhase::Downloading
        })
        .await;
        let mut snapshots = h.manager.subscribe_snapshot();
        snapshots.borrow_and_update();
        let deadline = Instant::now() + Duration::from_millis(250);
        let mut revisions = 0;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            if tokio::time::timeout(deadline - now, snapshots.changed())
                .await
                .is_ok()
            {
                revisions += 1;
                snapshots.borrow_and_update();
            } else {
                break;
            }
        }
        assert_eq!(revisions, 1, "同一 tick 内的多次进度应合并为一个修订");
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn pause_state_is_published_without_waiting_for_progress_tick() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let ids = h.manager.enqueue(vec![spec("BV-pause-now")]).await.unwrap();
        wait_for(&h.manager, |snapshot| {
            snapshot.tasks[0].phase == TaskPhase::Downloading
        })
        .await;
        let mut snapshots = h.manager.subscribe_snapshot();
        h.manager.pause(&ids[0]).await.unwrap();
        let deadline = Instant::now() + Duration::from_millis(80);
        loop {
            let now = Instant::now();
            assert!(now < deadline, "暂停状态不应等待 100ms 进度 tick");
            tokio::time::timeout(deadline - now, snapshots.changed())
                .await
                .expect("暂停状态应立即产生快照")
                .expect("快照通道不应关闭");
            if snapshots.borrow().tasks[0].phase == TaskPhase::Paused {
                break;
            }
        }
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cancel_cleans_and_records_history() {
        let mut b = behavior_default();
        b.wait_for_cancel = true;
        let h = harness(Settings::default(), b).await;
        let ids = h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let id = ids[0].clone();
        wait_for(&h.manager, |s| s.tasks[0].phase == TaskPhase::Downloading).await;
        h.manager.cancel(&id).await.unwrap();
        wait_for(&h.manager, |s| s.tasks[0].phase == TaskPhase::Cancelled).await;
        h.manager.flush().await.unwrap();
        let history = h.storage.db().list_history(&Default::default()).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "cancelled");
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn retry_reuses_same_row() {
        let mut b = behavior_default();
        b.fail_times = AtomicUsize::new(1);
        let h = harness(Settings::default(), b).await;
        let ids = h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let id = ids[0].clone();
        wait_for(&h.manager, |s| s.tasks[0].phase == TaskPhase::Failed).await;
        assert_eq!(h.manager.snapshot().tasks.len(), 1);
        h.manager.retry(&id).await.unwrap();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.len() == 1 && s.tasks[0].phase == TaskPhase::Done
        })
        .await;
        assert_eq!(snap.tasks[0].id, id, "重试必须复用原行");
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn artifact_checkpoints_keep_all_streams_and_total_size() {
        let mut behavior = behavior_default();
        behavior.artifact_count = 3;
        let h = harness(Settings::default(), behavior).await;
        h.manager.enqueue(vec![spec("BV-artifacts")]).await.unwrap();
        let snap = wait_for(&h.manager, |snapshot| {
            snapshot
                .tasks
                .first()
                .is_some_and(|task| task.phase == TaskPhase::Done)
        })
        .await;
        let task = &snap.tasks[0];
        assert_eq!(task.artifacts.len(), 3);
        assert_eq!(
            task.file_size,
            Some(task.artifacts.iter().map(|artifact| artifact.size).sum())
        );
        assert_eq!(
            task.output_path.as_deref(),
            Some(task.artifacts[0].path.as_str())
        );
        let history = h.storage.db().list_history(&Default::default()).unwrap();
        let stored: Vec<DownloadArtifact> = serde_json::from_str(&history[0].artifacts).unwrap();
        assert_eq!(stored.len(), 3);
        assert_eq!(history[0].file_size, Some(task.file_size.unwrap() as i64));
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_artifact_checkpoint_does_not_commit_in_memory_record() {
        let mut behavior = behavior_default();
        behavior.artifact_count = 1;
        let fail_once = Arc::new(AtomicBool::new(true));
        let h = harness_with_artifact_failure(Settings::default(), behavior, Some(fail_once)).await;
        h.manager
            .enqueue(vec![spec("BV-checkpoint-fail")])
            .await
            .unwrap();
        let snap = wait_for(&h.manager, |snapshot| {
            snapshot
                .tasks
                .first()
                .is_some_and(|task| task.phase == TaskPhase::Failed)
        })
        .await;
        let task = &snap.tasks[0];
        assert!(
            task.artifacts.is_empty(),
            "写库失败时不得把未确认产物留在内存"
        );
        assert!(
            task.error
                .as_ref()
                .is_some_and(|error| error.describe().contains("文件已保存但记录失败"))
        );
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn recovery_marks_active_as_paused_with_progress() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Arc::new(Storage::open_with_legacy(paths, None).unwrap());
        // 造一条「退出时正在下载」的记录
        let mut view = TaskView::new(spec("BVrecover"));
        view.phase = TaskPhase::Downloading;
        view.title = "恢复任务".into();
        view.progress = Progress {
            downloaded: 4096,
            total: 8192,
            stage: DownloadStage::Downloading,
            entries_done: 1,
        };
        storage.db().upsert_task(&task_record(&view, 0)).unwrap();
        let client = Arc::new(BiliClient::new(None, Default::default()).unwrap());
        let manager = TaskManager::start(TaskManagerConfig {
            client,
            storage: Arc::clone(&storage),
            settings: Settings::default(),
            ffmpeg: None,
            runner: Some(Arc::new(MockRunner {
                behavior: Arc::new(behavior_default()),
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
            })),
            download_http: Some(reqwest::Client::new()),
            artifact_store_fail_once: None,
            enqueue_store_fail_once: None,
            startup_defer_schedule: false,
        })
        .await
        .unwrap();
        let snap = manager.snapshot();
        assert_eq!(snap.tasks.len(), 1);
        let t = &snap.tasks[0];
        assert_eq!(t.phase, TaskPhase::Paused, "退出时进行中的任务应恢复为暂停");
        assert!(t.restored);
        assert_eq!(t.progress.downloaded, 4096, "进度不应丢失");
        assert_eq!(t.progress.entries_done, 1);
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn recovery_recomputes_non_contiguous_completed_entries() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let entry2 = downloads.join("p3.mp4");
        let entry12 = downloads.join("p13.mp4");
        std::fs::write(&entry2, b"p3").unwrap();
        std::fs::write(&entry12, b"p13").unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Arc::new(Storage::open_with_legacy(paths, None).unwrap());
        let mut spec = spec("BV-gap");
        spec.selected_entries = Some(vec![2, 7, 12]);
        let mut view = TaskView::new(spec);
        view.phase = TaskPhase::Downloading;
        view.selected_entries = vec![2, 7, 12];
        view.artifacts = vec![
            DownloadArtifact::media(entry2, 2, Some(2), Some("P3".into())),
            DownloadArtifact::media(
                downloads.join("missing-p8.mp4"),
                3,
                Some(7),
                Some("P8".into()),
            ),
            DownloadArtifact::media(entry12, 3, Some(12), Some("P13".into())),
        ];
        storage.db().upsert_task(&task_record(&view, 0)).unwrap();

        let settings = Settings {
            download_dir: Some(downloads.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let manager = TaskManager::start(TaskManagerConfig {
            client: Arc::new(BiliClient::new(None, Default::default()).unwrap()),
            storage,
            settings,
            ffmpeg: None,
            runner: Some(Arc::new(MockRunner {
                behavior: Arc::new(behavior_default()),
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
            })),
            download_http: Some(reqwest::Client::new()),
            artifact_store_fail_once: None,
            enqueue_store_fail_once: None,
            startup_defer_schedule: false,
        })
        .await
        .unwrap();

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.tasks[0].artifacts.len(), 3);
        assert_eq!(
            snapshot.tasks[0].progress.entries_done, 2,
            "缺失中间条目时只统计有效产物对应的两个条目"
        );
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn clear_finished_keeps_history() {
        let h = harness(Settings::default(), behavior_default()).await;
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        let outcome = h.manager.clear_finished().await.unwrap();
        assert_eq!(outcome.removed, 1);
        assert!(outcome.failed.is_empty());
        assert!(h.manager.snapshot().tasks.is_empty());
        h.manager.flush().await.unwrap();
        assert_eq!(h.storage.db().load_tasks().unwrap().len(), 0);
        assert_eq!(h.storage.db().history_count().unwrap(), 1, "历史应保留");
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn remove_queued_task() {
        let h = harness(
            Settings {
                max_concurrent: 1,
                ..Default::default()
            },
            behavior_default(),
        )
        .await;
        // 先占满一个并发，再入队第二个（排队中）
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let ids = h.manager.enqueue(vec![spec("BV2")]).await.unwrap();
        let second = ids[0].clone();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.len() == 2 && s.tasks.iter().any(|t| t.phase == TaskPhase::Queued)
        })
        .await;
        let queued = snap
            .tasks
            .iter()
            .find(|t| t.phase == TaskPhase::Queued)
            .unwrap()
            .id
            .clone();
        h.manager.remove(&queued).await.unwrap();
        let snap = h.manager.snapshot();
        assert_eq!(snap.tasks.len(), 1);
        assert!(!snap.tasks.iter().any(|t| t.id == second || t.id == queued));
        h.manager.shutdown().await.unwrap();
    }

    fn spec_in(bvid: &str, dir: &std::path::Path) -> TaskSpec {
        let mut s = spec(bvid);
        s.output_dir = Some(dir.to_path_buf());
        s
    }

    /// 轮询等待条件成立，返回等待耗时。
    async fn wait_until<F: FnMut() -> bool>(mut f: F, label: &str) -> Duration {
        let start = Instant::now();
        for _ in 0..600 {
            if f() {
                return start.elapsed();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timeout waiting for {label}");
    }

    #[tokio::test]
    async fn cleanup_targets_only_the_task_that_finished() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let out = h._dir.path().join("shared-out");
        let ids = h
            .manager
            .enqueue(vec![spec_in("BVA", &out), spec_in("BVB", &out)])
            .await
            .unwrap();
        let (task_a, task_b) = (ids[0].clone(), ids[1].clone());
        wait_for(&h.manager, |s| {
            s.tasks.len() == 2 && s.tasks.iter().all(|t| t.phase == TaskPhase::Downloading)
        })
        .await;
        let parent = out.join(".bilidown-tmp");
        for id in [&task_a, &task_b] {
            let dir = parent.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("entry-0.video.m4s"), b"breakpoint-data").unwrap();
        }
        // 取消 A：只允许删除 A 自己的临时目录（旧实现会递归删除共享父目录，导致 B 断点消失）。
        h.manager.cancel(&task_a).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Cancelled)
        })
        .await;
        wait_until(|| !parent.join(&task_a).exists(), "A 任务临时目录被清理").await;
        assert_eq!(
            std::fs::read(parent.join(&task_b).join("entry-0.video.m4s")).unwrap(),
            b"breakpoint-data",
            "B 的断点数据必须保持完整"
        );
        // 暂停 B 后，再对 A 做清理动作也不得破坏 B 的断点。
        h.manager.pause(&task_b).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_b && t.phase == TaskPhase::Paused)
        })
        .await;
        assert!(parent.join(&task_b).join("entry-0.video.m4s").exists());
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn removing_running_task_schedules_next_queued() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(
            Settings {
                max_concurrent: 1,
                ..Default::default()
            },
            behavior,
        )
        .await;
        let task_a = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        let task_b = h.manager.enqueue(vec![spec("BV2")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.manager.remove(&task_a).await.unwrap();
        // 移除运行任务后释放并发资源并自动调度下一项，无需用户再次点击开始。
        wait_for(&h.manager, |s| {
            s.tasks.len() == 1
                && s.tasks
                    .iter()
                    .any(|t| t.id == task_b && t.phase.is_active())
        })
        .await;
        assert!(h.manager.snapshot().tasks.iter().all(|t| t.id != task_a));
        h.manager.cancel(&task_b).await.unwrap();
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn removed_task_late_worker_events_do_not_resurrect() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let task_a = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.manager.remove(&task_a).await.unwrap();
        assert!(h.manager.snapshot().tasks.is_empty());
        // worker 的迟到进度/阶段/产物事件不得重建任务或写回任务记录。
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            h.manager.snapshot().tasks.is_empty(),
            "迟到事件不得复活已移除任务"
        );
        h.manager.flush().await.unwrap();
        assert_eq!(h.storage.db().load_tasks().unwrap().len(), 0);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_worker_is_terminated_after_convergence_window() {
        let mut behavior = behavior_default();
        // 长 delay 期间不响应取消，模拟卡住的 worker。
        behavior.delay_ms = 10_000;
        let h = harness(Settings::default(), behavior).await;
        let task_a = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.manager.cancel(&task_a).await.unwrap();
        // 历史写入发生在 worker 收敛之后，用来证明超时后确实被终止并收尾。
        let elapsed = wait_until(
            || {
                h.storage
                    .db()
                    .list_history(&Default::default())
                    .map(|rows| rows.iter().any(|r| r.status == "cancelled"))
                    .unwrap_or(false)
            },
            "取消收敛后写入历史",
        )
        .await;
        assert!(
            elapsed >= CANCEL_CONVERGE - Duration::from_millis(200),
            "收敛过早（{elapsed:?}），未覆盖超时路径"
        );
        assert!(elapsed < Duration::from_secs(6), "收敛过慢：{elapsed:?}");
        assert_eq!(h.manager.snapshot().tasks[0].phase, TaskPhase::Cancelled);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_uses_task_owned_dir_after_download_dir_changes() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let old_dir = h._dir.path().join("downloads");
        let task_a = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        let old_task_dir = old_dir.join(".bilidown-tmp").join(&task_a);
        std::fs::create_dir_all(&old_task_dir).unwrap();
        std::fs::write(old_task_dir.join("entry-0.video.m4s"), b"x").unwrap();
        let new_dir = h._dir.path().join("new-downloads");
        let settings = Settings {
            download_dir: Some(new_dir.to_string_lossy().into_owned()),
            ..Default::default()
        };
        h.manager.set_settings(settings).await.unwrap();
        h.manager.cancel(&task_a).await.unwrap();
        wait_until(
            || !old_task_dir.exists(),
            "按任务实际目录清理旧下载目录里的断点",
        )
        .await;
        assert!(!new_dir.join(".bilidown-tmp").join(&task_a).exists());
        h.manager.shutdown().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_failure_is_retried_without_blocking_schedule() {
        use std::os::unix::fs::PermissionsExt;
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(
            Settings {
                max_concurrent: 1,
                ..Default::default()
            },
            behavior,
        )
        .await;
        let task_a = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        let task_b = h.manager.enqueue(vec![spec("BV2")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        let downloads = h._dir.path().join("downloads");
        let task_dir = downloads.join(".bilidown-tmp").join(&task_a);
        std::fs::create_dir_all(task_dir.join("nested")).unwrap();
        std::fs::write(task_dir.join("nested/part.m4s"), b"x").unwrap();
        let mut perms = std::fs::metadata(&task_dir).unwrap().permissions();
        perms.set_mode(0o500);
        std::fs::set_permissions(&task_dir, perms).unwrap();
        h.manager.cancel(&task_a).await.unwrap();
        // 清理失败不得阻塞调度：B 仍然自动开始。
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_b && t.phase.is_active())
        })
        .await;
        assert!(task_dir.exists(), "清理失败时保留目录等待重试");
        let mut perms = std::fs::metadata(&task_dir).unwrap().permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(&task_dir, perms).unwrap();
        wait_until(|| !task_dir.exists(), "恢复权限后重试清理成功").await;
        h.manager.cancel(&task_b).await.unwrap();
        h.manager.shutdown().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_does_not_follow_symlinked_task_temp_dir() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let out = h._dir.path().join("out");
        let task_a = h.manager.enqueue(vec![spec_in("BVA", &out)]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        let victim = h._dir.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep.txt"), b"keep").unwrap();
        let parent = out.join(".bilidown-tmp");
        std::fs::create_dir_all(&parent).unwrap();
        std::os::unix::fs::symlink(&victim, parent.join(&task_a)).unwrap();
        h.manager.cancel(&task_a).await.unwrap();
        wait_until(|| !parent.join(&task_a).exists(), "符号链接本身被移除").await;
        assert!(
            victim.join("keep.txt").exists(),
            "清理不得跟随符号链接删除外部目录"
        );
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn completed_task_cleans_own_temp_and_keeps_published_files() {
        let mut behavior = behavior_default();
        behavior.delay_ms = 80;
        behavior.artifact_count = 1;
        let h = harness(Settings::default(), behavior).await;
        let downloads = h._dir.path().join("downloads");
        let task_a = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == task_a && t.phase == TaskPhase::Downloading)
        })
        .await;
        let task_dir = downloads.join(".bilidown-tmp").join(&task_a);
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("entry-0.video.m4s"), b"x").unwrap();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        let artifact = snap.tasks[0].artifacts.first().map(|a| a.path.clone());
        wait_until(|| !task_dir.exists(), "完成任务临时目录被清理").await;
        if let Some(path) = artifact {
            assert!(
                std::path::Path::new(&path).exists(),
                "已发布产物不得因清理被删除"
            );
        }
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrency_three_never_exceeds_setting() {
        let settings = Settings {
            max_concurrent: 3,
            ..Default::default()
        };
        let mut behavior = behavior_default();
        behavior.delay_ms = 30;
        let h = harness(settings, behavior).await;
        let specs: Vec<TaskSpec> = (0..8).map(|i| spec(&format!("BV{i}"))).collect();
        h.manager.enqueue(specs).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks.len() == 8 && s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        assert!(
            h.max_active.load(Ordering::SeqCst) <= 3,
            "并发超限: {}",
            h.max_active.load(Ordering::SeqCst)
        );
        assert_eq!(h.max_active.load(Ordering::SeqCst), 3);
        h.manager.shutdown().await.unwrap();
    }

    fn restart_with(
        h: &Harness,
        behavior: MockBehavior,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TaskManager> + '_>> {
        let storage = Arc::clone(&h.storage);
        let settings = h.settings.clone();
        let runner = Arc::new(MockRunner {
            behavior: Arc::new(behavior),
            active: Arc::new(AtomicUsize::new(0)),
            max_active: Arc::new(AtomicUsize::new(0)),
        });
        Box::pin(async move {
            TaskManager::start(TaskManagerConfig {
                client: Arc::new(BiliClient::new(None, Default::default()).unwrap()),
                storage,
                settings,
                ffmpeg: None,
                runner: Some(runner),
                download_http: Some(reqwest::Client::new()),
                artifact_store_fail_once: None,
                enqueue_store_fail_once: None,
                startup_defer_schedule: false,
            })
            .await
            .unwrap()
        })
    }

    #[tokio::test]
    async fn task_metadata_survives_restart_for_done_and_paused() {
        // 完成任务的作者/条目/选择重启后保留（D7/R8 回归）。
        let h = harness(Settings::default(), behavior_default()).await;
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        assert_eq!(snap.tasks[0].upper, "up");
        h.manager.shutdown().await.unwrap();
        let manager = restart_with(&h, behavior_default()).await;
        let restored = manager.snapshot();
        assert_eq!(restored.tasks.len(), 1);
        assert_eq!(restored.tasks[0].phase, TaskPhase::Done);
        assert_eq!(restored.tasks[0].upper, "up", "作者必须从元信息恢复");
        assert_eq!(restored.tasks[0].entries, vec!["正片".to_string()]);
        assert_eq!(restored.tasks[0].selected_entries, vec![0]);
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn task_metadata_survives_restart_for_paused() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let id = h.manager.enqueue(vec![spec("BV2")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == id && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.manager.pause(&id).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == id && t.phase == TaskPhase::Paused)
        })
        .await;
        h.manager.shutdown().await.unwrap();
        let manager = restart_with(&h, behavior_default()).await;
        let restored = manager.snapshot();
        assert_eq!(restored.tasks[0].phase, TaskPhase::Paused);
        assert_eq!(restored.tasks[0].upper, "up");
        assert!(restored.tasks[0].progress.downloaded > 0, "暂停进度应保留");
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn corrupt_or_unknown_metadata_keeps_task_without_fabricating() {
        for metadata in ["{not json", r#"{"version":99,"upper":"未来作者"}"#] {
            let dir = tempfile::tempdir().unwrap();
            let paths = AppPaths::new(dir.path().join("data"));
            let storage = Arc::new(Storage::open_with_legacy(paths, None).unwrap());
            let mut view = TaskView::new(spec("BVcorrupt"));
            view.phase = TaskPhase::Done;
            view.title = "可恢复任务".into();
            let mut record = task_record(&view, 0);
            record.metadata = metadata.into();
            storage.db().upsert_task(&record).unwrap();
            let manager = TaskManager::start(TaskManagerConfig {
                client: Arc::new(BiliClient::new(None, Default::default()).unwrap()),
                storage,
                settings: Settings::default(),
                ffmpeg: None,
                runner: Some(Arc::new(MockRunner {
                    behavior: Arc::new(behavior_default()),
                    active: Arc::new(AtomicUsize::new(0)),
                    max_active: Arc::new(AtomicUsize::new(0)),
                })),
                download_http: Some(reqwest::Client::new()),
                artifact_store_fail_once: None,
                enqueue_store_fail_once: None,
                startup_defer_schedule: false,
            })
            .await
            .unwrap();
            let snap = manager.snapshot();
            assert_eq!(
                snap.tasks.len(),
                1,
                "元信息损坏/未知不得丢弃任务：{metadata}"
            );
            assert_eq!(snap.tasks[0].title, "可恢复任务");
            assert!(snap.tasks[0].upper.is_empty(), "不得伪造作者：{metadata}");
            manager.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn enqueue_persistence_failure_leaves_no_partial_tasks_or_workers() {
        let fail = Arc::new(AtomicBool::new(true));
        let h = harness_full(
            Settings::default(),
            behavior_default(),
            None,
            Some(Arc::clone(&fail)),
        )
        .await;
        let result = h.manager.enqueue(vec![spec("BV1"), spec("BV2")]).await;
        assert!(result.is_err(), "写库失败必须返回错误");
        assert!(h.manager.snapshot().tasks.is_empty(), "不得发布部分任务");
        assert_eq!(h.storage.db().load_tasks().unwrap().len(), 0);
        assert_eq!(h.storage.db().history_count().unwrap(), 0);
        // 注入只生效一次次；恢复后可正常入队。
        let ids = h.manager.enqueue(vec![spec("BV3")]).await.unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(h.manager.snapshot().tasks.len(), 1);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn progress_flush_failure_is_visible_and_retryable() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let id = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == id && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.storage.db().fail_bulk_insert_at(0);
        let error = h.manager.flush().await.unwrap_err();
        assert!(matches!(error, BiliError::Disk(_)));
        wait_until(
            || {
                h.manager
                    .snapshot()
                    .tasks
                    .iter()
                    .any(|t| t.id == id && t.persist_error.is_some())
            },
            "进度保存失败可见",
        )
        .await;
        let snap = h.manager.snapshot();
        let task = snap.tasks.iter().find(|t| t.id == id).unwrap();
        assert!(
            task.persist_error
                .as_deref()
                .is_some_and(|e| e.contains("保存失败")),
            "进度保存失败必须可见：{:?}",
            task.persist_error
        );
        // 重试只写记录：成功后清除错误并保存最新进度，不重新下载。
        h.manager.retry_persistence().await.unwrap();
        wait_until(
            || {
                h.manager
                    .snapshot()
                    .tasks
                    .iter()
                    .any(|t| t.id == id && t.persist_error.is_none())
            },
            "重试后清除持久化错误",
        )
        .await;
        let stored = h
            .storage
            .db()
            .load_tasks()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let stored_progress: crate::api::types::Progress =
            serde_json::from_str(&stored.progress).unwrap();
        assert!(stored_progress.downloaded > 0);
        h.manager.cancel(&id).await.unwrap();
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn completion_transaction_failure_is_visible_and_retryable() {
        let h = harness(Settings::default(), behavior_default()).await;
        h.storage.db().fail_next_history_once();
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        h.manager.flush().await.ok();
        let snap = wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Done)
        })
        .await;
        assert!(
            snap.tasks[0]
                .persist_error
                .as_deref()
                .is_some_and(|e| e.contains("保存失败")),
            "完成事务失败必须可见"
        );
        assert_eq!(
            h.storage.db().history_count().unwrap(),
            0,
            "失败时历史不得半写"
        );
        h.manager.retry_persistence().await.unwrap();
        assert_eq!(h.storage.db().history_count().unwrap(), 1);
        wait_until(
            || {
                h.manager
                    .snapshot()
                    .tasks
                    .iter()
                    .all(|t| t.persist_error.is_none())
            },
            "完成事务重试后清除错误",
        )
        .await;
        // 再次重试不得重复写历史。
        h.manager.retry_persistence().await.unwrap();
        assert_eq!(h.storage.db().history_count().unwrap(), 1);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_task_history_failure_is_retryable_without_duplicates() {
        let mut behavior = behavior_default();
        behavior.fail_times = AtomicUsize::new(1);
        let h = harness(Settings::default(), behavior).await;
        h.storage.db().fail_next_history_once();
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks.iter().all(|t| t.phase == TaskPhase::Failed)
        })
        .await;
        wait_until(
            || {
                h.manager
                    .snapshot()
                    .tasks
                    .iter()
                    .any(|t| t.persist_error.is_some())
            },
            "失败历史写入错误可见",
        )
        .await;
        h.manager.retry_persistence().await.unwrap();
        assert_eq!(h.storage.db().history_count().unwrap(), 1);
        h.manager.retry_persistence().await.unwrap();
        assert_eq!(h.storage.db().history_count().unwrap(), 1);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepare_shutdown_failure_keeps_retry_capability() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(Settings::default(), behavior).await;
        let id = h.manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == id && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.storage.db().fail_bulk_insert_at(0);
        assert!(
            h.manager.prepare_shutdown().await.is_err(),
            "保存失败必须返回错误"
        );
        assert_eq!(h.manager.snapshot().tasks.len(), 1, "失败时不得丢弃状态");
        // 重试成功后可以正常准备并最终退出。
        h.manager.retry_persistence().await.unwrap();
        h.manager.prepare_shutdown().await.unwrap();
        h.manager.finalize_shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn clear_inactive_removes_only_queued_paused_cancelled() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(
            Settings {
                max_concurrent: 1,
                ..Default::default()
            },
            behavior,
        )
        .await;
        let mut ids: Vec<TaskId> = Vec::new();
        for i in 0..4 {
            ids.push(
                h.manager
                    .enqueue(vec![spec(&format!("BV{i}"))])
                    .await
                    .unwrap()[0]
                    .clone(),
            );
        }
        let (running, queued, paused, cancelled) = (
            ids[0].clone(),
            ids[1].clone(),
            ids[2].clone(),
            ids[3].clone(),
        );
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == running && t.phase == TaskPhase::Downloading)
        })
        .await;
        h.manager.pause(&paused).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == paused && t.phase == TaskPhase::Paused)
        })
        .await;
        h.manager.cancel(&cancelled).await.unwrap();
        wait_for(&h.manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == cancelled && t.phase == TaskPhase::Cancelled)
        })
        .await;
        let outcome = h.manager.clear_inactive().await.unwrap();
        assert_eq!(outcome.removed, 3, "排队/暂停/已取消都应移除");
        assert!(outcome.failed.is_empty());
        let snap = h.manager.snapshot();
        assert_eq!(snap.tasks.len(), 1, "运行中的任务必须保留");
        assert_eq!(snap.tasks[0].id, running);
        assert_eq!(snap.tasks[0].phase, TaskPhase::Downloading);
        assert_eq!(h.storage.db().load_tasks().unwrap().len(), 1);
        let _ = queued;
        h.manager.cancel(&running).await.unwrap();
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn clear_inactive_reports_partial_failure_and_keeps_rows() {
        let mut behavior = behavior_default();
        behavior.wait_for_cancel = true;
        let h = harness(
            Settings {
                max_concurrent: 1,
                ..Default::default()
            },
            behavior,
        )
        .await;
        h.manager.enqueue(vec![spec("BV1")]).await.unwrap();
        let queued_a = h.manager.enqueue(vec![spec("BV2")]).await.unwrap()[0].clone();
        let queued_b = h.manager.enqueue(vec![spec("BV3")]).await.unwrap()[0].clone();
        wait_for(&h.manager, |s| {
            s.tasks.iter().any(|t| t.phase == TaskPhase::Downloading)
        })
        .await;
        // 第一次删除失败：该行保留并计入 failed，另一行成功移除。
        h.storage.db().fail_next_delete_once();
        let outcome = h.manager.clear_inactive().await.unwrap();
        assert_eq!(outcome.removed, 1);
        assert_eq!(outcome.failed.len(), 1);
        let snapshot = h.manager.snapshot();
        assert_eq!(snapshot.tasks.len(), 2, "失败行必须保留可见");
        assert!(
            snapshot
                .tasks
                .iter()
                .any(|t| t.id == queued_a || t.id == queued_b)
        );
        // 再次清理可移除剩余目标。
        let retry = h.manager.clear_inactive().await.unwrap();
        assert_eq!(retry.removed, 1);
        h.manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn deferred_schedule_waits_for_network_phase() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let storage = Arc::new(Storage::open_with_legacy(paths, None).unwrap());
        let manager = TaskManager::start(TaskManagerConfig {
            client: Arc::new(BiliClient::new(None, Default::default()).unwrap()),
            storage: Arc::clone(&storage),
            settings: Settings::default(),
            ffmpeg: None,
            runner: Some(Arc::new(MockRunner {
                behavior: Arc::new(behavior_default()),
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
            })),
            download_http: Some(reqwest::Client::new()),
            artifact_store_fail_once: None,
            enqueue_store_fail_once: None,
            startup_defer_schedule: true,
        })
        .await
        .unwrap();
        let id = manager.enqueue(vec![spec("BV1")]).await.unwrap()[0].clone();
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(
            manager.snapshot().tasks[0].phase,
            TaskPhase::Queued,
            "本地就绪阶段不得开始传输"
        );
        manager.enable_scheduling().await.unwrap();
        wait_for(&manager, |s| {
            s.tasks
                .iter()
                .any(|t| t.id == id && t.phase.is_active() || t.phase == TaskPhase::Done)
        })
        .await;
        manager.shutdown().await.unwrap();
    }

    trait NoSpeed {
        fn no_speed(&self) -> bool;
    }
    impl NoSpeed for TaskView {
        fn no_speed(&self) -> bool {
            self.speed_bps == 0.0
        }
    }

    #[allow(dead_code)]
    fn _keep_mutex_import() -> Mutex<()> {
        Mutex::new(())
    }
}
