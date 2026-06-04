# 规则:零侵入(CRITICAL)

VibeTerm 的底线:agent 状态只能**纯嗅探 + 只读文件监听**。任何对 agent 配置的写入、
任何 hook、任何常驻 server、任何后台/启动期联网,都越线。

## 必须标记为 CRITICAL(阻止合并)

- 任何写入 `~/.claude`、`~/.codex`、`~/.gemini`、`~/.cursor` 等 agent 配置目录的代码
  (`fs::write` / `OpenOptions::new().write(true)` / `std::fs::create_dir` 指向这些路径;
  `tempfile` + `persist` 到这些路径;追加 settings.json / hooks.json / config.toml)。
- 安装 / 写入 agent hook 的任何逻辑(关键词:`hook_install`、`install_hook`、`settings.json` 写、
  `claude-wrapper`、PATH 前置同名 wrapper、`hooks setup`)。agent hook 层已被彻底删除,**不要复活**。
- 起常驻后台 server(HTTP/socket)用于接收 agent 上报。注意:本仓**没有**任何 agent hook server;
  既有 `tiny_http` 仅历史遗留语境,新代码不得新增监听端口的常驻服务。
- 启动期或定时器里**自动**发起网络请求(后台轮询、遥测、自动检查更新)。

## 允许(不要误报)

- **只读**监听 `~/.claude/projects`、`~/.codex/sessions`、`~/.claude/usage_cache.json`
  (notify watcher / read_to_string)。这是核心机制,合法。
- **仅用户手动点击**时联网:更新检查、模型价格更新、应用内更新下载安装、PR 状态查询(默认关)。
  判据:调用点必须由前端用户操作触发,绝无启动期 / interval / setup 自动调用。
- 落 VibeTerm 自己的配置目录(`~/Library/Application Support/VibeTerm` / `config_dir()`)。

## 审查动作

PR 改动若新增上述 CRITICAL 项,要求作者改为纯嗅探 / 只读;并提醒用 `grep` 确认无 install 调用、
`ps` 确认无多余 server(见 `CLAUDE.md` 红线 1)。
