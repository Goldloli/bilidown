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

## 五、冲突时的优先级

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
