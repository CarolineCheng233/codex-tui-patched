# 官方聊天展示与固定输入框 Workspace 修订方案

> 状态：实施中。默认 Workspace、持久化 CommandExecution/WebSearch、详情往返/锚点恢复和输入路由已完成代码与专项自动化；真实 iTerm2 GUI、PTY 终端生命周期、完整跨入口历史矩阵和性能采样尚未完成，因此本文不代表全部验收通过。
>
> 执行方式：使用 superpowers:executing-plans 按任务串行执行；未经用户明确授权不启动子代理、不创建或切换分支。
>
> 2026-09-09 修订：纳入四项对抗式审查意见。实施必须按测试驱动开发（TDD）的 RED → GREEN → REFACTOR 顺序推进；本文中的测试均为待实施要求，不代表已经编写或执行。

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
| WebSearch | 本次包含：按对应官方事件展示 Searching/Searched，恢复与分页使用官方完成态展示 | 使用该类型官方支持的详情，不承诺协议未提供的搜索正文 |

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
9. CommandExecution 与 WebSearch 的新消息、活动尾部、completion-only、orphan、初始 resume、旧页 prepend 均采用对应官方入口的正常展示；不要求不同事件序列具有相同分组。其他类型的保留范围见 4.6。
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

**先锁定事件语义，再决定分组：** 固定官方基线的 `handle_command_execution_completed_now` 对没有匹配 begin、且当前没有其它运行中 Exec/MCP cell 的新 call 进入 `NewCell`，刷新旧 cell 后创建新 cell；若其它执行仍活动，则走官方 orphan 分支。连续完成事件不等同于 live begin/end 序列，不应为历史强制调用 `add_call`。

| 入口 | 构建与分组契约 | 必须对照的输入 |
| --- | --- | --- |
| live begin/end | 沿用官方 active cell、`add_call`、flush 规则 | 相同顺序的 begin、delta、end 事件 |
| completion-only / orphan | 沿用官方完成路由；不补造 begin，不强并相邻 Read | 包含不同 call ID、活动命令与孤立完成事件的序列 |
| 初始 resume | 保留 `replay_thread_turns → handle_command_execution_completed_now` 路径 | 相同持久化 turns 经官方 replay 后的结果 |
| 历史分页 | 按持久化完成事件语义创建正常 cell；不跨页额外合并 | 相同历史一次载入与拆页载入；不是拿 live golden 强求相等 |

页边界对照只要求**相同持久化输入**的 cell 顺序、分组、内容、turn 归属一致。按 item ID 去重，保留 turn 边界；普通命令、UserShell、用户/助手消息和不同轮次均不得因分页被合并。若页接缝连接的是已存在的 live 分组，保留其既有成员，不从邻接关系推断应当合并。测试必须走真实 resume、分页响应入口，不手工填入期望 ExecCell。

**单份输出所有权：** 删除“正常 ExecCell + 常驻原详情行”方案。交互式 adapter 内的 ExecCell 持有一份命令输出；wrapper 只保存它不能表达的原始可选状态等小量元数据。普通预览与详情从这份数据借用读取，详情行仅在渲染调用中临时生成，不常驻第二份正文。必要的借用访问只开放到 TUI crate 内，不引入新的 presentation 状态机；既有会话存储不借本次重构。

交互式历史详情保留 `$ command`、真实 status/可选 exit_code 和完整输出，原记录有 duration 时补充耗时；没有该字段就不显示，不伪造为 0。正常展示对缺失退出码沿用官方规则，详情不得把推导值当作记录值。导出及 resume picker 继续走原 fallback，格式与字段保持原样；不能为交互式详情补耗时而改变它们。完整输出不丢行、不做存储截断，原有展示层缩进规则不等于修改持久化字节。

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

事件顺序：活动弹窗/选择器 → Workspace 专属导航 → Composer。只读详情保留官方 pager 空格翻页。Ctrl+B/F 不再作为 Workspace 翻页键，避免破坏编辑。是否请求更早历史也受相同优先级约束，不能在弹窗处理按键前触发底层分页。

### 4.5 完整详情入口

沿用官方 `open_transcript`（默认 Ctrl+T）作为主动进入完整详情的操作。在 Workspace 中按此键进入 Viewer；沿用 Viewer 的关闭键返回 Workspace，不额外改写 Esc 的官方 backtrack 含义。

**采用单一活动 overlay 的模式切换，不保存第二个脱离更新的 Workspace 实例。** 同一 overlay 的 cells 接收分页 prepend、消息追加与活动尾部更新；进入详情只保存 Workspace 的视图状态，切换内容 renderer 并使布局缓存失效。返回时从最新内容恢复视图，不重新按“默认底部”初始化，不创建第二套历史订阅或输出副本。

保存状态包含 thread ID、视口锚定 cell 身份与相对行、选中 turn、折叠集合和跟随底部状态。锚点不能只保存数组下标；prepend 后通过仍存在的 cell 身份恢复，数据重投影时使用既有 item/turn ID 映射。窗口宽度变化时按新布局夹紧相对行，保持锚定内容可见。Composer 和附件继续归现有 ChatWidget 所有，不复制或清空。

详情期间旧页追加到同一历史集合；返回可继续浏览这些旧页且不重复请求已载入页。非跟随模式保持原锚点，跟随模式显示最新底部；Viewer 自己的滚动不覆盖保存的 Workspace 状态。历史被回退而锚点删除时，定位同一 turn 的有效邻近内容；该 turn 已删除则定位最近仍存在的前序 turn，无前序时使用剩余历史起点。明确这是内容删除后的恢复规则，不用于正常 prepend。

| 转换 | 终端与状态契约 |
| --- | --- |
| Workspace → Viewer | 保持备用屏幕；清除旧布局的图片并按 Viewer 重绘；输入由 Viewer 处理，保留 Workspace 草稿 |
| Viewer → Workspace | 不调用完整 `close_transcript_overlay`；保留备用屏幕，恢复 Workspace 鼠标捕获、图片和 Composer，恢复保存的视图状态 |
| 真正退出 overlay / 应用 | 才调用完整清理：关闭捕获、清除图片、退出备用屏幕、处理 deferred history |
| 切换 thread / fork / 新会话 | 清除旧 thread 的恢复状态，走已有会话初始化；不得恢复旧会话内容或草稿到新会话 |

当前 `close_transcript_overlay` 会执行全量终端清理，不能直接复用为详情返回。修改仅限区分上述模式转换与最终关闭；不重写全局终端生命周期。

### 4.6 非命令事件范围

切换 `HistoryCell` 默认委托不等于所有持久化类型自动获得官方 renderer。当前 `fallback_transcript_cell` 将 WebSearch/MCP/FileChange 等转成普通文本，必须逐类说明范围。

| 类型 | 本次实施范围 | 验收边界 |
| --- | --- | --- |
| CommandExecution | live、resume、分页正常展示及主动详情均按 4.2 实施 | 对应入口官方 golden + 数据完整性 |
| WebSearch | 截图包含此行为；交互式 adapter 复用官方 WebSearch cell 构建，保留协议可用字段；live 正常展示复用现有官方路径 | 运行中与完成态分开对照；resume、分页不再显示自定义 `web search:` fallback；导出/picker 不变 |
| MCP、DynamicTool、CollabAgent、FileChange | live 已有 cell 随默认 display 委托使用正常展示；持久化 fallback 本轮保持现状，不扩写这些类型的 replay | 增加原有输出回归保护；不得宣称这些类型已完成跨入口官方一致性 |
| UserMessage、AgentMessage、Plan、Reasoning | 保留现有构建、可见性与布局语义 | 正文、轮次边界、折叠、超链接和可见性不回归 |
| ImageView、ImageGeneration、Hook、Review、Compaction、其余现有类型 | 保留既有类型处理与持久化 fallback；不新增协议字段或工具功能 | 已有图片能力必须不回归；未支持的历史图片能力不包装成本次已实现 |

验收结论必须限定为“CommandExecution 与 WebSearch 对应入口官方展示对齐，其余类型保留上述现状”，不能再写“全部事件完全一致”。

## 5. 文件范围与职责

所有路径均相对仓库；只在所列职责确有需要时修改。

| 文件 | 职责 |
| --- | --- |
| `codex-rs/tui/src/history_cell/mod.rs` | Workspace 默认 display 委托及一致高度测量 |
| `codex-rs/tui/src/pager_overlay/scrolling.rs`、`pager_overlay.rs` | 正常内容渲染、缓存、导航与锚点 |
| `codex-rs/tui/src/app_backtrack.rs`、`app_backtrack/workspace_input.rs`、`app_backtrack/legacy_input.rs` | 活动尾部、Composer 路由、详情往返 |
| `codex-rs/tui/src/exec_cell/model.rs`、`render.rs`、`mod.rs` | 移除 skill presentation，恢复官方正常/详情边界 |
| `codex-rs/tui/src/thread_transcript.rs`、`app/history_pagination.rs` | 持久化命令与 WebSearch 投影、单份输出、详情与跨页一致性 |
| `codex-rs/tui/src/chatwidget.rs`、`chatwidget/{constructor,exec_state,command_lifecycle,skills,protocol_requests}.rs` | 定向撤除展示状态；保留官方技能功能 |
| `codex-rs/tui/src/app/{background_requests,thread_routing,event_dispatch}.rs`、`app_event.rs` | 撤除展示专属 RPC/ticket/失效接线；会话切换清除详情恢复状态 |
| `codex-rs/tui/src/workspace_skill_output.rs`、`workspace_skill_output_tests.rs`、`lib.rs` | 删除废弃模块及注册；测试迁移到实际显示/输入边界 |
| `codex-rs/shell-command/src/bash.rs`、TUI Cargo.toml、Cargo.lock、MODULE.bazel.lock | 仅删除无消费者的本次专用 helper/依赖并刷新锁 |
| `codex-rs/tui/src/{thread_transcript_tests,pager_overlay_transcript_workspace_tests}.rs`、`chatwidget/tests/{exec_flow,history_replay}.rs`、`app/tests/transcript_composer.rs` | 真实投影、App 按键与三表面回归 |

需要额外复用 helper 时放在职责所属小模块，不在 ChatWidget/App 中扩散大段逻辑。单个非机械变更单元以 500 行以内为目标；超限先按可独立验证行为分段，不用硬性总代码行数牺牲需求。

## 6. 实施任务与提交边界

### 6.1 TDD 执行规则

每个行为单独完成以下闭环，不采用“先改完 B/C/D，最后补测试”：

1. **RED：** 先添加一个通过真实入口观察行为的测试。在未修复实现上执行，记录可执行断言失败、actual/expected、命令、退出码和基线 SHA。编译失败、认证失败、0 tests、只生成未审查快照均不算 RED。
2. **GREEN：** 只修改让该断言成立所需的代码；重跑相同测试及其相关回归。不能降低断言、修改官方 golden 或加测试专用生产分支来变绿。
3. **REFACTOR：** 绿色后才删除本目标造成的重复或废弃状态，重跑相关行为测试。不要测试“某段源码/符号不存在”，改为保护其消费者行为。
4. **持久化：** 一个可独立回滚的行为及测试完成后提交，再进入下一项。RED 证据保存在任务记录中；不把已知失败的测试单独作为完成态提交。

已经满足的契约只添加 characterization/regression 测试，记录“基线已通过”，不人为制造红灯，不声称它是修复证据。新旧行为由不同测试承担，例如“旧分页正文展开”应当 RED，“现有导出格式保持”可以一直 GREEN。

预期值来自固定上游独立生成并人工核对的 golden 或协议 fixture 的字面量；不能让被测本地 renderer 同时计算 actual 和 expected。对动态耗时使用固定事件 duration，对路径使用测试夹具路径；不删除样式、链接或关键字段以掩盖差异。使用真实 parser 产生 actions，实际文件读取场景创建真实临时文件；只替代外部网络/模型响应，不 mock App 路由、历史投影和 renderer。

### 任务 A：准备官方与本地基线（随第一个完整行为提交）

**文件：** 既有 `chatwidget/tests/{exec_flow,history_replay}.rs`、`thread_transcript_tests.rs`、`app/tests/transcript_composer.rs` 及其快照。只增加完成本次行为测试需要的测试夹具，不建立通用新测试框架。

- [ ] 保存官方 SHA、renderer/lifecycle/keymap 证据；官方对照在独立临时源码目录中验证，不切换用户分支，不依赖变化中的 upstream/main。
- [ ] 分开准备 live、completion-only、resume 的 golden；每份记录输入事件、宽度、路径和时间规范化规则。
- [ ] 先运行现有相关回归，保存通过/失败清单；后续不得用失败数量相同代替归因。
- [ ] 按下列 B–F 逐个补失败测试；测试名称统一 `workspace_parity_` 或 `workspace_input_`，第 7.2 节先枚举后执行，核对非零发现数量。

### 任务 B：默认内容与 skill 特判（TDD）

**修改文件：** `history_cell/mod.rs`、`exec_cell/{mod,model,render}.rs`、`pager_overlay/scrolling.rs`，以及 4.3 列出的旧状态接线。**测试文件：** `chatwidget/tests/exec_flow.rs`、`pager_overlay_transcript_workspace_tests.rs`。
**输入/输出契约：** 输入真实命令事件；输出 Workspace 正常行、详情行及对应测量高度。复用现有 `workspace_transcript_hyperlink_lines(width)`，不增加平行 renderer。

- [ ] RED：添加 `workspace_parity_live_read_default_display`：独立 sed 读取真实 SKILL.md/README，正文含 sentinel；Workspace 全量 Line/Span/链接等于官方正常 golden，详情保留原正文。当前预期因正文展开或旧 skill 门控失败。
- [ ] 在修改共用默认委托前，同样为 `workspace_parity_live_command_preview` 建立 RED：输入包含 100 行输出的普通命令及 UserShell，逐个对照对应官方 golden，当前预期因完整输出替代有限预览而失败。
- [ ] GREEN：按 4.1 改默认委托及活动尾部，移除 ExecCell 特判；运行该测试直到通过。
- [ ] GREEN：重跑 Read 与普通命令/UserShell 两组 RED；只修默认展示，不改官方截断算法。
- [ ] REFACTOR：在测试绿色保护下按 4.3 删除旧状态机，保护技能选择/注解/配置与原详情；80/120/窄屏快照及测量回归通过后提交 `fix: 让 Workspace 复用官方聊天展示`。

### 任务 C：命令历史、分组与输出所有权（TDD）

**修改文件：** `thread_transcript.rs`、`app/history_pagination.rs`，仅确有必要时增加 ExecCell 的 crate 内借用访问。**测试文件：** `thread_transcript_tests.rs`、`chatwidget/tests/history_replay.rs`、`app/tests/transcript_composer.rs`。
**输入/输出契约：** 输入带 item/turn ID、cwd、actions、source、可选状态与输出的持久化 items；输出按官方完成事件构建的正常 cell，详情按需读取同一输出。导出/picker 不接入交互式投影。

- [ ] RED：`workspace_parity_persisted_read_default_display` 经真实历史响应载入两条完成 Read；对照官方 replay golden，当前应因 `$ command`/正文出现在默认页而失败。
- [ ] GREEN：按 4.2 采用单份输出的命令投影，不保存正常 ExecCell 与完整详情行两份正文；运行上项直到通过。
- [ ] 保护分组：`workspace_parity_persisted_page_partition` 对同一 turn 的 Read A、Read B、普通命令、Read C，以及下一 turn 的 Read D，比较一次载入与逐个接缝拆页载入；比较 cell 顺序/分组/turn 归属/完整显示。两边也各自对照官方 replay golden，不能只让两份同样错误的本地结果相等。
- [ ] 保护生命周期：`workspace_parity_completion_only_not_live_group`、`workspace_parity_orphan_preserves_active_call` 分别对照官方完成事件、orphan golden；已有通过项标为基线保护，不强求 RED，不增加跨页 `add_call`。
- [ ] RED/GREEN：`workspace_parity_persisted_detail_duration` 使用 duration=1250ms 与 None 两个记录，分别断言有真实耗时／无伪造耗时；完整输出、status、可选 exit_code 同时保留。增加 `workspace_parity_export_picker_unchanged` 保护原 fallback，预期可基线通过。
- [ ] GREEN 后进行第 7.5 节大输出存储检查；测试通过后提交 `fix: 对齐命令历史展示并保留单份输出`。

### 任务 D：截图中的 WebSearch 与范围保护（TDD）

**修改文件：** `thread_transcript.rs` 中交互式投影，复用官方已有 WebSearch cell，不改协议/导出入口。**测试文件：** `thread_transcript_tests.rs`、`chatwidget/tests/history_replay.rs`、`app/tests/transcript_composer.rs`。
**输入/输出契约：** 输入官方支持的 WebSearch 事件及持久化 item；正常行使用对应官方 renderer，详情仅展示现有记录支持的信息。

- [ ] RED：`workspace_parity_websearch_persisted_display` 将同一搜索通过 resume 与旧页响应载入，对照固定官方完成态 golden；当前分页预期因自定义 `web search:` 行失败。
- [ ] GREEN：仅交互式 WebSearch adapter 构造官方 cell，删除该入口的普通文本替代，不改其它 fallback。
- [ ] 回归：`workspace_parity_websearch_live_display` 以独立运行中/完成事件对照官方；`workspace_parity_other_fallback_preserved` 按 4.6 逐类保护保留范围。已满足的断言记为基线保护。
- [ ] 审阅包含 Read、WebSearch、助手最终回答的组合快照后提交 `fix: 对齐 Workspace 搜索历史展示`。

### 任务 E：详情往返、历史同步和终端状态（TDD）

**修改文件：** `pager_overlay.rs`、`pager_overlay/scrolling.rs`、`app_backtrack.rs`、相关输入文件及会话切换接线。**测试文件：** `app/tests/transcript_composer.rs`、`pager_overlay_transcript_workspace_tests.rs`。
**输入/输出契约：** 通过 `App::handle_tui_event` 切换模式、真实分页/追加入口更新同一 overlay；输出包含当前模式、可见历史、Composer/附件、折叠与锚点。最终关闭和模式返回分别处理。

- [ ] RED：`workspace_parity_details_roundtrip` 在旧 turn 停留，输入草稿、附图并折叠一个 turn；按 Ctrl+T 后通过 Viewer 关闭键返回，断言正常 renderer 恢复且保存状态保持。当前应因未实现往返契约失败。
- [ ] GREEN：按 4.5 增加同一 overlay 的模式切换和小量视图状态保存，不复制完整 cell 集合或输出。
- [ ] RED/GREEN：依次补 `workspace_parity_details_prepend_sync`、`workspace_parity_details_append_anchor`、`workspace_parity_details_follow_bottom`；详情期间分别加载旧页、追加新消息，返回检查内容一次出现、锚点/跟随行为和下一次分页 cursor。状态同步若已通过，只记回归证据。
- [ ] RED/GREEN：`workspace_parity_details_resize_restore` 与 `workspace_parity_details_thread_change` 覆盖变窄、切 thread/fork、锚点删除；逐个按 4.5 的恢复规则断言，不用“overlay 还在”代替可见内容验证。
- [ ] 终端回归 `workspace_parity_details_terminal_lifecycle`：PTY 捕获往返及最终退出，往返不得包含退出备用屏幕动作；返回恢复输入/鼠标捕获，最终退出才完整清理。图片另按 7.4 取得 iTerm2 证据，不用 mock 图片存在冒充屏幕验证。
- [ ] 相关测试绿色、审阅窄屏快照后提交 `fix: 保持详情往返的历史与终端状态`。

### 任务 F：空格、编辑和弹窗路由（TDD）

**修改文件：** `app_backtrack/workspace_input.rs`、`pager_overlay/scrolling.rs` 中 Workspace 专属导航与分页判断；不改 Composer 官方编辑逻辑。**测试文件：** `app/tests/transcript_composer.rs`、`pager_overlay_transcript_workspace_tests.rs`。

- [ ] RED：先添加下方 `workspace_input_space_reaches_composer`，再补 `workspace_input_typing_preserves_anchor` 在真正可滚动的旧 turn 输入 `hello world`，同时断言准确草稿及锚点不变。
- [ ] GREEN：按 4.4 限定导航键；用原 RED 命令确认空格进入 Composer，而不是只检查 handler 返回 false。
- [ ] RED/GREEN：`workspace_input_popup_owns_navigation` 在弹窗打开时按 PageUp/Down、Space；断言弹窗自身行为正确、底层草稿/锚点不变且未请求旧页。使用实际请求通道/fixture server 捕获请求，不 mock 被测路由。
- [ ] 回归 `workspace_input_edit_navigation_matrix`：首字符空格、连续空格、Shift+Space、中文粘贴、Ctrl+B/F/U、左右/Home/End、Ctrl+C 分项断言；另测 Workspace PageUp/Down/滚轮可用、Viewer 空格仍翻页。输入法组合态用实际终端补验。
- [ ] 相关测试绿色后提交 `fix: 让 Workspace 空格与编辑键进入输入框`。

首个 RED 可直接放入现有 `app/tests/transcript_composer.rs`，复用已存在的真实 App 测试设施（本段是待写测试代码，不是已运行结果）：

```rust
#[tokio::test]
async fn workspace_input_space_reaches_composer() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget.apply_external_edit("hello".into());
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char(' ')).await?;
    assert_eq!(app.chat_widget.composer_text_with_pending(), "hello ");
    Ok(())
}
```

其余测试按上面具名用例中的输入、真实入口与断言逐项实现；复用同目录现有 fixture。不要为测试新增生产专用 getter、清理接口或改变执行结果。该空格单测只证明字符送达，不替代长历史视口与终端验收。

### 任务 G：包与运行态验收

- [ ] 完成第 7 节自动化、快照与真实终端验收，逐项记录通过/失败/未验证。
- [ ] 更新实施记录及遗留问题；未经明确要求不 push、不部署、不扩大到其它历史类型。

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
| 完成态 Read 跨页 | 一次载入与每个接缝拆页载入均对照官方 replay；不拿 live 分组作历史预期 |
| WebSearch 运行中/完成态/resume/分页 | 分别匹配对应官方正常展示；分页不能残留自定义 fallback |
| 其余持久化类型 | 按 4.6 保留范围做回归，不冒充全类型官方对齐 |
| 历史详情有/无 duration、有/无 exit_code | 原值保留、缺失不伪造；导出/picker 不变 |
| `hello world`、连续空格、首字符空格、Shift+Space | 草稿保留准确空格，滚动位置不变 |
| Ctrl+B/F/U、左右/Home/End、Ctrl+C | Composer 行为与既定官方编辑规则一致 |
| 弹窗、问题面板、技能选择器 | 导航和选择不会被底层 Workspace 抢走 |
| Ctrl+T 详情往返 | 详情完整；返回时草稿、附件、折叠集合和锚点相同 |
| 详情期间 prepend/append | 返回看到最新历史，旧页不丢不重复，分页 cursor 与锚点正确；跟随底部单独断言 |
| 详情期间 resize/回退/thread 切换 | 按 4.5 恢复或清除视图状态，不能带回旧会话状态 |
| 详情返回/最终退出 | 返回不退出备用屏幕，鼠标和 Composer 恢复；真正退出才清理终端 |
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

单项 TDD 示例，在实现空格修复前先执行以下命令；GREEN 和该行为 REFACTOR 后使用同一命令。其余任务将过滤项替换为第 6 节对应的完整测试名：

```sh
cargo nextest list --manifest-path "codex-rs/Cargo.toml" -p codex-tui -E 'test(~workspace_input_space_reaches_composer)'
just test -p codex-tui -E 'test(~workspace_input_space_reaches_composer)'
```

必须确认精确目标已被发现并执行；不以总计有其它测试运行替代。预期 RED 为 `"hello" != "hello "` 的草稿断言失败；如果是不同原因，先修测试条件再执行，不改预期来迎合实现。若当前代码已能通过，标为已保护，并以实际未满足的长历史场景建立 RED。

每个任务提交前在实施记录填写以下证据（无证据不勾选）：

| 项目 | 必填内容 |
| --- | --- |
| 用例及目标缺陷 | 第 6 节测试名、会捕获的错误行为 |
| RED | 实现修改前 SHA + 测试 diff 标识、命令、发现/执行数量、退出码、关键 actual/expected、日志位置 |
| GREEN | 同一测试和相关回归的命令/数量/退出码、实现 diff 标识、日志位置 |
| REFACTOR | 是否发生目标内清理、清理后测试结果；未清理写“无” |
| 独立预期 | 官方 SHA、fixture、golden 或人工核对的协议字段 |
| 提交/未覆盖 | 行为提交 SHA；真实终端、iTerm2 或性能尚未覆盖的项目 |

日志只保留本任务需要的测试证据，避免写入认证信息；不能把文档中的预期失败、示例命令当作已执行日志。通过了原问题测试仍须运行相关 crate 回归；因环境无法运行则记录阻塞，不勾选 GREEN。

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
- [ ] 同一场景含 WebSearch 运行中/完成态；恢复会话和上翻旧页后核对官方对应展示，不只验 sed。
- [ ] 详情期间接收新输出并载入旧页，再返回 Workspace；检查草稿、附件、折叠、锚点、滚轮及图片，最后正常退出检查终端恢复。
- [ ] 在旧轮次键入 `hello world` 与中文含空格句子，确认空格输入、视口不跳底。
- [ ] 完成轮次折叠/展开、Ctrl+C/U、PageUp/PageDown、滚轮和窗口缩放检查。
- [ ] iTerm2 验证本地图片预览和退出后的终端恢复。

若平台拒绝 Computer Use 访问 iTerm2，停止该 GUI 操作，不用其它注入技术绕过；继续进行程序级 PTY 测试并提供用户手动操作清单。PTY 可证明渲染/输入链路，不能替代 iTerm2 图片显示证据。用户截图和反馈是有效运行态证据，按矩阵记录。

### 7.5 性能与内存

- [ ] 同机器、同 fixture 对修改前后 10,000 个普通 cell + 100 个读取 cell 的首次展示、滚动、折叠、详情往返采样，记录原始耗时和峰值/RSS。
- [ ] 另用单条 8MiB 普通命令输出和同样大小 Read 输出，走真实持久化投影；依次观察构建后、正常渲染后、打开详情并释放临时行后、30 次往返后。用分配分析记录或测试侧计量定位长期保留分配，确认没有增加一份与输出体积成比例的详情字符串集合。比较同样的底层历史存储基线，不能把原有会话记录计算为本轮新副本；短暂 renderer 分配与长期持有分开报告。
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
9. **TDD：** 行为修复先取得真实断言 RED，再改生产代码；不倒补日志，不把基线已通过的回归说成失败复现，不为追求红灯改坏原本正确的行为。
10. **所有权与模式：** 不长期保存 ExecCell 输出和详情正文两份，不保存脱离更新的第二套 Workspace 历史；模式返回不得执行最终关闭的全量清理。

## 9. 回滚与交付物

每个独立实现任务通过相关验证后，只暂存明确文件并提交，提交说明包含原因和验证。回滚采用对应任务的可逆提交；先核对混合内容，不直接重置到上游，也不重建全部本地功能。

实施交付必须包含：

- [ ] 本方案的逐项完成状态与实施提交 SHA。
- [ ] 官方正常展示、默认 Workspace、完整详情的对比快照。
- [ ] 各行为 RED/GREEN/REFACTOR 证据；独立官方 golden 来源；基线已通过的回归单独标识。
- [ ] 空格/编辑/导航 App 入口断言，以及用户可见的实际输入证据。
- [ ] 初始 resume、分页、活动尾部和详情往返证据。
- [ ] 新包构建退出码、哈希与实际进程路径。
- [ ] 性能采样、全量失败归因、仍未覆盖的运行态项。

当前仅完成本方案编写和只读源码核验。未执行上述实施任务；不以本文更新改变既有代码或运行中的用户会话。

## 10. 本次方案修订记录

| 审查问题 | 修订决策 | 对应验证 |
| --- | --- | --- |
| 官方 replay 与强制连续分组冲突 | 区分 live、completion-only、orphan、resume；分页不新增跨页合并 | 任务 C：独立官方 golden、逐接缝拆页、turn 归属 |
| 详情返回遗漏历史更新与终端状态 | 同一 overlay 切换模式，保存视图状态；区分返回与最终关闭 | 任务 E：prepend、append、resize、切会话、PTY 及 iTerm2 |
| ExecCell 与原详情行形成双份输出 | ExecCell 单份持有，详情按需临时生成；仅保存小量原始元数据 | 任务 C 与 7.5：数据完整、大输出长期分配检查 |
| 非命令事件目标大于实现范围 | WebSearch 纳入本次；其它类型逐类列出保留范围 | 任务 D 与 4.6：搜索跨入口、其余 fallback 回归 |

本节记录的是方案修订当时的审查边界：当时仅修改本方案，未执行实施任务或运行态验收。后续实际执行状态以第 11 节为准。

## 11. 2026-09-09 实施状态

| 范围 | 状态 | 当前证据 | 仍需完成 |
| --- | --- | --- | --- |
| 默认 Workspace 正常展示与旧 skill 状态机移除 | 已完成代码/专项测试 | `73dc9e30b7`；Read 正常展示 RED→GREEN，相关正常展示测试 4/4 | 用真实包与官方同事件截图作运行态对照 |
| 空格、Shift+Space 与弹窗优先级 | 已完成代码/专项测试 | `737d74165e`、`a89c85f783`、`d6faeeb3d1`；空格 RED→GREEN，弹窗 PageDown 选择回归通过 | 输入法组合态、Ctrl+B/F/Home/End 的真实终端矩阵 |
| 持久化 CommandExecution | 已完成代码/专项测试 | `e593e0d5c6`、`1918691b75`、`bc6245b979`；默认摘要、完整正文、真实状态/可选 exit/耗时测试通过，通用 fallback 边界受回归保护 | 一次载入与拆页载入的完整对照 |
| 持久化 WebSearch | 已完成代码/专项测试 | `9b6b59dd16`；plain fallback RED→官方 WebSearchCell GREEN | 运行中 WebSearch、resume 和真实分页入口对照 |
| 详情往返、锚点、线程隔离与备用屏 | 已完成代码/专项测试 | `0a6d98e589`、`c3ee81391d`、`a583be1a9a`、`06dfcee580`、`ddab83bea0`；草稿、q 返回、prepend/append、跟随底部、缩放、锚点删除、新线程切换，以及“返回不退出备用屏、最终关闭才退出”的 App 入口回归通过 | 原始 PTY 控制序列捕获、图片和真实 iTerm2 视觉验收 |
| Workspace 专项集 | 已通过 | `just test -p codex-tui -E 'test(~workspace_parity_) | test(~workspace_input_) | test(~workspace_details_)'`：17 通过、4346 跳过、退出码 0 | 输入法组合态、完整编辑键矩阵和真实终端输入 |
| 本地包 | 已完成构建完整性核验 | 功能代码 `06dfcee580`（构建时 HEAD `3848ea7f69`）；`./scripts/build-patched-tui.sh` 退出码 0，release/package SHA-256 均为 `a2e10f…e97db73`，生成时间 `2026-09-09T18:48:33Z`；系统 Codex SHA-256 保持 `b973d4…1261e3` | 新 TUI 进程的实际交互与 iTerm2 图片显示证据 |
| 全量 TUI | 未通过，未归因 | 4315 通过、25 失败、1 超时；失败以网络 mock/异步超时、custom-terminal pending snapshot 为主 | 在本轮之前基线或独立环境复验，逐项归因；不能宣称无关 |

本次实现提交顺序：`737d74165e`、`73dc9e30b7`、`e593e0d5c6`、`9b6b59dd16`、`0a6d98e589`、`a89c85f783`、`a583be1a9a`、`d6faeeb3d1`、`c3ee81391d`、`1918691b75`、`bc6245b979`、`06dfcee580`。未跟踪的 custom-terminal `.snap.new` 和旧方案文件不属于本任务，保持原样且未提交。

`06dfcee580` 的 TDD 记录：`workspace_details_resize_restore_keeps_the_same_top_cell` 在实现前两次均为 `left: 3, right: 15`；`workspace_parity_details_thread_change_clears_old_overlay` 在实现前两次均因旧 overlay 未清除失败。最小实现后，新增缩放、删除锚点、详情期间追加/跟随底部与线程切换测试共同进入上述 17 项专项集。`just fix -p codex-tui` 退出码 0，仅报告既有 `Overlay` 枚举体积警告；`just fmt` 退出码 0。`cargo insta pending-snapshots` 仅列出已存在的两个 custom-terminal `.snap.new`，未接受或修改。

当前本地包构建于 `2026-09-09T10:34:49Z` 之后，`./scripts/build-patched-tui.sh` 在 13 分 33 秒后明确成功。`codex-rs/target/codex-tui-package/bin/codex --version` 可执行并输出 `codex-cli 0.0.0`；该版本字符串不用于判断新旧，判定依据为上表 package/release 相等的 SHA-256。构建期间仅出现 app-server/cloud-tasks 未使用项、链接器 compact-unwind 和依赖未来兼容性警告，均未阻止生成。

`ddab83bea0` 的 `workspace_parity_details_terminal_lifecycle` 为基线保护：实现已具备该行为，因此首次执行即通过（1 通过、4363 跳过、退出码 0），不伪造 RED。它验证 TUI 的备用屏状态，不替代尚未取得的真实 PTY 控制序列或 iTerm2 图片证据。

2026-09-09 GUI 证据：iTerm2 已运行，但 Computer Use 对 bundle ID `com.googlecode.iterm2` 明确返回策略拒绝。按 7.4 节红线停止 GUI 自动操作，未使用替代注入方式；该限制不影响已完成的自动化与包完整性证据，但 iTerm2 图片/真实按键视觉验收仍需用户手动完成。
