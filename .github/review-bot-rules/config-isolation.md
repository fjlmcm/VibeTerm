# 规则:配置隔离(CRITICAL)

防止测试 / 启动二进制污染用户真实 `tasks.json`(踩过坑,排查很久)。

## 必须标记

- **CRITICAL**:任何用 `TaskRegistry::new()` 的测试,首行未调用 `let _cfg = isolated_config();`
  (参考 `vibeterm-core/tests/flows.rs`)。未隔离会写到用户真实 `tasks.json`,把已删任务"复活"。
- **CRITICAL**:让 `VIBETERM_CONFIG_DIR` 在 **release** 构建生效的改动。该环境变量是防注入的安全门,
  **仅 debug 生效**,release 无条件落 `~/Library/Application Support/VibeTerm`
  (见 `vibeterm-config::config_dir()`)。不得移除这个 `#[cfg(debug_assertions)]` 门。
- **HIGH**:新增 smoke / 手动启动脚本未先 `export VIBETERM_CONFIG_DIR=/tmp/vt-$$` 隔离。

## 背景

`tasks.json` 是 last-writer-wins(原子写,无跨进程锁)。严禁起未隔离的并存实例去抢它。
