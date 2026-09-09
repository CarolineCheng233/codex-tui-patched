# 官方聊天展示与固定输入框 Workspace 修订方案

> 状态：方案待实施；用户截图已证明上一版交付未满足预期。本文不代表代码已修复或验收通过。
>
> 执行方式：使用 superpowers:executing-plans 按任务串行执行；未经用户明确授权不启动子代理、不创建或切换分支。

**目标：** 在固定输入框、可滚动历史的 Workspace 中复用官方正常聊天的输出展示规则，修复空格翻页，并保留轮次折叠、编辑快捷键和图片预览。

**架构：** Workspace 负责布局、历史导航和轮次折叠，内容来自官方正常聊天 renderer。完整 transcript 作为用户主动打开的独立详情视图；同一执行数据产生两种展示，不修改执行结果。

**技术栈：** Rust、ratatui、crossterm、现有 HistoryCell/ExecCell、insta、cargo-nextest、tmux。

**需求依据：** 用户 2026-09-09 提供的两张对比截图、空格翻页反馈，以及“看官方 Codex TUI 怎么做，照着做，不要自己发挥”的明确要求。

**替代关系：** 本文替代 `2026-09-08-official-output-parity-skill-workspace-plan.md` 的展示、skill 特判及验收契约。旧文档保留为历史，不再作为实施依据。特别废止“README 等普通读取必须在默认 Workspace 显示完整正文”和“只有通过技能校验才允许摘要”的要求。

## 1. 要解决的问题

### 1.1 默认界面用了完整记录的内容

图 #1 显示 `$ sed ...`、完整路径与 SKILL.md 正文；图 #2 显示 `Explored → Read SKILL.md (... skill), codex-tools.md`。用户需要图 #2 的正常聊天展示，同时保留本地布局功能。

当前 `pager_overlay/scrolling.rs::CellRenderable` 在 Workspace 中调用 `workspace_transcript_hyperlink_lines()`。该接口默认回退到完整 `transcript_hyperlink_lines()`；ExecCell 也仅在自定义 skill 条件满足时例外压缩。这造成默认工作界面持续输出详细记录。

不能仅凭截图断言它来自普通主聊天的 `display_lines()` 或启用了 raw-output 配置。已经确认的是：当前 Workspace 默认接入完整 transcript；这足以解释截图中的格式，后续以同一真实事件重放确认运行路径。

### 1.2 空格被历史翻页拦截

`app_backtrack/workspace_input.rs` 先调用 `handle_workspace_key()`，未处理时才转给 Composer。Workspace 的导航判断复用了完整 pager 的 `page_down` 绑定，其中包含 Space；因此用户打空格时会翻页。Shift+Space、Ctrl+B/F 也存在与输入编辑争用的问题。

### 1.3 上一版验证范围不等于用户体验

先前主要测试了少量 skill 命令形状与正文 sentinel，没有证明整个默认界面与官方正常聊天一致；`cat/固定 sed` 白名单、catalog、哈希、退出码门控形成了额外语义。还存在 `cfg!(test)` 对不存在文件放行的生产/测试差异。继续增加这些门控无法修复界面选错的问题。

先前报告“构建完成”“25 项都是既有失败”等结论也缺乏完整对应证据：版本 `0.0.0` 不证明包包含当前提交；两次失败数量相同不证明失败与本地改动无关。新验收必须纠正这些证据边界。

## 2. 官方基线与事实

本次只读核验固定到本地官方远端 `upstream/main`：

```text
官方基线：7769bccbb2b4e9469a36b12510e73594fa03c5d5
方案编写时本地 HEAD：f61cfb3866cc63aa0c91f4419cadf56856a0c559
官方远端：https://github.com/openai/codex.git
```

这表示已核验的本地官方源码版本，不宣称它是互联网最新版本。实施中不顺带升级上游。

| 官方源码 | 已核验行为 |
| --- | --- |
| `codex-rs/tui/src/exec_cell/render.rs::display_lines` | exploring 类进入摘要 renderer，其余进入普通命令 renderer |
| `exec_cell/model.rs::is_exploring_call` | 非 UserShell，且 actions 非空并全部为 Read/ListFiles/Search 时属于 exploring |
| `exec_cell/render.rs::exploring_display_lines` | 运行时 Exploring，完成后 Explored；连续 Read 文件名合并、去重，显示 Read/List/Search |
| `chatwidget/skills.rs::annotate_skill_reads_in_parsed_cmd` | 对匹配的 SKILL.md 名称追加技能注解；无 catalog 时仍能显示普通 Read 摘要 |
| `exec_cell/render.rs::command_display_lines` | Running/Ran/You ran、状态颜色、有限输出预览与折叠提示；按官方宽度规则折行 |
| `exec_cell/render.rs::transcript_lines` | `$ command`、完整输出、退出状态和耗时；与正常展示是独立接口 |
| `keymap.rs::PagerKeymap` | 完整记录 pager 可使用 Space 翻页；不应将其直接用于同时编辑的 Workspace |

官方对普通命令和 UserShell 有不同预览预算（相关常量分别为 5 和 50），实际屏幕行数还经过折行和截断布局处理。实现复用 renderer，不重新硬编码行数算法。

## 3. 用户可见目标

| 行为 | 默认 Workspace / 正常聊天 | 主动打开的完整详情 |
| --- | --- | --- |
| Read：SKILL.md、参考文档、README、源码 | 官方 Exploring/Explored + Read 摘要 | 原始命令与完整输出 |
| ListFiles/Search | 官方 List/Search 摘要 | 完整记录 |
| 普通命令、curl、构建 | 官方 Running/Ran + 有限输出预览 | 完整输出与状态 |
| UserShell | 官方 You ran 与对应预览 | 完整记录 |
| 失败、缺失退出码、Interaction 等特殊情况 | 完全沿用该基线官方生命周期和 renderer，不额外定义隐藏策略 | 沿用官方详情规则 |
| skill 未加载、禁用、文件变动 | 不阻止官方 Read 摘要；技能注解以官方现有逻辑为准 | 原数据完整保留 |

执行过程保留官方进度摘要，不默认铺开全部执行正文。它不是删除、隐瞒执行结果的安全机制，不要求证明输出字节确实来自某个技能文件。

具体要求：

1. 输入框固定在终端底部；历史浏览不带走输入框。
2. 在旧轮次位置继续输入文字、空格或粘贴，历史视口不被强制拉到底。
3. 保留按用户轮次定位、折叠和展开，以及既定选中轮次标记。
4. 空格始终按输入字符处理；输入框为空也不能翻页。
5. Ctrl+C 在有输入时只清空输入；空输入时沿用已约定的官方中断/退出行为。
6. Ctrl+U、Ctrl+B/F、左右方向键、Home/End 等编辑键归 Composer；不借本次改变官方编辑含义。
7. 历史导航保留 PageUp/PageDown、鼠标滚轮；轮次导航保留既定 Alt+方向键。弹窗和选择器打开时，它们优先处理自身按键。
8. 保留 iTerm2 本地图片预览，折叠、展开、滚动和窗口缩放时不留下错位图像。
9. 新消息、活动尾部、completion-only、orphan、初始 resume、旧页 prepend 均遵守同样展示规则。
10. 完整记录仍可主动查看；切入详情、返回 Workspace 后，输入草稿、附件、折叠状态和历史锚点保持。

## 4. 如何修改

### 4.1 分离内容与布局

```text
同一组执行数据 / 历史 cell
  ├─ 默认 Workspace → 官方 display_hyperlink_lines(width)
  │                   + 固定 Composer、滚动、轮次折叠、图片预览
  └─ 完整详情       → 官方 transcript_hyperlink_lines(width)
```

默认 Workspace 内容明确使用 Rich 正常展示，不从 raw scrollback 配置或详情 renderer 隐式继承全量输出。显式 raw 模式在其已有界面继续生效，不能悄悄改变用户配置。

调整 `HistoryCell::workspace_transcript_hyperlink_lines()` 的默认实现为正常 display 路径，删除 ExecCell 的 skill 特判 override。活动尾部必须调用同样的正常展示；高度测量必须与实际渲染使用同一接口、宽度、缩进和图片保留行。

预期委托关系：

```rust
fn workspace_transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
    self.display_hyperlink_lines(width)
}
```

保留必要的 Workspace 布局钩子，避免为了删一个接口波及所有历史类型。不复制 Read/Ran 的颜色、标题、合并与截断算法。

### 4.2 处理持久化历史，避免只修 live

目前分页 CommandExecution 的 fallback 已压成完整文本，单纯换默认渲染接口仍会显示 `$ command`。必须在交互式历史 adapter 保留 command、actions、source、item cwd、status、exit_code、duration 与 output。

沿用官方 replay 的命令构建与完成规则生成 ExecCell，用于正常 Workspace 展示；完整详情仍保持原有持久化 fallback 文本契约。可让既有 `WorkspaceCommandHistoryCell` 持有正常展示 ExecCell 与原详情行，在自己的 `workspace_transcript_hyperlink_lines` 中委托正常 renderer。不要复制官方 lifecycle 条件，更不要为了完成态伪造 begin/output。

连续 exploring 分组复用 `ExecCell::add_call`；跨普通命令、UserShell、用户/助手消息不能合并。分页边界的连续 Read 要与一次性载入保持一致，不能将交互式 adapter 接入导出或 resume picker 后改变它们现有行为。

初始 resume 已核验通过 `replay_thread_turns → handle_command_execution_completed_now`；仍需真实 App 入口测试，而不是手工向 `transcript_cells` 填入期望 cell。

### 4.3 撤除上一版 skill 展示状态机

定向移除为 skill 正文隐藏新增的：

- `workspace_skill_output.rs` 及其展示专用 candidate/catalog/ticket/fingerprint/outcome。
- ExecCall、RunningCommand、ChatWidget 和 constructor 中仅服务于该状态机的字段与接线。
- 启动、交互刷新、配置开关、历史分页中的展示专用 ticket、按需 cwd 请求和失效通知。
- 为该功能新增、没有其他消费者的 `codex-skills` TUI 直接依赖与专用 shell AST helper。
- 测试中的不存在文件放行、只允许固定 sed 的契约和基于注解字符串的隐藏规则。

官方 skills/list、技能选择器、注解、插件、配置持久化功能继续保留。删除前逐项与固定官方基线及调用者核对；不可整文件覆盖，也不可整体 revert 早期混合提交。parser 兼容测试可保留，官方 parser 语义不变。

撤除后 renderer 不新增文件读取、哈希、RPC，也不把输出复制进另一套 presentation。候选专属“高度不稳定”处理随候选移除；其他动态 cell 的缓存规则仍保留。

### 4.4 输入与导航规则

Workspace 不再把整组 pager 的 page_up/page_down 别名作为导航入口。明确匹配 PageUp/PageDown 和已有轮次快捷键，其余交给 Composer；可打印字符（含 Space/Shift+Space）、输入法提交、粘贴不能被 pager 抢占。

事件顺序：活动弹窗/选择器 → Workspace 专属导航 → Composer。只读详情保留官方 pager 空格翻页。Ctrl+B/F 不再作为 Workspace 翻页键，避免破坏编辑。

### 4.5 完整详情入口

沿用官方 `open_transcript`（默认 Ctrl+T）作为主动进入完整详情的操作。在 Workspace 中按此键切换到 Viewer；详情中关闭操作返回原 Workspace。保留同一 Workspace 实例或完整状态快照，不将它丢弃后以底部位置重建。

状态至少包括：视口顶部 cell/相对行、选中 turn、折叠集合、跟随底部状态；Composer 继续归现有 ChatWidget 所有，不复制或清空草稿。详情期间的新消息按既有顺序追加，返回时保持原视口或原先的跟随底部状态。

## 5. 文件范围与职责

所有路径均相对仓库；只在所列职责确有需要时修改。

| 文件 | 职责 |
| --- | --- |
| `codex-rs/tui/src/history_cell/mod.rs` | Workspace 默认 display 委托及一致高度测量 |
| `codex-rs/tui/src/pager_overlay/scrolling.rs`、`pager_overlay.rs` | 正常内容渲染、缓存、导航与锚点 |
| `codex-rs/tui/src/app_backtrack.rs`、`app_backtrack/workspace_input.rs`、`app_backtrack/legacy_input.rs` | 活动尾部、Composer 路由、详情往返 |
| `codex-rs/tui/src/exec_cell/model.rs`、`render.rs`、`mod.rs` | 移除 skill presentation，恢复官方正常/详情边界 |
| `codex-rs/tui/src/thread_transcript.rs`、`app/history_pagination.rs` | 持久化命令正常展示与详情保留、跨页一致性 |
| `codex-rs/tui/src/chatwidget.rs`、`chatwidget/{constructor,exec_state,command_lifecycle,skills,protocol_requests}.rs` | 定向撤除展示状态；保留官方技能功能 |
| `codex-rs/tui/src/app/{background_requests,thread_routing,event_dispatch}.rs`、`app_event.rs` | 撤除展示专属 RPC/ticket/失效接线 |
| `codex-rs/tui/src/workspace_skill_output.rs`、`workspace_skill_output_tests.rs`、`lib.rs` | 删除废弃模块及注册；测试迁移到实际显示/输入边界 |
| `codex-rs/shell-command/src/bash.rs`、TUI Cargo.toml、Cargo.lock、MODULE.bazel.lock | 仅删除无消费者的本次专用 helper/依赖并刷新锁 |
| `codex-rs/tui/src/{thread_transcript_tests,pager_overlay_transcript_workspace_tests}.rs`、`chatwidget/tests/{exec_flow,history_replay}.rs`、`app/tests/transcript_composer.rs` | 真实投影、App 按键与三表面回归 |

需要额外复用 helper 时放在职责所属小模块，不在 ChatWidget/App 中扩散大段逻辑。单个非机械变更单元以 500 行以内为目标；超限先按可独立验证行为分段，不用硬性总代码行数牺牲需求。

## 6. 实施任务与提交边界

### 任务 A：建立正确的对比基线

- [ ] 保存固定上游的 renderer、parser、keymap 与官方输出快照证据，记录 SHA。
- [ ] 用真实 parser/actions 构造同一事件 fixture，覆盖 Read、List、Search、普通命令和 UserShell；原始正文含唯一 sentinel。
- [ ] 添加 `workspace_parity_` 测试，证明当前 Workspace 的完整输出不等于官方正常展示；添加 `workspace_input_` 测试，经 App 入口键入 `hello world`，证明空格被消费。RED 必须是可执行断言失败，不把编译错误/0 tests 当复现。
- [ ] 正常展示用固定上游 golden；详情用当前原始数据契约。不要从修改后的 renderer 自动生成并无审查接受全部 golden。

### 任务 B：修正默认内容并撤除 skill 特判

- [ ] 按 4.1 节接入正常 display，活动尾部同步；移除 ExecCell skill override。
- [ ] 按 4.3 节收拢废弃状态，逐文件核对官方功能未被移除。
- [ ] 验证 SKILL.md、参考文档、README、源码均按官方摘要，catalog 未到达也不回退全量正文；普通命令仍是官方有限预览。
- [ ] 运行对应 RED 与 ExecCell 正常/详情回归，审查快照，提交 `fix: 让 Workspace 复用官方聊天展示`。

### 任务 C：修正历史投影与详情往返

- [ ] 按 4.2 节重建历史命令正常展示；复用原详情格式，校验 item cwd 和 source 保留。
- [ ] 按 4.5 节接入 Ctrl+T 正常/详情往返并保持草稿、附件、折叠和锚点。
- [ ] 通过真实 resume App 入口和旧页 prepend 验证相同事件的正常展示一致，详情仍含 sentinel、状态与耗时（若原记录具有该字段）。
- [ ] 覆盖页边界连续 Read、浏览期间新消息和窗口变窄场景，提交 `fix: 统一恢复历史和详情视图的展示边界`。

### 任务 D：修正输入、弹窗和导航

- [ ] 按 4.4 节限定 Workspace 导航键，保留官方 Viewer 的键位。
- [ ] App 入口输入测试同时断言：草稿精确等于预期文本，历史锚点不变；不只断言 handler 的返回值。
- [ ] 验证空/非空草稿、Shift+Space、输入法/粘贴、Ctrl+B/F/U、Ctrl+C、弹窗导航、鼠标滚轮和 PageUp/PageDown。
- [ ] 提交 `fix: 让 Workspace 空格与编辑键进入输入框`。

### 任务 E：包与运行态验收

- [ ] 完成第 7 节所有自动化、快照与真实终端验收，记录通过/失败/未验证。
- [ ] 更新实施记录及遗留问题；必要的纯机械清理独立提交。未经明确要求不 push、不部署。

## 7. 如何验收

### 7.1 测试矩阵

| 检查项 | 必须成立的断言 |
| --- | --- |
| 截图同形独立 `sed -n '1,240p' SKILL.md`，含 shell wrapper | 正常 Workspace 与官方 display 等价；不会要求必须有 pwd 前缀 |
| SKILL.md → reference → README 连续读取 | 按官方分组/去重展示 Read；正常视图不铺开文件正文，详情完整 |
| `pwd && sed ...`、混合命令、awk、额外 operand | 官方 parser 判什么就展示什么；Unknown 保持 Ran/预览，不强行改成 Read |
| curl 大输出、编译成功/失败、UserShell | 标题、颜色、有限预览、折叠提示与固定上游一致 |
| Pending、Completed+None、非零退出码、Declined、UnifiedExecInteraction | 逐个与官方行为比较，不复用旧 skill 成功门控期望 |
| begin/end、orphan、completion-only、resume、分页 | 同一事件语义不因入口变化而变成完整默认输出 |
| `hello world`、连续空格、首字符空格、Shift+Space | 草稿保留准确空格，滚动位置不变 |
| Ctrl+B/F/U、左右/Home/End、Ctrl+C | Composer 行为与既定官方编辑规则一致 |
| 弹窗、问题面板、技能选择器 | 导航和选择不会被底层 Workspace 抢走 |
| Ctrl+T 详情往返 | 详情完整；返回时草稿、附件、折叠集合和锚点相同 |
| 80/120/窄屏宽度，长文件名、中文、超链接 | 行内容、样式、换行与高度测量匹配；无错位、缺行 |
| 浏览旧轮次时产生新输出 | 不跳底、不改变 Composer；原先跟随底部则继续跟随 |

核心等价断言是同 fixture、同 width 的完整 `Line/Span`（含样式）及 hyperlink metadata 对比；sentinel 的 contains 检查只作为补充。不只验证“出现 Read”或“进程未崩溃”。

### 7.2 自动化命令与失败处理

在仓库根，给当前命令补工具链 PATH，不修改持久配置：

```sh
export PATH="/Users/chy/.cargo/bin:/opt/homebrew/opt/rustup/bin:$PATH"
cargo nextest list --manifest-path "codex-rs/Cargo.toml" -p codex-tui -E 'test(~workspace_parity_) | test(~workspace_input_)'
just test -p codex-tui -E 'test(~workspace_parity_) | test(~workspace_input_)'
just test -p codex-tui
# 仅 shell-command 实际变化时：
just test -p codex-shell-command
# 仅依赖实际变化时：
just bazel-lock-update
just bazel-lock-check
```

快照在 `codex-rs` 下执行 `cargo insta pending-snapshots`（本机该版本不接受 `-p`），逐个审阅，仅接受本次相关快照。既有两个 custom_terminal `.snap.new` 与未跟踪旧方案不纳入提交。

测试完成后按仓库顺序执行 `just fix -p codex-tui`，必要时 `just fix -p codex-shell-command`，最后 `just fmt`；按 AGENTS.md 不在 fix/fmt 后机械重复跑测试，但检查最终差异。如自动修复引入实质行为变化，重新进入该行为的修复验证周期。

所有长命令保留工具 session ID 或唯一日志与退出码。观察超时继续等待同一进程，禁止凭一次空 ps 输出认定完成、重复启动构建或杀掉 Rust 进程。

全量失败不能以“专项通过”消除。记录失败名称、错误、测试 SHA 和环境；在修改前基线或保留的独立构建产物上复验才能归因为基线问题。无法归因则标记“未解决”，不能写“全部无关”。Bazel 的 Python 版本问题可使用已安装 Python 3.13 调用相同包装脚本检查，但明确标准命令与替代检查各自结果。

### 7.3 新包真实性

- [ ] 记录构建开始时间、Git SHA、系统 codex 路径/版本/哈希和 wrapper 实际目标。
- [ ] 运行 `./scripts/build-patched-tui.sh`，等待退出码 0 与 `Built local Codex TUI package ...` 最终输出；版本字符串不作为新包证明。
- [ ] 校验 package 内 codex 与本轮 release/codex 的 SHA-256 一致，包生成时间不早于构建开始；host 可执行。
- [ ] 新开进程运行 wrapper，确认进程执行路径；不把此前已运行的老进程算作最新验收。
- [ ] 系统 codex 路径、版本、哈希不变；不重启用户其它会话。

### 7.4 真实终端验收

准备真实临时 skill（符合实际 loader 配置），正文放唯一 sentinel；同时准备 README 和普通命令输出。使用正常授权登录或本地 fixture server，不能把等待登录首屏当作功能验收。

- [ ] 官方基线与本地包执行同一事件场景，记录两者截图或 PTY 帧，不以两次模型随机选择不同命令作唯一对照。
- [ ] 正常默认界面复现图 #2 的 Read 摘要；打开详情能看到完整 sentinel，返回仍保持草稿。
- [ ] 在旧轮次键入 `hello world` 与中文含空格句子，确认空格输入、视口不跳底。
- [ ] 完成轮次折叠/展开、Ctrl+C/U、PageUp/PageDown、滚轮和窗口缩放检查。
- [ ] iTerm2 验证本地图片预览和退出后的终端恢复。

若平台拒绝 Computer Use 访问 iTerm2，停止该 GUI 操作，不用其它注入技术绕过；继续进行程序级 PTY 测试并提供用户手动操作清单。PTY 可证明渲染/输入链路，不能替代 iTerm2 图片显示证据。用户截图和反馈是有效运行态证据，按矩阵记录。

### 7.5 性能与内存

- [ ] 同机器、同 fixture 对修改前后 10,000 个普通 cell + 100 个读取 cell 的首次展示、滚动、折叠、详情往返采样，记录原始耗时和峰值/RSS。
- [ ] 单次操作中位耗时若回退超过 20% 且绝对增加超过 10ms，先定位再交付；不以全量重建 renderer 的方式处理每次按键。
- [ ] 连续详情/Workspace 往返 30 次后释放 overlay，引用不得遗留；第 10→30 次 RSS 持续增长超过 10MiB 时排查所有权，不能仅用 allocator 高水位判断泄漏。
- [ ] 不因渲染而解析 shell、读文件、计算哈希或发 RPC；不为普通命令新增输出副本。

## 8. 红线

1. **官方语义：** 读取摘要按官方分类，不增加 skill/path/哈希/固定参数的隐藏策略；不擅自改成“所有过程消失”或“所有普通输出完整展开”。
2. **执行安全：** 不改 parser 分类、执行器、审批、sandbox、协议、模型上下文和真实命令结果。Unknown 不因视图需要改成 Read。
3. **数据完整：** 不删除、截断或改写会话持久化数据；摘要与详情共享事实来源，完整详情必须可达。
4. **输入可用：** 默认 Workspace 的空格和编辑键不得被 pager 消耗；有弹窗时不能把键误发给底层输入框。
5. **已需功能：** 不牺牲固定 Composer、历史浏览时输入、轮次折叠、Ctrl+C/U 和图片预览来恢复官方展示。
6. **范围：** 只撤除此前展示特判，不整体重置仓库，不覆盖用户变更，不顺带升级依赖或上游。不得改系统 codex、shell 配置或 iTerm2 设置。
7. **验证一致：** 不使用 cfg(test) 放宽生产门控，不手写与真实事件不同的 action 来让测试通过，不把 0 tests、启动首屏或旧包版本作为验收成功。
8. **交付诚实：** 未取得官方对照、真实空格输入、resume/分页或 iTerm2 证据时逐项列明；不能再次凭专项绿色宣称全部完成。

## 9. 回滚与交付物

每个独立实现任务通过相关验证后，只暂存明确文件并提交，提交说明包含原因和验证。回滚采用对应任务的可逆提交；先核对混合内容，不直接重置到上游，也不重建全部本地功能。

实施交付必须包含：

- [ ] 本方案的逐项完成状态与实施提交 SHA。
- [ ] 官方正常展示、默认 Workspace、完整详情的对比快照。
- [ ] 空格/编辑/导航 App 入口断言，以及用户可见的实际输入证据。
- [ ] 初始 resume、分页、活动尾部和详情往返证据。
- [ ] 新包构建退出码、哈希与实际进程路径。
- [ ] 性能采样、全量失败归因、仍未覆盖的运行态项。

当前仅完成本方案编写和只读源码核验。未执行上述实施任务；不以本文更新改变既有代码或运行中的用户会话。
