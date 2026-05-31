<div align="center">

# VibeTerm

**满屏星火,一目了然。**

专为 vibe coding 打造的现代终端管理器。本地优先,CJK 原生支持,让 agent 各自奔忙,谁燃、谁熄、谁在唤你,尽收眼底 —— 不侵入、不登录、不上云。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](#技术栈)

[**官网**](https://www.vibeterm.org) · [**下载**](https://github.com/fjlmcm/VibeTerm/releases) · [**CJK Showdown**](docs/CJK_SHOWDOWN.md)

</div>

---

## 它解决什么

agent 一多就乱:满屏终端,哪个在干活、哪个卡住了、哪个在等你回话?

VibeTerm 不抢 agent 的活,只帮你看住它们——把卡住的、等你回话的排到眼前,你不用挨个点开确认谁该管了。它先把终端的本分做好,再顺手替你盯住满屏 agent 的动静。

## 特性

### Agent 感知

- **状态纯嗅探** — 运行 / 等待输入 / 卡死 / 完成,不碰你的配置直接认出来
- **卡死检测 + 紧迫度排序** — 卡住的、等你的自动排到最前
- **实时用量** — 上下文 %、5h/7d 额度、burn rate、cache TTL、成本,一屏看完
- **用量统计** — Claude / Codex 的 token 与成本估算,离线聚合、可导出

### 终端

- **n 叉分屏 + git worktree** — 每个任务一棵独立终端树
- **Canvas 画布** — 任务卡片化、框选、命令广播到多个终端
- **浮窗** — 把任意任务弹成独立窗口
- **GPU 渲染(WebGL)+ CJK 原生** — 流畅且不丢字

### 效率与定制

命令面板 · Prompt 模板库 · 可配状态栏(拖拽 widget) · 桌面通知(24 内置音效 + 免打扰) · 10 套内置主题热切换。

## 零侵入(底线)

agent 状态全靠**纯嗅探 + 只读监听**得来,三层判定:

1. **OSC 133 / 633 序列** — shell 集成的命令边界标记,最可靠
2. **agent 输出规则** — 对 11 个常见 agent 的授权提示做匹配,认出「等你拍板」
3. **OSC 标题 spinner** — 窗口标题里的 braille 在转 = agent 在干活

**绝不**写入 `~/.claude` / `~/.codex`,不装 hook,不起常驻 server。无账号、无遥测、默认不联网(仅你手动检查更新时才联一下,只读不上传)。

## CJK 一等公民

英文那几个主流 AI 终端,没一个把中日韩当回事——几乎每个仓库都躺着长期没修的 CJK issue,被英文用户的急活盖了过去。VibeTerm 把它当正事:

- IME 合成全程拦截(`isComposing` / keyCode 229),不误发、不卡
- 东亚宽字符与 ambiguous width 量得准,表格不错位
- 中文换行不截断,流式 UTF-8 边界不破字
- `Intl.Segmenter` 守门复制,不撕裂代理对与 ZWJ

→ 带 GitHub issue 实锤的竞品对比:[docs/CJK_SHOWDOWN.md](docs/CJK_SHOWDOWN.md)

## 安装

**下载**:[GitHub Releases](https://github.com/fjlmcm/VibeTerm/releases) — macOS(`.dmg`)与 Windows,同一个包页。macOS 11+。

**自行构建**(需 Rust、Node、pnpm):

```bash
pnpm install
pnpm build      # = tauri build,产物在 src-tauri/target/release/bundle/
pnpm dev        # 本地开发(Vite 热重载 + tauri dev)
```

## 技术栈

**Tauri 2 + Rust(workspace,8 业务 crate)+ SolidJS + xterm.js(WebGL GPU 渲染)**,pnpm monorepo。

- Rust 侧 `src-tauri/`:领域核心 / PTY / 状态嗅探 / agent 只读监听 / 配置 / 任务 / IPC / git
- Web 侧 `web/packages/`:`@vibeterm/main`(根 app)· `ui-core`(组件库)· `ipc-types`(IPC schema 镜像)
- 官网 `site/`:Astro 静态站(三语 + 多主题),见 [`site/README.md`](site/README.md)

## 致谢

特别感谢 [ryoppippi/ccusage](https://github.com/ryoppippi/ccusage)(MIT)—— 用量聚合、模型定价、5 小时块逻辑的参考来源;价格数据渊源 [LiteLLM](https://github.com/BerriAI/litellm) 与 Anthropic 官方定价。

也借鉴 / 站在这些项目肩上:[Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm)(portable-pty)· [Tabby](https://github.com/Eugeny/tabby) 等。完整清单见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。

## License

[MIT](LICENSE) © 2026 VibeTerm contributors
