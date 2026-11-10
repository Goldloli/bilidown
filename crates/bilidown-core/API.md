# bilidown-core 接线 API（给 src-tauri / Tauri 壳）

> 版本：rebuild-desktop-client 第 2–3 组任务产出。core 无 UI 依赖；所有用户可见文案/图标由前端按枚举映射。
> 详细 rustdoc 见 `cargo doc -p bilidown-core --no-deps --open`。

## 1. 壳启动顺序（建议）

```rust
use bilidown_core::*;

// 1) 数据目录 + 迁移（v1 ~/.bilidown 自动迁移，失败不阻塞）
let paths = AppPaths::standard()?;                    // ~/Library/Application Support/bilidown（可用 BILIDOWN_DATA_DIR 覆盖）
let storage = Arc::new(Storage::open(paths)?);        // 建目录/打开 queue.sqlite（WAL）/迁移
let settings = storage.load_settings();               // 损坏自动备份恢复默认
let creds = storage.load_credentials();               // cookie + 设备凭据（0600）

// 2) HTTP 客户端（cookie 原地可替换，无需重建）
let client = Arc::new(BiliClient::new(settings.proxy.as_deref(), creds.stored())?);
client.apply_device(&creds.device);
client.activate_buvid().await;                        // 匿名风控凭据（失败仅日志）

// 3) 日志（写 <data>/logs/bilidown.log，按天滚动）
bilidown_core::logging::init(&storage.paths().logs_dir(), "info")?;

// 4) TaskManager（启动分阶段：先恢复队列但暂不调度，网络准备结束后再启用）
let mut config = TaskManagerConfig::new(client.clone(), storage.clone(), settings.clone());
config.startup_defer_schedule = true;   // design D5/5.2：本地就绪不等待外网
let manager = TaskManager::start(config).await?;

// 5) 网络阶段（壳负责 5 秒预算 + 代次保护）：先激活设备/恢复账号，再启用调度
client.activate_buvid().await;
let account = account::restore(&client, &storage).await;  // -> AccountState
account::persist_device(&client, &storage).await.ok();    // 持久化 buvid3/4/_uuid
manager.enable_scheduling().await?;                       // 无论成功/超时都应最终调用
let mut snapshot_rx = manager.subscribe_snapshot();   // watch<QueueSnapshot>
let mut event_rx = manager.subscribe_events();        // broadcast<TaskEvent>
```

## 2. 公开类型（`bilidown_core::{...}` / `bilidown_core::api::types::*`）

| 类型 | 说明 |
| --- | --- |
| `TaskId = String`（UUID） | 队列内所有操作的唯一寻址方式 |
| `TaskView` | 任务全量视图（serde，可直接 emit 给前端）：id/spec/phase/title/upper/cover_url/page_url/content_kind/entries/selected_entries/progress/speed_bps/output_path/file_size/error/restored/actual_quality/actual_codec/actual_audio/extras/persist_error/created_at/updated_at |
| `TaskMetadata` | 版本化持久化元信息（version/upper/entries/selected_entries/actual_*/extras）；tasks 表 `metadata` 列，旧记录 `{}` |
| `ClearOutcome` | 批量清理结果：`{removed, failed: Vec<TaskId>}`；部分失败行保留在队列 |
| `QueueSnapshot` | `{ seq, tasks: TaskView[], active_count, queued_count, paused_count, done_count, failed_count, total_speed_bps }`（seq 单调递增，前端可丢弃旧快照） |
| `TaskExtras` | 可选后处理产物：`{ subtitles: Vec<String>, cover: Option<String>, subtitle_error: Option<String>, cover_error: Option<String> }`；`TaskView.extras` 与 `TaskOutcome.extras` 均为 `#[serde(default)]`（旧数据兼容） |
| `TaskPhase` | `queued/resolving/downloading/merging/paused/done/failed/cancelled`（serde snake_case） |
| `TaskSpec` | 入队参数：`{ id: BiliId, selected_entries: Option<Vec<usize>>, page_hint: Option<usize>, quality: Option<Quality>, codec: Option<Codec>, audio_quality: Option<AudioQuality>, to_mp3, output_dir: Option<PathBuf>, note }`；`TaskSpec::new(id)` |
| `BiliId` | `{ kind: "video"/"bangumi"/"cheese"/"audio"/"audio_list"/"article", ... }`，Serde 往返可用 |
| `Quality(u32)` / `Codec` | qn 档位 / `avc|hevc|av1|unknown`（前端负责中文标签：1080P…） |
| `AudioQuality(u32)` / `AudioFormat` | abr / `m4a|flac|eac3|mp3` |
| `Progress` | `{ downloaded, total, stage: DownloadStage, entries_done }` |
| `BiliError` | 结构化错误（见 §5） |
| `Settings` | 见 `storage::Settings`（serde JSON，可直接给前端表单） |
| `AccountState` | `{ logged_in, user: UserInfo, expired, cookie_refresh_required, last_error: Option<BiliError> }` |
| `StoredCookies` / `DeviceCredentials` / `Credentials` | 凭据文件模型（`Credentials { cookies, device, updated_at }`） |

## 3. TaskManager API（全部 async，返回 `BiliResult<T>`）

```rust
manager.subscribe_snapshot() -> watch::Receiver<QueueSnapshot>   // 全量快照，≤10Hz
manager.subscribe_events()   -> broadcast::Receiver<TaskEvent>   // 重要事件立即推送
manager.snapshot()           -> QueueSnapshot                    // 当前值
manager.enqueue(Vec<TaskSpec>) -> Vec<TaskId>
manager.pause(&str)  manager.resume(&str)  manager.cancel(&str)
manager.retry(&str)  manager.remove(&str)
manager.clear_finished() -> ClearOutcome   // 清空已完成/失败/取消（历史保留）
manager.clear_inactive() -> ClearOutcome   // 清空排队/暂停/已取消（actor 内单批确定目标并统一补调度）
manager.pause_all() -> usize  manager.resume_all() -> usize  manager.retry_all_failed() -> usize
manager.flush() -> ()                       // 强制落盘（退出前；写库失败返回错误）
manager.set_settings(Settings) -> ()        // 并发/目录/ffmpeg/代理热更新
manager.set_client(Arc<BiliClient>) -> ()   // 代理变更后重建 client 时调用
manager.enable_scheduling() -> ()           // 启动分阶段：网络准备结束后启用传输调度
manager.retry_persistence() -> ()           // 重试待保存动作（完成事务/历史/任务记录）
manager.prepare_shutdown() -> ()            // 退出准备：暂停 + 保存；失败可重试且不退出
manager.finalize_shutdown() -> ()           // 最终退出（停止 worker/监督任务）
manager.shutdown() -> ()                    // 便捷组合：prepare + finalize（⌘Q/退出前必须调用）
```

**可选后处理（change add-download-features D2）**：设置开启 `download_subtitle`/`download_cover`
（默认 false）时，视频/番剧/课程在**最终视频落盘后**执行字幕/封面下载：
字幕写 `{视频文件名去扩展}.{lan}.srt`（多语言分别保存，URL `//` 自动补 `https:`），封面写
`{视频文件名去扩展}.jpg`（`@.jpg` 缩放优先、失败回退原图）；与视频同目录、`dedupe_in_dir` 防覆盖。
任一失败只 `tracing::warn` + 写入 `TaskExtras.*_error`，**主任务仍为 Done**；开关全关时不发任何请求。
`TaskView.extras` 随快照/Done 事件下发（注意：SQLite tasks 表目前不落 extras，重启恢复后为空，
需要持久化时由后续任务给 tasks 表加列）。

**状态机**：`Queued → Resolving → Downloading → Merging → Done`；旁路 `Paused/Failed/Cancelled`。
暂停=安全点（下载立即停并保留 `.part`；合并 kill 并清半成品，继续时重取直链续传/重合并）。
重试复用同一 task id，不新增重复行。移除不删除文件与历史。

## 4. 快照/事件 → 前端接线建议

- `queue://snapshot`：`snapshot_rx.changed().await` → `emit("queue://snapshot", &*rx.borrow_and_update())`
- `queue://events`：`event_rx.recv().await` → `emit("queue://events", &event)`（shell 也可先 `serde_json` 转换）
- `TaskEvent`（serde tag=`type`）：
  `created{view}` / `updated{view}` / `phase_changed{id,phase,view}` / `done{view}` / `failed{view,error}` /
  `cancelled{id}` / `paused{id}` / `resumed{id}` / `removed{id}` / `history_changed` / `queue_changed`
- 前端命令建议直接映射：`enqueue/pause/resume/cancel/retry/remove/clear_finished/pause_all/resume_all/retry_all_failed/set_settings`；
  其它命令自行组合：
  - 解析/入队：`api::parse::parse_input(&client, text)` → `ParsedInput { id, page_hint }`（多条用 `parse_many`）→ 构造 `TaskSpec::new(p.id)`，`page_hint` 用于默认勾选该 P
  - 详情/添加面板：`api::video::play_url(&client, &id, &entry, logged_in)` → `PlayUrl { qualities: Vec<VideoQuality{quality, streams: Vec<MediaStream{codec,width,height,frame_rate,...}>}>, audios: Vec<AudioTrack>, quality_tiers: Vec<QualityTier{quality,name,unavailable: None|not_provided|login_required|vip_required|paid_required|region}> }`；选流回退用 `media::quality::select(&play, StreamChoice{...})` 得到实际 `quality/codec/audio`（回写任务信息）
  - 内容信息：`api::video::resolve_content` / `api::audio::resolve_song|resolve_menu` / `api::article::fetch`；收藏夹：`api::favorite::list_folders|resources`
  - 登录：`api::login::QrLogin::start(&client)`（含 `matrix: QrMatrix{size, modules: Vec<bool>}`，前端 canvas 绘制）→ 前端每 1.5s `account::login_poll(&client, &storage, &mut flow)` → `LoginOutcome::{Pending(state), Success(AccountState), Ended(state)}`；`account::logout(&client,&storage)`；`account::refresh_session(&client,&storage,None,None)`（见 §6 限制）
  - 历史：直接读 `storage.db().list_history(&HistoryQuery{status,keyword,limit,offset})`；`delete_history/clear_history/history_count`；打开文件/Finder 由壳实现（core 不碰 UI/系统 opener）
  - 设置读写：`storage.load_settings()/save_settings(&s)`（`save_settings` 会校验）；`Settings::validate() -> Result<(), Vec<ValidationError{field,message}>>` 供前端行内报错；新增开关 `Settings.download_subtitle` / `Settings.download_cover`（默认 false，旧 settings.json 缺字段自动回退 false，无需迁移）
  - 字幕/封面（可选后处理，一般由管线自动完成；单独预览可用）：`api::subtitle::player_subtitles(&client, aid, cid)` → `Vec<SubtitleTrack{lan,lan_doc,subtitle_url}>`；`api::subtitle::download_srt` / `srt_filename` / `save_subtitles`；`media::cover::cover_url_candidates` / `download_cover` / `cover_filename`
  - ffmpeg：`media::ffmpeg::Ffmpeg::detect(settings.ffmpeg_path.as_deref())`（设置页「重新探测」直接用）
  - 日志：`logging::init(&storage.paths().logs_dir(), level)`；日志中不含 cookie 值

## 5. BiliError（serde，tag=`kind`）

`kind` ∈ `api | risk_control | auth | permission | region | vip_required | network | timeout | cdn | ffmpeg | disk | parse | invalid_input | cancelled | other`；
部分变体带 `detail`（`api` → `{code,message}`，newtype 变体 → 字符串）。辅助：`err.kind()`、`err.is_retryable()`、`err.is_cancelled()`、`err.describe()`。
前端文案/建议操作按 `kind` 映射（如 auth→重新扫码，vip_required→需大会员，ffmpeg→安装指引）。

## 6. 已知限制 / 后续

- cookie 自动续期与 `bili_ticket`/`_uuid` 初始化：**已实现**（RSA-OAEP correspondPath + cookie/refresh + confirm/refresh；`bili_ticket` HMAC-SHA256）。接线与失败语义见 §8。
- 限速、多连接分段下载、字幕/弹幕/NFO 属于后续 change（v2 Non-Goals）。
- `PlayUrl` 为单条目粒度：添加面板切换条目需重新调用 `play_url`（逐条目解析，符合 spec）。
- 历史记录写入在 manager 终态时自动完成；清理策略按 `Settings.history_limit` 自动裁剪。
- 字幕/封面仅做 fixture/wiremock 单测（不发起真实网络请求）；真实账号下的字幕/封面 E2E 在 tasks 6.2 验证。
- `TaskExtras` 目前**不写 SQLite**（tasks 表无 extras 列）；完成任务的 extras 在重启恢复后为空，若需持久化需追加迁移与 db.rs 字段（后续任务）。
- shell DTO 需要同步：`TaskDTO` 增加 `extras`（来自 `TaskView.extras`）；`SettingsDTO` 增加 `downloadSubtitle`/`downloadCover` 并在 DTO↔Settings 映射中往返（见 src-tauri/src/dto.rs）。

## 7. 内容源扩展（change add-download-features D3，capability `content-sources`）

> 定位：把「链接/ID → 列表 → 勾选 → 现有 TaskSpec 入队」的解析层全部放到 core。
> core **不引入新的任务类型**：每个选中条目仍由壳层构造独立 `TaskSpec`（视频/音频/专栏），
> 走现有队列/暂停/重试/历史。opus 图文导出暂不经过任务管线（复用 article 导出能力，壳层可直接调用）。

### 7.1 统一模型与 URL 识别（`api::source`）

```rust
use bilidown_core::api::source::{SourceItem, SourceKind, SourceCollection, SourcePage, SourceRef};

pub struct SourceItem { pub bili_id: String, pub kind: SourceKind, pub title: String,
                        pub cover: Option<String>, pub duration: Option<u64>, pub upper: Option<String> }
impl SourceItem {
    pub fn to_bili_id(&self) -> Option<BiliId>;   // Opus/readlist 返回 None（走 Markdown 导出）
}
pub struct SourceCollection { pub kind, pub id: String, pub title: String, pub cover: Option<String>,
                              pub upper: Option<String>, pub description: Option<String>, pub items: Vec<SourceItem> }
pub struct SourcePage { pub items: Vec<SourceItem>, pub page: u32, pub has_more: bool,
                        pub total: Option<u64>, pub skipped: usize }

pub fn parse_source(raw: &str) -> Option<SourceRef>;          // 纯字符串识别，不联网
pub async fn resolve_source(client: &BiliClient, raw: &str) -> BiliResult<SourceRef>; // 展开 b23.tv
pub fn require_login(client: &BiliClient) -> BiliResult<()>;  // 未登录 → NotLoggedIn（不发起请求）
```

`SourceRef`（serde tag=`kind`）覆盖：`watch_later` / `favorite{mid,fid?}` / `user_space{mid,target?}` /
`user_opus{mid}` / `user_audio{mid}` / `opus{id}` / `opus_list{readlist_id}` / `music_list{menu_id}` / `music{song_id}`。
常见链接：`space.bilibili.com/{mid}`、`/lists/{id}`、`/favlist?fid=`、`/upload/video|opus|audio`、
`www.bilibili.com/watchlater/list`、`/opus/{id}`、`/read/cv{n}`、`/read/readlist/rl{n}`、`/audio/am{n}`、`/audio/au{n}`。
直接内容链接（BV/ep/ss/md）请继续走 `api::parse`；列表源用 `parse_source`/`resolve_source`。
分页节流常量 `source::PAGE_THROTTLE`（400ms）。

### 7.2 UGC 合集（`api::video`）

```rust
pub async fn resolve_ugc_season(client: &BiliClient, bvid: &str) -> BiliResult<Option<UgcSeason>>;
pub struct UgcSeason { pub season_id: u64, pub title: String, pub cover_url: Option<String>,
                       pub intro: Option<String>, pub upper: String,
                       pub sections: Vec<UgcSection>, pub episodes: Vec<UgcEpisode>, pub current_bvid: String }
pub struct UgcEpisode { pub bvid: String, pub aid: Option<u64>, pub cid: u64, pub title: String,
                        pub cover: Option<String>, pub duration: Option<u64>,
                        pub section_index: usize, pub section_title: String, pub index: usize, pub is_current: bool }
impl UgcSeason {
    pub fn current_index(&self) -> Option<usize>;
    pub fn to_source_items(&self) -> Vec<SourceItem>;
    pub fn to_collection(&self) -> SourceCollection;
}
```
接线：`parse → resolve_content` 成功后调用 `resolve_ugc_season`；`Ok(Some)` 且 `episodes` 非空时弹
「仅当前视频 / 整个合集」确认框（复用 ConfirmDialog），选合集则把 `to_source_items()` 交给现有列表勾选；
`Ok(None)`/空合集保持现有单视频入队。**现有 `resolve_content` 行为不变**（视频仍按 pages 解析，`Entry.aid` 已回填）。

### 7.3 稍后再看（`api::watchlater`）

```rust
pub async fn page(client: &BiliClient, pn: u32, ps: u32) -> BiliResult<SourcePage>; // 需登录
pub async fn all(client: &BiliClient, cancel: &CancellationToken) -> BiliResult<(Vec<SourceItem>, usize)>;
```
入口建议：添加面板/底栏「稍后再看」→ 未登录先 `source::require_login` 提示扫码；
分页 `ps=20`，失效条目由 `SourcePage.skipped` 计数。

### 7.4 任意用户收藏夹（`api::favorite`）

```rust
pub async fn list_folders(client: &BiliClient, mid: u64) -> BiliResult<Vec<FavFolder>>; // mid 可为任意公开用户
pub async fn resources_page(client, fav_id: u64, pn: u32, ps: u32) -> BiliResult<FavPage>;
pub async fn all_resources(client, fav_id: u64, cancel) -> BiliResult<(Vec<SourceItem>, usize)>;
pub async fn resources(client, fav_id: u64, cancel) -> BiliResult<(Vec<BiliId>, usize)>; // 既有签名保留
pub struct FavPage { pub folder: Option<FavFolder>, pub page: SourcePage }
```
接线：粘贴 `space.bilibili.com/{mid}/favlist?fid=` → `parse_source` 得 `Favorite{mid,fid}` →
`list_folders(any_mid)` 出 tabs（`fid` 选中对应夹）→ `resources_page` 展示带标题/封面/时长的条目。
类型 2/12/8（视频/音频/专栏）可入队；type 24（番剧）等暂跳过并计入 skipped。

### 7.5 UP 主投稿/合集（`api::user_space`）

```rust
pub async fn seasons_series(client, mid: u64) -> BiliResult<SpaceSeriesLists>;
pub async fn season_archives(client, mid, season_id, page, page_size) -> BiliResult<SpaceArchivePage>;
pub async fn series_archives(client, mid, series_id, page, page_size) -> BiliResult<SpaceArchivePage>;
pub async fn uploads(client, mid, page, page_size) -> BiliResult<SpaceArchivePage>;   // WBI
pub async fn resolve_space(client, mid, target: Option<u64>, page, page_size) -> BiliResult<SpaceView>;
pub async fn upper_info(client, mid) -> BiliResult<Option<SpaceUpper>>;
```
`SpaceView::{Collections{lists,archives}, Videos(page)}`：有合集/系列时返回 tabs（`lists`）与默认分组第一页，
否则返回普通投稿第一页。`SpaceArchivePage.to_collection()` 产出统一合集；`page.page.has_more` 驱动「加载更多」。
接线：粘贴空间链接 → `resolve_space(mid, target, 1, 25)`；合集 tab 点击 → `season_archives|series_archives`。

### 7.6 图文/动态与图文合集（`api::opus`）

```rust
pub async fn fetch(client, input: &str) -> BiliResult<OpusInfo>;    // 支持 opus id / opus URL / cv URL
pub async fn export(client, info: &OpusInfo, out_dir: &Path, cancel) -> BiliResult<(PathBuf, usize, usize)>;
pub async fn readlist(client, readlist_id: u64) -> BiliResult<OpusList>;
pub struct OpusInfo { pub opus_id, pub title, pub author, pub author_mid, pub pub_time,
                      pub cover_url, pub images: Vec<String>, pub html: String, pub degraded: bool }
```
`fetch` 先解析 opus 页面 `window.__INITIAL_STATE__`（正文节点 → HTML）；失败时用
`/x/polymer/web-dynamic/v1/forward/preview` 兜底标题/封面并 `degraded=true`，`export` 会额外写原文 `.html`，
仍失败返回带 URL 的 Parse 错误。导出复用 `api::article::export_html`（Markdown + 图片本地化 + 命名去重）。
接线：粘贴 `/opus/{id}`、`/read/cv{n}`、`/read/readlist/rl{n}`；单篇可直接调 `fetch`+`export`，
合集用 `readlist` 列出条目再逐篇调用。**注意**：`BiliId` 尚无 Opus 变体，图文导出暂不进任务管线。

### 7.7 音乐歌单（`api::audio`）

```rust
pub async fn menu_info(client, menu_id: u64) -> BiliResult<AudioMenu>;
pub async fn menu_page(client, menu_id: u64, page: u32, ps: u32) -> BiliResult<AudioMenuPage>;
pub async fn menu_all(client, menu_id: u64) -> BiliResult<AudioMenu>;       // 全量（分页节流）
pub async fn resolve_menu(client, menu_id: &str) -> BiliResult<Content>;    // 既有签名；内部已改为全量
```
`AudioEntry` 新增 `cover: Option<String>` / `duration: Option<u64>`（serde default，旧数据兼容）；
`AudioMenu::to_collection()` → `SourceCollection{kind: AudioList, items: SourceItem{kind: Audio}}`，
壳层可把所选曲目按 `TaskSpec::new(BiliId::Audio{song_id})` 入队（复用现有音频管线）。

### 7.8 壳层/DTO/UI 接线点清单

- 命令（建议）：`resolve_source_preview`（parse/source 后返回来源类型）、`list_watchlater(pn|all)`、
  `list_user_favorites(mid,fid,pn)`、`resolve_space(mid,target,page)`、`resolve_ugc_season(bvid)`、
  `fetch_opus(id)`/`export_opus(id)`、`list_readlist(rl)`、`list_audio_menu(am,pn|all)`。
- DTO：`SourceItemDTO`（camelCase：biliId/kind/title/cover/duration/upper）、`SourceCollectionDTO`、
  `SourcePageDTO`（items/page/hasMore/total/skipped）、`UgcSeasonDTO`（sections/episodes/currentIndex）、
  `OpusDTO`、`AudioMenuDTO`；`Page<T>` 风格分页直接映射。
- UI：复用现有「添加链接」面板与勾选列表（D3/D7：不新增主导航）；UGC 合集用 ConfirmDialog；
  ≤30 条提示、分页 300–500ms 节流、失败保留已选、未登录引导。
- 现状：`src-tauri` 的 `list_favorites/list_favorite_entries` 仍固定当前账号 mid；任意用户收藏夹需壳层
  新增 mid/链接参数并改用 `resources_page`（带标题/封面后无需再逐条 `resolve_content`）。

## 8. 会话自动续期与风控凭据（cookie refresh / bili_ticket）

> 实现：design D4。协议依据 bilibili-API-collect `docs/login/cookie_refresh.md`：
> correspondPath = RSA-OAEP(SHA-256) 加密 `refresh_{timestamp_ms}`，**小写 hex**（不是 PKCS1v15/base64url）。
> 失败按步骤分类返回、不 panic；单测全 wiremock，不发真实请求。

### 8.1 启动 / 账号刷新接线（现有壳调用顺序即可生效）

- `BiliClient::activate_buvid()`：`ensure_uuid()` → buvid3/4（缺失时 `/x/frontend/finger/spi`）→ `bili_ticket`
  （`GenWebTicket`，HMAC-SHA256 key `XgwSnGZ1p`、`key_id=ec02`；ticket 存在且未临近过期时跳过）；失败仅 warn。
- `account::restore()`：nav 校验后调用 `refresh_session()` 做一次自动续期（`cookie/info.refresh==true` 才走完整链路）。
- `commands.rs::refresh_account` 已调用 `restore()`，因此「账号刷新」同样触发一次续期，**无需新增命令**。
- `account::persist_device()` 已扩展为同时持久化匿名风控 cookie（bili_ticket/buvid）。
- `account::apply_credentials()` 改为 `merge_cookies`：不会丢掉壳启动时先 `activate_buvid()` 取到的匿名 cookie。

### 8.2 公开 API

```rust
// api::login
pub enum RefreshOutcome { NotNeeded, Refreshed { refresh_token: String } }
pub enum RefreshError { NotLoggedIn, MissingRefreshToken, Info(BiliError), CorrespondPath(String),
                        CorrespondPage(BiliError), RefreshRejected { code: i64, message: String }, Refresh(BiliError) }
impl RefreshError { pub fn needs_relogin(&self) -> bool; pub fn step(&self) -> &'static str; }
pub async fn refresh_cookie(client, stored_refresh_token: Option<&str>) -> Result<RefreshOutcome, RefreshError>;
pub fn correspond_path(timestamp_ms: i64) -> BiliResult<String>;      // RSA-OAEP(SHA-256) → 小写 hex
pub fn parse_refresh_csrf(html: &str) -> Option<String>;               // correspond 页 #1-name
// api::account
pub enum SessionRefresh { NotNeeded, Refreshed, ReloginRequired { reason: RefreshError }, Failed { reason: RefreshError } }
pub async fn refresh_session(client: &BiliClient, storage: &Storage) -> SessionRefresh;
pub async fn restore(client, storage) -> AccountState;                 // 内嵌一次自动续期
pub async fn login_poll(...) -> BiliResult<LoginOutcome>;              // 成功时持久化 refresh_token + bili_ticket
// api::client
pub fn ticket_hexsign(timestamp_secs: i64) -> String;
pub fn activate_ticket(&self);  pub fn ticket_is_fresh(&self) -> bool;
pub fn merge_cookies(&self, stored: &StoredCookies);
pub struct Endpoints { pub api: String, pub passport: String, pub www: String }  // 测试注入，生产默认 B 站域名
```

### 8.3 失败语义（前端映射建议）

`SessionRefresh::ReloginRequired` → `AccountState.expired=true`，提示「登录已失效，请重新扫码」；
`SessionRefresh::Failed` → 保持登录态，仅 `cookie_refresh_required=true`（提示「下次启动自动重试/建议重新登录延长有效期」），
不展示底层协议错误。refresh_token 缺失/被拒（-101/-111/86095）→ 重新扫码；网络/5xx/风控 → 下次再试。

## 9. 可靠性与性能（change improve-client-reliability-and-performance）

- **任务临时数据隔离（D1）**：worker 启动时记录任务实际临时目录（`<cache>/<task-id>` 与
  `<output>/.bilidown-tmp/<task-id>`）；完成/取消/移除/重试只递归删除目标任务目录，
  `.bilidown-tmp` 父目录仅空时非递归移除；清理失败记录并排队重试（1s、最多 3 次），不阻塞调度。
- **移除后持续调度（D2）**：数据库删除成功后才发布移除；运行中任务保留 `tails` 收尾记录，
  worker 确认结束后统一清理并 `schedule()`；取消/移除后 3 秒收敛监督（`WorkerEvent::Converge`），
  超时 abort 并确认结束后收尾；已移除任务的迟到事件与产物 checkpoint 不再写回记录。
- **元信息持久化（D7）**：迁移 `004_task_metadata.sql` 增加 `tasks.metadata`；
  `TaskView::metadata()/apply_metadata()` 与 `TaskRecord.metadata` 往返；恢复时独立解析，
  损坏/未知版本只丢元信息，不丢任务、不伪造作者/条目/实际参数/附件。
- **可靠写库（D8）**：`Db::insert_tasks`（批量入队单事务）、`Db::finish_task`（完成视图 +
  历史 upsert + 裁剪原子提交）；`persist_view`/`flush_dirty` 失败保留 dirty 并做有界退避；
  `TaskView.persist_error` 暴露失败原因；`retry_persistence` 只重试记录，不重新下载。
- **性能（D10）**：`TaskManager` 空闲时不运行定时轮询（按 deadline 唤醒）；进度发布 ≤10Hz；
  测试可注入 `enqueue_store_fail_once`、`artifact_store_fail_once` 故障。
