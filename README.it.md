<div align="center">

<img src="docs/hero.webp" alt="VibeTerm" width="820">

# VibeTerm

**Scintille ovunque, tutto in un colpo d'occhio.**

Un gestore di terminali moderno per il vibe coding. Local-first, CJK nativo. Lascia correre gli agenti — quale brucia, quale si è spento, quale ti chiama, tutto sotto gli occhi. Niente intrusione, niente login, niente cloud.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Scarica**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · **Italiano** · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## Non esegue gli agenti al posto tuo. Li tiene solo d'occhio.

- **Non tocca mai la tua config** — Capisce lo stato osservando: legge l'output e sorveglia i file in sola lettura. Non scrive mai in ~/.claude o ~/.codex, non installa hook, non avvia servizi in background. Nemmeno un byte della config dei tuoi agenti viene toccato.
- **Tiene sotto controllo un mucchio di agenti** — Con qualche agente diventa subito un caos. Porta in cima quelli bloccati e quelli che ti aspettano, così non devi aprirli uno a uno per vedere chi ha bisogno di te.
- **Un terminale resta un terminale** — Fa bene le cose base di un terminale. Niente funzioni di troppo, nessuna ambizione di diventare un banco di lavoro per agenti.
- **CJK che funziona e basta** — Caratteri larghi, input IME, copia con emoji dentro — ciò che i terminali in inglese sbagliano di continuo: qui è tutto risolto.
- **Tutto resta sul tuo computer** — Niente login, niente raccolta dati, offline di default. Va online solo quando cerchi tu gli aggiornamenti, e anche allora legge soltanto.
- **MIT, open source** — Tutto il codice è pubblico. Leggilo, modificalo, come vuoi.

## Cinque stati, chiari in un colpo d'occhio.

- 🔵 **In corso** — Punto blu fisso con bagliore. L'agente sta lavorando.
- 🟡 **In attesa** — Punto ambra che respira. Ti sta aspettando, vale un'occhiata.
- 🔴 **Bloccato** — Anello rosso-arancio. Silenzioso da oltre 5 minuti, probabilmente bloccato.
- ⚪ **Inattivo** — Punto grigio fermo. Non succede niente.
- 🟢 **Fatto** — Anello contornato, barrato. Questa è davvero finita.

## Tutto ciò che un terminale dovrebbe fare, più la parte agenti.

_Tutte le solite funzioni di un terminale, più consapevolezza dello stato e orchestrazione pensate per uno schermo pieno di agenti IA._

### Agenti

- **Vede cosa fa un agente** — In corso, in attesa, bloccato o fatto — capito senza toccare la tua config.
- **Rilevamento blocchi + ordine per urgenza** — Schermo pieno di agenti? Quelli bloccati e quelli che ti aspettano salgono in cima.
- **Uso in tempo reale** — Contesto rimasto, quota 5h/7d, ritmo di consumo, cache, costo — tutto su una barra.
- **Statistiche d'uso** — Numeri di token e costo per Claude / Codex. Calcolati offline, esportabili.

### Terminale

- **Divisioni + worktree** — Monta un worktree git, un albero di terminali per attività.
- **Lavagna Canvas** — Disponi le attività come schede, selezione a riquadro, un comando inviato a più terminali.
- **Finestre fluttuanti** — Stacca qualsiasi attività in una finestra a sé e continua a tenerla d'occhio.
- **Rendering GPU** — Accelerato da WebGL, e il CJK comunque non perde glifi né scatta.

### Workflow

- **Palette dei comandi** — Scorciatoie e azioni personalizzabili. Tutto dalla tastiera.
- **Modelli di prompt** — Preset comodi per claude / codex / shell, a un tasto.
- **Barra di stato configurabile** — Trascina i widget per disporli; ogni tipo di agente ha la sua configurazione.
- **Notifiche desktop** — 24 suoni integrati + ore di silenzio, solo quando cambia lo stato di un agente.
- **Temi al volo** — 10 temi integrati, cambiali quando vuoi, macOS e Windows.

## Come fa a sapere cosa fa un agente senza toccare la tua config?

Tre modi di osservare, più la sorveglianza dei file in sola lettura. Niente hook, niente login, niente di scritto.

1. **Sequenze OSC 133 / 633** — Marcatori di confine dei comandi dall'integrazione della shell. Lo strato più affidabile: sa esattamente quando un comando inizia, finisce o resta in attesa di input.
2. **Leggere l'output dell'agente** — Riconosce i prompt di autorizzazione di 11 agenti comuni per capire quando uno ti aspetta.
3. **Quello spinner nel titolo** — Se lo spinner braille nel titolo della finestra gira, l'agente sta lavorando.

> **La regola: giù le mani dalla tua roba** — Non scrive mai in ~/.claude o ~/.codex, non installa hook, non avvia servizi in background. Ogni stato è osservato, mai iniettato.

## Nessuno dei grandi terminali IA in inglese prende sul serio il CJK.

Quasi ogni repo di un grande terminale IA ha bug CJK aperti, sepolti sotto quelli urgenti degli utenti inglesi. Nessuno ha fatto davvero questa parte. VibeTerm la prende sul serio.

- Composizione IME intercettata fino in fondo (isComposing / keyCode 229). Niente invii errati, niente lag.
- Larghezze piene e ambigue misurate bene, le tabelle restano allineate.
- L'a capo non viene troncato; lo streaming non spezza mai un glifo.
- La copia è protetta da Intl.Segmenter, non spezza coppie surrogate né ZWJ.
- Il CJK non sparisce né si sposta sotto il rendering GPU.

## Lo provi?

macOS 11+ e Windows, dalla stessa pagina.

**[Scarica →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Oppure compila dal sorgente:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Sulle spalle di questi.

Un grazie speciale a ccusage di ryoppippi (MIT). Le statistiche d'uso, i prezzi dei modelli e i blocchi da 5 ore vengono da lì; i dati sui prezzi arrivano da LiteLLM e dai numeri ufficiali di Anthropic.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
