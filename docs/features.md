# 功能使用说明

VibeTerm 的若干功能靠配置文件 / 命令面板驱动,这里说明用法。全部遵循**零侵入**:纯本地、只读、用户手动触发,绝不写 `~/.claude`/`~/.codex`、无 hook、无遥测。

## 命令面板(⌘ 打开)

| 命令 | 说明 |
|---|---|
| **查看 Diff** | 当前任务 cwd 的三源 git diff:未暂存 / 已暂存 / 对比基准分支。纯只读 `git diff`,超大自动截断。 |
| **布局:<名>** | 应用 `layouts.toml` 里的任务预设(见下)。 |
| **恢复 agent 会话** | 只读读取当前任务 cwd 的最新 agent session id,开一个新 pane 跑 `claude --resume <id>` / `codex resume <id>`。**不自动执行**,由你手动触发。 |

## 布局模板 `layouts.toml`

放在配置目录(`~/Library/Application Support/VibeTerm/layouts.toml`)。编辑即时生效(命令面板每次读盘)。
链式模型:第一个 pane 是根,后续每个把上一个按其 `split` 方向(`h`=右 / `v`=下)劈开。

```toml
[[layouts]]
name = "Dev 三联屏"
keywords = ["dev", "web"]
cwd = "~/proj/web"            # 任务工作目录(可选)

[[layouts.panes]]
command = "npm run dev"      # 终端就绪后自动发送(可选)

[[layouts.panes]]
command = "npm test --watch"
split = "v"                   # 在上一个 pane 下方劈开

[[layouts.panes]]
command = "claude"
split = "h"                   # 在上一个 pane 右侧劈开
cwd = "packages/api"         # 该 pane 单独 cwd(命令前自动 cd)
```

## 事件流 `events.jsonl`

task 状态变更 / agent 完成会 append 到配置目录的 `events.jsonl`(append-only,超限自动截尾),外部脚本可订阅:

```bash
tail -f "~/Library/Application Support/VibeTerm/events.jsonl"
# 每行: {"seq":N,"ts_ms":..,"kind":"status_changed"|"agent_completed","task_id":..,"terminal_id":..,"status":..}
```

## 会话 scrollback 恢复(自动)

各终端的可见缓冲会定期 + 退出前快照,重启后按 task/slot 回放旧历史(旧 shell 进程已不在,纯展示;布局 / cwd 本就持久化)。回放前剥离主题色序列,防换主题后串色。

## 软件更新

设置 → 更新页:**检查更新** + **下载并安装**(校验签名后原地更新并重启)。可勾选**启动时自动检查更新**(默认开;仅比对版本号、只读不上传、零遥测,关闭后开箱完全不主动联网)。若有终端正在运行,安装前会确认以防打断。
