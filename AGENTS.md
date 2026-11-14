# AGENTS.md — bilidown 项目协作规范

本项目的所有 Agent（含所有 AI 编码助手）在动代码之前必须先读完本文件并严格遵守。

## 工具链总览

| 工具 | 职责 | 目录 |
| --- | --- | --- |
| git | 版本控制（默认分支 `main`） | `.git/` |
| OpenSpec | 变更提案与规格文档（唯一的功能变更入口） | `openspec/` |
| Beads (`bd`) | 持久化任务/问题跟踪 | `.beads/` |
| CodeGraph | 代码索引与符号级检索 | `.codegraph/` |

文档语言约定：OpenSpec 产物及项目文档一律使用 zh-CN（见 `openspec/config.yaml`），结构性标题与 SHALL/MUST 关键字保留英文。

---

## 一、强制工作流：功能变更必须走 OpenSpec Change（最高优先级规则）

**新增或修改任何功能，必须先写 OpenSpec change 文档；文档写好后必须交由用户确认；用户明确确认之后，才能开始写实现代码。**

未经用户确认 change 文档，禁止提交任何功能实现代码。此规则没有例外，优先级高于其他任何效率或进度考量。

标准流程（五步，不许跳步）：

1. **提出变更**：使用 `/openspec-propose`（或 `openspec` CLI）创建 change，产出 proposal、design（如需要）、specs delta、tasks 四类文档，放在 `openspec/changes/<change-id>/` 下。
2. **自检**：运行 `openspec validate --all` 确保文档完整、格式合法； deltas 必须落在 `openspec/specs/` 对应 capability 上。
3. **用户确认（强制闸门）**：把 change 文档的要点（动机、方案、影响范围、任务清单）呈现给用户，**明确等待用户回复确认**。用户未确认前：
   - 不写任何实现代码；
   - 不创建/修改任何待变更功能的源文件；
   - 可以做的只有：文档本身、只读的代码调研、以及回答用户的提问。
4. **确认后开工**：用户确认后，使用 `/openspec-apply-change` 按 tasks 逐项实现；实现中发现方案需要偏离文档时，先用 `/openspec-update-change` 更新文档，重大偏离需再次征求用户确认。
5. **归档收尾**：全部任务完成且验证通过后，用 `/openspec-archive-change` 归档 change，将 delta 合入 `openspec/specs/`。

范围界定：

- **必须走 change**：新增功能、修改现有功能行为、接口/数据结构变更、任何影响用户的可见变化。
- **建议走 change**：改变行为的 bug 修复（因为修复后行为与原 spec 不一致，需要 delta 说明）。
- **可以不走 change**：纯格式化、错别字修正、注释与文档补充、不改变行为的重构。此类改动须在 commit message 中写明"不改变行为"。

紧急情况也不得先斩后奏：即使问题再急，也要先落一份最简 change 文档并获得用户确认。

## 二、Beads：任务与问题跟踪

Beads 是本项目的共享任务系统（`.beads/`，issue 前缀 `bilidown-`）。会话开始时先跑 `bd prime` 恢复上下文。

- 认领工作前先看：`bd ready`、`bd list --status=open`、`bd show <id>`；
- 原子认领：`bd update <id> --claim`；
- 实现过程中发现的新任务、阻塞项，用 `bd create "标题" --description="..." --type=task --priority=N` 落库，不要只留在对话或临时文件里；
- 完成后：`bd close <id> --reason="..."`。不完成不关闭，不确定就留着。
- 禁止用 `bd edit`（交互式编辑器），统一用 `bd update` 的参数。
- 不要把 markdown TODO 文件当作任务真相源；本地 todo 只用于当轮执行清单。

注意：change 经用户确认后，应在 beads 中建立对应 issue 跟踪实现进度（一个 change 至少一条，复杂 change 按任务拆分）。

## 三、CodeGraph：代码索引与检索

`.codegraph/` 为本地索引（已被 git 忽略，不入库）。改完代码后及时同步索引。

- 提交前/检索前同步：`codegraph sync`（或 `codegraph index` 全量重建）；
- 查符号：`codegraph query <关键词>`；
- 摸清一块逻辑：`codegraph explore <主题>`；
- 看某个符号的上下游：`codegraph node <名称>`、`codegraph callers <符号>`、`codegraph callees <符号>`；
- 评估改动影响面（动手改公共符号前必做）：`codegraph impact <符号>`；
- 找受影响的测试：`codegraph affected <改动文件...>`。

约定：修改任何被多处引用的公共符号前，先用 `codegraph impact` 看清影响面，并把结论写进 change 的 design/tasks 里。

## 四、Git 约定

- 默认分支 `main`，直接在 `main` 上小步提交（后续如需分支流程再在此更新）。
- Commit message 用中文，首行 ≤ 72 字符，说清"做了什么"；涉及 change 的提交带上 change id，例如 `feat: 实现 xxx（change: add-xxx）`。
- `.beads/hooks/` 下的 git hooks 已通过 `core.hooksPath` 接管 pre-commit / post-checkout 等，请勿删除或绕过。
- 提交前确认 `git status` 干净、`openspec validate --all` 通过；索引数据（`.codegraph/`、`.beads/embeddeddolt/`）不入库，靠 `.gitignore` 排除。

## 五、工程约束与验收门（从 OpenSpec 规格沉淀）

`openspec/specs/` 是行为契约。修改相关代码前先读对应 capability 规格；以下为 agent 最容易踩的 MUST 级约束，细则以规格为准。

### 5.1 通用

- 行为修改（含 hotfix、补丁发布）也必须先建 change 文档并等用户确认，再写代码；不得以“紧急/发版/CI 失败”为由跳步。
- 平台差异按规格执行：`desktop-shell` / `ui-theme` / `windows-distribution` 定义 Windows 惯例，`macos-distribution` 定义 macOS 发布约束；不得用一端的既有行为覆盖另一端的明确规格。

### 5.2 窗口、设置与 Tauri 外壳

- 主窗口关闭语义：Windows 隐藏到托盘、macOS 隐藏窗口，二者都不得直接退出进程（退出走退出保护）；设置窗口必须有可用的关闭入口，并在关闭前完成保存/放弃决策，不得出现点击无反应。
- Tauri v2 的前端窗口 API（`destroy` / `close` / `setSize` 等）受 capability 限制：新增或修改这类调用时，必须同步 `src-tauri/capabilities/default.json` 并用测试锁住权限，避免权限缺失导致静默失败。
- 菜单、Dock/任务栏、通知等系统集成按可见状态变化更新，不得在空闲时做周期性刷新；跨平台文案使用对应平台术语。

### 5.3 更新与发布

- 更新链路（`app-update`）：按平台精确匹配资产（`bilidown_<version>_aarch64.dmg` / `_x64-setup.exe`）；下载后必须用 `SHA256SUMS.txt` 校验；不得静默替换正在运行的应用；Windows 安装器启动失败必须返回可读错误并保持应用运行，不得静默退出。
- 更新进度单位：shell/状态事件使用 `0–100`，UI 统一转换为 `0–1` 后展示；查询与事件必须在同一归一化边界只换算一次，缺失进度保持未知，不得当作 `0%`。
- 版本一致性：用 `node scripts/version.mjs --bump <版本>` 同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`web/src/lib/constants.ts`，需通过 `node scripts/version.mjs` 检查。
- Release：提前提交 `.github/release-notes/v<版本>.md`（`release.yml` 用 `--notes-file` 读取），推送 `v<版本>` tag 触发构建；资产必须同时包含 macOS DMG、Windows NSIS 与唯一 `SHA256SUMS.txt`。
- README / 发布说明必须写明 macOS ad-hoc 签名与 Windows 未签名的放行方式（SmartScreen「更多信息 → 仍要运行」）。

### 5.4 任务、持久化与数据

- 任务清理只删除该任务拥有的临时数据；共享临时目录为空才可移除；不得删除已发布产物。
- 运行任务被移除/暂停后必须继续调度，迟到事件不得复活已移除任务。
- 任务元信息（作者、实际条目、实际清晰度/编码/音质、字幕与封面）必须持久化，不得用请求参数冒充实际采用参数。
- 手动重试失败/取消任务时，先真实校验并复用有效产物、追加文件和目标任务断点，只重新处理失败、缺失或失效条目；全部条目有效时直接恢复完成。重试状态必须先持久化成功再调度，失效记录可以从任务结果移除，但不得删除磁盘中的原文件。自动重试瞬时磁盘错误仍保持清理后完整重跑。
- 入队、删除、完成、历史写入与退出保存的持久化失败必须可见且可重试，不得只写日志后宣称成功。
- 下载全链路错误必须显示用户可读的中文原因，不得导致应用崩溃。
- 登录凭据只能保存在平台应用数据目录并限制为当前用户可读；登录 cookie 仅在有效登录态下用于高清和私有数据请求，不得写入仓库、当前工作目录或日志。头像加载失败不得阻断昵称、UID、会员状态或其他账号功能。

### 5.5 启动、设置与添加

- 启动本地阶段（凭据/设置/队列）优先就绪，网络准备 5 秒预算并支持降级重试；单项加载失败必须局部重试，不得整窗空白或用空列表冒充成功。
- 设置按字段原子提交，用 revision 防止旧响应覆盖；草稿与已保存分离，保存失败保留草稿并可重试/放弃。
- 添加确认按真实条目数量判断；空选择不得回落默认；质量回退必须记录并展示实际采用值。

### 5.6 性能与缓存

- 首屏：辅助页面/面板懒加载；首屏必载 JS 相对同条件基线必须降低 ≥15%（`node scripts/measure-first-load.mjs web/dist`）。
- 空闲零轮询；文件状态缓存不得替代恢复/文件操作前的真实校验；图片缓存有界（128 项 / 64 MiB、≤8 并发、单图 8 MiB 流式上限）。
- 进度反馈 ≤10Hz、离散状态即时；不得通过降低并发、静默降画质或移除正常反馈换取性能数字。

### 5.7 验收门（每次交付前）

```bash
pnpm typecheck && pnpm test && pnpm build:web && pnpm qa:interactions
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
openspec validate --all --strict
```

- 真机验收：macOS 行为按 `docs/qa/` 证据；Windows 真机不可用 CI 完全替代，未执行项不得勾选（CI 只补编译/单测证据）。
- 仅文档、不触发 CI 的 push：commit message 加 `[skip ci]`（GitHub Actions 会跳过该次 push 的 workflow）。

### 5.8 事故教训（2026-11-10）

- v2.4.0/v2.4.1 两次修复未经 change 直接改 main 并发布，被用户纠正：hotfix 也必须先建 change 并等待确认。
- Tauri capability 缺失会让前端窗口 API 静默拒绝；依赖 capability 的调用必须显式授权并加回归测试（`capability_allows_destroying_preferences_window` 为范例）。

## 六、冲突时的优先级

用户当轮明确指示 > 本文件规则 > 各工具默认行为。若用户指示与本文件冲突（例如要求跳过 change 直接改功能），先向用户指出规则并确认，得到明确许可后才可执行，并在 beads 中留痕。

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:970c3bf2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

<!-- BEGIN BEADS CODEX SETUP: generated by bd setup codex -->
## Beads Issue Tracker

Use Beads (`bd`) for durable task tracking in repositories that include it. Use the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or `~/.agents/skills/beads/SKILL.md` (global install) for Beads workflow guidance, then use the `bd` CLI for issue operations.

### Quick Reference

```bash
bd ready                # Find available work
bd show <id>            # View issue details
bd update <id> --claim  # Claim work
bd close <id>           # Complete work
bd prime                # Refresh Beads context
```

### Rules

- Use `bd` for all task tracking; do not create markdown TODO lists.
- Run `bd prime` when Beads context is missing or stale. Codex 0.129.0+ can load Beads context automatically through native hooks; use `/hooks` to inspect or toggle them.
- Keep persistent project memory in Beads via `bd remember`; do not create ad hoc memory files.

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.
<!-- END BEADS CODEX SETUP -->
