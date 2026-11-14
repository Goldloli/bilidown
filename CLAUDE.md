# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
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
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->


## Build & Test

```bash
pnpm typecheck && pnpm test && pnpm build:web && pnpm qa:interactions
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
openspec validate --all --strict
node scripts/version.mjs
```

## Architecture Overview

Rust 核心位于 `crates/bilidown-core/`，Tauri 桌面壳位于 `src-tauri/`，React 界面位于 `web/`。`openspec/specs/` 是行为契约，`AGENTS.md` 是完整协作规范；功能或可见行为变更必须先完成 OpenSpec change 并获得用户确认。

## Conventions & Patterns

- 平台差异以 `openspec/specs/` 为准；Windows、macOS 的行为和要求不得互相覆盖。
- Tauri 前端窗口 API 受 capability 限制；新增或修改相关调用必须同步权限配置并增加回归测试。
- 下载失败/取消任务手动重试时，先校验并复用有效产物、附加信息和目标任务断点，只重下失败、缺失或失效条目；全部条目有效时直接恢复完成。重试状态持久化成功后才能调度，且不得删除已发布文件。
- 应用更新 shell/事件进度使用 `0–100`，UI 在统一边界转换为 `0–1` 后展示；缺失进度保持未知。
- 软件更新代理与 B 站/媒体代理必须分离；更新可跟随系统、直连或手动代理，但视频、字幕和封面不得继承该设置。
- 入队、删除、完成、历史写入与退出保存的持久化失败必须可见且可重试；下载错误必须以可读中文提示展示且不得崩溃。
- 登录凭据只保存在平台应用数据目录并限制为当前用户可读；cookie 不得写入仓库、工作目录或日志。
- 发布前必须同步四处版本号并准备 `.github/release-notes/v<版本>.md`；推送版本 tag 后必须同时产出 macOS DMG、Windows NSIS 与唯一 `SHA256SUMS.txt`。
