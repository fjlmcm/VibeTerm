# CLAUDE.md

---

## 一句话项目定位
> 一个**现代化、CJK 一等公民的本地优先终端管理器**(Tauri 2 + Rust + SolidJS + xterm.js);为多 AI agent 工作流提供**纯嗅探的 agent 状态(OSC 0/2 标题 spinner + OSC 133/633 + 输出时序)/ Stalled 卡死检测 / 任务紧迫度排序(urgency)**等针对性增强;坚守"**终端是终端**",不做 agent 工作台。**零侵入**——纯嗅探 + 只读文件监听,绝不写 `~/.claude`/`~/.codex`,无 hook、无账号、无遥测、不主动联网。MIT 开源。

---

## 技术栈与架构

**Tauri 2 + Rust(workspace)+ SolidJS + xterm.js(WebglAddon GPU 渲染);pnpm monorepo。** 当前版本 0.3.0,标识 `com.vibeterm.desktop`,macOS 11+。

### Rust 侧(`src-tauri/`)— 主 app + 8 个业务 crate(单向依赖分层)
| crate | 职责 |
|---|---|
| `vibeterm-core` | 领域核心:`TerminalRegistry` + `TaskRegistry`(纯领域,不依赖 Tauri) |
| `vibeterm-pty` | PTY 抽象:每 PTY 独立阻塞读线程,`ChunkSink` 多 sink 订阅(浮窗 attach),256KB scrollback ring |
| `vibeterm-status` | **状态嗅探**:OSC 133/633 解析 + agent stdout 规则(11 个 agent)+ 16KB ring 跨 chunk 正则,5 态判定 |
| `vibeterm-agent-watch` | **只读**监听 transcript/rollout → model / ctx / cost / effort / 额度;`AgentProvider` trait 统一 claude/codex |
| `vibeterm-config` | 配置加载 / 原子写 / 主题 / 热加载(notify 50ms debounce);含 `config_dir()` 安全门(见红线 2) |
| `vibeterm-tasks` | 任务持久化(`tasks.json` 原子写),分屏树 + git worktree 挂载 |
| `vibeterm-ipc` | 跨 Rust/Web IPC schema(`TaskDto` / `SpawnPtyOpts` / `SplitNode` 等)+ 统一 `IpcError` |
| `vibeterm-git` | git worktree CLI 薄封装(L1):`is_git_repo` / `list/add/remove_worktree` / `worktree_status` |

主 app `src-tauri/src/main.rs`:Tauri 入口 + 全部 IPC handler + 200ms 状态 tick + 3s agent 识别轮询。

### Web 侧(`web/packages/`)
| 包 | 职责 |
|---|---|
| `@vibeterm/main` | 根 SolidJS app(`main.tsx`):任务列表(侧栏)+ 工作区(终端网格 / canvas 卡片)+ 浮窗 + 设置 / 命令面板 / prompt picker |
| `@vibeterm/ui-core` | 组件库:`Terminal`(xterm+Webgl)/ `TaskList` / `SplitView`(n 叉分屏树)/ `StatusBar`(widget registry)/ `CanvasViewport` / keybindings / i18n / theme / ipc bridge |
| `@vibeterm/ipc-types` | Rust IPC schema 的 TS 镜像 |
| `e2e` | Playwright(开发 smoke)+ WebdriverIO(Windows 真 Tauri E2E) |

**StatusBar 配置**:`statusline.toml` schema **v2** —— `profiles` 按 `agent_kind`(default / claude / codex)分组,每 profile 一组 `items`(各带 color/bold/max_width)。前端编辑器 `web/packages/main/src/settings-statusline.tsx`(拖拽排序),Rust schema `src-tauri/crates/vibeterm-config/src/statusline.rs`(v1→v2 自动迁移)。

---

## 核心机制:agent 状态纯嗅探(零侵入)

**数据流**:PTY chunk → `StatusDetector::feed()`(`main.rs` 的 `LazyChannelSink::push`,逐 chunk)+ 全局 200ms `tick()` → 状态变更 emit `task_status_changed` / 通知。

**三层嗅探**(`vibeterm-status/src/lib.rs`):
1. **OSC 133/633**(shell 集成,最可靠):`133;C`→Running、`133;D[;code]`→Idle 并 finalize 真完成、`133;A/B`→prompt ready;`633` 取 VSCode cmdline/cwd。
2. **agent stdout 正则**:claude/codex/aider 等 11 个 agent 的授权框文案匹配 → WaitingInput。
3. **OSC 0/2 标题 braille spinner**(U+2800–28FF 在动 = working)→ Running。

**5 个状态(任务列表圆点,`web/packages/ui-core/src/tasklist/index.tsx`)**:
- `Idle` 灰点静止 · `Running` 蓝点常亮+辉光 · `WaitingInput` 黄点 2s 呼吸+强辉光 · `Stalled` 红橙描边环 3s 呼吸 · `Done` 描边环+文字删除线。

**关键阈值**:`IDLE_TIMEOUT=800ms`(无输出→Idle)、`STALL=5min`(静默→Stalled)、OSC 限速 10/s。Stalled **仅对识别出的 agent 终端开启**(纯 idle shell 不误报),且需用户在 agent 最后输出后有过输入。

**只读监听**(`vibeterm-agent-watch`,notify 200ms debounce):`~/.claude/projects/<cwd>/<sid>.jsonl`、`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`、`~/.claude/usage_cache.json`(5h/7d 额度)—— **只读取,从不写入**。

---

## 常用命令

```bash
# 整体(项目根)
pnpm dev          # tauri dev(Vite :1420 热重载)
pnpm build        # tauri build(typecheck + cargo + web bundle + 打包)
pnpm typecheck    # web 子包递归 tsc

# Rust(在 src-tauri/ 下)
cargo build --release
cargo test -p vibeterm-{config,core,ipc,pty,status,tasks}   # 排除 tauri 主包
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

# 验证 / 工具
scripts/smoke-app.sh [--build]   # 隔离启动 .app ~8s,验存活/无 panic/PTY/截图 → .smoke/
scripts/fix-launchpad.sh         # 装包后修 macOS Launchpad/Spotlight 找不到
scripts/build-sounds.py          # ffmpeg 压缩提示音 → src-tauri/resources/sounds/
scripts/update-model-data.py     # 拉 LiteLLM 刷新内嵌模型快照(价格+ctx 窗口);每次发版前必跑
```

CI(`.github/workflows/ci.yml`):lint + cargo test(6 子 crate)+ Playwright + app-smoke + build-smoke。发布(`release.yml`)为 `workflow_dispatch` 手动触发(tag 自动发布已禁用)。

---

## 红线与约束(务必遵守)

1. **零侵入是底线**:agent 状态只能纯嗅探 + 只读监听;**绝不写** `~/.claude`/`~/.codex`。agent hook 层已彻底删除——**不要复活**,不要为"更精确"再引入任何写 agent 配置 / 起 hook server 的方案。改动后用 `grep` 确认无 install 调用、`ps` 确认无多余 server。
2. **配置隔离(踩过坑,排查绕过很久)**:`VIBETERM_CONFIG_DIR` 仅在 **debug 构建**生效,release 无条件落 `~/Library/Application Support/VibeTerm`(防环境变量注入的安全门,在 `vibeterm-config::config_dir()`)。
   - 任何 smoke / 手动启动二进制前先 `export VIBETERM_CONFIG_DIR=/tmp/vt-$$`,否则抢用户真实 `tasks.json`——用户已删任务会"复活"。
   - `cargo test` 里凡用 `TaskRegistry::new()` 的测试,**首行必须** `let _cfg = isolated_config();`(见 `vibeterm-core/tests/flows.rs`),否则污染真实 `tasks.json`。
   - 别 `pkill` 用户正在跑的实例;启动验证前先 `ps` 确认没有用户实例。
3. **`tasks.json` 是 last-writer-wins**(原子写,无跨进程锁)——别起未隔离的并存实例去抢。
4. **CJK 一等公民**:改终端文本 / 复制 / 输入相关代码要顾及 Unicode 15 graphemes、东亚宽度、IME composition 拦截(`isComposing`/keyCode 229)、`Intl.Segmenter` 复制守门(不撕裂代理对 / ZWJ)。
5. **主题 / 配色别擅自改**:内置主题(gruvbox 等)的 ANSI 映射是刻意的;"颜色不对"先排查数据(如关闭任务里的死终端),不要先怀疑配色代码。

---

# 工作方式与规范

## 编码前先思考

**不要想当然。不要掩饰疑惑。把权衡讲清楚。**

在开始实现之前：

- 明确说明你的假设。如果不确定，就提问。
- 如果存在多种理解方式，全部列出来——不要默默替用户做决定。
- 如果有更简单的方案，要直接指出。必要时要敢于反驳。
- 如果某些地方不清晰，先停下来。明确指出哪里有歧义，然后提问。
- 尽可能复用成熟的开源仓库的代码或参考它的逻辑，确认出处，避免重复造轮子。

**不要想当然。不要掩饰疑惑。把权衡讲清楚。**

## 简单优先

**只写解决问题所需的最少代码。不做额外设计。**

- 不为一次性代码做抽象。
- 不为了“灵活性”或“可配置性”提前设计。
- 不为不可能发生的情况写错误处理。
- 如果你写了 200 行，但 50 行就能解决，重写。

时刻问自己：

> “一个资深工程师会觉得这段代码过度设计了吗？”

如果答案是“会”，那就继续简化。

---

## 外科手术式修改

**找到根因，精确修改**

修改已有代码时：

- 遇到不确定的问题不猜测，查询相关文档或自己动手验证（读配置/日志、调API、写脚本复现）。
- 用实际数据定位问题，确认根因后再改代码。
- 不做补丁，不做屎山代码，不考虑向旧版兼容。
- 保持代码、目录整洁，供他人审查。

---

## 目标驱动执行

**定义可验证的成功标准。持续循环直到验证通过。**

把任务转换成可验证目标：

- “增加校验”  
  → “先写非法输入测试，再让测试通过”

- “修复 bug”  
  → “先写能复现问题的测试，再修复并验证”

- “重构 X”  
  → “确保重构前后测试全部通过”

对于多步骤任务，先给出简要计划：

```text
1. [步骤] → 验证：[检查方式]
2. [步骤] → 验证：[检查方式]
3. [步骤] → 验证：[检查方式]
```

强成功标准意味着你可以自主推进。

弱成功标准（例如“让它能工作”）则会导致频繁反复确认。

