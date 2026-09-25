# Third-Party Notices

VibeTerm bundles or derives from the following third-party open-source software.

---

## ccusage

VibeTerm's agent usage logic — the 5-hour rolling **block** detection
(`vibeterm-agent-watch/src/claude/blocks.rs`, `.../codex/blocks.rs`) — is derived
from or inspired by **ccusage** by ryoppippi.

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

VibeTerm does **not** redistribute ccusage's code verbatim; only the 5-hour block
algorithm was ported. Model context-window sizes are derived from the model id by a
version rule in `claude/models.rs` — no external model table is bundled or fetched.

---

## ureq

App-version checks use **ureq** for synchronous HTTPS GET requests to GitHub.

- Project: https://github.com/algesten/ureq
- License: MIT OR Apache-2.0

---

## Network

VibeTerm checks for software updates from **Settings → Update**, and at startup
when automatic update checks are enabled. Version checks read these endpoints:

- `https://github.com/fjlmcm/VibeTerm/releases/latest/download/latest.json` — latest app version
- `https://api.github.com/repos/fjlmcm/VibeTerm/releases/latest` — release notes when an update is available

These checks use plain `GET`s with a `User-Agent: VibeTerm` header. Downloading and
installing a signed update requires a user action. No telemetry or user data is
uploaded, and there is no background polling or automatic installation.
Agent configuration and session files are only read locally; VibeTerm does not
write to `~/.claude` or `~/.codex`.

---

## Acknowledgements (inspiration / references)

Several parts of VibeTerm were informed by reading these projects — design and
approach only; no third-party code is redistributed:

- **ccusage** (ryoppippi, MIT) — usage aggregation, 5-hour block algorithm — https://github.com/ryoppippi/ccusage
- **WezTerm** (wez, MIT) — macOS clipboard file-URL handling — https://github.com/wez/wezterm
- **Tabby** (Eugeny, MIT) — window vibrancy & n-ary split recursion — https://github.com/Eugeny/tabby
- **Prowl** — process-level agent classification
- **CodexBar** — provider fallback-chain design
- **ccstatusline** — status bar widget design

## Assets

- **JetBrains Mono** — UI / terminal monospace font (SIL Open Font License 1.1)
- **Notification sounds** — sourced from **Pixabay** (Pixabay Content License)
- **Color themes** — Gruvbox, Nord, Tokyo Night, Catppuccin, Solarized and others; ANSI palettes credit their respective authors
