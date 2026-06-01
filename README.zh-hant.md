<div align="center">

<img src="docs/hero.png" alt="VibeTerm" width="820">

# VibeTerm

**滿螢幕星火,一目了然。**

專為 vibe coding 打造的現代終端機。本機優先,CJK 原生支援,讓 agent 各自奔忙,誰燃、誰熄、誰在喚你,盡收眼底 —— 不侵入、不登入、不上雲。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**下載**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · **繁體中文** · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## 它不搶 agent 的活,只幫你盯住它們。

- **不碰你的設定** — 狀態全靠「看」出來:讀輸出、唯讀監聽,從不往 ~/.claude、~/.codex 寫東西,不裝 hook,也不起常駐服務。你的 agent 設定一個位元組都不動。
- **管得住一堆 agent** — agent 一多就亂。它把卡住的、等你回話的排到前面,你不用一個個點開看誰該管了。
- **終端機還是終端機** — 把終端機該做的做好,不堆功能,也不想變成什麼 agent 工作台。
- **中日韓不出岔子** — 全形字、輸入法、帶 emoji 的複製 —— 這些英文終端機老踩的坑,這裡都處理好了。
- **東西都在你機器上** — 不用登入,不蒐集資料,預設不連網;只有你手動查更新時才連一下,而且唯讀不傳。
- **MIT 開源** — 程式碼全公開,隨便看、隨便改。

## 五種狀態,一眼分清。

- 🔵 **執行中** — 藍點常亮帶光暈 —— agent 在幹活。
- 🟡 **等輸入** — 黃點在呼吸 —— 它在等你回話,該看一眼了。
- 🔴 **卡住** — 紅橙描邊環 —— 5 分鐘沒動靜,八成卡了。
- ⚪ **閒置** — 灰點不動 —— 沒在忙。
- 🟢 **完成** — 描邊環加刪除線 —— 這事真幹完了。

## 終端機該有的都有,外加為一螢幕 agent 準備的那些。

_一般終端機的功能一樣不少,再加上專為滿螢幕 AI agent 做的狀態感知與編排。_

### Agent

- **看出 agent 在幹嘛** — 在跑、等輸入、卡住、跑完 —— 不碰你的設定,直接認出來。
- **卡住偵測 + 緊急排序** — 一螢幕 agent,卡住的和等你的自動排最前。
- **用量即時看** — 上下文用了多少、5h/7d 額度、燒得多快、快取、花了多少錢,一螢幕看完。
- **用量統計** — Claude / Codex 的 token 與花費,離線算,能匯出。

### 終端機

- **分割 + worktree** — 掛上 git worktree,每個任務一棵自己的終端機樹。
- **Canvas 畫布** — 任務擺成卡片,框選,一條指令發給好幾個終端機。
- **浮動視窗** — 把任意任務拖成獨立視窗,邊跑邊盯。
- **GPU 渲染** — WebGL 加速,中日韓照樣不掉字、不卡。

### 效率

- **命令面板** — 快捷鍵和動作都能自己設,鍵盤全包了。
- **Prompt 範本** — claude / codex / shell 常用的預設,隨手就叫。
- **狀態列隨便配** — 拖一拖擺 widget,不同 agent 各用各的設定。
- **桌面通知** — 24 個內建音效 + 勿擾時段,只在 agent 狀態變了才響。
- **主題熱切換** — 10 套內建主題隨時換,macOS / Windows 都能用。

## 不碰你的設定,怎麼還知道 agent 在幹嘛?

三層「看」,加唯讀監聽檔案。沒有 hook,不用登入,不寫任何東西。

1. **OSC 133 / 633 序列** — shell 整合發的指令邊界標記 —— 最準的一層,能精確知道指令何時開始、結束、在等輸入。
2. **認 agent 的輸出** — 對 11 個常見 agent 的授權提示做比對,認出它在「等你拍板」。
3. **標題列那個轉圈** — 視窗標題裡的 braille 小轉圈在動,就代表 agent 在幹活。

> **底線:不碰你的東西** — 從不寫 ~/.claude 或 ~/.codex,不裝 hook,不起常駐服務。所有狀態都是「看」來的,不是「插」進去的。

## 英文那幾個主流 AI 終端機,沒一個把中日韓當回事。

幾乎每個主流 AI 終端機的儲存庫裡,都躺著沒修的中日韓 bug,被英文使用者的急事蓋了過去。這塊一直沒人好好做 —— VibeTerm 當正事在做。

- 輸入法全程攔著(isComposing / keyCode 229),不誤送、不卡
- 全形和模糊寬度量得準,表格不錯位
- 中文換行不截斷,串流傳輸也不會把字劈開
- 複製用 Intl.Segmenter 守著,不撕裂代理對和 emoji
- GPU 渲染下中日韓不掉字、不錯位

## 試試?

支援 macOS 11+ 與 Windows,同一個下載頁。

**[下載 →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

或者自己從原始碼編譯:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## 站在這些專案的肩膀上。

特別感謝 ryoppippi 的 ccusage(MIT)—— 用量統計、模型價格、5 小時區塊這些都參考了它;價格資料來自 LiteLLM 和 Anthropic 官方。

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
