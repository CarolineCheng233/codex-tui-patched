# Transcript Workspace 最终实施方案

> 状态：**已实施并完成定向验收。** 第一版之后发现的两个 P1 根因（Composer 高度未纳入工作区输入定位、底部追加用户消息后未更新自动目标）已由 `b6f33bb613` 修复；静态检查收口由 `d7f86ca961` 完成。本文件保留问题归因、设计与对抗审查，作为可追溯的实施和验收记录。
>
> 本文覆盖标题栏提示、技能正文输出、轮次折叠目标、历史用户消息样式，以及 `/exit` 后向 shell 泄漏鼠标事件。第一版的已通过项保留为历史证据；第 7–9 节记录 P1 的实现不变量、对抗审查与验收边界。

## 1. 目标与完成标准

在不替换系统 `codex`、不修改会话数据的前提下，完善本仓库本地包的 Transcript Workspace：

1. 标题栏清晰解释快捷键，且不显示无意义的 `/` 填充字符。
2. 工作区不铺开实际技能读取的 `SKILL.md` 正文；原始会话和普通完整 Transcript 仍保留内容。
3. 滚动到旧记录后，`Option+Left` 折叠会话区底边对应的轮次，而不是始终折叠最新轮次。
4. 所有用户消息使用与当前高亮消息相同的浅色背景；折叠目标仍可辨识。
5. `/exit`、异常退出和取消路径均不向 zsh 泄漏形如 `35;76;26M` 的鼠标事件字节。

完成的可观察标准：

- 在 iTerm2 中滚动、折叠、输入时，Composer 始终固定在底部且不会跳到底部。
- 滚到图中旧轮次，按 `Option+Left` 后只隐藏该用户消息之后、下一条用户消息之前的内容。
- 已确认技能的读取在工作区显示摘要而不显示正文；非技能文件读取不受影响。
- `/exit` 后继续滚动或移动鼠标，zsh 提示符不会出现 `…M` 文本。
- 官方 `codex`、`Ctrl+U`、非空输入时的 `Ctrl+C`、会话持久化和图片预览行为不变。

## 2. 当前事实与问题归因

### 2.1 标题栏

当前 `⌥` 表示 macOS 的 Option 键：

| 提示 | 含义 |
| --- | --- |
| `⌥↑↓ turns` | 选择前一轮或后一轮。 |
| `⌥← fold` | 折叠当前折叠目标。 |
| `⌥→ expand` | 展开当前折叠目标。 |
| `wheel/PgUp/PgDn scroll` | 只滚动会话区。 |
| `Ctrl+T close` | 关闭会话工作区。 |

右侧重复的 `/` 没有交互含义，只是通用 pager 标题栏的装饰性填充。

### 2.2 技能正文被展开

主聊天区使用命令单元的紧凑摘要；当前 Transcript Workspace 则使用完整 `transcript_lines`，因此读取 `SKILL.md` 的命令输出会显示完整正文。问题是工作区渲染策略不一致，不是技能文件被写入或会话数据被改变。

### 2.3 折叠了错误轮次

当前轮次状态初始化为最后一个用户消息；鼠标滚轮、`PageUp` 和 `PageDown` 不会更新该状态。因此视口虽然已移到旧轮次，`Option+Left` 仍会操作最新轮次。

### 2.4 只有最近用户消息是浅色背景

浅色背景是“当前选中轮次”的反色样式。历史用户消息采用常规样式，所以视觉不一致。

### 2.5 `/exit` 后显示鼠标事件文本

`35;76;26M` 的格式符合 SGR 鼠标上报事件残留。现有清理路径已经发送 `DisableMouseCapture`，本次补丁在恢复终端模式后刷新 stdin 内核队列，覆盖事件流丢弃时尚未解析的鼠标序列；隔离 iTerm2 已验证正常 `/exit` 回到 shell，移动、点击和滚轮仍按 5.2 完成人工验证。

## 3. 设计

### 3.1 工作区专属标题栏

不修改通用 `PagerView` 的标题行为。只为 Transcript Workspace 增加标题策略，按可用宽度渲染为单行：

```text
TRANSCRIPT · Option+Up/Down target · Option+Left fold · Option+Right expand · wheel/PgUp/PgDn scroll · Ctrl+T close
（窄窗口自动按显示宽度截断并以 `…` 结尾）
```

标题绝不换行；剩余空间使用细横线或留白，不使用 `/` 填充。

### 3.2 工作区渲染策略

增加只供工作区使用的渲染策略，避免更改普通聊天区或完整 Transcript 的语义：

```text
WorkspaceRenderProfile
├─ 已确认的技能 SKILL.md 读取组：紧凑摘要
├─ 其他命令：完整 Transcript 输出
├─ 已完成历史单元：使用同一策略
└─ 正在执行的 live tail：使用同一策略
```

当前实现的紧凑条件是：

1. 命令解析为 `Read`；
2. 路径与当前运行时已启用技能元数据中的精确 `SKILL.md` 路径匹配；
3. 同一 exploring 命令组内的所有解析项都能确认是上述技能读取；
4. 无法确认（未标注、混合命令或非 exploring 组）时保持完整输出。

不得只依据文件名 `SKILL.md` 判断，避免隐藏项目中的普通同名文件。紧凑摘要沿用现有 `Exploring/Explored` 与读取名称展示，但不渲染正文。此规则只改变视觉展示，不删除会话事件、原始输出或会话日志，也不是保密机制。

### 3.3 轮次与视口索引（第一版设计）

轮次定义保持不变：从一条用户消息开始，到下一条用户消息之前结束。

新增宽度键控的 `WorkspaceLayoutIndex`，缓存每个可见 cell 的物理高度和每轮起始行。计算计入：

- 文本换行和消息上下留白；
- 助手消息、命令输出和流式尾部（live tail 的高度仍由 pager 正常计算）；
- 本地图片预留行；
- 已折叠轮次的占位行；
- 当前终端宽度与工作区渲染配置文件。

缓存按终端宽度保存，并在历史插入/裁剪、折叠展开、图片预览切换、live tail 变化时失效。仅在这些条件变化时重建；滚轮和分页事件用二分查找定位轮次，目标标记通过共享状态更新，不重建整份会话。

滚动后的自动目标规则：计算会话区底边所在的最后一个可见轮次起始行；图片预留行和折叠占位行归属于其所在轮次，不会把目标错误地固定在最新轮次。

交互规则：

- 打开工作区时，目标为最底部可见轮次，通常是最新轮次。
- 鼠标滚轮、`PageUp`、`PageDown` 后，目标自动跟随底边上下文。
- `Option+Up/Down` 手动切换相邻轮次；下一次滚动后恢复自动跟随。
- `Option+Left` 保留该轮用户问题，隐藏其后的回复与工具内容。
- `Option+Right` 只展开当前目标。
- 所有用户消息背景一致时，目标消息的前缀从 `›` 显示为独特的 `▸`；不使用不同背景色表示目标。

加载更早历史、历史裁剪、追加消息或展开/折叠必须使索引失效并安全重建。插入旧历史时保留有效目标；目标已失效时重新按视口规则计算。

### 3.4 用户消息样式

工作区中的每个 `UserHistoryCell` 使用当前最新选中消息的主题派生浅色样式，包括换行、空白填充和附图标签区域。不得硬编码纯白；必须复用现有终端调色板派生逻辑，使深色和浅色 iTerm2 主题均保持可读性。

普通主聊天区、标准 Transcript、助手消息、工具输出和终端主题均不改动。

### 3.5 `/exit` 的终端状态恢复

先建立共享的 TUI 资源所有权表，明确由 Codex 自己启用并负责关闭的终端模式。只恢复 Codex 自己启用的鼠标捕获模式。

退出清理必须覆盖：

- 正常 `/exit`；
- `Ctrl+C` 退出路径；
- 错误返回和 Drop 清理；
- 工作区 Overlay 的关闭与应用整体退出。

实际退出路径先由 `restore_common` 关闭 Codex 启用的鼠标上报并恢复 raw/cursor 状态，再刷新 stdin 内核队列，最后完成 stderr 收尾；Overlay 关闭路径另行关闭鼠标捕获并离开备用屏幕。不得关闭 shell 自身使用的 bracketed paste，不得写入 iTerm2 Profile、zsh 配置或全局鼠标设置。

## 4. 实施结果

1. **渲染隔离**：工作区专属渲染策略已接入已完成 cell 与 live tail；普通 Transcript 使用原路径。
2. **缓存索引**：`WorkspaceLayoutIndex` 已覆盖换行、留白、图片预留、折叠占位和宽度变化；滚动定位为缓存二分查找。
3. **UI 表现**：工作区标题单行截断、用户消息统一主题、目标轮次使用 `▸` 标记已完成。
4. **退出清理**：`restore_after_exit` 已在恢复终端后刷新 stdin 队列；原有鼠标关闭序列保持不变。
5. **自动化验收**：定向测试和隔离验收脚本通过；完整套件存在与本改动无关的外部服务/环境失败，详见 5.1。
6. **持久化**：方案、功能代码、静态检查与验收记录均拆为可回滚提交；不推送官方仓库或替换系统 `codex`。
7. **P1-A 实时几何**：绘制与输入事件共用 `workspace_input::transcript_workspace_layout`。键盘、滚轮和分页只接收当前 `layout.transcript`，而不是整个终端区域；普通 Pager 仍从原完整视口入口处理。
8. **P1-B 目标来源**：新增私有 `WorkspaceTargetMode`。滚动同步为 `FollowViewport`，`Option+Up/Down` 成功切换后为 `Manual`；只有“底部 + 自动模式 + 新用户消息”才选择最新轮次。
9. **P2 静态检查**：布局索引改为独占 `Box`，并移除两处与严格 clippy 冲突的等价测试克隆；不改变缓存生命周期或产品行为。

## 5. 验收记录与复核结果

### 5.1 自动化验收

P1 复核新增的五个回归场景全部通过：

1. 底部自动模式追加用户消息后，目标立即切换至该新轮次。
2. `Option+Up` 手动选择旧轮次后，即使底部追加用户消息，也不覆盖该选择。
3. 随后一次滚动恢复自动跟随；下一条用户消息再次成为目标。
4. 3 行 Composer、9 行完整窗口下的 `PageUp` 以实际 Transcript 区域定位，而非完整窗口高度。
5. 同一几何下的滚轮也只以实际 Transcript 区域定位。

已有功能的技能摘要、统一用户背景、折叠/展开、图片预览、固定 Composer、`Ctrl+U`、非空输入时 `Ctrl+C` 和 Code Mode host 均由既有定向用例回归。

本次验证记录：

- `just test -p codex-tui pager_overlay`：46/46 通过。
- `just test -p codex-tui exec_cell`：21/21 通过。
- `cargo clippy -p codex-tui --tests -- -D warnings`：通过。
- `./scripts/verify-patched-tui.sh`：通过（配置、工作区/图片、Code Mode host 检查均通过）。
- `./scripts/build-patched-tui.sh`：release 包构建成功；包内 `bin/codex --version` 为 `codex-cli 0.0.0`。仅有上游 linker 与 future-incompatibility 警告。
- `cargo fmt --all -- --config imports_granularity=Item` 与 `git diff --check`：通过。`just fmt` 因本机缺少仓库 Bazel formatter 所需的 `dotslash` 而在 Rust 格式检查前失败；这不是源码格式错误，且未修改 Bazel/工具链配置。
- `just test -p codex-tui`：4,337 项中 4,312 通过、25 失败、6 跳过。25 项均在未改动的 app-server/wiremock、响应服务重连、会话恢复或 `custom_terminal` 终端配色快照中；新增及相关 Workspace 测试均通过。因此它不是“全套通过”的证据，但没有本次路径的失败。
- 隔离 PTY 的既有 `/exit` 回归已记录为回到 shell 且无 `35;76;26M` 残留；本次 P1 没有修改退出或终端恢复路径。复跑物理 UI 时，包在连接本机 app-server 阶段持续等待，未进入可交互主界面，故不把该环境受阻复跑冒充为人工验收通过。

运行门槛：

```sh
cd "/Users/chy/projects/codex-tui-patched"
just fmt
just test -p codex-tui
./scripts/verify-patched-tui.sh
./scripts/build-patched-tui.sh
./scripts/codex-tui.sh --version
```

### 5.2 iTerm2 人工验收

在专用 iTerm2 测试窗口中完成，不操作用户已有会话：

1. 打开包含技能读取的历史，确认正文不显示、其他命令输出仍显示。
2. 滚到旧轮次，确认底边目标前缀为 `▸`；按 `Option+Left` 后只折叠该轮。
3. 检查全部历史用户消息均使用与最新用户消息一致的浅色背景。
4. 滚动历史时输入 Composer，确认输入框固定且不跳底。
5. `/exit` 后移动鼠标、点击和滚轮，确认 zsh 提示符不出现 `…M` 字节。
6. 启动系统官方 `codex`，确认其启动、聊天和退出均未受影响。

### 5.3 第一版对抗式审查（结论已被后续复核更新）

以下场景按“最容易暴露错误实现”的方式复核；结论只覆盖当前证据：

| 对抗场景 | 防线 | 结果 |
| --- | --- | --- |
| 把任意同名 `SKILL.md` 当成技能并隐藏 | 必须与运行时启用技能的精确路径匹配；混合/未标注读取 fail-open | 定向测试通过 |
| 滚轮已到旧轮次但折叠仍作用于最新轮次 | 宽度键控布局索引 + 视口底边二分定位 + 目标状态同步 | **部分通过**：常规高度下通过；多行 Composer 会把完整终端区域误当 Transcript 区域，P1，见第 7.2.1 |
| 停在底部后提交一条新的用户消息再折叠 | `refresh_after_append` 保留原选择 | **失败**：新增用户轮次没有成为自动目标，P1，见第 7.2.2 |
| 展开/折叠或改窗口宽度后使用旧高度 | 所有结构变化失效索引并清除 pager 高度缓存 | 定向测试通过 |
| 所有用户消息同色后无法识别目标 | 统一主题，仅目标前缀改为 `▸` | 快照/定向测试通过 |
| 工作区优化误伤普通 Transcript | 渲染策略只在 Transcript Workspace 分支启用 | 定向测试通过 |
| `/exit` 后残留 SGR 鼠标字节进入 zsh | 先恢复 Codex 终端状态，再刷新 stdin 队列 | 隔离 iTerm2 冒烟通过；其后以同一 PTY 写入 `/exit\\r` 和 SGR 鼠标字节，确认回到 shell 且未泄漏 `35;76;26M`。真实物理事件仍待 8.2 |
| 临时包覆盖系统安装或修改用户配置 | 启动脚本只调用仓库包；未改 `/Applications/Codex`、`.zshrc` 或 iTerm2 Profile | 只读检查通过 |

更新后的审查结论：两个 P1 根因均已按第 7 节实现，并由第 8.1 节的反例测试覆盖。全量套件的 25 个环境/上游失败不构成 P1 失败，但阻止把结果表述为“完整 TUI 套件全绿”；真实 iTerm2 人工交互仍应在本机 app-server 可用时按第 8.2 节补录。

## 6. 红线

1. 不修改 `/Applications/Codex`、系统 `codex`、iTerm2 Profile、`.zshrc`、会话格式、模型调用或网络逻辑。
2. 不删除、改写、迁移或伪造任何会话、技能文件、工具输出、图片或凭证。
3. 不以文件名猜测技能；无法确认技能身份时保留完整内容。
4. 不在滚动事件中扫描、重建或分配完整 transcript。
5. 不修改通用 pager 标题栏、普通 Transcript 或主聊天区的样式语义。
6. 不改变 `Ctrl+U`、非空输入时的 `Ctrl+C`、Enter、会话持久化或图片预览行为。
7. 不将视觉折叠宣称为安全或保密能力。
8. 不把本地 release 包当作生产发布；隔离冒烟只证明启动和正常 `/exit` 路径，不得替代 5.2 的完整人工验收，也不推送远端。
9. 轮次定位只在本地包和第 5.1 节覆盖的自动化范围内验收；不得将该结果扩大为官方 App、系统安装或用户配置已被修改。

## 7. P1 修复实施记录

以下方案已实施。保留设计细节以便后续升级 Codex TUI 时可按相同不变量处理冲突；实际变更见 `b6f33bb613` 与 `d7f86ca961`。

### 7.1 目标与非目标

本次只修复两个已证实的问题，并收口本次工作区状态引入的静态检查告警：

1. Composer 有多行输入、补全提示或高度变化时，鼠标滚动、`PageUp`、`PageDown` 和 `Option+Left/Right` 都必须按**实际 Transcript 可视区域**定位轮次，不能将 Composer 行计入会话区。
2. 视口原本停在底部且目标处于自动跟随模式时，追加新用户消息必须让新轮次成为折叠目标；用户用 `Option+Up/Down` 手动选择旧轮次后，追加消息不得覆盖该选择。
3. `cargo clippy -p codex-tui --tests -- -D warnings` 必须通过，不留下本次工作区状态导致的 `large_enum_variant` 或可读性告警。

不在范围内：改变折叠语义、增加快捷键、重写 pager、改变普通 Transcript、调整系统 Codex/iTerm2/zsh，或解决尚无证据的内存问题。

### 7.2 当前框架与根因

```text
App::draw(frame)
  └─ TranscriptWorkspaceLayout::new(frame.area(), composer.desired_height(width))
       └─ render_workspace(layout.transcript, ...)

输入事件（现状）
  └─ workspace_navigation_key / workspace_mouse
       └─ sync_workspace_turn_to_viewport(tui.terminal.viewport_area)  ← 含 Composer，错误

追加 cell（现状）
  └─ TranscriptOverlay::insert_cell
       └─ refresh_after_append()：保留仍有效的旧 selected_turn  ← 新用户轮次未成为目标
```

#### 7.2.1 P1-A：输入与绘制使用不同几何区域

`codex-rs/tui/src/app_backtrack.rs` 的绘制路径已经按照 `composer.desired_height(width)` 创建 `TranscriptWorkspaceLayout`，并用 `layout.transcript` 渲染。工作区键盘、鼠标和同步逻辑在 `codex-rs/tui/src/pager_overlay.rs`，却向 `sync_workspace_turn_to_viewport` 传入完整的 `tui.terminal.viewport_area`。Composer 变高时，二分定位的底边低于实际会话区，因而选中更晚、不可见的轮次。

仅缓存上一帧的 `Rect` 不是修复：输入改变 Composer 高度后，下一次滚动可发生在 draw 前，缓存仍会过期。

#### 7.2.2 P1-B：选中轮次缺少来源语义

`TranscriptOverlay::insert_cell` 会判断是否在底部、追加 cell、刷新轮次并重建 renderables。`TranscriptTurnState::refresh_after_append` 的规则是“旧 `selected_turn` 仍有效就保留”，所以没有把新用户轮次设为目标。若改为总是选择最新轮次，又会破坏用户刚用 `Option+Up/Down` 手动选择的历史轮次。

`selected_turn` 只表达“选哪一轮”，不能表达“该选择来自视口还是用户”；需要一个最小的目标来源状态。

### 7.3 设计与实现

#### 7.3.1 单一真实布局来源（P1-A）

在 `codex-rs/tui/src/app_backtrack/workspace_input.rs` 增加工作区私有帮助函数：输入当前完整区域和当前 Composer，输出 `TranscriptWorkspaceLayout`。它必须使用与绘制相同的 `composer.desired_height(width)` 公式，绝不保存或复用上帧矩形。

- `App::draw` 使用该布局并以 `layout.transcript` 渲染。
- 工作区键盘和鼠标事件在处理前，基于当前 `tui.terminal.viewport_area` 与当前 Composer 得到同一布局，并把 `layout.transcript` 传给同步逻辑。
- `sync_workspace_turn_to_viewport` 改为接收 `transcript_area: Rect`，不再自行接收完整终端区域。
- `PagerView` 增加仅模块内部可见的“指定可视区域处理按键”入口。现有普通 pager 入口继续以完整终端区域委托它，行为不变；工作区传入 `layout.transcript`。分页高度必须从传入区域即时计算，不能取上次渲染的全屏高度。

布局索引、二分查找和 renderables 缓存维持现有职责：wheel/PageUp 热路径只读取已缓存索引和高度，不得全量测量、分配或重建 transcript。

#### 7.3.2 显式目标模式与追加规则（P1-B）

在 `TranscriptOverlay` 增加私有状态，且不替代 `TranscriptTurnState::selected_turn`：

```rust
enum WorkspaceTargetMode {
    FollowViewport,
    Manual,
}
```

- 打开工作区、视口同步、鼠标滚动、`PageUp/PageDown` 后设为 `FollowViewport`，然后按实际 Transcript 区域重新定位。
- `Option+Up/Down` 成功选择相邻轮次后设为 `Manual`。
- `Option+Left/Right` 只折叠/展开当前目标，不改变来源模式。
- `insert_cell` 追加的是 `UserHistoryCell`、追加前确实位于底部、且模式为 `FollowViewport` 时，调用明确命名的 `select_latest_turn()`；助手/工具 cell、非底部追加或 `Manual` 模式均保留原目标。
- 旧历史 prepend 只平移有效索引并保留模式；替换全部 cells、历史裁剪使目标失效或会话重置时，重置为 `FollowViewport`，再按实际视口定位。

该规则同时满足“底部自动跟随新用户”和“手动选择不被后台追加覆盖”，不再以 `usize` 是否有效猜测用户意图。

#### 7.3.3 静态质量收口（P2）

第一版新增的工作区索引使 `Overlay::Transcript(TranscriptOverlay)` 比其他变体大 208 bytes。将 `workspace_layout_index` 改为 `Option<Box<WorkspaceLayoutIndex>>`：索引中的向量本就位于堆上，Box 只缩小枚举载荷，不改变缓存生命周期；失效或 Overlay Drop 时自然释放，不引入循环引用或常驻增长。

`pager_overlay.rs` 的 `then_some(1).unwrap_or(0)` 改为等价且更清晰的 `if`，消除 `clippy::obfuscated_if_else`。不借此重排无关代码、格式或公共 API。

### 7.4 方案对抗式审查

| 反例 | 防线 | 不通过时处理 |
| --- | --- | --- |
| 输入后、下一帧 draw 前 Composer 变高 | 每个输入事件即时计算布局；不缓存 `Rect` | 逐一检查键盘、鼠标、同步入口均传 `layout.transcript` |
| 只修 mouse，首个 PageUp 仍按全屏高度跳转 | Pager 接受显式区域，鼠标和键盘共用 | 增加无先前 draw 的分页测试，禁止只测 wheel |
| 新用户总抢占手动选择 | `Manual` 阻止追加重选 | 增加“手动选旧轮次 + 底部追加”回归测试 |
| 自动模式没有跟随新用户 | 仅 `FollowViewport && was_at_bottom && UserHistoryCell` 时选最新 | 增加“底部自动模式 + 新用户 + Option+Left”状态测试 |
| 在滚动热路径重建索引 | 延用缓存，事件只查找 | 保留并扩展高度测量计数测试 |
| Box 造成泄漏 | Overlay 独占 Box，无 `Rc`/循环所有权 | 审查无 `mem::forget`/静态缓存；运行 clippy 与回归测试 |
| 修复误伤普通 pager | 旧入口只委托，普通路径不传 WorkspaceLayout | 现有普通 pager 测试原样通过 |

审查结论：设计对两个 P1 都有直接且可测试的因果链，尤其避免了“缓存旧几何”和“总选最新轮次”这两个表面修复。若实现必须修改公共 Pager API、会话模型或终端恢复逻辑，应停止该实现路径并重新出方案，不能扩大补丁。

## 8. 修复后验收方案

### 8.1 自动化验收

**结果：通过（定向范围）。** 下列 P1-A/P1-B 场景已实现为 `pager_overlay_transcript_workspace_tests.rs` 回归测试，并由 `just test -p codex-tui pager_overlay` 的 46 项结果覆盖。第 5.1 节记录了命令、版本和完整套件的环境边界。

新增或扩展 `codex-rs/tui/src/pager_overlay_transcript_workspace_tests.rs`；必要时补充相邻 App 集成测试。最少覆盖：

1. 10 行完整终端、3 行 Composer、7 行 Transcript 的边界：底边定位只能选择实际 Transcript 底边的轮次，不得选择完整终端底边的下一轮。
2. Composer 从单行变多行后，不依赖旧 draw 缓存；鼠标滚动和 `PageUp/PageDown` 都传入新的 Transcript 区域。
3. 位于底部且为 `FollowViewport` 时追加 `UserHistoryCell`：目标立即变为最后一轮，随后 `Option+Left` 折叠该新轮次。
4. 位于底部但先 `Option+Up/Down` 进入 `Manual`：追加新用户消息后目标仍为旧轮次。
5. 滚到旧历史后追加用户、助手或工具 cell：滚动偏移、目标与模式均不被意外覆盖。
6. 连续至少 32 次 wheel/分页：布局高度测量计数不随每次事件线性增长；改宽、折叠、展开和图片占位仍正确触发失效重建。
7. 普通 Pager、标准 Transcript、技能摘要、统一用户背景、图片预览、`Ctrl+U` 与非空输入时 `Ctrl+C` 的既有测试全部保持通过。

执行门槛：在仓库根目录依次运行 `just fmt`、`just test -p codex-tui pager_overlay`、`just test -p codex-tui exec_cell`、`cargo clippy -p codex-tui --tests -- -D warnings`、`./scripts/verify-patched-tui.sh`、`./scripts/build-patched-tui.sh`、`./scripts/codex-tui.sh --version` 与 `git diff --check`，必须全部通过。

完整 `just test -p codex-tui` 仅在执行阶段环境允许时运行；若仍出现既有 wiremock/交互环境失败，必须逐项证明与改动无关，不能把“未运行”或“有失败”写成全套通过。

### 8.2 iTerm2 隔离人工验收

**状态：待本机 app-server 可交互时补录，不作为已通过项。** 当前环境启动隔离本地包后持续停留在 app-server 连接阶段，无法在不伪造对话的前提下完成鼠标、分页和发送新消息的真实 UI 操作。不得以此失败扩大补丁到网络、认证或系统配置。

只在专用 iTerm2 测试窗口使用仓库脚本，不操作用户已有终端或官方 Codex：

1. 输入多行草稿使 Composer 升高，滚到旧轮次，分别用 wheel、`PageUp`、`PageDown` 定位，按 `Option+Left`；每次只折叠边界对应轮次。
2. 停在底部且不手动切换目标，发送新用户消息；`Option+Left` 必须折叠刚发送的轮次。
3. 停在底部，先用 `Option+Up/Down` 选旧轮次，再发送新用户消息；折叠仍必须作用于旧轮次，直到下一次滚动恢复自动跟随。
4. 检查每条历史用户消息均为浅色背景，当前目标仅以 `▸` 区分；技能正文仍不会意外展开。
5. `/exit` 后移动、点击和滚轮；zsh 提示符不得出现 SGR 鼠标字节。再启动系统 `codex`，确认未受影响。

每项必须记录通过/失败、包路径和截图或可复现步骤。任一项失败都不能标记完成。

### 8.3 实施提交、回滚与交付门槛

实际提交按最小可回滚单元完成：

1. `b6f33bb613 fix: 修复 Transcript Workspace 轮次定位`：P1-A 和 P1-B 共用事件/Overlay 状态链，连同全部回归测试作为一个可独立回滚的用户可见修复提交。
2. `d7f86ca961 chore: 收口 TUI 严格 clippy 告警`：仅包含两处等价测试克隆清理。
3. 本文档的验收记录单独提交，不混入功能代码。

每次提交前检查 `git status --short`、`git diff`、`git diff --cached`，只暂存明确文件；不创建或切换分支，不推送远端。若某个提交需要包含无关改动，必须拆开而非混入。

## 9. 最终红线

除第 6 节的全局红线外，执行第 7 节时必须遵守：

1. 不以缓存前一帧布局替代实时 Composer 几何；这会重新引入 P1-A。
2. 不以“每次追加都选最新轮次”替代目标模式；这会破坏手动选择。
3. 不把索引重建、文本测量或 `Vec` 分配放进 wheel/PageUp 热路径。
4. 不扩大 `PagerView` 的公开语义；显式区域入口必须保持模块私有，普通调用保持原行为。
5. 不改变会话数据、原始命令输出、普通 Transcript、主聊天区、终端模式恢复、系统 Codex、iTerm2 或 zsh 配置。
6. 不因为本地 `codex-cli 0.0.0` 构建成功就推送或替换官方安装；定向验收完成不等于发布，iTerm2 人工项未补录前也不得声明完整人工验收完成。
