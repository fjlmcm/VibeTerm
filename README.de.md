<div align="center">

<img src="docs/hero.png" alt="VibeTerm" width="820">

# VibeTerm

**Überall Funken, alles auf einen Blick.**

Ein moderner Terminal-Manager für Vibe Coding. Local-first, CJK-nativ. Lass die Agents laufen — was brennt, was erloschen ist, was nach dir verlangt, alles im Blick. Kein Eingriff, kein Login, keine Cloud.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Download**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · **Deutsch** · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## Es führt deine Agents nicht für dich aus. Es behält sie nur im Blick.

- **Rührt deine Config nie an** — Den Zustand erkennt es durchs Beobachten: Es liest die Ausgabe und überwacht Dateien nur lesend. Es schreibt nie nach ~/.claude oder ~/.codex, installiert keine Hooks, startet keine Hintergrunddienste. Kein Byte deiner Agent-Config wird angefasst.
- **Behält viele Agents im Griff** — Mit ein paar Agents wird es schnell unübersichtlich. Die hängenden und die, die auf dich warten, kommen nach oben — du musst nicht jeden einzeln öffnen, um zu sehen, wer dich braucht.
- **Ein Terminal bleibt ein Terminal** — Es macht die Terminal-Grundlagen richtig. Kein Feature-Ballast, kein Anspruch, eine Agent-Werkbank zu werden.
- **CJK, das einfach funktioniert** — Breite Zeichen, IME-Eingabe, Kopieren mit Emoji darin — all das, was englische Terminals ständig falsch machen, hier sauber gelöst.
- **Alles bleibt auf deinem Rechner** — Kein Login, keine Datensammlung, standardmäßig offline. Online geht es nur, wenn du selbst nach Updates suchst — und dann liest es nur.
- **MIT, quelloffen** — Der ganze Code liegt offen. Lesen, ändern — wie du willst.

## Fünf Zustände, klar auf einen Blick.

- 🔵 **Läuft** — Stetiger blauer Punkt mit Glühen. Der Agent arbeitet.
- 🟡 **Wartet** — Bernsteinfarbener Punkt, der pulsiert. Er wartet auf dich — ein Blick lohnt sich.
- 🔴 **Hängt** — Rot-oranger Ring. Über 5 Minuten still, wahrscheinlich hängengeblieben.
- ⚪ **Leerlauf** — Stiller grauer Punkt. Nichts los.
- 🟢 **Fertig** — Umrissener Ring, durchgestrichen. Diese Aufgabe ist wirklich fertig.

## Alles, was ein Terminal können sollte, plus das Agent-Zeug.

_Alle üblichen Terminal-Funktionen, dazu Zustandserkennung und Orchestrierung für einen Bildschirm voller KI-Agents._

### Agents

- **Sieht, was ein Agent tut** — Läuft, wartet, hängt oder fertig — erkannt, ohne deine Config anzufassen.
- **Hängt-Erkennung + Dringlichkeitssortierung** — Bildschirm voller Agents? Die hängenden und die, die auf dich warten, kommen nach oben.
- **Live-Verbrauch** — Restkontext, 5h/7d-Kontingent, Burn-Rate, Cache, Kosten — alles in einer Leiste.
- **Verbrauchsstatistik** — Token- und Kostenzahlen für Claude / Codex. Offline berechnet, exportierbar.

### Terminal

- **Splitscreen + Worktrees** — Ein git-Worktree gemountet, ein Terminal-Baum pro Aufgabe.
- **Canvas-Board** — Aufgaben als Karten anordnen, per Rahmen auswählen, einen Befehl an mehrere Terminals schicken.
- **Schwebende Fenster** — Jede Aufgabe in ein eigenes Fenster lösen und weiter im Blick behalten.
- **GPU-Rendering** — WebGL-beschleunigt, und CJK verliert trotzdem keine Glyphen und ruckelt nicht.

### Workflow

- **Befehlspalette** — Eigene Tastenkürzel und Aktionen — alles per Tastatur.
- **Prompt-Vorlagen** — Praktische Vorlagen für claude / codex / shell, sofort per Tastendruck abrufbar.
- **Konfigurierbare Statusleiste** — Widgets per Drag-and-drop anordnen; jeder Agent-Typ bekommt seine eigene Konfiguration.
- **Desktop-Benachrichtigungen** — 24 eingebaute Sounds + Ruhezeiten, nur wenn sich der Zustand eines Agents ändert.
- **Themes live umschalten** — 10 eingebaute Themes, jederzeit wechseln, macOS und Windows.

## Wie weiß es, was ein Agent tut, ohne deine Config anzufassen?

Drei Arten zu beobachten, plus nur-lesende Dateiüberwachung. Keine Hooks, kein Login, nichts geschrieben.

1. **OSC 133 / 633-Sequenzen** — Befehlsgrenzen-Marker aus der Shell-Integration. Die zuverlässigste Schicht: Sie weiß genau, wann ein Befehl startet, endet oder auf Eingabe wartet.
2. **Agent-Ausgabe lesen** — Erkennt an den Bestätigungs-Prompts von 11 gängigen Agents, wann einer auf deine Entscheidung wartet.
3. **Dieser Spinner im Titel** — Wenn sich der Braille-Spinner im Fenstertitel dreht, arbeitet der Agent.

> **Die eine Regel: Finger weg von deinem Zeug** — Schreibt nie nach ~/.claude oder ~/.codex, installiert keinen Hook, startet keinen Hintergrunddienst. Jeder Zustand wird beobachtet, nie eingeschleust.

## Keines der großen englischen KI-Terminals nimmt CJK ernst.

Fast jedes große KI-Terminal-Repo hat offene CJK-Bugs, vergraben unter den dringenden Issues der englischen Nutzer. Diesen Teil hat sich nie jemand richtig vorgenommen. VibeTerm behandelt ihn als echte Arbeit.

- IME-Komposition durchgehend abgefangen (isComposing / keyCode 229). Keine Fehlauslösungen, keine Verzögerung.
- Breite und mehrdeutige Breiten korrekt gemessen, Tabellen bleiben ausgerichtet.
- Zeilenumbruch wird nicht abgeschnitten; Streaming zerlegt keine Glyphe.
- Kopieren ist durch Intl.Segmenter geschützt, zerreißt keine Surrogatpaare und keine ZWJ-Sequenzen.
- CJK fällt beim GPU-Rendering nicht aus und verrutscht nicht.

## Lust, es auszuprobieren?

macOS 11+ und Windows, von derselben Download-Seite.

**[Download →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Oder aus dem Quellcode bauen:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Auf den Schultern dieser Projekte.

Besonderer Dank an ryoppippis ccusage (MIT). Verbrauchsstatistik, Modellpreise und die 5-Stunden-Blöcke stammen daher; die Preisdaten kommen von LiteLLM und Anthropics offiziellen Zahlen.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
