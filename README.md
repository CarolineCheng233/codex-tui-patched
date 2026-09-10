# Codex TUI Workspace

> A private downstream of [OpenAI Codex](https://github.com/openai/codex) that makes long-running terminal conversations easier to read, navigate, and continue writing in.
>
> 基于 [OpenAI Codex](https://github.com/openai/codex) 的私有下游仓库，为长时间终端对话提供更易读、更易浏览且可持续输入的 Workspace 体验。

## Why this repository exists

Codex's normal chat presentation is intentionally compact: reads, searches, and commands communicate progress without turning the conversation into a wall of raw terminal output. A transcript viewer, meanwhile, needs the complete record. This repository combines those two needs in one terminal workflow:

- keep the **official normal chat presentation** as the default Workspace view;
- keep a **fixed Composer** at the bottom while history remains independently scrollable;
- let you open the **full transcript on demand**, without losing the draft or the current place in history;
- make older turns practical to navigate, select, fold, and revisit.

Codex 的正常聊天界面会以紧凑形式呈现读取、搜索和命令进度，而完整 transcript 则适合查看原始记录。本仓库将两种需求组合到同一终端工作流中：

- Workspace 默认复用**官方正常聊天展示**；
- Composer 固定在底部，历史可独立滚动；
- 按需打开**完整 transcript**，同时保留草稿和当前历史位置；
- 便于定位、选择、折叠和回看旧轮次。

## What it changes

| Capability | Behavior |
| --- | --- |
| Official-style presentation | Read, list, search, and command cells use the same compact display path as normal Codex chat. Full raw output remains available in transcript details. |
| Fixed Composer | The input area stays at the bottom while you scroll through history and continue composing. |
| Transcript Workspace | `PageUp`/`PageDown` and the mouse wheel browse history without consuming ordinary text input. |
| Turn navigation | `Alt+Up` / `Alt+Down` select a user turn; `Alt+Left` / `Alt+Right` fold or expand it. |
| Detail round trip | In Workspace, `Ctrl+T` opens the complete transcript. Closing the detail view returns to the same Workspace context. |
| Persistent history | Resumed and paginated command and web-search history use the normal display path while retaining detail data. |
| Local image previews | Workspace preserves the existing local input-image preview behavior supported by the TUI. |

| 能力 | 行为 |
| --- | --- |
| 官方风格展示 | Read、List、Search 和命令 cell 默认使用与 Codex 正常聊天一致的紧凑展示；完整原始输出仍可在详情中查看。 |
| 固定 Composer | 浏览历史或查看旧轮次时，输入区始终固定在终端底部。 |
| Transcript Workspace | `PageUp` / `PageDown` 与鼠标滚轮浏览历史，不会吞掉普通文本输入。 |
| 轮次导航 | `Alt+Up` / `Alt+Down` 选择用户轮次；`Alt+Left` / `Alt+Right` 折叠或展开。 |
| 详情往返 | 在 Workspace 中按 `Ctrl+T` 打开完整 transcript；关闭详情后回到原来的 Workspace 上下文。 |
| 持久化历史 | 恢复会话和分页载入的命令、网页搜索历史保持正常展示，并保留详情所需的数据。 |
| 本地图片预览 | Workspace 保留 TUI 已有的本地输入图片预览能力。 |

## Quick start

Build a local package from this checkout, then run its launcher:

```sh
cd "/Users/chy/projects/codex-tui-patched"
./scripts/build-patched-tui.sh
./scripts/codex-tui.sh
```

To enable the Workspace presentation, add the following to `~/.codex/config.toml`:

```toml
[tui]
transcript_workspace = true
```

The local package lives in `codex-rs/target/codex-tui-package`. It is separate from the system-installed `codex`; use the system binary, or disable `transcript_workspace`, to return to the stock TUI.

从此工作树构建本地包，然后运行启动器：

```sh
cd "/Users/chy/projects/codex-tui-patched"
./scripts/build-patched-tui.sh
./scripts/codex-tui.sh
```

要启用 Workspace，请在 `~/.codex/config.toml` 中加入：

```toml
[tui]
transcript_workspace = true
```

本地包位于 `codex-rs/target/codex-tui-package`，与系统安装的 `codex` 完全分离。运行系统 `codex` 或关闭 `transcript_workspace`，即可回到官方 TUI。

## How to use Workspace

1. Open the transcript in Codex as usual.
2. Read, search, and command activity stays compact in the Workspace view.
3. Scroll history with the mouse wheel or `PageUp` / `PageDown`; keep writing in the Composer at any time.
4. Use `Alt+Up` / `Alt+Down` to choose a turn, then `Alt+Left` / `Alt+Right` to fold or expand it.
5. Press `Ctrl+T` when you need the complete transcript, then close the detail view to return to Workspace.

1. 按 Codex 原有方式打开 transcript。
2. Workspace 默认以紧凑形式展示读取、搜索和命令活动。
3. 用鼠标滚轮或 `PageUp` / `PageDown` 浏览历史，同时可继续在 Composer 中输入。
4. 使用 `Alt+Up` / `Alt+Down` 选择轮次，再用 `Alt+Left` / `Alt+Right` 折叠或展开。
5. 需要原始完整记录时按 `Ctrl+T`，关闭详情后即可回到 Workspace。

## Downstream relationship

This repository keeps Codex's agent runtime, authentication, configuration, protocol, and session formats aligned with upstream whenever possible. Its local changes focus on `codex-rs/tui/`; the system-installed Codex remains untouched as an immediate fallback.

本仓库尽可能保持 Codex 的 agent runtime、认证、配置、协议和会话格式与上游一致。本地改动主要集中在 `codex-rs/tui/`，系统安装的 Codex 不会被修改，可随时作为回退方案。

- [Upstream maintenance policy / 上游维护策略](UPSTREAM.md)
- [Workspace implementation plan / Workspace 实施方案](docs/superpowers/plans/2026-09-09-official-chat-workspace-parity-plan.md)
- [Codex upstream repository / Codex 上游仓库](https://github.com/openai/codex)

## License

This repository retains the upstream [Apache-2.0 License](LICENSE).

本仓库沿用上游的 [Apache-2.0 License](LICENSE)。
