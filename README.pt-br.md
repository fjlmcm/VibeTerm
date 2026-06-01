<div align="center">

<img src="docs/hero.webp" alt="VibeTerm" width="820">

# VibeTerm

**Faíscas por toda parte, tudo num relance.**

Um gerenciador de terminal moderno para vibe coding. Local primeiro, CJK nativo. Deixe os agentes correrem — qual está em chamas, qual apagou, qual está chamando você, tudo à vista. Sem intrusão, sem login, sem nuvem.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Baixar**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · **Português** · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## Ele não roda seus agentes por você. Só fica de olho neles.

- **Nunca toca na sua config** — Descobre o estado observando: lê a saída e monitora arquivos só de leitura. Nunca escreve em ~/.claude ou ~/.codex, não instala hooks, não sobe serviços em segundo plano. Nem um byte da config dos seus agentes é tocado.
- **Mantém um monte de agentes sob controle** — Com alguns agentes já vira bagunça. Ele joga pra cima os travados e os que estão te esperando, então você não precisa abrir cada um pra ver quem precisa de você.
- **Um terminal continua um terminal** — Faz bem o básico de um terminal. Sem inchaço de recursos, sem pretensão de virar uma bancada de agentes.
- **CJK que simplesmente funciona** — Caracteres largos, entrada IME, copiar com emoji no meio — aquilo que os terminais em inglês vivem errando, aqui resolvido.
- **Tudo fica na sua máquina** — Sem login, sem coleta de dados, offline por padrão. Só se conecta à internet quando você mesmo verifica se há atualizações, e mesmo assim só lê.
- **MIT, open source** — Todo o código é público. Leia, mude, o que quiser.

## Cinco estados, claros num relance.

- 🔵 **Rodando** — Ponto azul fixo com brilho. O agente está trabalhando.
- 🟡 **Aguardando** — Ponto âmbar pulsando. Está esperando você, vale um olhar.
- 🔴 **Travado** — Anel vermelho-laranja. Mais de 5 minutos em silêncio, provavelmente travou.
- ⚪ **Ocioso** — Ponto cinza parado. Nada acontecendo.
- 🟢 **Pronto** — Anel contornado, riscado. Essa terminou de verdade.

## Tudo que um terminal deve fazer, mais a parte dos agentes.

_Todos os recursos comuns de terminal, mais percepção de estado e orquestração feitas para uma tela cheia de agentes de IA._

### Agentes

- **Vê o que um agente está fazendo** — Rodando, aguardando, travado ou pronto — descoberto sem tocar na sua config.
- **Detecção de travamento + ordem por urgência** — Tela cheia de agentes? Os travados e os que te esperam sobem pro topo.
- **Uso ao vivo** — Contexto restante, cota 5h/7d, velocidade de consumo, cache, custo — tudo numa barra.
- **Estatísticas de uso** — Números de tokens e custo para Claude / Codex. Calculados offline, exportáveis.

### Terminal

- **Divisões + worktrees** — Monte um git worktree, uma árvore de terminais por tarefa.
- **Quadro Canvas** — Disponha as tarefas como cartões, seleção por moldura, um comando enviado a vários terminais.
- **Janelas flutuantes** — Destaque qualquer tarefa na própria janela e continue de olho.
- **Renderização GPU** — Acelerado por WebGL, e o CJK mesmo assim não perde glifos nem trava.

### Fluxo

- **Paleta de comandos** — Atalhos e ações personalizáveis. Faça tudo pelo teclado.
- **Modelos de prompt** — Predefinições práticas para claude / codex / shell, a uma tecla.
- **Barra de status configurável** — Arraste os widgets pra organizar; cada tipo de agente tem a própria configuração.
- **Notificações na área de trabalho** — 24 sons embutidos + horas de silêncio, só quando o estado de um agente muda.
- **Temas na hora** — 10 temas embutidos, troque quando quiser, macOS e Windows.

## Como ele sabe o que um agente faz sem tocar na sua config?

Três jeitos de observar, mais monitoramento de arquivos só de leitura. Sem hooks, sem login, nada escrito.

1. **Sequências OSC 133 / 633** — Marcadores de limite de comando da integração do shell. A camada mais confiável: sabe exatamente quando um comando começa, termina ou fica esperando entrada.
2. **Ler a saída do agente** — Compara os prompts de autorização de 11 agentes comuns para identificar quando algum deles está esperando você.
3. **Aquele spinner no título** — Se o spinner braille no título da janela está girando, o agente está trabalhando.

> **A regra: não mexe nas suas coisas** — Nunca escreve em ~/.claude ou ~/.codex, não instala hook, não sobe serviço em segundo plano. Todo estado é observado, nunca injetado.

## Nenhum dos grandes terminais de IA em inglês leva o CJK a sério.

Quase todo repositório de grande terminal de IA tem bugs de CJK em aberto, soterrados sob os urgentes dos usuários de inglês. Ninguém fez de verdade essa parte. O VibeTerm trata isso como trabalho de verdade.

- Composição IME interceptada o tempo todo (isComposing / keyCode 229). Sem envios falsos, sem lag.
- Larguras plenas e ambíguas medidas certo, as tabelas ficam alinhadas.
- A quebra de linha não corta; o streaming nunca parte um glifo.
- A cópia é protegida pelo Intl.Segmenter: não quebra surrogate pairs nem ZWJ.
- O CJK não some nem desloca sob renderização GPU.

## Quer testar?

macOS 11+ e Windows, da mesma página.

**[Baixar →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Ou compile a partir do código:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Sobre os ombros destes.

Um agradecimento especial ao ccusage do ryoppippi (MIT). As estatísticas de uso, os preços dos modelos e os blocos de 5 horas vieram dele; os dados de preço vêm do LiteLLM e dos números oficiais da Anthropic.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
