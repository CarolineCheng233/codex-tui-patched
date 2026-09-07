# 定制 Codex TUI

此分支保留官方 Codex 的 agent、认证、配置和会话格式，只替换 TUI 行为；系统安装的 `codex` 不会被覆盖。

## 位置与启动

镜像仓库位于 `/Users/chy/projects/codex-tui-patched`，定制功能工作树位于：

```sh
/Users/chy/projects/.worktrees/codex-tui-workspace
```

首次启动或源码更新后，在真实终端中运行：

```sh
cd "/Users/chy/projects/.worktrees/codex-tui-workspace/codex-rs"
just codex
```

已编译的版本可直接启动，不会再次编译：

```sh
"/Users/chy/projects/.worktrees/codex-tui-workspace/codex-rs/target/debug/codex"
```

功能开关已在 `~/.codex/config.toml` 启用：

```toml
[tui]
transcript_workspace = true
```

关闭该开关或使用系统的 `codex`，即可立即回到官方 TUI。

## 交互

开启后会显示 Transcript Workspace：上方是可独立浏览的历史，下方是固定的官方 Composer。

| 操作 | 结果 |
| --- | --- |
| `Ctrl+U` | 保持官方含义：清空当前输入。 |
| `Ctrl+C`（输入非空） | 只清空当前输入，不取消正在执行的任务。 |
| `PageUp` / `PageDown` | 仅滚动历史；输入框保持固定，输入也不跳到底部。 |
| `Alt+Up` / `Alt+Down` | 选择上一轮 / 下一轮用户回合。 |
| `Alt+Left` / `Alt+Right` | 折叠 / 展开所选回合。 |
| `Ctrl+T` | 关闭工作区；之后可再次按此键打开。 |

## 本地输入图片

在 iTerm2 3.6 或更新版本中，工作区会显示仍存在于本机的输入图片。图片通过终端的本地文件引用传输，TUI 不缓存或解码图片字节；滚出可见区、折叠回合或关闭工作区时会删除终端图像。

历史消息中如果只保留了 `[image1]` 而原始附件文件已不存在，终端无法重建图像像素，仍会显示文本占位。这是会话数据本身的限制，而不是 TUI 可安全补回的内容。

## 验收

1. 启动一个需要持续执行的任务；在 Composer 输入文本，按 `Ctrl+C`，确认输入被清空且任务仍继续。
2. 用 `PageUp` 浏览旧记录，再输入文字，确认视图位置不跳到最底部，Composer 仍在底部。
3. 使用 `Alt+Up/Down` 选中回合，并用 `Alt+Left/Right` 折叠和展开；确认摘要显示隐藏条目数量。
4. 附上一张本地 PNG 后发送；在 iTerm2 中确认图片出现在对应用户回合。折叠或关闭工作区后，确认图片被清理。
5. 运行已覆盖的回归测试：

   ```sh
   cd "/Users/chy/projects/.worktrees/codex-tui-workspace/codex-rs"
   just test -p codex-tui workspace_places_local_input_images_in_reserved_user_rows
   just test -p codex-tui local_input_preview_references_the_file_and_skips_unchanged_requests
   ```

## 更新与回滚

本分支的三个功能各自是独立提交，可逐个回滚。远端分支为 `origin/feat/transcript-workspace`；完整上游同步策略见 [`UPSTREAM.md`](UPSTREAM.md)。

跟进官方 Codex 时，先让 `main` 快进到 `upstream/main`，再将本功能分支变基到更新后的 `main`，逐个处理 `codex-rs/tui/` 的冲突并重跑上面的测试。不要覆盖系统安装版，也不要对 `main` 使用 `git reset --hard`。如果某项功能出现问题，优先 `git revert` 对应功能提交，或者直接运行系统的 `codex`。
