<div align="center">

<img src="docs/hero.png" alt="VibeTerm" width="820">

# VibeTerm

**满屏星火,一目了然。**

专为 vibe coding 打造的现代终端管理器。本地优先,CJK 原生支持,让 agent 各自奔忙,谁燃、谁熄、谁在唤你,尽收眼底 —— 不侵入、不登录、不上云。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**下载**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · **简体中文** · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## 它不抢 agent 的活,只帮你看住它们。

- **不碰你的配置** — 状态全靠「看」出来:读输出、只读监听,从不往 ~/.claude、~/.codex 写东西,不装 hook,也不起后台服务。你的 agent 配置一个字节都不动。
- **管得住一堆 agent** — agent 一多就乱。它把卡住的、等你回话的排到前面,你不用挨个点开看谁该管了。
- **终端还是终端** — 把终端该做的做好,不堆功能,也不想变成什么 agent 工作台。
- **中日韩不出岔子** — 宽字符、输入法、带 emoji 的复制——这些英文终端老踩的坑,这里都处理好了。
- **东西都在你机器上** — 不用登录,不收集数据,默认不联网;只有你手动查更新时才联一下,而且只读不传。
- **MIT 开源** — 代码全公开,随便看、随便改。

## 五种状态,一眼分清。

- 🔵 **运行中** — 蓝点常亮带光晕——agent 在干活。
- 🟡 **等输入** — 黄点在呼吸——它在等你回话,该看一眼了。
- 🔴 **卡住** — 红橙描边环——5 分钟没动静,八成卡了。
- ⚪ **空闲** — 灰点不动——没在忙。
- 🟢 **完成** — 描边环加删除线——这事真干完了。

## 终端该有的都有,外加为一屏 agent 准备的那些。

_普通终端的功能一样不少,再加上专门为满屏 AI agent 做的状态感知和编排。_

### Agent

- **看出 agent 在干嘛** — 在跑、等输入、卡住、跑完——不碰你的配置,直接认出来。
- **卡住检测 + 紧急排序** — 一屏 agent,卡住的和等你的自动排最前。
- **用量实时看** — 上下文用了多少、5h/7d 额度、烧得多快、缓存、花了多少钱,一屏看完。
- **用量统计** — Claude / Codex 的 token 和花费,离线算,能导出。

### 终端

- **分屏 + worktree** — 挂上 git worktree,每个任务一棵自己的终端树。
- **Canvas 画布** — 任务摆成卡片,框选,一条命令发给好几个终端。
- **浮窗** — 把任意任务拽成单独窗口,边跑边盯。
- **GPU 渲染** — WebGL 加速,中日韩照样不丢字、不卡。

### 效率

- **命令面板** — 快捷键和动作都能自己配,键盘一把梭。
- **Prompt 模板** — claude / codex / shell 常用的预设,随手就调。
- **状态栏随便配** — 拖一拖摆 widget,不同 agent 各用各的配置。
- **桌面通知** — 24 个内置音效 + 免打扰时段,只在 agent 状态变了才响。
- **主题热切换** — 10 套内置主题随时换,macOS / Windows 都能用。

## 不碰你的配置,怎么还知道 agent 在干嘛?

三层「看」,加只读监听文件。没有 hook,不用登录,不写任何东西。

1. **OSC 133 / 633 序列** — shell 集成发的命令边界标记——最准的一层,能精确知道命令啥时候开始、结束、在等输入。
2. **认 agent 的输出** — 对 11 个常见 agent 的授权提示做匹配,认出它在「等你拍板」。
3. **标题栏那个转圈** — 窗口标题里的 braille 小转圈在动,就说明 agent 在干活。

> **底线:不碰你的东西** — 从不写 ~/.claude 或 ~/.codex,不装 hook,不起后台服务。所有状态都是「看」来的,不是「插」进去的。

## 英文那几个主流 AI 终端,没一个把中日韩当回事。

几乎每个主流 AI 终端的仓库里,都躺着没修的中日韩 bug,被英文用户的急活盖了过去。这块一直没人好好做——VibeTerm 当正事在做。

- 输入法全程拦着(isComposing / keyCode 229),不误发、不卡
- 全角和模糊宽度量得准,表格不错位
- 中文换行不截断,流式传输也不会把字劈开
- 复制用 Intl.Segmenter 守着,不撕裂代理对和 emoji
- GPU 渲染下中日韩不丢字、不错位

## 试试?

支持 macOS 11+ 与 Windows,同一个包页。

**[下载 →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

或者自己从源码编译:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## 站在这些项目的肩膀上。

特别感谢 ryoppippi 的 ccusage(MIT)——用量统计、模型价格、5 小时块这些都参考了它;价格数据来自 LiteLLM 和 Anthropic 官方。

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
