# src-tauri IPC 契约（4.5 完成版）

> 事实源：Rust `src-tauri/src/dto.rs` + 前端 `web/src/lib/contract.ts`（ui-engineer 产出，已对齐）。
> 4.5 已完成 core 接线：全部命令/事件都由 `bilidown-core` 驱动，Rust 侧把 core 的 snake_case
> 类型映射为**前端 camelCase DTO**。

## 0. 命名与错误约定

- **命令参数**：Tauri 自动把 Rust snake_case 参数转 camelCase（`task_id` → JS `{ taskId }`）。
- **返回值与事件 payload**：一律 **camelCase** DTO（见 §2/§3）。
- **错误**：`invoke` reject payload 为
  `AppErrorDTO { kind: string, message: string, actions: string[] }`；
  `kind ∈ risk / auth / vip / paid / region / network / cdn / ffmpeg / disk / parse / unknown`，
  `actions ∈ retry / relogin / open-settings / copy-diagnostics / open-log / choose-dir / install-ffmpeg`。
  （core `BiliError {kind,detail}` 在 `dto.rs` 中归一为上述分类，`message = err.describe()`。）

## 1. 运行方式（开发）

```bash
# 终端 1：前端（vite 固定 127.0.0.1:1420 strictPort）
pnpm dev:web
# 终端 2：Tauri 壳（tauri.conf.json devUrl=http://127.0.0.1:1420/、frontendDist=../web/dist）
CARGO_NET_OFFLINE=true pnpm tauri dev
```

`withGlobalTauri=false`；前端用 `@tauri-apps/api` npm 包。`src-tauri/dev/` 临时页已废弃（保留仅作历史参考，不参与打包）。

## 2. 命令（invoke 名 / 参数 / 返回）

### 应用
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_app_info` | — | `AppInfoDTO {productName,version,tauriVersion,os,arch,identifier,dataDir,logDir,ffmpegFound,ffmpegPath,glassSupported,glassUnavailableReason,notificationsSupported,justUpdated}` |
| `open_preferences` | — | `void`（唯一偏好设置窗口，880×600 overlay） |
| `quit_app` | — | `void`（`manager.shutdown()` 后优雅退出；前端确认退出时调用） |
| `get_diagnostics` | — | `string`（版本/系统/路径/队列/账号摘要） |

### 解析 / 队列
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `parse_input` | `input` | `ParseResultDTO`（见 §3.1，含 `qualities/codecs/audioFormats` 扩展） |
| `add_tasks` | `input`, `options: AddOptionsDTO` | `{taskIds: string[], rejected: [{line,reason}]}` |
| `list_queue` | — | `QueueSnapshotDTO` |
| `pause_task` / `resume_task` / `cancel_task` / `retry_task` / `remove_task` | `taskId` | `void` |
| `start_all` / `pause_all` / `retry_all` | — | `number`（受影响任务数） |
| `clear_finished` | — | `number` |
| `clear_inactive` | — | `number`（壳实现：排队/暂停/取消逐个 remove） |

`AddOptionsDTO { quality: string /*"best"/"1080P"/…*/, codec: string /*auto/avc/hevc/av1*/, audioFormat: string /*original/mp3*/, outputDir: string, entryIndexes?: number[], note?: string }`

### 账户 / 登录
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_account` | — | `AccountInfoDTO \| null` |
| `login_start` | — | `LoginStartDTO {key,qrMatrix:boolean[][],qrUrl,expiresInSecs,status?}` |
| `login_poll` | — | `LoginPollDTO {status: waiting\|scanned\|expired\|success\|error, message, account?}`（前端 1.5s 轮询；壳内对网络类错误额外重试 2 次、间隔 400ms，仍失败保留 AppErrorDTO 分类） |
| `login_logout` | — | `void` |
| `refresh_account` | — | `AccountInfoDTO \| null` |

### 设置
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_settings` | — | `SettingsDTO` |
| `set_settings` | `settings: SettingsDTO` | `SettingsDTO`（保存 + 热更新；非法值返回 `kind=parse`） |

`SettingsDTO`：`downloadDir,maxConcurrent,quality,codec,audioFormat,ffmpegPath,proxy{mode,host,port,user,pass},theme,notifications,watchClipboard` +
扩展 `parseConcurrency,historyLimit,historyClearPolicy,dockBadge,reduceMotion,showSpeedInStatus,writeMetadata,soundOnComplete,revealInFinderOnComplete,keepQueueOnExit,ffmpegDetected,ffmpegVersion`。
core 未实现的扩展字段由 Rust 回显默认值（parseConcurrency 固定 2、dockBadge 固定 true、soundOnComplete 固定 false 等）。

### 历史
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `list_history` | `filter?: {status?,keyword?}` | `HistoryEntryDTO[]` |
| `remove_history` | `id` | `void` |
| `clear_history` | — | `number` |

`HistoryEntryDTO {id,taskId,title,coverUrl,sourceKind,sourceId,pageUrl,quality,outputPath,fileSize,status(completed/failed/cancelled),error,finishedAt,owner?,fileExists}`

### 系统集成
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `pick_directory` | — | `string \| null` |
| `reveal_output` | `taskId` | `void`（在 Finder 显示） |
| `reveal_path` | `path` | `void` |
| `open_file` | `path` | `void` |
| `show_logs` | — | `void`（Finder 定位日志目录） |
| `get_menu_state` | — | `{enabled: Record<string, boolean>}` |
| `request_quit_check` | — | `boolean`（有活跃任务时 emit `shell://quit-requested`） |

### EXTENSION（contract.ts 标注，4.5 新增）
| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `copy_text` | `text` | `void`（clipboard-manager） |
| `read_clipboard_text` | — | `string` |
| `open_external` | `url` | `void`（opener 默认浏览器） |
| `detect_ffmpeg` | — | `{detected,version,path}`（`ffmpeg -version` 解析） |
| `test_proxy` | `proxy: ProxyDTO` | `{ok,message}`（经代理 GET finger/spi，返回 HTTP 码与耗时） |
| `list_favorites` | — | `{folders:[{id,title,itemCount,coverUrl}]}`（需登录） |
| `list_favorite_entries` | `folderId` | `ParseResultDTO`（收藏夹条目；标题仅解析前 30 条，其余占位） |
| `enqueue_favorite` | `folderId`, `entryIndexes: number[]` | `{taskIds,rejected}`（120s 缓存复用列表） |
| `fetch_cover` | `url` | `string`（**data URL**；带 Referer/UA + 设置代理抓取，缓存 `<appData>/covers/<hash>.<ext>`，失败返回空串） |

### dev 辅助（**仅 debug 构建注册**；`pnpm tauri build` 的 release 包不包含，`src-tauri/dev/` 也不进包）
`dev_report` / `dev_menu_state` / `dev_menu_dispatch` / `dev_notify` / `dev_set_badge` / `dev_get_badge` /
`dev_emit_notice` / `dev_sample_output` / `dev_stat_file` / `dev_close_window` / `dev_show_window` /
`dev_window_visible` / `dev_request_quit`。

## 3. 事件（listen）

| 事件 | payload |
| --- | --- |
| `queue://snapshot` | `QueueSnapshotDTO {revision,tasks:TaskDTO[],stats:QueueStatsDTO,account:AccountInfoDTO\|null,capability{anonymousMax,signedInMax,vipMax},updatedAt}`（core watch ≤10Hz 驱动） |
| `queue://events` | `TaskEventDTO {taskId,kind,message?,at}`；`kind ∈ enqueued/started/phase-changed/paused/resumed/completed/failed/cancelled/removed`（core `TaskEvent` 映射；`history_changed/queue_changed` 走各自域事件） |
| `account://changed` | `AccountInfoDTO \| null` |
| `settings://changed` | `SettingsDTO` |
| `history://changed` | `HistoryEntryDTO[]`（完整数组，前端直接 setEntries） |
| `app://notice` | `NoticeDTO {level:info\|success\|warning\|error, code?, message, taskId?}` |
| `shell://menu` | `MenuActionDTO {id,label?}` |
| `shell://drag` | `{active:boolean}`（HTML5 拖拽时前端自用；Rust 原生 drop 兜底已因 `dragDropEnabled:false` 不触发） |
| `shell://drop` | `{paths:string[],text:string}`（单实例唤起携带链接；HTML5 drop 由前端直接处理） |
| `shell://quit-requested` | `QueueStatsDTO`（前端弹确认框，确认后调 `quit_app`） |

### 3.1 TaskDTO 关键字段
`{id,status,phase,sourceKind,sourceId,title,coverUrl,pageUrl,quality,outputPath,error:AppErrorDTO|null,restored,sortIndex,createdAt,updatedAt,progress,owner?,entries?:string[],entryCount?,codec?,audioFormat?,outputDir?,durationSecs?}`

- `status` = core `TaskPhase`（queued/resolving/downloading/merging/paused/done/failed/cancelled）
- `phase` = 渲染阶段（`resolve/video/merge/postprocess`，由 core `progress.stage` 映射）
- `progress {bytesDone,bytesTotal,stageDone,stageTotal,percent(0–1),speedBps,etaSecs}`
- `quality` 为展示标签（"480P"/"1080P"/"4K"…，为空的表示未回写）
- `ParseResultDTO`：`{kind,sourceId,title,coverUrl,pageUrl,owner,entries[{index,title,durationSecs,selectedDefault,sizeBytes?,unavailableReason?}],rawInput,durationSecs,publishedAt,qualities?,codecs?,audioFormats?,wordCount?}`；
  `qualities[{id(qn),label,resolution,fps,vip,available,unavailableReason?}]`、`codecs[{id,label,available}]`、`audioFormats[{id,label,available,hint?}]`（由 core `PlayUrl.quality_tiers` 生成，含登录/大会员置灰）。

## 4. 原生菜单动作 id（`shell://menu`）

| id | 菜单 / 快捷键 |
| --- | --- |
| `open-link` | 文件 / ⌘O |
| `clipboard-add` | 文件 / ⌃⌘V |
| `start-all` | 文件 / ⌘S；控制菜单同动作无快捷键 |
| `pause-all` | 文件 / ⌃⌘S；控制菜单 |
| `retry-all` | 文件 / ⇧⌘R；控制菜单 |
| `clear-finished` | 文件 / ⌘K；控制菜单 |
| `clear-inactive` | 文件 / ⇧⌘K；控制菜单 |
| `preferences` | 应用菜单 / ⌘, |
| `login` / `logout` / `refresh-account` | 账户菜单 |
| `show-logs` / `copy-diagnostics` | 帮助菜单 |
| `quit` | 应用菜单 / ⌘Q（**自定义项**，非系统 Quit role，见 §5） |

控制菜单项使用 `ctl-<id>` 存储 id，事件里已归一为动作 id。菜单可用性由 Rust 依据 core 快照自动置灰（快捷键随禁用态失效）。

## 5. 关键实现决策（相对 4.2 契约的变化）

1. **DTO 字段统一 camelCase**：core 的 snake_case 类型在 `dto.rs` 映射后才发给前端（TaskView→TaskDTO、QueueSnapshot→QueueSnapshotDTO、BiliError→AppErrorDTO、QrLogin→LoginStartDTO、HistoryRecord→HistoryEntryDTO、Settings 双向映射）。
2. **移除 `get_play_url` / `validate_settings`**：清晰度档位并入 `parse_input.qualities`；设置校验由前端 `validateSettings` + `set_settings` 错误兜底。
3. **新增 8 条 EXTENSION 命令**（§2）。
4. **退出保护必须自定义菜单项**：macOS 系统 Quit role 的 ⌘Q 走 `NSApp.terminate:`，不触发 Tauri `RunEvent::ExitRequested`，`prevent_exit()` 无效（4.3 实测）。因此菜单用自定义「退出 bilidown」（⌘Q）→ `shell://menu{id:"quit"}` → 前端 `request_quit_check` 有活跃任务时弹确认 → `quit_app()`（内部 `manager.shutdown()`）。
5. **历史 payload 为数组**（前端 `setEntries`）；`history_changed` core 事件触发重新查询 SQLite。
6. **通知**：core `TaskEvent::Done/Failed` 且 `settings.notify_on_done` 时发 macOS 通知（标题/正文），点击激活窗口依赖系统默认 + `RunEvent::Reopen`（7.3 联调细化）。
7. **Dock 徽标**：快照桥按 `active_count + queued_count` 调 NSDockTile（objc2），空时清除。
8. **设置热更新**：代理变化时重建 `BiliClient` 并 `manager.set_client`；其余走 `manager.set_settings`。
9. **收藏夹**：`list_favorite_entries` 仅解析前 30 条标题（后续条目占位），并缓存 120s 供 `enqueue_favorite` 复用。
10. **封面防盗链**：`fetch_cover(url) -> data URL`，内部用 `download_client(设置代理)` + `Referer: https://www.bilibili.com/` + UA 抓取；缓存 `<appData>/covers/<hash>.<ext>`，启动时清理 >30 天或超 500 张；失败返回空串由前端回退占位图。
    **前端待接**：`web/src/lib/contract.ts` 增加 `fetchCover` 扩展、`tauri-backend.ts` 调 `invoke<string>('fetch_cover',{url})`，`Thumbnail` 的 `src` 用返回值（命中缓存后与普通 URL 等价，仅首次联网）。
11. **dev 隔离**：`dev_*` 命令用 `#[cfg(debug_assertions)]` 只在 debug 注册；release 包的 `frontendDist` 指向 `web/dist`，`src-tauri/dev/` 不会被打包。
12. **封面 URL 归一**：B 站接口返回 `http://i0.hdslb.com/...`，DTO 层统一升级为 `https://`（生产 CSP `img-src` 只放行 https；另可用 `fetch_cover` 拿 data URL 完全绕过）。

## 6. 构建 / 运行环境备注（本机）

**crates.io 网络极慢**（sparse index 30 分钟仅 511 条），已用 rsproxy 一次性 fetch 并拷回标准 cache：

```bash
cargo fetch \
  --config 'source.crates-io.replace-with="rsproxy"' \
  --config 'source.rsproxy.registry="sparse+https://rsproxy.cn/index/"'
cp -n ~/.cargo/registry/cache/rsproxy.cn-*/*.crate ~/.cargo/registry/cache/index.crates.io-*/
# 索引元数据也要补齐（否则 offline 解析报 no matching package）：
CIDX=$(echo ~/.cargo/registry/index/index.crates.io-*); RIDX=$(echo ~/.cargo/registry/index/rsproxy.cn-*)
cd "$RIDX" && for f in $(find .cache -type f); do dst="$CIDX/$f"; [ -f "$dst" ] || { mkdir -p "$(dirname "$dst")"; cp "$f" "$dst"; }; done
```

日常构建统一 `CARGO_NET_OFFLINE=true`（`cargo check -p bilidown-app` / `pnpm tauri dev`）。
新增 crate 时先重跑上面的 fetch+copy，否则 offline 报 `no matching package named ... found`。

**本机 LuLu 防火墙会拦截未签名 dev 二进制的直连**（curl/nc 正常，Rust 进程 TCP 直连超时）。
真实下载/登录联调时，在应用「设置 → 网络 → 代理」填 `http://127.0.0.1:7897`（Clash 本机端口；**仅本机测试，不要提交为默认值**），
或在 LuLu 弹窗放行 `bilidown-app`。core 默认 `no_proxy()` 是刻意的（v1 起系统代理 CONNECT 会挂起）。

**图标**：唯一母版为 `docs/design/app-icon.svg`；运行 `node scripts/generate-icons.mjs`（或 `pwsh -File scripts/make-ico.ps1`），复用已有 Playwright 与系统 Chrome，统一导出前端 SVG/PNG 和 Windows PNG/七尺寸 ICO（16/24/32/48/64/128/256）。也可设置 `ICON_BROWSER_CHANNEL=msedge` 使用已安装的 Edge。不从旧 PNG 生成，不改 ICNS/移动端图标；详细输出清单见 `docs/design/BRAND.md`。生成后重新构建 EXE/NSIS，并核对产物内嵌图标与运行时各入口。

**devUrl**：`http://127.0.0.1:1420/`（vite `strictPort`），**frontendDist**：`../web/dist`。
**dev 命令的 watcher 同时监听 `crates/bilidown-core`**，core 改动会自动重启 app。

**release 打包（已验证）**：

```bash
CARGO_NET_OFFLINE=true pnpm tauri build
# 产物（本机 2026-10-29 实测，ad-hoc 签名）：
#   target/release/bundle/macos/bilidown.app            14.74 MiB
#   target/release/bundle/dmg/bilidown_2.0.0_aarch64.dmg 6.49 MiB
```

- `bundle.macOS.signingIdentity: "-"` → ad-hoc 签名（`codesign` 显示 `Signature=adhoc`），未公证（本机自用）。
- `dev_*` 命令与 `src-tauri/dev/` 均不进 release 包（cfg(debug_assertions) + frontendDist 指 web/dist）。
- 冒烟：`open target/release/bundle/macos/bilidown.app` → 内嵌前端正常渲染、core ready、队列/历史从 SQLite 恢复。
- `cargo clippy -p bilidown-app --all-targets -- -D warnings` 零警告；`cargo check --release -p bilidown-app` 零警告。

## 7. 已知限制 / 后续

- cookie 自动续期缺浏览器 RSA 产物（core API.md §6）：过期走重新扫码。
- 重启恢复的任务（`restored=true`）只恢复标题/进度，`owner/entries` 未持久化，UI 显示默认 "B 站"。
- 通知点击「定位到任务」未实现（macOS 激活事件需额外监听；7.3 与 UI 联调，暂列为已知限制）。
- 登录轮询已加壳内短重试（网络类错误最多 3 次尝试 / 400ms 间隔）；真实扫码闭环留用户验收（cookie 续期受限见上）。
- 封面首次加载需前端改调 `fetch_cover`（见 §5.10）；未改前仍使用原始 URL，B 站防盗链下会破图。
- 收藏夹条目标题仅前 30 条（其余占位），后续可用批量 view 接口优化。
- HTML5 拖拽由前端实现（`dragDropEnabled:false`）；Rust 原生 DragDrop 逻辑保留但不会触发。
- `src-tauri/dev/` 临时自检页面已废弃（`withGlobalTauri=false` 后不可用），打包不包含；可在需要 shell 级联调时临时打开。

---

# 8. 应用内更新（change `add-download-features` 4.1–4.3）

> 实现在 `src-tauri/src/update.rs`（下载/校验/安全边界）+ `commands.rs`（包一层 `AppErrorDTO`）+ `main.rs`（`app.manage(update::AppUpdateState::default())` 与命令注册）。
> 本机 `src-tauri` 为 `main.rs` 入口（无 `lib.rs`），state 注册在 `main.rs` setup 中。

## 8.1 命令

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_app_update_state` | — | `AppUpdateSnapshot`（见 8.2） |
| `get_app_version` | — | `AppVersionInfo`（EXTENSION，关于页静态信息：`{version,target,installerKind}`；也可用 `get_app_info.version`） |
| `check_app_update` | — | `AppUpdateCheck`（见 8.2；30s 超时；只选非 draft/prerelease 的最高 semver 正式版） |
| `start_app_update_download` | `version: string` | `AppUpdateSnapshot`（必须先是最近一次 `check_app_update` 选中的版本；后台任务开始下载） |
| `cancel_app_update_download` | — | `AppUpdateSnapshot`（置 cancelled 并清理 `.part`） |
| `open_downloaded_update` | — | `void`（系统默认程序打开已校验 DMG） |
| `open_update_file_location` | — | `void`（Finder 中显示 DMG） |
| `open_about_link` | `link: string` | `void`（白名单 key：`repository` / `releases` / `issues` / `license` / `author` / `acknowledgements`，任何 URL 会被拒绝） |

## 8.2 DTO（camelCase）

事件 `app-update://progress` 的 payload 与 `get_app_update_state` 返回同一结构：

```ts
interface AppUpdateSnapshot {
  status: 'idle' | 'checking' | 'available' | 'downloading' | 'verifying' | 'completed' | 'error' | 'cancelled';
  version?: string;
  filename?: string;       // 如 bilidown_2.1.0_aarch64.dmg
  path?: string;           // completed 后为 ~/Downloads/<filename>
  downloadedBytes: number;
  totalBytes?: number;
  progress?: number;       // 0–100
  bytesPerSecond?: number;
  etaSeconds?: number;
  error?: string;          // 可读中文错误
}

interface AppUpdateCheck {
  currentVersion: string;
  updateAvailable: boolean;
  checkedAtMs: number;     // Unix ms
  release: {
    version: string; tagName: string; name: string; notes: string;   // notes 已去 Markdown、≤600 字
    publishedAt?: string; htmlUrl: string; assetName?: string; assetSize?: number;
  } | null;
  releasesUrl: string;     // 失败/无 Release 时也返回，供「打开发布页」兜底
}

interface AppVersionInfo { version: string; target: string; installerKind: string | null }
```

## 8.3 下载与安全边界

- 下载目录：系统「下载」目录；先写 `<name>.part`，SHA256 校验通过后原子 rename 为 `<name>`；已存在且校验通过直接复用（不重复下载）。
- 进度事件 ≥250ms 节流；idle 60s 无数据中止；总大小上限 2 GiB；`.part`/目标文件/下载目录拒绝符号链接。
- SHA256 来源：Release 资产 `SHA256SUMS.txt`，精确匹配 `bilidown_<version>_aarch64.dmg`；不匹配则删除文件并报错，**不得进入 completed**。
- URL 白名单：资产地址必须 `https://github.com/Goldloli/bilidown/releases/download/...`；重定向仅允许 `github.com` / `objects.githubusercontent.com` / `*.githubusercontent.com` / `api.github.com`。
- 无 Release / 限流（403/429）/404 / 网络失败 → 可读错误 + `releasesUrl` 兜底；不阻塞应用。
- **不自动安装**：完成后只提供打开/定位；用户自行拖入 Applications。DMG 为 ad-hoc 签名（未公证），首次打开可能需右键→打开。
- Release 资产命名约定（发版 4.6 需遵守）：`bilidown_<version>_aarch64.dmg` + `SHA256SUMS.txt`（`sha256sum *.dmg > SHA256SUMS.txt`）。仓库当前为私密，Releases API 匿名访问需仓库/Release 公开后可用。

## 8.4 启动检查（本任务只留接口）

`update::startup_check_interface()` 为空实现，`main.rs` setup 已调用占位；等 core `Settings.check_updates_on_startup` 字段落地后接：启动后台静默 `check_app_update`，仅通知/红点，不自动下载（spec app-update）。

## 8.5 单测（不联网，13 条）

semver 选择（忽略 draft/prerelease/非法 tag、取最高正式版）、无正式版返回 None、资产选择（缺 SHA256SUMS / 版本不匹配 / 路径穿越 / 其他项目命名）、SHA256SUMS 解析（精确文件名 + 64 hex，拒绝 `./`、嵌套、错误长度/非 hex）、资产 URL 白名单、semver 比较、notes 清洗上限、About 链接白名单、状态默认/active 判定、平台 target/installerKind。

---

# 9. 内容源扩展（change `add-download-features` D3）

> 实现：`src-tauri/src/dto.rs`（DTO/转换）+ `commands.rs`（4 条通用命令，内部调 core `api::source` /
> `watchlater` / `favorite` / `user_space` / `video` / `opus` / `audio`）。
> 现有 `list_favorites` / `list_favorite_entries` 命令保留（当前账号），新列表 UI 建议统一走 §9 通用命令。

## 9.1 命令

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `resolve_source` | `raw: string` | `SourceRefDTO \| null`（可直接内容链接 BV/ep/ss/md 等返回 null，前端回退 `parse_input`；b23 短链会展开） |
| `list_source` | `source: SourceRefDTO`, `page?: u32`, `pageSize?: u32` | `SourcePageDTO`（page 1 基；pageSize 默认 20、范围 1–100；连续翻页自动按 core `PAGE_THROTTLE`=400ms 限速） |
| `resolve_ugc_season` | `bvid: string` | `UgcSeasonDTO \| null`（null = 普通单视频，保持原入队行为） |
| `export_opus` | `raw: string` | `ExportOpusDTO`（输出到 core 设置的有效下载目录；返回 Markdown 路径 + 图片数/跳过数） |

`list_readlist` / `list_audio_menu` 不单独提供命令，统一用 `list_source(kind="opusList"|"musicList")` 翻页。

## 9.2 DTO（camelCase）

```ts
interface SourceRefDTO {
  kind: 'watchLater' | 'favorite' | 'userSpace' | 'userOpus' | 'userAudio'
      | 'opus' | 'opusList' | 'musicList' | 'music';
  id?: string;        // opus id / rl id / am id / au id（数字串）
  mid?: u64;          // favorite/userSpace/userOpus/userAudio
  fid?: u64;          // favorite（缺 fid 会返回可读错误，提示带 fid 的收藏夹地址）
  target?: string;    // userSpace 的合集/系列 id（数字串，可缺省=默认分组）
}

interface SourceItemDTO {
  biliId: string;     // video: "BV…"；audio: 数字 song_id 字符串；opus: opus id
  kind: 'video'|'bangumi'|'course'|'audio'|'audioList'|'article'|'opus';
  title: string;
  cover?: string;     // 已统一为 https
  duration?: number;  // 秒
  upper?: string;
}

interface SourcePageDTO { items: SourceItemDTO[]; page: number; hasMore: boolean; total?: number; skipped: number }

interface UgcSeasonDTO {
  seasonId: number; title: string; coverUrl?: string; intro?: string; upper: string;
  currentIndex: number;
  episodes: Array<{ bvid: string; aid?: number; cid: number; title: string; cover?: string;
                    duration?: number; sectionTitle?: string; index: number; isCurrent: boolean }>;
}

interface ExportOpusDTO { path: string; images: number; skipped: number }
```

## 9.3 各来源语义

| kind | core 调用 | 登录 | 分页/说明 |
| --- | --- | --- | --- |
| `watchLater` | `watchlater::page(pn,ps)` | 需要（未登录 `auth` 错误，先引导扫码） | 原生分页 |
| `favorite` | `favorite::resources_page(fid,pn,ps)` | 需要 | 需 `fid`；`skipped` 为失效/不可下载条目数；`total`=夹内总数 |
| `userSpace` | `user_space::resolve_space(mid,target,pn,ps)` | 否 | 有合集/系列时返回默认分组合集页（合集 tab 由 UI 另行解析 `target`）；无合集回退投稿页 |
| `opus` | `opus::fetch` | 否 | 单条（返回 1 条 items，hasMore=false），正文导出用 `export_opus` |
| `opusList` | `opus::readlist(rl)` | 否 | 全量返回（page=1, hasMore=false），total=条目数 |
| `musicList` | `audio::menu_page(am,pn,ps)` | 否 | 原生分页（pageCount 驱动 hasMore；无 pageCount 时按页大小判断） |
| `music` | `audio::resolve_song(au)` | 否 | 单条 |
| `userOpus` / `userAudio` | — | — | **core 暂未提供列表 API**：返回可读 `unknown` 错误（content-sources 后续补齐）；前端可提示暂不支持 |

其它注意：
- `SourceItemDTO.biliId` 对音频是**数字 song_id**（非 `au…` 前缀）；入队时转 `BiliId::Audio{song_id}` 或拼 `au{song_id}` 交给 `add_tasks`。
- UGC 合集流程：前端 `parse_input` → `add_tasks` 前调 `resolve_ugc_season(bvid)`，非 null 时弹确认框；选整个合集则把 episodes 的 bvid 列表交给确认面板/`add_tasks`（每 bvid 一个 TaskSpec，沿用现有队列）。
- opus 导出不进任务管线（core 无 `BiliId::Opus`）；导出的 Markdown + 本地图片写入下载目录，不产生历史记录。
- `skipped` 是接口侧跳过（失效/付费/番剧类型等）数量，UI 可提示「已跳过 n 条」。

## 9.4 启动时检查更新（task-14 顺带接线）

- `state::spawn_init()` 在 core ready 后读取 `Settings.check_updates_on_startup`；为 true 时后台 spawn
  `update::check_in_background(app)`：仅更新 `AppUpdateState` 并发 `app-update://progress`（checking/available），
  **不发通知、不自动下载**；失败只 `tracing::warn`，不阻塞主流程。
- `SettingsDTO` 已含 `checkUpdatesOnStartup?: boolean` 透传（设置页可绑定；缺省保持原值）。

## 10. 存储统计与缓存清理（change: add-appearance-and-maintenance-settings）

命令（camelCase）：
- `get_storage_stats` → `{dataDir, downloadDir, dataBytes, coverBytes, tempBytes, logBytes, downloadFreeBytes?, downloadTotalBytes?, ffmpegPath?}`
- `clear_app_cache(target)`，target ∈ `covers|temp|logs|all` → `{freedBytes, removedFiles}`
- `open_data_dir` → void（在 Finder 打开应用数据目录）

安全：
- 仅允许清理应用数据根目录下的 `covers/`、`tmp/`、`logs/`（canonicalize 校验，拒绝越界/符号链接）；
- 不删除下载产物、`settings.json`、`credentials.json`、`queue.sqlite` 与历史；
- `logs` 清理保留最新一个日志文件，保证当前日志仍可写。

SettingsDTO 新增字段（旧配置缺省兼容）：
- `accentColor?: string`（`#RRGGBB`，缺省 = `#FB7299`；空字符串 = 恢复默认）
- `motion?: "system" | "reduce" | "normal"`（默认 `system`）
