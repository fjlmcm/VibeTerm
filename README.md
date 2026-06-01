<div align="center">

<img src="docs/hero.png" alt="VibeTerm" width="820">

# VibeTerm

**Sparks everywhere, clear at a glance.**

A modern terminal manager built for vibe coding. Local-first, CJK-native. Let the agents run — who's burning, who's burned out, who's pinging you — all in plain sight. No intrusion, no login, no cloud.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Download**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

**English** · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## It doesn't run your agents for you. It just keeps an eye on them.

- **Never touches your config** — It works out state by sniffing: reading output and read-only file watching. It never writes to ~/.claude or ~/.codex, installs no hooks, runs no background services. Not a byte of your agent config gets touched.
- **Keeps a bunch of agents in check** — Agents get messy once you've got a few. It floats the stuck ones and the ones waiting on you to the top, so you're not clicking through each one to see who needs you.
- **A terminal stays a terminal** — It does the terminal basics well. No feature bloat, no ambition to become an agent workbench.
- **CJK that just works** — Wide characters, IME input, copying text with emoji in it — the things English terminals keep getting wrong. Handled here.
- **Everything stays on your machine** — No login, no data collection, offline by default. It only goes online when you check for updates — and even then, it only reads.
- **MIT, open source** — All the code is public. Read it, change it, do whatever you want with it.

## Five states, clear at a glance.

- 🔵 **Running** — Steady blue dot with a glow. The agent's working.
- 🟡 **Waiting** — Amber dot, breathing — it's waiting on you. Worth a look.
- 🔴 **Stalled** — Red-orange ring. Quiet for over 5 minutes, probably stuck.
- ⚪ **Idle** — Still grey dot. Nothing going on.
- 🟢 **Done** — Outlined ring, struck through. This one's actually finished.

## Everything a terminal should do — plus the agent layer.

_All the usual terminal features, plus state-awareness and orchestration built for a screen full of AI agents._

### Agents

- **Sees what an agent's doing** — Working, waiting, stalled, or done — figured out without touching your config.
- **Stall detection + urgency sort** — Screen full of agents? The stuck ones and the ones waiting on you go to the top.
- **Live usage** — Context left, 5h/7d quota, burn rate, cache, cost — all on one bar.
- **Usage stats** — Token and cost numbers for Claude / Codex. Computed offline, exportable.

### Terminal

- **Splits + worktrees** — Mount a git worktree, one terminal tree per task.
- **Canvas board** — Lay tasks out as cards, drag-select, send one command to several terminals.
- **Floating windows** — Pop any task into its own window and keep watching.
- **GPU rendering** — WebGL-accelerated — and CJK still won't drop glyphs or stutter.

### Workflow

- **Command palette** — Custom keybindings and actions — drive the whole thing from the keyboard.
- **Prompt presets** — Handy presets for claude / codex / shell, a keystroke away.
- **Configurable status bar** — Drag the widgets around; each agent type gets its own layout.
- **Desktop notifications** — 24 built-in sounds + quiet hours, only when an agent's state changes.
- **Hot-swap themes** — 10 built-in themes, switch anytime, macOS and Windows.

## How does it know what an agent's doing without touching your config?

Three layers of sniffing, plus read-only file watching. No hooks, no login, nothing written.

1. **OSC 133 / 633 sequences** — Command boundary markers from shell integration. The most reliable layer: it knows exactly when a command starts, ends, or sits waiting for input.
2. **Reading agent output** — Matches the approval prompts of 11 common agents to tell when one's waiting on you.
3. **That spinner in the title bar** — If the braille spinner in the window title is moving, the agent's working.

> **The one rule: hands off your stuff** — Never writes ~/.claude or ~/.codex, installs no hooks, runs no background services. Every state is watched, never injected.

## Not one of the major English AI terminals takes CJK seriously.

Almost every major AI-terminal repo has CJK bugs sitting open, buried under English users' urgent ones. Nobody's really done this part. VibeTerm treats it as actual work.

- IME composition held the whole way (isComposing / keyCode 229). No misfires, no lag.
- Wide and ambiguous widths measured right, so tables stay lined up.
- Chinese line-wrap doesn't truncate; streaming never splits a glyph.
- Copy is guarded by Intl.Segmenter, so it won't tear surrogate pairs or break ZWJ emoji.
- CJK doesn't drop or shift under GPU rendering.

## Give it a go?

macOS 11+ and Windows — same download page.

**[Download →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Or build it from source:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Standing on these shoulders.

Special thanks to ryoppippi's ccusage (MIT). The usage stats, model pricing, and 5-hour blocks all drew from it; pricing data comes from LiteLLM and Anthropic's official numbers.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
