# 官方输出一致性：Transcript Workspace 技能正文隐藏最终方案

> **已被替代：** 用户真实截图验收否定了本方案的“默认完整 transcript + skill 特判”方向。后续实施以 [2026-09-09 官方聊天展示与固定输入框 Workspace 修订方案](2026-09-09-official-chat-workspace-parity-plan.md) 为准。以下状态与结论仅是历史记录，不代表当前验收状态。

> 状态：**已吸收截至 2026-09-08 的全部对抗式审查发现；仅修订并提交方案，尚未修改生产代码、构建本地包或推送。**
>
> 审查结论：本版已通过最新一轮对抗式复审，可以开始 TDD RED；在第 8.1 节的真实失败证据成立前，不得编写生产实现。
>
> 本方案替代 `2026-09-08-skill-output-pwd-prefix-tdd-plan.md`。旧方案及当前未提交测试所要求的全局 parser `Unknown → Read` 行为明确废弃。

## 1. 目标与完成标准

### 1.1 用户目标

保留已经明确要求的本地 `codex-tui` 功能：

- 固定 Composer；浏览旧记录时可以继续输入，输入不强制跳到底部；
- 按用户轮次定位、折叠和展开；
- 输入非空时 `Ctrl+C` 只清空输入；`Ctrl+U` 的官方含义不变；
- iTerm2 中预览本地图片；
- Transcript Workspace 中不展开实际调用 skill 的指令正文。

除以上明确功能外，命令标题、输出行数、颜色、状态、执行过程、完整 transcript、普通文件、curl/Web、用户 shell、会话恢复等行为必须与对应上游 Codex 保持一致。

### 1.2 可观察的完成标准

对于截图同形命令：

```sh
pwd && sed -n '1,240p' "/absolute/enabled-skill/SKILL.md"
```

必须同时满足：

1. 官方 `parse_command` 结果仍为 `Unknown`，主聊天区仍按官方 `Ran …` 逻辑展示；
2. 官方完整 `transcript_lines` 仍保留原始命令、完整输出、退出码和耗时；
3. 只有自定义 Transcript Workspace 将该调用展示成复用官方样式的 `Read SKILL.md (<skill> skill)` 摘要，不显示正文；对此精确 `pwd && Read` 形状，前置 `pwd` 的 cwd 输出也属于 Workspace 中被省略的实现细节，必须由快照锁定；
4. 新会话、无 begin 的 completion、resume replay、旧历史分页均得到相同 Workspace 结果；
5. README、项目内普通 `SKILL.md`、curl、代码输出、混合命令和 `UserShell` 不被误隐藏；
6. 运行中的命令和缺少原始成功证据的完成事件保持完整显示；只有原始 `status == Completed && exit_code == Some(0)` 才可 compact；
7. 人工验收前必须重建 `codex-tui-package`，并证明本地 wrapper 使用新包、系统 `codex` 未变化；
8. 不改变系统 `codex`、`~/.zshrc`、iTerm2 配置、协议或会话文件。

## 2. 当前框架与已验证根因

### 2.1 官方输出有两条独立路径

上游 `ExecCell` 的契约是：

```text
主聊天区
  HistoryCell::display_lines
    ├─ 结构化 Read/List/Search → exploring_display_lines
    └─ 其他命令 → command_display_lines（Agent 输出预览最多 5 行）

官方完整 transcript
  HistoryCell::transcript_lines
    └─ 原始命令 + 全量 CommandOutput + 状态
```

上游没有“隐藏 skill 正文”的通用 renderer 分支。该行为只能是本地 Workspace 的明确例外，不能回写到上述两条官方路径。

### 2.2 本地 Workspace 的差异

本地为固定输入框和历史浏览新增：

```text
HistoryCell::workspace_transcript_hyperlink_lines
  └─ 默认回退到 transcript_hyperlink_lines
```

因此 Workspace 默认显示完整命令输出。现有补丁只在 `ExecCell` 被识别为 exploring、且 `ParsedCommand::Read` 名称带 skill 注解时压缩输出。

截图命令的真实链路是：

```text
zsh -lc 'pwd && sed .../SKILL.md'
  → parse_command_impl: [Unknown("pwd"), Read(SKILL.md)]
  → parse_command: 任一 Unknown 导致整体折叠为 Unknown
  → 现有 skill Read 注解无入口
  → ExecCell 不是 exploring cell
  → Workspace 回退完整 transcript
  → SKILL.md 正文全部显示
```

当前 renderer 单测手写了 `ParsedCommand::Read`，绕过了真实 parser、事件生命周期和历史投影，所以测试绿不能证明截图问题已解决。

### 2.3 官方已有可复用的 skill 读取识别

对抗式审查发现，上游已有：

```text
codex-skills::implicit_skill_accesses_for_command(command, workdir)
```

它复用官方 tokenizer、`parse_command_impl`、PathUri 和 Windows/Unix 路径规则，用于识别命令里的 skill 文档访问；因此不应在 TUI 再复制一套字符串路径猜测逻辑。

但该函数只回答“命令里存在什么文档访问”，不保证聚合输出只来自该文档。Workspace 仍需额外证明命令是纯读取形状，才能安全隐藏整段聚合输出。

### 2.4 `instruction_source_paths` 不能代替 skill catalog

`SessionConfigured` 的 `instruction_source_paths` 来自 `agents_md_manager`，表示 AGENTS/启动指令来源，不是已加载 skill 清单。把它拿来判定 skill 会漏报，不能采用。

### 2.5 实时与持久化历史不是同一条渲染链

实时命令经 `ChatWidget::command_lifecycle` 构建 `ExecCell`。

旧会话和分页历史经 `thread_items_to_transcript_cells`；其中 `ThreadItem::CommandExecution` 当前进入 `fallback_transcript_cell`，被压平成 `PlainHistoryCell`：

```text
$ command
status: ...
  full output
```

`PlainHistoryCell` 不保留 command action、命令 cwd 或 Workspace presentation。只修改 `ExecCell` 会导致新消息看似修好，但 resume/分页仍泄露完整 skill 正文。

### 2.6 技能清单存在加载时序

启动技能刷新明确使用后台任务，不阻塞第一帧；SessionConfigured 后另有正常的 `ListSkills` 请求。方案不能把空 `skills_all` 同时解释为“尚未加载”和“已加载但没有 skill”。

本功能采用明确状态：

```text
Unrequested | Loading | Ready(enabled skill roots) | Failed
```

为遵守“不误隐藏普通输出”的红线，`Unrequested`、`Loading` 和 `Failed` 时 fail-open 显示完整内容；Unrequested 只在 render 外触发一次去重刷新，技能清单到达后，已保留候选元数据的 Workspace cell 重新判定并重绘。也就是说，RPC 尚未发起、尚未返回或失败时不承诺隐藏；这是有意选择的安全边界，不用路径猜测换取表面上的“绝不闪现”。

## 3. 对抗式审查发现

| 级别 | 发现 | 证据与影响 | 修订结果 |
| --- | --- | --- | --- |
| P1 / 10 | 原方案会漏掉 resume/分页历史 | 持久化 `CommandExecution` 被压成 `PlainHistoryCell`，不会经过 `ExecCell` metadata | 增加持久化命令专用 cell，普通输出逐行保持现状，仅 Workspace 可覆盖 |
| P1 / 10 | 原方案未建模技能清单加载时序 | 启动 refresh 是非阻塞后台请求，空列表含义不明确 | 增加共享 `Loading/Ready/Failed` catalog，并测试两种事件顺序 |
| P1 / 10 | 当前 RED 测试契约错误 | 它要求全局 parser 将截图命令改成 `Read`，会改变官方主聊天语义 | 删除错误期望；新增“parser 必须继续 Unknown”的兼容测试和真实 Workspace 红测 |
| P1 / 9 | 原方案重复实现了上游已有识别能力 | `codex-skills` 已有 command/workdir → skill document access 逻辑 | 复用官方访问提取，只新增聚合输出安全门控 |
| P1 / 9 | 当前本地 `skill.enabled &&` 改了官方注解语义 | 上游 `annotate_skill_reads_in_parsed_cmd` 对所有 catalog skill 注解；本地多加 enabled 条件 | 该行恢复上游实现；Workspace 是否隐藏单独按 enabled catalog 判定 |
| P2 / 9 | “同一个 exploring cell 先建立 root”不成立 | 截图调用是 Unknown，不能并入 exploring group；独立 reference read 也可能在另一 cell | 每条纯 Read 都直接对权威 skill root 判定，不依赖同 cell 的前序调用 |
| P2 / 9 | 原方案使用 thread cwd 会误判历史 | `ThreadItem::CommandExecution` 自带执行时 cwd；会话中可切换目录 | 实时和持久化分类都使用 item 自带 cwd |
| P2 / 8 | 原验收没有真正锁定官方输出 | 二进制路径检查不能证明标题、正文、行数和样式未变 | 增加主聊天、完整 transcript、Workspace 三表面的同 fixture 快照/差分测试 |
| P2 / 8 | metadata 可能给长会话增加常驻内存 | 大字段直接塞进每个 `ExecCall` 会放大历史内存 | `ExecCall` 只增加可空 Box 指针；catalog 只存 cwd、path、name，不复制正文或输出 |
| P1 / 10 | catalog 请求失败、部分响应和乱序响应没有可归属的 cwd/generation | 当前失败 handler 只有 `Result`，后台与交互刷新都可能异步到达；旧的 enabled 结果不能覆盖新的 disabled 结果 | 引入每 cwd `RefreshTicket { cwd, generation }`；只接受最新 ticket 的成功、失败和缺项结果，过期响应丢弃 |
| P1 / 10 | catalog 到达后，已完成 cell 与已打开 Workspace 没有明确的重判定/布局失效所有权 | `HistoryCell` 渲染接口只接收 width；Pager 还会缓存 Workspace 高度 | 候选 cell 单向持有共享 catalog；catalog 更新统一使 live tail revision 与 Workspace layout 失效，并保持视口锚点 |
| P1 / 9 | 正常 begin/end 配对会丢失执行时 cwd | 当前 `RunningCommand` 不保存 cwd；不能用 completion 时的 thread/current cwd 替代 | begin 时将已验证 item cwd 写入 candidate/running state；orphan/completion-only 从事件 cwd 构造；无法转换即 fail-open |
| P1 / 9 | `pwd && Read` 的聚合输出包含 cwd，不可能证明全部来自文档 | 直接 compact 会省略 cwd 行，不能暗中宣称“仅隐藏正文” | 明确接受该精确形状连同 cwd 行一起隐藏；用快照锁定。若不能接受，必须对此形状 fail-open |
| P2 / 8 | UnifiedExecInteraction 的 live output 已被官方 lifecycle 清空 | 不能用“sentinel 被隐藏”覆盖一个本来没有正文的 live 路径 | Agent/UnifiedExecStartup 才是正例；Interaction live 一律保持现有 outputless 视图，持久化 Interaction 也 fail-open |
| P1 / 10 | Unknown command 无法直接复用只读取 `ParsedCommand::Read` 的官方摘要 helper | 截图命令必须保持整体 Unknown；伪造或覆盖原 parsed 会破坏官方输出红线 | 增加独立 `WorkspaceReadSummary` presentation；共享排版 helper 接受摘要数据，不读取或改写原 parsed |
| P1 / 10 | candidate 没有命令 outcome，且 live completion 会把缺失 exit code 归一化为 0 | 运行中输出可能被提前隐藏；`Completed + None` 可能被当成成功 | presentation 明确保存 `Pending | Succeeded | Failed`；只按原始事件的 `Completed + Some(0)` 进入 Succeeded |
| P1 / 9 | 非当前 item cwd 可能没有 catalog 请求 | 仅按 Session/cwd 切换刷新时，历史或 orphan candidate 会永久 fail-open | catalog 增加 `Unrequested`；live 和 Workspace 历史投影收集未知 cwd，由 App 在 render 外去重并批量刷新 |
| P2 / 8 | 旧历史 source 缺失时 serde 会默认 Agent | 理论上可能误把 UserShell 当 Agent；但 v2 `CommandExecution.source` 与所有 source 枚举同一提交引入，当前 app-server 生成的 typed item 会显式序列化 source | 不改协议；只信任当前 app-server typed item，并增加真实 UserShell 持久化/恢复测试。外部伪造或缺字段 v2 payload 不在本地 TUI 目标范围内 |
| P1 / 10 | catalog 更新后 committed cell 仍可能被 `CachedRenderable` 缓存 | 只失效 Workspace layout 不会自动重建稳定 cell 的内容 | 含 Workspace candidate 的 cell 覆盖 `has_stable_transcript_height() -> false`；catalog 变化仍失效全局 layout index |
| P1 / 10 | `SetSkillEnabled` 成功后只更新 `skills_all` | 禁用后 catalog 可能继续保留 enabled root；旧 in-flight response 还可能把刚禁用的 skill 写回 | 配置写成功后使所有旧 ticket 过期、全部缓存 cwd 转为 Unrequested 并立即批量刷新；刷新期间 fail-open |
| P1 / 10 | 自动化命令违反仓库 `AGENTS.md` | 直接 `cargo test` 被禁止；Cargo 依赖变化还要求更新 Bazel lock，UI 变化要求审阅 insta snapshots | 改用 `just test/fix/fmt`，补 `cargo nextest list`、`cargo insta pending-snapshots` 与 `just bazel-lock-update` |
| P1 / 10 | iTerm2 验收可能运行旧的 packaged binary | `~/.local/bin/codex-tui` 只是 wrapper，不会自动构建 | 人工验收前执行 `scripts/build-patched-tui.sh`，校验包内二进制时间/哈希、wrapper 路径和系统 `codex` 独立性 |

修订后的方案已处理以上缺口；仍保留第 10 节声明的证据边界。

## 4. 最终设计

### 4.1 总体数据流

```text
SkillsListResponse / SkillsConfigWrite + RefreshTicket(cwd, generation)
  └─ WorkspaceSkillCatalog（共享；Unrequested/Loading/Ready/Failed）
             │
             ├──────────────┐
             ▼              ▼
实时 ThreadItem          持久化 ThreadItem
CommandExecution         CommandExecution
  │ raw command            │ raw command
  │ item cwd               │ item cwd
  │ source/actions         │ source/actions
  ▼                        ▼
derive_workspace_skill_read_candidate（同一个纯函数；只在事件/投影时 parse）
  │
  ├─ 普通/混合/用户 shell → None → 官方输出不变
  └─ WorkspaceSkillReadPresentation
       ├─ candidate：document path + item cwd + read filename
       ├─ outcome：Pending | Succeeded | Failed
       ├─ cell 单向持有 Arc<catalog>，Workspace render 只查轻量索引
       ├─ outcome Succeeded + catalog Ready + enabled root 命中
       │    └─ catalog resolve → WorkspaceReadSummary → Compact
       ├─ Pending/Failed/Unrequested/Loading/catalog Failed → Full
       └─ Unrequested cwd → render 外去重发起 ListSkills
             │
             ├─ ExecCall（实时）
             └─ PersistedCommandHistoryCell（resume/分页）

display_lines             → 永远不读取 Workspace metadata
transcript_lines          → 永远不读取 Workspace metadata
workspace_transcript_*    → 仅 Compact 时复用官方 Read 摘要并省略正文
```

### 4.2 WorkspaceSkillCatalog

在 `codex-rs/tui/src/workspace_skill_output.rs` 集中定义 catalog、ticket、candidate、outcome 和 Workspace Read 摘要。`App` 创建唯一 `Arc<WorkspaceSkillCatalog>` 并把 clone 传给 `ChatWidget` 与候选 cell；catalog 绝不持有 cell。中央 `App`/`ChatWidget` 文件只保留事件接线，不承载分类规则。

Catalog 要求：

- 按 cwd 保存 `{ generation, Unrequested | Loading | Ready | Failed }`。缺少 key 与显式 `Unrequested` 语义相同；`begin_refresh(cwds)` 必须为每一个确切 cwd 递增 generation，并返回不可伪造的 `RefreshTicket { cwd, generation }`；
- `Ready` 只保存 `enabled == true` 的主 `SKILL.md` 绝对路径和显示名；不以目录后代、符号链接或前缀关系扩展隐藏范围；
- 不保存 description、SKILL.md 正文、命令输出或会话内容；
- `SessionConfigured`、cwd 切换、启动后台刷新和交互刷新都先取得 ticket；响应 handler 必须保留 ticket 的 cwd/generation，不能只传 `Result`；
- 成功响应只更新 ticket 包含的 cwd，且仅当 generation 仍相等。请求中某 cwd 在 response 中没有唯一 entry、entry 有任何 `errors`、或路径不能形成权威轻量索引时，该 cwd 标记 `Failed` 并 fail-open；存在唯一 entry、`skills` 为空且 `errors` 为空时必须写入 `Ready(empty)`，不得重新混淆“已加载为空”和“加载失败”；
- 请求失败时，逐个将 ticket 覆盖的 cwd 标记 `Failed`，但只在 generation 仍相等时执行；不影响其他 cwd；
- 过期成功/失败响应必须完全丢弃，不能覆盖更新后的 enabled 状态；
- live candidate 建立后，如 item cwd 为 Unrequested，`ChatWidget` 只发送 `AppEvent::EnsureWorkspaceSkillCatalog { cwds }`；Workspace-aware 历史投影返回去重后的 `required_skill_cwds`。`App` 合并两路 cwd，只对 Unrequested cwd 调用一次 `begin_refresh` 并批量请求；render 本身不得发请求；
- Failed 不因每个 candidate 自动重试，避免历史渲染形成请求风暴；只有显式 refresh、SessionConfigured、cwd 切换或成功的 skill 配置变更允许重试；
- `SkillsConfigWrite` 成功是独立的权威状态变更：对全部缓存 cwd 递增 generation，使所有旧 in-flight ticket 立即过期，并统一转为 Unrequested；随后 `App` 对“全部缓存 cwd + 当前 cwd”去重并发起一次批量 refresh。刷新完成前 Workspace 全部 fail-open。写入失败不改变 catalog；enable/disable 成功都必须立即失效 Workspace 视图；
- 成功或失败状态确实变化后，调用一个由 `App` 拥有的 `invalidate_workspace_catalog_view()`：它必须使 active live tail revision 变化、令打开的 `PagerOverlay` Workspace layout index 失效，并保留当前 turn 与相对视口锚点。单独 `request_redraw()` 不构成完成；
- 只有候选 cell 持有 `Arc<WorkspaceSkillCatalog>`。candidate 保存结构化 document path、执行 cwd、来源和摘要所需标识，不保存已决的 Compact/Full 结果；每次 Workspace render 仅做 catalog 索引查询，不做 shell parse、路径 canonicalize 或文件系统访问；
- catalog 内部同步必须满足 `HistoryCell: Send + Sync`；读取锁失败/poison 时直接 Full，禁止 render panic；
- catalog 不持有 cell，禁止形成 Arc 引用环。用不含 `ChatWidget` owner 的 isolated Arc fixture：依次 drop 初始 owner 与全部候选 cell 后，`Weak<WorkspaceSkillCatalog>` 必须不可升级。

普通官方 skill 注解仍按上游原逻辑执行。当前本地在 `annotate_skill_reads_in_parsed_cmd` 多加的 `skill.enabled` 过滤必须恢复上游版本，因为 Workspace metadata 已不再依赖改写 `ParsedCommand::Read.name`。

### 4.3 纯读取候选分类

新增一个 TUI 私有纯函数，输入必须来自真实 `ThreadItem::CommandExecution`，并只产生与 catalog 无关的结构化候选：

```text
raw command string + round-trippable argv + item cwd + source + command_actions
  → Option<WorkspaceSkillReadPresentation {
       candidate,
       outcome: Pending,
       catalog,
     }>
```

只有 `Agent` 和 `UnifiedExecStartup` 可产生候选；`UserShell` 永远返回 None。`UnifiedExecInteraction` 的 live lifecycle 已有 outputless 语义，不得为了此功能制造 Read 摘要或输出；持久化历史的 Interaction 也 fail-open。catalog Unrequested/Loading/Ready/Failed 的查询发生在 Workspace render，而不在本函数中。

只接受两种聚合输出可证明安全的语法：

1. argv 必须精确为无额外 operand 的 `cat <document>`，或 `sed -n 1,240p <document>`；同时官方 action/解析结果恰为一个 `Read`；
2. shell AST 恰为两个 plain command，由唯一顶层 `&&` 连接：左边 token 精确为无参数 `pwd`，右边必须满足上一条的精确 argv 约束，并单独调用官方 `parse_command` 后恰为一个 `Read`。整体 command 的官方 action 仍必须是 `Unknown`。

同时调用上游 `codex_skills::implicit_skill_accesses_for_command(raw_command, item_cwd)`：

- 必须恰好得到一个 `Document`；
- 不得得到 `Script`；
- Document 必须与前述 Read 对应；
- 路径比较使用 PathUri/路径组件，不使用字符串前缀；
- Ready 时，Document 必须精确等于 catalog 的主 `SKILL.md`；附属/reference 文档、任意目录后代和符号链接目标一律完整显示；
- skill 名称和最终 `WorkspaceReadSummary` 只能由 Ready catalog 的精确主文件匹配产生；Unrequested/Loading 阶段的 candidate 不得预填或猜测 skill 名；
- renderer 只有在 presentation outcome 为 `Succeeded` 时才可使用上述 Ready 匹配；
- 无匹配、相对路径无法解析、路径协议不一致时完整显示。

`raw_command` 只用于 `implicit_skill_accesses_for_command`；AST helper 只接收经过 round-trip 验证的 argv。不得把整体 `Unknown` action 当作右侧 Read，也不得从 `ParsedCommand::Read.name` 推断 candidate。外层命令无法 round-trip、argv 不是 shell wrapper、或 AST/右侧 parser 不完全匹配时一律 fail-open。

`pwd && Read` 是唯一允许省略非文档输出的例外：Workspace compact 会省略这一个 `pwd` 输出行和 skill 正文。它是产品契约，不是“输出完全来自文档”的证明；相应快照必须包含并断言 cwd sentinel 不在 Workspace、仍在完整 transcript。

以下全部 fail-open：

- `pwd --physical && read`；
- `pwd ; read`、`pwd || read`、`pwd | read`；
- `pwd && read && curl`；
- `echo ... && read`、环境变量前缀、重定向、命令替换、subshell；
- 同一命令读取两个文件；
- skill 脚本执行；
- 非零退出码的读取；
- 项目普通 `SKILL.md`、未启用 skill、跨 root 读取；
- 解析不完整或来源为 `UserShell`。

在 `codex-rs/shell-command/src/bash.rs` 只新增无副作用的 AST 查询帮助函数，例如：

```rust
pub fn parse_shell_lc_two_plain_commands_joined_by_and(
    command: &[String],
) -> Option<(Vec<String>, Vec<String>)>
```

它不得修改 `parse_command`、`parse_command_impl`、简化规则或既有调用结果。单文档路径解析和跨平台路径解析复用 `codex-skills`，不在 TUI 复制。

### 4.4 实时生命周期

`RunningCommand` 在 begin 时缓存 raw command、已验证的执行 cwd 与 boxed Workspace-only presentation；`ExecCall` 只增加：

```rust
workspace_skill_read: Option<Box<WorkspaceSkillReadPresentation>>
```

使用 Box 的原因是普通命令仅付出一个指针大小，不把 PathBuf/String/Arc 的最大 enum payload 塞进每个 `ExecCall`。

正常 begin/end 配对必须一直使用 begin item 的 cwd，绝不能回退到当前 thread cwd。completion-only、orphan 与 replay 从自身 event 的 cwd 产生 presentation；cwd 不能转换为 `PathUri` 时返回 None。候选接入 `ExecCall` 后保持 `Arc<catalog>`，等待 catalog 从 Unrequested/Loading 变 Ready 时可重判定。

命令 outcome 与 catalog state 是两条正交状态机：

```text
begin/output delta                → Pending → Workspace Full
raw Completed + exit_code Some(0) → Succeeded
raw Completed + exit_code None    → Failed  → Workspace Full
raw Failed/Declined/其他非成功状态 → Failed  → Workspace Full

Succeeded + catalog Ready + enabled root match → Compact
其他组合                                      → Full
```

outcome 必须在 `handle_command_execution_completed_now` 对原始 `status` 和原始 `Option<i32>` 判定，早于当前 `unwrap_or_default()` 的显示层归一化。运行中 skill 正文可能暂时在 Workspace 可见，这是遵守“未确认时 fail-open”的明示取舍；不得在 Pending 时提前隐藏，也不得吞掉失败诊断。

同一个候选构造函数必须覆盖：

- begin → running command → active `ExecCell`；
- begin/end 正常配对；
- UnifiedExec Unknown begin 未物化 cell、只在 completion 建 cell；
- orphan completion；
- replay completion。

completion 没有 running state 时，从 completion 自带的 raw command、cwd、source、actions 重新计算，再按同一个原始 outcome 规则完成 presentation，禁止依赖 begin 一定存在。`UnifiedExecInteraction` 不属于本功能的正例或 sentinel 测试，必须保持其现有空 output 行为。

### 4.5 持久化历史与分页

保留现有 `thread_items_to_transcript_cells` 公开行为，新增 Workspace-aware 内部投影入口。只有实际填充交互式 Workspace 的历史路径使用它，resume picker、导出和其他官方消费者继续走原入口。

对于有 Workspace candidate 的持久化 `CommandExecution`，构建轻量 `PersistedCommandHistoryCell`：

- `display_lines`、`transcript_lines`、`raw_lines` 必须逐行等于现有 `fallback_transcript_cell`；
- 复用同一个 `command_execution_fallback_lines` 帮助函数，禁止复制 `$ command / status / output` 格式；
- 只覆盖 `workspace_transcript_hyperlink_lines`；
- 分类必须使用 item 自己的 `cwd`，不是 thread 当前 cwd；
- 只有持久化 item 的原始 `status == Completed && exit_code == Some(0)` 才设置 Succeeded；InProgress、None、Failed、Declined 全部 Full；
- 当前 app-server 生成的 typed `CommandExecutionSource` 是来源权威。v2 `CommandExecution.source` 与 Agent/UserShell/UnifiedExec 枚举在同一历史变更中引入，UserShell 又早于该 v2 结构存在；当前未发现 app-server 生成 source-less UserShell item 的路径。增加真实 UserShell 持久化与恢复测试，不为外部伪造或缺字段 payload 修改协议；
- 普通命令仍使用原 `PlainHistoryCell`，不承担额外状态。

初始 resume 的真实渲染路径不能靠“通常会走实时事件”假定。必须用 `resume → 打开 Workspace` 集成测试确认：若初始完成命令经过实时 replay，则使用第 4.4 节；若它经过 fallback projection，则只在那个交互式 adapter 接入本节入口。旧页加载使用本节。两条路径必须使用同一候选构造函数和相同 fixture。

### 4.6 Workspace 渲染

现有 `display_lines` 和 `transcript_lines` 不改语义。Workspace 按 call/cell 独立决定：

```text
Compact → 官方 Read 摘要样式；不渲染 CommandOutput
Loading → 完整 transcript（fail-open）；catalog 到达后通过 `invalidate_workspace_catalog_view()` 使 cell、live tail 和布局一并重绘
Full    → 完整 transcript
```

将现有 `exploring_display_lines_for_calls` 中的 Read 摘要排版提取为一个私有共享 helper，正常 exploring 与 Workspace 都调用它。不得在 Workspace 重新实现颜色、缩进、折行、状态图标，也不得伪造或覆盖原始 `ParsedCommand`。

共享 helper 不再读取 `ExecCall.parsed`，而是接收最小 `WorkspaceReadSummary { name }`/普通 Read 摘要列表。正常 exploring 路径先从真实 `ParsedCommand::Read` 映射为摘要；Workspace 路径在 render 时用 presentation candidate 查询 Ready catalog，由匹配到的 enabled skill name 生成摘要。截图命令的原 `ExecCall.parsed` 必须始终保持整体 Unknown，presentation 只影响 Workspace hook。

含任一 Workspace presentation 的 `ExecCell` 以及 `PersistedCommandHistoryCell` 必须返回 `has_stable_transcript_height() == false`，复用 Pager 对外部状态 cell 的既有非缓存路径；普通 cell 保持 `true`。catalog 状态改变时仍需失效全局 Workspace layout index，因为 turn/cell 的累计行高已变化。

现有 `call_reads_skill_content` 基于“注解 name + 同 cell root”的推断必须移除或完全退出决策链，避免两套真相源。

## 5. 文件与改动范围

预计生产文件：

- `codex-rs/tui/Cargo.toml` / `MODULE.bazel.lock`：增加内部 workspace 依赖 `codex-skills`，并按仓库规则刷新 Bazel lock；
- `codex-rs/shell-command/src/bash.rs`：严格的 `pwd && read` AST 查询 helper；
- `codex-rs/tui/src/lib.rs` / `workspace_skill_output.rs`：注册私有模块，集中定义 catalog、ticket、candidate、outcome、summary 和纯分类逻辑；
- `codex-rs/tui/src/chatwidget.rs` / `chatwidget/constructor.rs`：只接入共享 catalog 字段，不新增独立分类规则；
- `codex-rs/tui/src/chatwidget/skills.rs`：以 ticket 同步 catalog，并恢复上游注解条件；
- `codex-rs/tui/src/chatwidget/session_flow.rs`：refresh 前标记 Loading；
- `codex-rs/tui/src/app/background_requests.rs` / `app/thread_routing.rs` / `app_event.rs` / `app/event_dispatch.rs`：在成功、失败、按需 cwd 和 skill toggle 事件中保留 ticket，以正确标记 Failed、更新 enabled 状态或丢弃过期结果；
- `codex-rs/tui/src/chatwidget/exec_state.rs`：running metadata；
- `codex-rs/tui/src/chatwidget/command_lifecycle.rs`：实时 begin/end/orphan/replay 分类；
- `codex-rs/tui/src/exec_cell/model.rs`：boxed presentation，不复制输出；
- `codex-rs/tui/src/exec_cell/render.rs`：仅 Workspace 使用 presentation；
- `codex-rs/tui/src/thread_transcript.rs`：持久化命令 Workspace 投影及共享 fallback lines；
- `codex-rs/tui/src/app/history_pagination.rs`：旧页使用 Workspace-aware 投影。
- `codex-rs/tui/src/pager_overlay.rs`：catalog 更新时失效 Workspace layout，并保持视口锚点。

这是跨实时生命周期、历史投影与 Workspace 缓存的单一状态机变更，不拆成并行工作树。预计生产 diff 必须控制在 500 行左右；若实现超过 500 行或需要在上述中央文件加入大段逻辑，先停止并把逻辑继续收敛到 `workspace_skill_output.rs`，不得通过散布 helper 越过仓库规模约束。

如实施中需要修改 app-server 协议、会话 schema、执行器、审批器、系统启动器或上述范围外的官方输出模块，必须停止并重新审查方案；不得以“顺手重构”扩大范围。

## 6. 明确不做什么

- 不修改命令执行、审批、安全判断和 shell 语义；
- 不修改 `parse_command` 的返回结果或 Unknown 折叠规则；
- 不删除、截断或改写会话数据；官方完整 transcript 仍可看到原输出；
- 不按 `/skills/`、`.codex`、文件名或字符串前缀猜测 skill；
- 不隐藏 skill 脚本输出、普通工具输出、curl/Web 输出或用户 shell 输出；
- 不增加配置开关、数据库字段、协议字段、后台服务或常驻进程；
- 不为外部伪造或缺少 `source` 的 v2 payload 增加协议迁移；本地 TUI 只消费当前 app-server 生成的 typed items；
- 不修改系统 `codex`、`~/.zshrc`、iTerm2、全局别名或安装目录；
- 不修改 `scripts/codex-tui.sh` 或 `scripts/build-patched-tui.sh`；它们只作为既有构建与验收入口；
- 不借本修复调整轮次折叠、Composer、快捷键、图片、退出终端恢复或其他已完成功能；这些只做回归测试。

## 7. 红线

1. **官方输出红线：** 除 Transcript Workspace 的 confirmed skill Read 外，任何标题、行数、颜色、状态或内容差异都是回归。精确 `pwd && Read` 正例中 cwd 输出被 Workspace 省略是唯一明示例外；完整 transcript 仍保留它。
2. **parser 红线：** 截图命令在官方 parser 中必须继续是 `Unknown`；不得为 Workspace 改全局解析语义。
3. **误隐藏红线：** 无权威 enabled skill root、混合输出、用户 shell、解析失败、catalog Unrequested/Loading/Failed、含 `SkillsListEntry.errors`、缺少 response entry、过期状态、Pending/Failed outcome、缺少原始 exit code 和 `UnifiedExecInteraction` 一律完整显示。
4. **数据红线：** 不修改协议、rollout、resume 数据和官方完整 transcript；只改变 Workspace 的视图投影。
5. **范围红线：** 每行代码必须能追溯到 skill 正文隐藏或其必要回归保护；不重构相邻模块。
6. **内存红线：** 不复制正文或命令输出进 metadata，不为每个 cell 建 catalog，不形成 Arc 环，不在 render 中解析 shell或访问文件系统。
7. **上游同步红线：** 自定义逻辑必须集中在 Workspace hook、独立 classifier 和少量 lifecycle 接线；不得散布修改官方 renderer 主路径，避免后续 rebase 冲突。
8. **仓库规则红线：** 不直接运行 `cargo test`；依赖变化必须刷新 `MODULE.bazel.lock`，UI 变化必须审阅 insta snapshot，本地人工验收必须先重建 packaged binary。

## 8. TDD 与验收设计

### 8.1 RED：先证明当前实现必失败

实施第一步不是写生产代码，而是完成以下测试；至少前三项必须在当前代码上稳定失败：

1. **真实截图命令 / live Agent**：用真实 argv/action 和 sentinel 正文，不手写已注解 Read；Workspace 当前仍包含 sentinel。
2. **真实截图命令 / UnifiedExecStartup completion-only**：begin 因 Unknown 未物化 cell，completion 仍必须最终 compact；当前失败。
3. **持久化历史分页**：`ThreadItem::CommandExecution` 经 Workspace-aware page projection 后不得包含 sentinel；当前 `PlainHistoryCell` 路径失败。
4. **状态时序与乱序**：分别执行 `skills → command`、`command → skills`；后者在 Loading 时先 fail-open，Ready + `invalidate_workspace_catalog_view()` 后必须隐藏。再执行 `generation 1 enabled → generation 2 disabled → generation 1 late success`，最终必须完整显示；`generation 1` 的 late error 也不得把 `generation 2` 的状态改为 Failed。
5. **官方 parser 锁定**：截图 argv 必须继续得到一个完整 `Unknown`；该测试当前应为绿。
6. **执行 cwd 锁定**：begin 在 cwd A、当前 thread 切到 cwd B 后 completion 到达；相对 skill 路径只可按 A 判定。cwd 无法转为 `PathUri` 时必须完整显示。
7. **既有 cell 重判定**：已完成 live cell 与已分页 persisted cell 在 catalog 从 Loading 变 Ready 后同时 compact；Workspace 停在旧 turn 时，turn、相对视口锚点和 Composer 输入均不得变化。
8. **presentation 与原 parsed 隔离**：截图命令的 `ExecCall.parsed` 逐对象等于原始整体 Unknown；Unrequested/Loading candidate 不含 skill 名，Ready 后由 catalog resolve 生成独立 `WorkspaceReadSummary` 并显示 Read 摘要。
9. **原始 outcome 门控**：Pending、`Completed + None`、Failed、Declined 均完整显示；只有 `Completed + Some(0)` compact。该断言必须发生在显示层把 None 归一化为 0 之前。
10. **Unrequested cwd 按需加载**：current cwd 为 B、candidate item cwd 为 A 且 catalog 不含 A；第一次 Full，App 只发一个 A 的请求，Ready 后 compact；反复 render 不得重复请求。
11. **skill toggle**：Workspace 已打开时 enable/disable 成功都立即使全部旧 ticket 过期并恢复完整输出；批量 refresh Ready 后按新 enabled 状态重新判定。配置写失败不得改变 catalog 或 Workspace；toggle 前发出的 late success/error 均不得覆盖新 generation。
12. **动态 cell 禁止缓存**：含 candidate 的 live/persisted cell 的 `has_stable_transcript_height()` 为 false，普通 cell 为 true；Loading→Ready 后无需重建历史数据即可改变可见正文。

当前未提交的 `zsh_pwd_then_sed_skill_file_is_read` 必须先移除，因为其期望本身违反第 7.2 条；用第 5 项替换，不能通过修改实现把错误测试“修绿”。

### 8.2 三表面同 fixture 验收

每个正负例都对同一个 cell/item 同时断言：

| 表面 | 正例要求 | 普通命令要求 |
| --- | --- | --- |
| `display_lines` | 与上游 golden snapshot 完全一致 | 与上游完全一致 |
| `transcript_lines` | 包含原始命令、sentinel、状态 | 与上游完全一致 |
| `workspace_transcript_hyperlink_lines` | Ready 后无 sentinel，含官方 Read 摘要 | 与现有完整 Workspace 输出一致 |

不能只用 `contains/does_not_contain`；标题、缩进、空行、状态、折行必须用 snapshot 锁定。Golden snapshot 在实现前依据 `upstream/main` 生成并人工核对，避免测试代码与被测代码共享同一个错误期望。

对于截图同形正例，fixture 还必须输出独立的 cwd sentinel。完整 transcript 必须保留这个 cwd sentinel 和 skill 正文 sentinel；Workspace 必须同时不包含两者。这是第 1.2 节明确接受的、唯一可省略非文档输出的例外。

同一 fixture 还必须深比较原始 `ParsedCommand`、`display_lines` 和 `transcript_lines`；Workspace 摘要使用独立 presentation，不能通过改写原 parsed 让测试转绿。

### 8.3 正例矩阵

- 直接读取 enabled skill 的 `SKILL.md`；
- 截图同形 `zsh -lc 'pwd && sed …/SKILL.md'`；
- Agent 与 UnifiedExecStartup 的非用户 shell 路径；
- begin/end、completion-only、orphan completion；
- 实际 `resume → 打开 Workspace` 路径，以及旧历史 page prepend；
- item cwd 与 thread cwd 不同；
- Unrequested item cwd 经 App 去重刷新后转为 Ready；
- 成功的 skill enable 后当前 cwd 重新 compact。

### 8.4 对抗式负例矩阵

- `/project/SKILL.md`、README、源码、curl 大 JSON；
- disabled skill；
- `UserShell`；
- catalog Loading、Failed、Ready 无匹配；
- catalog Unrequested、锁 poison；
- response 缺少被请求 cwd、entry 含 `errors`、请求失败、同 cwd generation 乱序响应；
- response 中同一 cwd 重复 entry；`skills=[] && errors=[]` 必须得到 Ready(empty) 而不是 Failed；
- `pwd ; read`、`pwd || read`、`pwd | read`；
- `pwd --physical && read`；
- `pwd && read && curl`；
- `echo hi && read`、重定向、命令替换、subshell、环境前缀；
- 一条命令读取两个文件或两个 skill root；
- skill 脚本执行；
- 非零退出码读取，错误输出必须可见；
- 路径前缀相似但组件不匹配，如 `skill-root-extra`；
- 相对路径无法与 item cwd 安全合成；
- outer command 无法 shell round-trip、shell argv 不是 wrapper、右侧 parser 不是唯一 Read；
- skill list 刷新前后顺序颠倒；
- skill metadata 到达时 Workspace 已打开并停留在旧位置，重绘不得跳底。
- `UnifiedExecInteraction` live 与 persisted history：保持完整既有视图，不生成新的 Read 摘要。
- Pending、`Completed + exit_code None`、Failed、Declined；
- 成功 enable/disable、失败的配置写、toggle 前请求在 toggle 后晚到；
- 当前 app-server 产生并恢复的 `UserShell` item，来源必须保持 UserShell 且完整显示。

### 8.5 自动化命令

新增相关测试统一使用 `workspace_skill_` 前缀。实施时先用 `cargo nextest list` 核实每个过滤表达式至少命中一个测试；测试执行必须使用仓库 `just test`，不得直接运行 `cargo test`：

```sh
cd "/Users/chy/projects/codex-tui-patched/codex-rs"
PATH="/opt/homebrew/opt/rustup/bin:$PATH" cargo nextest list -p codex-shell-command -E 'test(~workspace_skill)'
PATH="/opt/homebrew/opt/rustup/bin:$PATH" cargo nextest list -p codex-tui -E 'test(~workspace_skill)'

cd "/Users/chy/projects/codex-tui-patched"
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-shell-command -E 'test(~workspace_skill)'
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-shell-command
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-tui -E 'test(~workspace_skill)'
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-tui

cd "/Users/chy/projects/codex-tui-patched/codex-rs"
PATH="/opt/homebrew/opt/rustup/bin:$PATH" cargo insta pending-snapshots -p codex-tui

cd "/Users/chy/projects/codex-tui-patched"
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just bazel-lock-update
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just bazel-lock-check
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just fix -p codex-shell-command
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just fix -p codex-tui
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just fmt
```

测试过滤器若显示 0 tests，整项验收失败；不得把命令退出码 0 误当作测试已执行。必须逐个阅读 `*.snap.new`；只有确认全部属于本功能后，才在 `codex-rs` 目录执行 `cargo insta accept -p codex-tui`。按仓库约定，`just fix`/`just fmt` 后不重复跑测试，但必须检查最终 diff，确认自动修复没有扩大范围。

### 8.6 iTerm2 人工验收

在隔离临时 `CODEX_HOME` 创建测试 skill，正文放唯一 sentinel，避免拿真实 skill 大段正文做肉眼判断：

1. 记录 `command -v codex`、`command -v codex-tui`、系统 `codex --version`、当前 package binary 的修改时间和哈希，并记录构建开始时间；
2. 在仓库根执行 `PATH="/opt/homebrew/opt/rustup/bin:$PATH" ./scripts/build-patched-tui.sh`，确认 `codex-rs/target/codex-tui-package/bin/codex` 与 `codex-code-mode-host` 均可执行；
3. 再次记录 package binary 修改时间和哈希；mtime 必须不早于构建开始时间，哈希仅用于记录，不要求同一提交的确定性重建改变哈希。确认 `codex-tui --version` 可运行，wrapper 最终指向该 package，`command -v codex` 与系统 `codex --version` 完全未变；
4. 用系统 `codex` 跑普通命令，保存官方主聊天表现基线；
5. 用刚构建的 `codex-tui` 触发同形 skill 读取；主聊天与官方基线一致；
6. 打开 Workspace，命令运行中按 fail-open 显示；原始 `Completed + Some(0)` 后 sentinel 不存在并显示 Read 摘要；`Completed + None`/Failed fixture 仍显示完整诊断；
7. 同轮执行 README、curl、普通 shell 和 UserShell，Workspace 仍显示完整输出；
8. 在 Workspace 已打开时 disable skill，现有摘要立即恢复完整输出；重新 enable 并取得当前 cwd Ready 后再次 compact；
9. resume 后重复；向前加载旧页后重复；包含不同 item cwd 的页面应触发一次按需刷新并在 Ready 后 compact；
10. 在 Workspace 停在旧轮次并输入文字时，让 skills 从 Loading 变 Ready；正文与 cwd sentinel 消失，但当前 turn、相对视口和输入不得改变；
11. 回归固定 Composer、轮次折叠、`Ctrl+C`、`Ctrl+U`、图片和 `/exit` 终端恢复。

### 8.7 失败模式与可观察结果

| 路径 | 现实失败模式 | 自动化覆盖 | 用户可见结果 |
| --- | --- | --- | --- |
| skills refresh | 旧 generation 晚到或请求失败 | generation success/error 乱序测试 | 过期响应被丢弃；不误隐藏，失败时完整显示 |
| empty catalog | 成功响应没有任何 skill，却被误当成失败或 Loading | Ready(empty) 与缺/重复 entry 对照测试 | Ready(empty) 稳定完整显示，不重复请求 |
| candidate cwd | 历史/孤儿 completion 的 cwd 尚未请求 | Unrequested 去重与单次批量刷新测试 | Ready 前完整显示；Ready 后原地 compact |
| skill toggle | 配置写成功但旧 catalog/旧 in-flight response 未失效 | enable/disable/写失败/late response 测试 | 成功后立即完整显示；新 refresh 后按新状态判定 |
| committed cell | catalog 已变但 Pager 仍缓存旧 renderable | stable-height 与已打开 Overlay 测试 | 正文和布局同步变化，视口不跳 |
| command outcome | Pending、None exit、Failed 被误当成功 | 原始 status/exit 全矩阵 | 未确认或失败时完整诊断；只有 Some(0) 隐藏 |
| summary renderer | 为显示 Read 摘要而改写整体 Unknown | parsed 深比较 + 三表面 snapshots | 主聊天/完整 transcript 保持官方，Workspace 才显示摘要 |
| persisted source | UserShell 恢复后来源丢失 | 当前 app-server 真实持久化/恢复测试 | UserShell 永远完整显示；来源不可信时验收失败 |
| local package | wrapper 继续运行旧构建产物 | 构建前后 hash/mtime 与版本检查 | iTerm2 验收明确运行刚构建的本地包 |

以上每个失败模式都必须有断言或明确 fail-open 处理；不允许出现“静默保持旧摘要/旧正文”的无测试状态。

## 9. 性能与内存验收

### 9.1 设计预算

- 一个 App/ChatWidget 只有一个 catalog；
- catalog 仅存轻量路径和名称，目标在常见数百个 skill 下低于 1 MiB；
- 普通 `ExecCall` 仅增加一个可空 Box 指针；候选才分配 metadata；
- metadata 不持有 output/body；catalog 不持有 cell；
- command 解析只在事件/历史投影时执行一次；render 只做已缓存状态或轻量索引查询；
- 普通 cell 继续使用 `CachedRenderable`；只有含 candidate、确实依赖 catalog 外部状态的 cell 走既有非缓存渲染路径；
- 不启动新进程、runtime、线程或 WebView。

### 9.2 验收

- 单元测试在 isolated Arc fixture 中 drop 初始 catalog owner 与全部候选 cells 后，以 `Weak` 证明 catalog 不被环引用；普通 cell 不持有 catalog；
- 合成 10,000 个普通 command cell，确认普通 `ExecCall` 不分配 candidate，且没有正文副本；
- 通过类型与模块边界保证 renderer 只接收 catalog resolve 产生的 `WorkspaceReadSummary` 和已解析 candidate，不接收 raw command/argv；禁止为此在生产实现增加仅测试可见的 parse counter；
- 在 10,000 个普通 cell + 100 个 candidate cell fixture 上反复 render/scroll 100 次，确认普通 cell 仍命中缓存，candidate cell 可随 catalog generation 改变且无每帧 shell parse；
- iTerm2 中对同一长会话连续打开/关闭 Workspace 30 次并采样 RSS：预热后不能持续单调增长；
- 记录首轮、10 次、30 次 RSS。若第 30 次仍稳定增长，或相对第 10 次增长超过 10 MiB，判定不通过并用 Instruments/Leaks 定位；
- 在 10,000 cell fixture 上记录 Workspace 首开和滚动耗时，修复后不得出现随每帧重新 shell parse 的线性抖动。

这些阈值用于发现明显泄漏，不把 allocator 的正常高水位误判为泄漏；最终结论必须结合对象生命周期和 RSS 趋势。

## 10. 证据边界与已知限制

1. 技能 catalog 尚在 Unrequested/Loading、请求失败、response 不完整、entry 带 errors、锁读取失败或命令 outcome 仍为 Pending 时按红线 fail-open，正文可能暂时/持续可见；不以路径猜测或提前隐藏掩盖该事实。
2. 成功的 skill enable/disable 会让全部缓存 cwd 暂时进入 Unrequested/Loading，因此刷新完成前，其他 skill 的既有摘要也可能短暂恢复成完整正文；这是使旧 ticket 失效、避免误隐藏的安全取舍。
3. 恢复一个“对应 skill 已从本机删除或移动”的旧会话时，当前 catalog 无法权威证明该历史路径曾是 skill，因而完整显示。要跨卸载永久隐藏，必须持久化 UI annotation 或维护 sidecar 历史索引，这会扩大数据和迁移范围，本次不做。
4. 附属/reference 文档始终完整显示，即使位于当前 enabled skill 目录中；这避免把目录后代、符号链接或混合读取输出误当作主技能文档。
5. `UnifiedExecInteraction` 的现有 live 行为不展示 command output；为避免无正文路径变成新的 Workspace 摘要，本次明确不对它 compact，持久化 Interaction 也 fail-open。
6. 当前 app-server 生成的 typed `CommandExecutionSource` 作为来源权威；本次不支持外部伪造或缺少 `source` 的 v2 payload。v2 source 字段与所有 source 枚举同一历史变更引入，UserShell 又早于该 v2 结构存在；当前未发现 app-server 生成 source-less UserShell item 的路径，仍以真实 UserShell 持久化恢复测试锁定运行态，不把源码推导冒充运行态证明。
7. 本方案针对当前 macOS/iTerm2 使用场景；上游 `codex-skills` 的 Windows 路径识别保持不变，但本次人工验收不宣称覆盖 Windows 终端。

若用户要求“即使 catalog 失败、skill 已卸载，也绝不能显示旧正文”，必须先放宽“无持久化 metadata、不得路径猜测”的红线并重新设计，不能偷偷改变取舍。

## 11. 实施顺序与提交边界

1. **TDD RED 检查点（默认不提交）：** 删除错误 parser 红测；增加真实 live、presentation 隔离、原始 outcome、completion-only、actual resume、分页、Unrequested cwd、toggle、dynamic cell cache、layout anchor 和官方三表面 RED 测试，逐项运行并保存失败证据。失败测试不得作为普通 commit；只有必须跨 session 保存时才创建明确的 WIP commit，最终合并前整理掉。
2. **识别提交：** 在私有 `workspace_skill_output` 模块复用 `codex-skills` 访问提取，增加严格 AST 门控、独立 `WorkspaceReadSummary`、outcome 和 `pwd` 输出语义快照；提交时对应 classifier/catalog 单测必须已绿。
3. **catalog 提交：** 接入 Unrequested/Loading/Ready/Failed、generation ticket、按需 cwd、skill toggle 与锁失败 fail-open；跑状态、失败和乱序矩阵。
4. **视图状态提交：** 接入 cell 对 catalog 的单向引用、`has_stable_transcript_height` 非缓存路径、catalog view invalidation 与 anchor 保持；跑 Loading/Ready/Failed/乱序矩阵。
5. **实时修复提交：** 接入 begin/output/completion/orphan/replay 与含执行 cwd/outcome 的 ExecCell Workspace renderer；live RED 矩阵全部转绿后与对应测试一起提交。
6. **历史修复提交：** 接入 Workspace-aware persisted projection，并用真实 resume 路径确认初始历史入口与 UserShell 来源；resume/page RED 矩阵全部转绿后与对应测试一起提交。
7. **回归收口提交：** 恢复本地偏离的官方 annotation 条件，执行第 8.5 节仓库标准命令、审阅 snapshots、更新 Bazel lock、构建 packaged binary，并完成 iTerm2 和内存验收。

每一步只暂存明确路径；不得把旧方案文档、用户改动或其他 session 的文件混入提交。未经用户明确要求，不 push、不改分支、不部署。

## 12. 审查后的当前状态

- 已完成：对照上游主聊天、完整 transcript、官方隐式 skill 识别和本地 Workspace 入口；
- 已完成：确认全局 parser 修改方向错误；
- 已完成：确认实时、completion-only、resume/分页、item cwd、skill catalog 时序五条必要链路；
- 已完成：将方案修订为官方识别复用 + 独立 Workspace presentation，并补足 generation、Unrequested cwd、raw outcome、dynamic cell cache、toggle、视口失效、仓库验证和本地包构建契约；
- 已完成：最新一轮对抗式复审未发现新的 P1 阻断；Ready(empty)、toggle 旧 ticket、summary 延迟解析、fresh package 和 RED 提交边界均已写入可验证契约；
- 未完成：移除错误红测、写正确 RED、生产实现、构建、自动化测试、iTerm2 验收、内存验收，以及任何生产代码的 commit 和 push。

因此当前仍是“方案阶段”，不能向用户声称问题已修复或可以直接使用。
