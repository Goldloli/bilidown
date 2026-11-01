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
    DownloadArtifact, QueueSnapshot, TaskId, TaskMeta, TaskOutcome, TaskPhase, TaskResume,
    TaskView, WorkerEvent, completed_entry_indices, now_ms,
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
            sem: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            reduction_permits: Vec::new(),
            dirty: HashSet::new(),
            auto_retried: HashSet::new(),
            last_progress_flush: Instant::now(),
            seq: 0,
            snapshot_tx,
            snapshot_dirty: false,
            event_tx: event_tx.clone(),
            worker_tx,
            artifact_store_fail_once: config.artifact_store_fail_once,
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

    /// 清空已完成/失败/取消的任务（历史保留），返回移除数量。
    pub async fn clear_finished(&self) -> BiliResult<usize> {
        self.call(|reply| ManagerCommand::ClearFinished { reply })
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

    /// 优雅退出：暂停进行中任务并落盘，等待 worker 结束。
    pub async fn shutdown(&self) -> BiliResult<()> {
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
        reply: oneshot::Sender<BiliResult<usize>>,
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
    sem: Arc<Semaphore>,
    reduction_permits: Vec<OwnedSemaphorePermit>,
    dirty: HashSet<TaskId>,
    /// 已对哪个任务做过一次「瞬时磁盘错误」自动重试（避免死循环）。
    auto_retried: HashSet<TaskId>,
    last_progress_flush: Instant,
    seq: u64,
    snapshot_tx: watch::Sender<QueueSnapshot>,
    snapshot_dirty: bool,
    event_tx: broadcast::Sender<TaskEvent>,
    worker_tx: mpsc::UnboundedSender<WorkerEvent>,
    artifact_store_fail_once: Option<Arc<AtomicBool>>,
}

impl Inner {
    async fn run(
        mut self,
        mut cmd_rx: mpsc::Receiver<ManagerCommand>,
        mut worker_rx: mpsc::UnboundedReceiver<WorkerEvent>,
    ) {
        let mut ticker = tokio::time::interval(SNAPSHOT_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut pending_shutdown: Option<oneshot::Sender<BiliResult<()>>> = None;
        loop {
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
                _ = ticker.tick() => {
                    self.flush_progress_if_due().await;
                    self.publish_if_due();
                }
            }
        }
        self.shutdown_workers().await;
        self.flush_all().await;
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
    }

    fn publish_if_due(&mut self) {
        if self.snapshot_dirty {
            self.publish();
        }
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
                let mut ids = Vec::with_capacity(specs.len());
                let mut views = Vec::with_capacity(specs.len());
                for spec in specs {
                    let view = TaskView::new(spec);
                    ids.push(view.id.clone());
                    self.order.push(view.id.clone());
                    views.push(view);
                }
                for view in views {
                    let record_idx =
                        self.order.iter().position(|id| id == &view.id).unwrap_or(0) as i64;
                    self.tasks.insert(view.id.clone(), view.clone());
                    self.persist_record(task_record(&view, record_idx)).await;
                    self.emit(TaskEvent::Created { view });
                }
                self.publish();
                self.schedule();
                let _ = reply.send(Ok(ids));
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
                let n = self.clear_finished().await;
                let _ = reply.send(Ok(n));
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
                self.flush_all().await;
                let _ = reply.send(Ok(()));
            }
            ManagerCommand::SetSettings { settings, reply } => {
                let result = self.apply_settings(settings);
                let _ = reply.send(result);
            }
            ManagerCommand::SetClient { client, reply } => {
                self.client = client;
                let _ = reply.send(Ok(()));
            }
            ManagerCommand::Shutdown { reply } => {
                // 先应答前把进行中任务标记暂停并落盘，避免壳退出后丢状态
                for (id, control) in self.controls.iter() {
                    control.pause.store(true, Ordering::Relaxed);
                    control.cancel.cancel();
                    let _ = id;
                }
                for view in self.tasks.values_mut() {
                    if view.phase.is_active() {
                        view.phase = TaskPhase::Paused;
                        view.speed_bps = 0.0;
                        view.restored = true;
                        view.touch();
                    }
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
            if !self.workers.contains_key(id) {
                self.finish_cancelled(id).await;
            }
        } else {
            self.set_phase(id, TaskPhase::Cancelled).await?;
            self.finish_cancelled(id).await;
        }
        Ok(())
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
        self.persist_view(id).await;
        self.emit(TaskEvent::Updated {
            view: self.tasks[id].clone(),
        });
        Ok(())
    }

    async fn remove_task(&mut self, id: &str) -> BiliResult<()> {
        if !self.tasks.contains_key(id) {
            return Err(BiliError::Other(format!("task not found: {id}")));
        }
        if let Some(control) = self.controls.remove(id) {
            control.pause.store(false, Ordering::Relaxed);
            control.cancel.cancel();
        }
        self.workers.remove(id);
        self.tasks.remove(id);
        self.order.retain(|x| x != id);
        self.dirty.remove(id);
        self.cleanup_temp(id).await;
        let db = Arc::clone(self.storage.db());
        let task_id = id.to_string();
        let _ = spawn_blocking_db(move || db.delete_task(&task_id)).await;
        self.emit(TaskEvent::Removed { id: id.to_string() });
        self.publish();
        Ok(())
    }

    async fn clear_finished(&mut self) -> usize {
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
        for id in &ids {
            self.tasks.remove(id);
            self.controls.remove(id);
            self.dirty.remove(id);
            let db = Arc::clone(self.storage.db());
            let task_id = id.clone();
            let _ = spawn_blocking_db(move || db.delete_task(&task_id)).await;
            self.emit(TaskEvent::Removed { id: id.clone() });
        }
        self.order.retain(|id| self.tasks.contains_key(id));
        self.emit(TaskEvent::QueueChanged);
        self.publish();
        ids.len()
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
                    self.persist_view(&id).await;
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
                let result = self.record_artifact(&id, artifact).await;
                let _ = ack.send(result);
            }
            WorkerEvent::Finished { id, result, paused } => {
                self.workers.remove(&id);
                self.controls.remove(&id);
                self.enforce_permits();
                if !self.tasks.contains_key(&id) {
                    // 任务已被 remove：清理临时文件即可
                    self.cleanup_temp(&id).await;
                    return;
                }
                self.handle_finished(&id, result, paused).await;
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
                self.cleanup_temp(id).await;
                self.persist_view(id).await;
                self.write_history(id, "done").await;
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
                self.persist_view(id).await;
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
                self.persist_view(id).await;
                self.write_history(id, "cancelled").await;
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
                self.persist_view(id).await;
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
                self.persist_view(id).await;
                self.write_history(id, "failed").await;
                let view = self.tasks[id].clone();
                self.emit(TaskEvent::Failed { view, error: e });
            }
        }
    }

    async fn finish_cancelled(&mut self, id: &str) {
        self.cleanup_temp(id).await;
        self.persist_view(id).await;
        self.write_history(id, "cancelled").await;
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
        self.persist_view(id).await;
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
        self.persist_view(id).await;
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

    async fn persist_view(&mut self, id: &str) {
        let Some(view) = self.tasks.get(id).cloned() else {
            return;
        };
        let idx = self.order.iter().position(|x| x == id).unwrap_or(0) as i64;
        self.persist_record(task_record(&view, idx)).await;
    }

    async fn persist_record(&self, record: TaskRecord) {
        let db = Arc::clone(self.storage.db());
        if let Err(e) = spawn_blocking_db(move || db.upsert_task(&record)).await {
            tracing::warn!(error = %e, "persist task failed");
        }
    }

    async fn persist_record_checked(&self, record: TaskRecord) -> BiliResult<()> {
        let db = Arc::clone(self.storage.db());
        spawn_blocking_db(move || db.upsert_task(&record)).await
    }

    async fn flush_progress_if_due(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        if self.last_progress_flush.elapsed() < PROGRESS_FLUSH_INTERVAL {
            return;
        }
        self.flush_dirty().await;
    }

    async fn flush_dirty(&mut self) {
        let ids: Vec<TaskId> = self.dirty.drain().collect();
        let mut records = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(view) = self.tasks.get(id) {
                let idx = self.order.iter().position(|x| x == id).unwrap_or(0) as i64;
                records.push(task_record(view, idx));
            }
        }
        if records.is_empty() {
            return;
        }
        let db = Arc::clone(self.storage.db());
        if let Err(e) = spawn_blocking_db(move || {
            for r in &records {
                db.upsert_task(r)?;
            }
            Ok(())
        })
        .await
        {
            tracing::warn!(error = %e, "flush progress failed");
        }
        self.last_progress_flush = Instant::now();
    }

    async fn flush_all(&mut self) {
        self.dirty = self.tasks.keys().cloned().collect();
        self.flush_dirty().await;
    }

    async fn write_history(&self, id: &str, status: &str) {
        let Some(view) = self.tasks.get(id) else {
            return;
        };
        let record = history_record(view, status);
        let db = Arc::clone(self.storage.db());
        let limit = self.settings.history_limit;
        if let Err(e) = spawn_blocking_db(move || {
            db.insert_history(&record)?;
            db.prune_history(limit)?;
            Ok(())
        })
        .await
        {
            tracing::warn!(error = %e, "write history failed");
        }
        self.emit(TaskEvent::HistoryChanged);
    }

    async fn cleanup_temp(&self, id: &str) {
        // 应用缓存目录 + 输出目录下的同卷临时任务目录都清理（成功/取消/移除）
        let mut dirs: Vec<PathBuf> = vec![self.storage.tmp_dir().join(id)];
        if let Some(view) = self.tasks.get(id) {
            let out = view
                .spec
                .output_dir
                .clone()
                .unwrap_or_else(|| self.settings.effective_download_dir());
            let parent = out.join(".bilidown-tmp");
            dirs.push(parent.join(id));
            dirs.push(parent);
        }
        let _ = tokio::task::spawn_blocking(move || {
            for dir in &dirs {
                let _ = std::fs::remove_dir_all(dir);
            }
        })
        .await;
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
        storage: Arc<Storage>,
        manager: TaskManager,
        max_active: Arc<AtomicUsize>,
    }

    async fn harness(settings: Settings, behavior: MockBehavior) -> Harness {
        harness_with_artifact_failure(settings, behavior, None).await
    }

    async fn harness_with_artifact_failure(
        mut settings: Settings,
        behavior: MockBehavior,
        artifact_store_fail_once: Option<Arc<AtomicBool>>,
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
        let config = TaskManagerConfig {
            client,
            storage: Arc::clone(&storage),
            settings,
            ffmpeg: None,
            runner: Some(mock),
            download_http: Some(reqwest::Client::new()),
            artifact_store_fail_once,
        };
        let manager = TaskManager::start(config).await.unwrap();
        Harness {
            _dir: dir,
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
        let removed = h.manager.clear_finished().await.unwrap();
        assert_eq!(removed, 1);
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
