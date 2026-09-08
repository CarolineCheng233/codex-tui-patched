# Transcript Workspace 技能内容隔离修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. 未经用户明确批准，禁止启动 subagent。

**Goal:** 修复 Transcript Workspace 在同一探索命令组中读取 skill 主文件及其引用文件后，错误展开 skill 详细内容的问题，同时保持普通命令和 Codex 官方 TUI 既有行为不变。

**Architecture:** 保留 Codex 官方的 `ExecCell` 分组、普通 `display_lines`、完整 `transcript_lines` 和 app-server 命令协议；只在 Workspace 专属渲染入口内，把判定粒度从“整个 `ExecCell`”缩小到“单个 `ExecCall`”。以已经由官方技能元数据精确标记的 `SKILL.md` 为可信根，只压缩该 skill 目录内的读取调用，其他调用继续复用原完整 transcript 渲染。

**Tech Stack:** Rust、`codex-tui`、ratatui、insta snapshot、Cargo/nextest、iTerm2。

**Spec:** `TRANSCRIPT_WORKSPACE_FINAL_PLAN.md`

## Global Constraints

- 只解决 skill 内容在 Transcript Workspace 中被完整展开这一项问题。
- 普通命令、Web 搜索、`curl`、脚本、项目文件读取及其输出必须与当前 Codex 官方逻辑保持一致。
- 只改变视觉渲染；不得删除、改写或迁移会话事件、命令输出和 skill 文件。
- 不修改系统 `codex`、`/Applications/Codex`、`~/.zshrc`、iTerm2 Profile 或用户配置。
- 不增加依赖、配置项、快捷键、公共 API 或 app-server 协议字段。
- 不处理尚未证实的技能列表启动时序问题。
- 未经用户明确批准，禁止启动 subagent。

---

## 1. 解决什么问题

### 1.1 用户可见问题

一次真实会话依次执行：

```text
Read /skill-root/using-superpowers/SKILL.md
Read /skill-root/using-superpowers/references/codex-tools.md
```

两条命令都是成功的探索型读取。Codex 官方会把连续的 `Read/Search/ListFiles` 合并到同一个 `ExecCell`。本地 Workspace 补丁随后按整个 cell 判断是否压缩输出：

```rust
any(Self::is_skill_read) && all(Self::is_skill_read)
```

第一条读取经过官方技能元数据标记，能够识别为 skill；第二条是 skill 目录中的引用文件，但名称不是 `SKILL.md`。因此 `all(...)` 返回 `false`，整个 cell 回退到完整 transcript，导致两条读取的正文都显示出来。

### 1.2 已验证事实

1. app-server 已把主文件命令正确解析为 `ParsedCommand::Read`，不是 `Unknown`。
2. 实际路径与 `skills/list` 中启用的 `superpowers:using-superpowers` 路径完全一致。
3. 中间的 reasoning 为空，不会创建历史 cell，也不会分隔两次读取。
4. 成功的 exploring cell 不会在第一次读取结束后立即 flush，第二次读取会追加到同一个 cell。
5. 当前 Workspace 仅有“整个 cell 紧凑”或“整个 cell 完整”两个分支，没有逐调用渲染能力。

### 1.3 非目标

- 不改变 Codex 官方将连续探索命令合并为 `ExecCell` 的规则。
- 不改变普通主聊天区对 exploring 命令的摘要方式。
- 不改变普通 Transcript 的完整输出语义。
- 不隐藏任意名为 `SKILL.md` 的项目文件。
- 不按命令字符串、`sed`、`cat`、目录名称或硬编码用户路径猜测 skill。
- 不把视觉隐藏描述为安全、脱敏或保密能力。

## 2. 当前框架

```text
app-server CommandExecution
  └─ ChatWidget::annotate_skill_reads_in_parsed_cmd
       └─ 只对“启用 skill 的精确 SKILL.md 路径”添加可信名称标记

ChatWidget command lifecycle
  └─ ExecCell::add_call
       └─ 连续 Read/Search/ListFiles 进入同一个 ExecCell（官方行为）

普通主聊天区
  └─ ExecCell::display_lines
       └─ exploring_display_lines（官方摘要，保持不变）

普通完整 Transcript
  └─ ExecCell::transcript_lines
       └─ 命令和输出完整显示（官方行为，保持不变）

Transcript Workspace
  └─ ExecCell::workspace_transcript_hyperlink_lines
       ├─ 整个 cell 都是 skill read → display_lines
       └─ 否则 → transcript_hyperlink_lines
                          ↑ 当前缺陷：判断粒度过粗
```

涉及文件：

- `codex-rs/tui/src/exec_cell/model.rs`：`ExecCell`/`ExecCall` 模型以及当前 skill 读取判断。
- `codex-rs/tui/src/exec_cell/render.rs`：官方 display/transcript 渲染与 Workspace 专属渲染入口；现有相关测试也位于该文件的既有测试模块。
- `codex-rs/tui/src/chatwidget/skills.rs`：现有权威 skill 主文件标记来源；本次只读取和复用其结果，不修改该文件。

## 3. 如何解决

### 3.1 核心原则：按 `ExecCall` 隔离，不按 `ExecCell` 二选一

将当前 `reads_skill_instructions(&self) -> bool` 替换为单次调用判断：

```rust
fn call_reads_skill_content(&self, call: &ExecCall) -> bool
```

该方法必须同时满足：

1. `call` 是非 `UserShell` 的 exploring 调用；
2. `call.parsed` 非空且全部是 `ParsedCommand::Read`；
3. 每个读取路径都位于当前 cell 中某个“已精确标记的 skill 主文件”的父目录内；
4. 路径边界使用 `Path::starts_with`/路径组件语义，不使用字符串前缀。

可信 skill 根只能由现有精确标记产生：

```rust
fn annotated_skill_root(parsed: &ParsedCommand) -> Option<&Path> {
    match parsed {
        ParsedCommand::Read { name, path, .. }
            if name.starts_with("SKILL.md (") && name.ends_with(" skill)") =>
        {
            path.parent()
        }
        _ => None,
    }
}
```

单次调用判断应保持 fail-open：

```rust
fn call_reads_skill_content(&self, call: &ExecCall) -> bool {
    Self::is_exploring_call(call)
        && !call.parsed.is_empty()
        && call.parsed.iter().all(|parsed| {
            let ParsedCommand::Read { path, .. } = parsed else {
                return false;
            };
            self.calls
                .iter()
                .flat_map(|candidate| &candidate.parsed)
                .filter_map(Self::annotated_skill_root)
                .any(|root| path.starts_with(root))
        })
}
```

该设计能识别：

- 已标记的主 `SKILL.md`；
- 同一 skill 目录下的 `references/*.md`；
- 同一 skill 目录下被读取的其他内部说明文件。

该设计不会识别：

- 未注册或未启用的普通 `SKILL.md`；
- skill 目录以外的项目文件；
- `curl`、搜索、脚本执行或包含非 skill 读取的混合 shell 调用。

### 3.2 保持官方渲染函数的观察结果不变

从现有 `transcript_lines` 提取一个只负责渲染给定调用切片的私有帮助函数：

```rust
fn transcript_lines_for_calls(calls: &[ExecCall], width: u16) -> Vec<Line<'static>>
```

原 `HistoryCell::transcript_lines` 仅委托全部调用：

```rust
fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
    Self::transcript_lines_for_calls(&self.calls, width)
}
```

从 `exploring_display_lines` 提取对应的私有切片渲染：

```rust
fn exploring_display_lines_for_calls(
    &self,
    calls: &[ExecCall],
    width: u16,
) -> Vec<Line<'static>>
```

原 `display_lines` 仍把全部 `self.calls` 交给该函数，生成的文字、样式、顺序和换行必须与修改前完全一致。提取仅用于复用，不允许顺手重排或美化官方输出。

### 3.3 Workspace 逐块渲染

`workspace_transcript_hyperlink_lines` 按连续、相同可见性状态的 `ExecCall` 分块：

```text
[skill 主文件读取]       → 官方 exploring 摘要，不显示输出
[skill reference 读取]  → 官方 exploring 摘要，不显示输出
[普通 README 读取]       → 原完整 transcript，显示命令和输出
[普通 curl/脚本]         → 原完整 transcript，显示命令和输出
```

伪代码约束如下：

```rust
fn workspace_transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
    let mut lines = Vec::new();
    for block in self.workspace_call_blocks() {
        let mut block_lines = if block.compact_skill_content {
            self.exploring_display_lines_for_calls(block.calls, width)
        } else {
            Self::transcript_lines_for_calls(block.calls, width)
        };
        if !lines.is_empty() && !block_lines.is_empty() {
            lines.push(Line::default());
        }
        lines.append(&mut block_lines);
    }
    plain_hyperlink_lines(lines)
}
```

实现时不要求创建持久化的 `WorkspaceCallBlock` 类型；如果局部索引循环更短、更清晰，应直接使用局部索引，避免 YAGNI。不得缓存命令输出副本，不得克隆 `CommandOutput`，不得引入新的常驻集合。

### 3.4 无法逐项分离的混合调用

单个 `ExecCall` 可能包含多个 parsed 子命令，但只有一个聚合输出。如果其中任意子命令不属于已确认的 skill 目录，就无法可靠区分输出归属。该调用必须完整显示：

```text
Read enabled-skill/SKILL.md + Read project/README.md
→ 整个 ExecCall 完整显示
```

fail-open 只作用于这一个无法拆分的调用，不能再扩大到同一 `ExecCell` 中已经能够独立判断的其他调用。

## 4. 如何验收

### 4.1 正确的自动化回归测试

在 `codex-rs/tui/src/exec_cell/render.rs` 的既有测试模块中新增：

```rust
#[test]
fn workspace_compacts_skill_content_per_call_in_mixed_exploring_group()
```

测试必须通过 `ExecCell::new`、`add_call` 和 `complete_call` 构造与真实会话一致的同一 exploring cell：

```text
call-1: /tmp/using-superpowers/SKILL.md
        name = "SKILL.md (superpowers:using-superpowers skill)"
        output = "PRIVATE MAIN SKILL BODY"

call-2: /tmp/using-superpowers/references/codex-tools.md
        name = "codex-tools.md"
        output = "PRIVATE SKILL REFERENCE BODY"

call-3: /tmp/project/README.md
        name = "README.md"
        output = "VISIBLE ORDINARY FILE BODY"
```

断言：

```rust
assert!(!rendered.contains("PRIVATE MAIN SKILL BODY"));
assert!(!rendered.contains("PRIVATE SKILL REFERENCE BODY"));
assert!(rendered.contains("VISIBLE ORDINARY FILE BODY"));
assert!(rendered.contains("Read SKILL.md"));
assert!(rendered.contains("Read codex-tools.md"));
```

同时添加 inline `insta::assert_snapshot!`，锁定用户可见顺序、摘要和普通输出，满足 TUI 可见变化必须有 snapshot 的仓库要求。

修改实现前运行：

```bash
cd "/Users/chy/projects/codex-tui-patched"
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-tui workspace_compacts_skill_content_per_call_in_mixed_exploring_group
```

预期：失败，输出包含 `PRIVATE MAIN SKILL BODY`，证明测试复现本次问题。

实现后重复运行同一命令，预期：通过。

### 4.2 必须保留的反例测试

扩展现有 `workspace_compacts_skill_instruction_output` 或新增相邻测试，覆盖：

| 场景 | 预期 |
| --- | --- |
| 单独读取已确认的 `SKILL.md` | 只显示摘要 |
| 随后读取同一 skill 的 `references/codex-tools.md` | 只显示摘要 |
| 同一 cell 随后读取项目 `README.md` | 命令和正文完整显示 |
| 普通项目中未标记的 `SKILL.md` | 命令和正文完整显示 |
| 单个调用混合 skill 路径与普通路径 | 该调用完整显示 |
| `curl` 返回大段 JSON | 完整显示，行为不变 |
| 普通 Search/ListFiles | 与修改前一致 |
| live tail 与已完成历史 cell | 使用同一逐调用规则 |

### 4.3 定向回归门槛

依次执行：

```bash
cd "/Users/chy/projects/codex-tui-patched/codex-rs"
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just fmt
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-tui exec_cell
PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-tui pager_overlay
PATH="/opt/homebrew/opt/rustup/bin:$PATH" cargo clippy -p codex-tui --tests -- -D warnings
cd "/Users/chy/projects/codex-tui-patched"
./scripts/verify-patched-tui.sh
./scripts/build-patched-tui.sh
./scripts/codex-tui.sh --version
git diff --check
```

验收标准：

- 新增精确回归测试通过；
- `exec_cell` 与 `pager_overlay` 定向测试全部通过；
- clippy、格式、仓库验收脚本和 release 构建通过；
- 官方主聊天与普通 Transcript 的既有 snapshot 没有非预期变化；
- 不产生新的依赖、配置或协议 diff。

完整 `just test -p codex-tui` 应在环境允许时运行。若仍存在已有 wiremock、外部响应服务或终端主题环境失败，必须逐项与修改前基线比较，不能表述为“全套通过”。

### 4.4 iTerm2 人工验收

只在新的专用 iTerm2 窗口运行 `codex-tui`，不得操作用户已有会话：

1. 新建会话，让 Agent 读取某个启用 skill 的 `SKILL.md` 及其 `references/*.md`。
2. 确认 Workspace 只显示文件读取摘要，正文中的独特句子不可见。
3. 同一轮再运行普通 `curl` 或读取项目 `README.md`，确认命令和输出仍完整显示。
4. 等该轮完成后关闭并重新打开 Workspace，确认已提交历史仍保持相同行为。
5. 使用 `codex-tui resume <session-id>` 恢复该会话，确认回放后行为一致。
6. 启动系统官方 `codex`，执行普通读取和 `curl`，确认其显示、快捷键和退出路径没有变化。

人工验收必须记录使用的本地包路径、会话 ID、每项通过/失败以及截图。未执行的项目必须标记为未验收，不能推断为通过。

## 5. 红线

1. **官方行为红线：** 不修改 `ExecCell::add_call` 的分组条件，不改变普通 `display_lines`、普通 `transcript_lines` 的可观察输出，不修改 `curl`、Search、ListFiles、脚本或项目文件的展示。
2. **识别红线：** 不以文件名、命令字符串、`sed`/`cat`、硬编码 `~/.codex`/`~/.agents` 路径识别 skill。唯一可信入口是已经由启用技能精确路径产生的主 `SKILL.md` 标记；引用文件只能通过路径组件确认位于该可信根内。
3. **误隐藏红线：** 无法确认输出完全属于 skill 内容时必须完整显示；但 fail-open 仅限当前 `ExecCall`，不得让整个 `ExecCell` 的其他已确认 skill 调用重新展开。
4. **数据红线：** 不删除、裁剪、改写或重新序列化会话数据和命令输出；只改变 Workspace 渲染结果。
5. **范围红线：** 不修改 `chatwidget/skills.rs`、command lifecycle、app-server、协议、模型上下文、会话恢复格式、快捷键、轮次折叠、固定 Composer、图片预览或 `/exit` 清理。
6. **资源红线：** 不克隆大段命令输出，不为每帧缓存输出副本，不引入全局/static 状态、循环引用或新依赖。
7. **工程红线：** 不顺手格式化或重构相邻官方代码；必要的私有帮助函数提取必须由“普通输出 snapshot 不变”证明等价。
8. **发布红线：** 不覆盖系统 `codex`，不推送 OpenAI upstream，不修改用户配置；是否推送个人 GitHub 由用户另行明确要求。
9. **协作红线：** 未经用户明确批准，不启动 subagent。

## 6. 对抗式方案审查

| 攻击场景 | 方案防线 | 验收证据 |
| --- | --- | --- |
| skill 后读取普通项目文件，普通内容被误隐藏 | 逐 `ExecCall` 判断，路径必须位于可信 skill 根 | mixed exploring group 回归测试 |
| 一个 shell 调用同时读取 skill 和普通文件 | 单调用聚合输出无法拆分，局部 fail-open | mixed subcommand 反例测试 |
| 项目内存在同名 `SKILL.md` | 必须有现有启用技能精确路径标记 | unannotated `SKILL.md` 测试 |
| `/tmp/skill-x` 字符串前缀误匹配 `/tmp/skill` | 使用 `Path::starts_with` 的组件边界 | sibling-prefix 路径测试 |
| skill 主文件隐藏，但 reference 正文仍泄漏 | 可信主文件父目录覆盖同组 descendant read | reference 回归测试 |
| 修复后所有探索命令都变成摘要 | 普通调用继续走提取后的原 transcript helper | 普通 README 与 `curl` snapshot |
| 历史正确、live tail 仍泄漏 | Workspace 的 active cell 与已提交 cell 共用同一方法 | live/completed 两态测试和人工验收 |
| 为修复问题引入每帧大对象复制 | 仅借用 `&[ExecCall]`，不 clone output | 代码审查、clippy、资源红线 |

审查结论：在不改变官方通用渲染和命令分组的前提下，逐 `ExecCall` 的 Workspace 专属策略是能够同时满足“skill 内容不展开”和“普通命令完整显示”的最小修改。若实现需要修改 app-server 协议、命令解析器、官方 `display_lines` 语义或用户配置，应停止并重新出方案，不能扩大范围。

## 7. 实施任务

### Task 1: 增加真实回归测试并完成最小渲染修复

**Files:**

- Modify: `codex-rs/tui/src/exec_cell/model.rs`
- Modify: `codex-rs/tui/src/exec_cell/render.rs`
- Test: `codex-rs/tui/src/exec_cell/render.rs` 的既有 `#[cfg(test)]` 模块

**Interfaces:**

- Consumes: 现有 `ParsedCommand::Read`、`ExecCall`、`ExecCell::is_exploring_call`、精确 skill 名称标记、`HistoryCell` Workspace 渲染入口。
- Produces: 私有的 `annotated_skill_root`、`call_reads_skill_content`、切片版 transcript/exploring 渲染帮助函数；不增加 crate 公共接口。

- [ ] **Step 1: 写入真实失败测试**

  构造同一 `ExecCell` 内连续的主 skill、skill reference 和普通项目文件读取，加入正文存在/不存在断言及 inline insta snapshot。

- [ ] **Step 2: 运行测试并确认修改前失败**

  ```bash
  cd "/Users/chy/projects/codex-tui-patched"
  PATH="/opt/homebrew/opt/rustup/bin:$PATH" just test -p codex-tui workspace_compacts_skill_content_per_call_in_mixed_exploring_group
  ```

  预期：测试失败，渲染结果包含 `PRIVATE MAIN SKILL BODY`。

- [ ] **Step 3: 将 skill 判断改为单次调用粒度**

  在 `model.rs` 中删除整个 cell 的 `any && all` 二选一判断，实现第 3.1 节的 `annotated_skill_root` 与 `call_reads_skill_content`。不得修改官方分组逻辑。

- [ ] **Step 4: 提取等价的调用切片渲染函数**

  在 `render.rs` 中按第 3.2 节提取 transcript/exploring 私有帮助函数；原官方入口继续传入全部 calls，输出必须与原 snapshot 一致。

- [ ] **Step 5: 实现 Workspace 连续分块渲染**

  只在 `workspace_transcript_hyperlink_lines` 中按第 3.3 节选择紧凑 skill 块或完整普通块。不得修改其他 `HistoryCell` 实现。

- [ ] **Step 6: 运行精确测试并审查 snapshot**

  重复 Step 2 命令，预期通过；逐行检查 inline snapshot，确认只移除两个 skill 正文，普通文件正文仍存在。

- [ ] **Step 7: 运行定向和静态验收**

  执行第 4.3 节全部命令。任何普通 display/transcript snapshot 变化都按回归处理，不得直接接受。

- [ ] **Step 8: 完成 iTerm2 人工验收**

  按第 4.4 节验证 live、历史、resume 和普通命令，并记录真实结果；未完成的项目不得标记通过。

- [ ] **Step 9: 提交最小可回滚变更**

  提交前执行：

  ```bash
  git status --short
  git diff
  git diff --cached
  git add "codex-rs/tui/src/exec_cell/model.rs" "codex-rs/tui/src/exec_cell/render.rs"
  git diff --cached
  git commit -m "fix: 隔离 Workspace 中的技能读取输出" -m "变更：按 ExecCall 压缩已确认 skill 目录内的读取，并保留普通命令完整输出。" -m "原因：连续 skill/reference 读取被整组 fail-open，导致技能正文重新展开。" -m "验证：精确回归、exec_cell、pager_overlay、clippy、构建脚本及 iTerm2 验收按方案记录。"
  ```

  只提交上述两个源文件及其中的测试；不得夹带本方案文档、构建产物或其他会话的修改。
