# 借鉴 cmux 新增的功能(使用说明)

本轮从 cmux 对照分析中借鉴、且严格守住零侵入红线的新功能。全部纯本地 / 只读 / 用户手动触发。

## 命令面板新增项(⌘ 打开命令面板)

| 命令 | 说明 |
|---|---|
| **查看 Diff** | 当前任务 cwd 的三源 diff:未暂存 / 已暂存 / 对比基准分支。纯只读 `git diff`,2MB 限额。 |
| **布局:<名>** | 应用 `layouts.toml` 里的任务预设(见下)。 |
| **恢复 agent 会话** | 只读嗅探当前任务 cwd 的最新 agent session_id,开一个新 pane 跑 `claude --resume <id>` / `codex resume <id>`。**不自动执行**,命令由 VibeTerm 内部构造、你手动触发。 |

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

task 状态变更 / agent 完成 会 append 到配置目录的 `events.jsonl`(append-only,超 2MB 启动时截尾)。
外部脚本可订阅:

```bash
tail -f "~/Library/Application Support/VibeTerm/events.jsonl"
# 每行: {"seq":N,"ts_ms":..,"kind":"status_changed"|"agent_completed","task_id":..,"terminal_id":..,"status":..}
```

应用内也可经 IPC `read_events(after_seq)` 做断线游标续传(内存保最近 512 条)。零侵入:只落 VibeTerm 自己目录。

## 会话 scrollback 恢复(自动,无需配置)

退出 / 每 20s,各终端的可见缓冲(最近 1000 行)序列化存 `scrollback.json`(每条上限 256KB)。
重启后按 `taskId:slotId` 回放旧历史(旧 shell 进程已不在,纯展示;布局 / cwd 本就由 `tasks.json` 持久化)。
回放前剥离 OSC 10/11/12 主题色,防换主题后串色。

## 软件自动更新

见 [release-and-updates.md](release-and-updates.md)。设置 → 更新页「下载并安装」:校验 minisign 签名 → 原地更新 → 重启。仍仅用户手动触发。
