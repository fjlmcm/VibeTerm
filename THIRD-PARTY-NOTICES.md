# Third-Party Notices

VibeTerm bundles or derives from the following third-party open-source software.

---

## ccusage

VibeTerm's agent usage logic — the 5-hour rolling **block** detection
(`vibeterm-agent-watch/src/claude/blocks.rs`, `.../codex/blocks.rs`), the offline
**pricing / cost** model (`.../claude/pricing.rs`), and the **historical usage
aggregation** behind the Usage Statistics panel (`.../stats/`) — is derived from or
inspired by **ccusage** by ryoppippi.

- Project: https://github.com/ryoppippi/ccusage
- License: MIT

```
MIT License

Copyright (c) 2025 ryoppippi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

VibeTerm does **not** redistribute ccusage's code verbatim. The model price table
ships as a hand-maintained **offline snapshot** (`claude/pricing.rs`) and is used by
default with no network access. The user may *optionally* refresh prices from the
**Settings → Update** page; that, plus the manual app-version check, are the only
network requests VibeTerm makes — see **Network** below.

Model pricing figures originate from Anthropic's public pricing page; ccusage's own
pricing data derives from LiteLLM (BerriAI/litellm, MIT).

---

## ureq

The manual update checks (app version via the GitHub Releases API, and the model
price table) use **ureq** for synchronous HTTPS GET requests.

- Project: https://github.com/algesten/ureq
- License: MIT OR Apache-2.0

---

## Network

VibeTerm makes network requests **only** when the user explicitly clicks a button on
the **Settings → Update** page:

- `https://api.github.com/repos/fjlmcm/VibeTerm/releases/latest` — latest app version
- `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` — model price table (LiteLLM, MIT)

Both are plain `GET`s (only a `User-Agent: VibeTerm` header). No telemetry, no user
data is ever uploaded, there is no background polling and no auto-update/install.
VibeTerm never reads or writes `~/.claude` or `~/.codex`; a refreshed price table is
stored only in VibeTerm's own config directory and can be reset to the built-in
snapshot at any time.

---

## Acknowledgements (inspiration / references)

Several parts of VibeTerm were informed by reading these projects — design and
approach only; no third-party code is redistributed:

- **ccusage** (ryoppippi, MIT) — usage aggregation, pricing, 5-hour block algorithm — https://github.com/ryoppippi/ccusage
- **WezTerm** (wez, MIT) — macOS clipboard file-URL handling — https://github.com/wez/wezterm
- **Tabby** (Eugeny, MIT) — window vibrancy & n-ary split recursion — https://github.com/Eugeny/tabby
- **LiteLLM** (BerriAI, MIT) — model price table (data source for the manual price update) — https://github.com/BerriAI/litellm
- **Prowl** — process-level agent classification
- **CodexBar** — provider fallback-chain design
- **ccstatusline** — status bar widget design
- **panzoom** — canvas pan / zoom algorithm

## Assets

- **JetBrains Mono** — UI / terminal monospace font (SIL Open Font License 1.1)
- **Notification sounds** — sourced from **Pixabay** (Pixabay Content License)
- **Color themes** — Gruvbox, Nord, Tokyo Night, Catppuccin, Solarized and others; ANSI palettes credit their respective authors
