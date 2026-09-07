# 定制 Codex TUI

此分支保留官方 Codex 的 agent、认证、配置和会话格式，只替换 TUI 行为；系统安装的 `codex` 不会被覆盖。

## 位置与启动

镜像仓库位于 `/Users/chy/projects/codex-tui-patched`，定制功能工作树位于：

```sh
/Users/chy/projects/.worktrees/codex-tui-workspace
```

首次启动或源码更新后，构建独立的本地包（其中包含 Code Mode 所需的配套 host）：

```sh
cd "/Users/chy/projects/.worktrees/codex-tui-workspace"
./scripts/build-patched-tui.sh
```

之后始终通过下面的启动器运行；它只执行该工作树生成的包，绝不会覆盖系统的 `codex`：

```sh
"/Users/chy/projects/.worktrees/codex-tui-workspace/scripts/codex-tui.sh"
```

生成包的位置是 `codex-rs/target/codex-tui-package`。不要直接运行 `target/debug/codex`：它不保证携带 Code Mode host。

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
| 鼠标滚轮 / `PageUp` / `PageDown` | 仅滚动历史；输入框保持固定，输入也不跳到底部。工作区打开时鼠标滚轮由 TUI 接管。 |
| `Alt+Up` / `Alt+Down` | 选择上一轮 / 下一轮用户回合。 |
| `Alt+Left` / `Alt+Right` | 折叠 / 展开所选回合。 |
| `Ctrl+T` | 关闭工作区；之后可再次按此键打开。 |

## 本地输入图片

在 iTerm2 3.6 或更新版本中，工作区会显示仍存在于本机的输入图片。PNG 直接通过终端的本地文件引用显示；JPEG、GIF 和 WebP 会先转换为最大 `1024×1024` 的临时 PNG，再用同样的本地文件方式显示。为避免异常图片造成内存峰值，源图超过 1,200 万像素时不生成预览。

转换只在首次进入可见区时发生，状态中只保留路径而不保留图像字节；滚出可见区、折叠回合或关闭工作区时会删除终端图像及临时 PNG。

历史消息中如果只保留了 `[image1]` 而原始附件文件已不存在，终端无法重建图像像素，仍会显示文本占位。这是会话数据本身的限制，而不是 TUI 可安全补回的内容。

## 验收

1. 先构建并启动本地包：

   ```sh
   cd "/Users/chy/projects/.worktrees/codex-tui-workspace"
   ./scripts/build-patched-tui.sh
   ./scripts/codex-tui.sh --version
   ```

   确认 `codex-rs/target/codex-tui-package/bin/` 同时存在可执行的 `codex` 和 `codex-code-mode-host`。
2. 启动一个需要持续执行的任务；在 Composer 输入文本，按 `Ctrl+C`，确认输入被清空且任务仍继续。`Ctrl+U` 的官方含义不变。
3. 用鼠标滚轮或 `PageUp` 浏览旧记录，再输入文字，确认视图位置不跳到最底部，Composer 仍在底部；滚到已加载记录顶部时确认可继续加载更早历史。
4. 使用 `Alt+Up/Down` 选中回合，并用 `Alt+Left/Right` 折叠和展开；确认摘要显示隐藏条目数量。
5. 分别附上一张本地 PNG 和 JPEG 后发送；在 iTerm2 中确认图片出现在对应用户回合。折叠或关闭工作区后，确认图片及 JPEG 的临时转换文件被清理。
6. 运行自动验收：

   ```sh
   cd "/Users/chy/projects/.worktrees/codex-tui-workspace"
   ./scripts/verify-patched-tui.sh
   ```

自动验收覆盖配置解析、键盘/鼠标路由、折叠、图片转换与清理、包内 Code Mode host 和启动器。iTerm2 的固定 Composer、滚动视觉效果与图片像素渲染仍必须按上述步骤人工确认。

## 更新与回滚

本分支的三个功能各自是独立提交，可逐个回滚。远端分支为 `origin/feat/transcript-workspace`；完整上游同步策略见 [`UPSTREAM.md`](UPSTREAM.md)。

跟进官方 Codex 时，先让 `main` 快进到 `upstream/main`，再将本功能分支变基到更新后的 `main`，逐个处理 `codex-rs/tui/` 的冲突并重跑上面的测试。不要覆盖系统安装版，也不要对 `main` 使用 `git reset --hard`。如果某项功能出现问题，优先 `git revert` 对应功能提交，或者直接运行系统的 `codex`。
