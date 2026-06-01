<div align="center">

<img src="docs/hero.png" alt="VibeTerm" width="820">

# VibeTerm

**Des étincelles partout, tout en un coup d'œil.**

Un gestionnaire de terminal moderne pour le vibe coding. Local d'abord, CJK natif. Laissez les agents tourner — lequel s'embrase, lequel s'éteint, lequel vous appelle, tout sous les yeux. Sans intrusion, sans connexion, sans cloud.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Télécharger**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · **Français** · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## Il n'exécute pas vos agents à votre place. Il les garde simplement à l'œil.

- **Ne touche jamais à votre config** — Il déduit l'état en observant : lecture de la sortie, surveillance des fichiers en lecture seule. Il n'écrit jamais dans ~/.claude ou ~/.codex, n'installe aucun hook, ne lance aucun service en arrière-plan. Pas un octet de votre config d'agent n'est touché.
- **Garde une bande d'agents sous contrôle** — Dès que vous en avez plusieurs, ça part vite en vrille. Il fait remonter ceux qui sont bloqués et ceux qui vous attendent, pour ne pas avoir à ouvrir chacun afin de voir qui a besoin de vous.
- **Un terminal reste un terminal** — Il fait bien les bases d'un terminal. Pas de surcharge de fonctions, aucune ambition de devenir un atelier d'agents.
- **Le CJK qui marche, tout simplement** — Caractères larges, saisie IME, copier du texte avec des emojis : ce que les terminaux anglophones ratent sans arrêt, réglé ici.
- **Tout reste sur votre machine** — Sans connexion, sans collecte de données, hors ligne par défaut. Il ne se connecte que lorsque vous vérifiez les mises à jour vous-même, et encore, en lecture seule.
- **MIT, open source** — Tout le code est public. Lisez-le, modifiez-le, comme vous voulez.

## Cinq états, clairs en un coup d'œil.

- 🔵 **En cours** — Point bleu fixe avec halo. L'agent travaille.
- 🟡 **En attente** — Point ambre qui respire. L'agent vous attend — ça vaut un coup d'œil.
- 🔴 **Bloqué** — Anneau rouge-orangé. Silencieux depuis plus de 5 minutes, sans doute bloqué.
- ⚪ **Inactif** — Point gris immobile. Rien ne se passe.
- 🟢 **Terminé** — Anneau contouré, texte barré. Cette tâche est vraiment terminée.

## Tout ce qu'un terminal doit faire, plus le côté agents.

_Toutes les fonctions habituelles d'un terminal, plus la détection d'état et l'orchestration pensées pour un écran rempli d'agents IA._

### Agents

- **Voit ce que fait un agent** — En cours, en attente, bloqué ou terminé — déduit sans toucher à votre config.
- **Détection de blocage + tri par urgence** — Écran rempli d'agents ? Les bloqués et ceux qui vous attendent remontent tout en haut.
- **Usage en direct** — Contexte restant, quota 5h/7d, vitesse de consommation, cache, coût — tout sur une seule barre.
- **Statistiques d'usage** — Chiffres de tokens et de coûts pour Claude / Codex. Calculés hors ligne, exportables.

### Terminal

- **Divisions + worktrees** — Montez un worktree git, un arbre de terminaux par tâche.
- **Tableau Canvas** — Disposez les tâches en cartes, sélection au lasso, une commande envoyée à plusieurs terminaux.
- **Fenêtres flottantes** — Détachez n'importe quelle tâche dans sa propre fenêtre et continuez à la surveiller.
- **Rendu GPU** — Accéléré par WebGL — et le CJK ne perd pas une glyphe, sans ramer pour autant.

### Workflow

- **Palette de commandes** — Raccourcis et actions personnalisables. Tout se pilote au clavier.
- **Modèles de prompts** — Des préréglages pratiques pour claude / codex / shell, à portée de touche.
- **Barre d'état configurable** — Glissez les widgets pour les agencer ; chaque type d'agent a sa propre configuration.
- **Notifications bureau** — 24 sons intégrés + heures calmes, seulement quand l'état d'un agent change.
- **Thèmes interchangeables** — 10 thèmes intégrés, à changer quand vous voulez, macOS et Windows.

## Comment sait-il ce que fait un agent sans toucher à votre config ?

Trois façons d'observer, plus une surveillance de fichiers en lecture seule. Aucun hook, aucune connexion, rien d'écrit.

1. **Séquences OSC 133 / 633** — Marqueurs de limites de commande issus de l'intégration shell. La couche la plus fiable : elle sait exactement quand une commande démarre, se termine ou attend une saisie.
2. **Lecture de la sortie de l'agent** — Compare les invites d'autorisation de 11 agents courants pour repérer quand l'un vous attend.
3. **Ce petit spinner dans le titre de la fenêtre** — Si le spinner braille dans le titre de la fenêtre tourne, l'agent travaille.

> **La seule règle : on ne touche pas à vos affaires** — N'écrit jamais dans ~/.claude ou ~/.codex, n'installe aucun hook, ne lance aucun service en arrière-plan. Chaque état est observé, jamais injecté.

## Aucun des grands terminaux IA anglophones ne prend le CJK au sérieux.

Presque chaque dépôt de grand terminal IA a des bugs CJK ouverts, enfouis sous ceux, urgents, des utilisateurs anglophones. Personne n'a vraiment fait cette partie. VibeTerm la traite comme un vrai travail.

- Composition IME interceptée de bout en bout (isComposing / keyCode 229). Pas de faux envois, pas de lag.
- Largeurs pleines et ambiguës mesurées correctement, les tableaux restent alignés.
- Le retour à la ligne ne tronque pas ; le flux ne coupe jamais une glyphe.
- La copie est protégée par Intl.Segmenter, elle ne casse ni les paires de substitution ni les séquences ZWJ.
- Le CJK ne saute pas et ne se décale pas sous le rendu GPU.

## Envie d'essayer ?

macOS 11+ et Windows, depuis la même page.

**[Télécharger →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Ou compilez depuis les sources:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Sur les épaules de ces projets.

Un grand merci à ccusage de ryoppippi (MIT). Les statistiques d'usage, les tarifs des modèles et les blocs de 5 heures en viennent ; les données de prix proviennent de LiteLLM et des chiffres officiels d'Anthropic.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
