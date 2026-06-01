<div align="center">

<img src="docs/hero.webp" alt="VibeTerm" width="820">

# VibeTerm

**Chispas por todas partes, todo de un vistazo.**

Un gestor de terminal moderno para vibe coding. Local primero, CJK nativo. Deja que los agentes corran — cuál arde, cuál se apagó, cuál te llama, todo a la vista. Sin intrusión, sin inicio de sesión, sin nube.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Descargar**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · **Español** · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## No ejecuta tus agentes por ti. Solo los mantiene vigilados.

- **Nunca toca tu config** — Averigua el estado observando: lee la salida y vigila archivos en solo lectura. Nunca escribe en ~/.claude ni ~/.codex, no instala hooks, no levanta servicios en segundo plano. Ni un byte de la config de tus agentes se toca.
- **Tiene controlados a un montón de agentes** — Con unos cuantos agentes la cosa se vuelve un lío. Pone al principio los atascados y los que te esperan, así no tienes que abrir cada uno para ver quién te necesita.
- **Una terminal sigue siendo una terminal** — Hace bien lo básico de una terminal. Sin funciones de relleno, sin ánimo de volverse un banco de trabajo de agentes.
- **CJK que simplemente funciona** — Caracteres anchos, entrada IME, copiar con emoji dentro — en lo que las terminales en inglés fallan una y otra vez, aquí resuelto.
- **Todo se queda en tu equipo** — Sin inicio de sesión, sin recopilar datos, sin conexión por defecto. Solo se conecta cuando buscas actualizaciones tú mismo, y aun así solo lee.
- **MIT, código abierto** — Todo el código es público. Léelo, cámbialo, lo que quieras.

## Cinco estados, claros de un vistazo.

- 🔵 **En curso** — Punto azul fijo con brillo. El agente trabaja.
- 🟡 **Esperando** — Punto ámbar que respira. Te espera, vale un vistazo.
- 🔴 **Atascado** — Anillo rojo-naranja. Más de 5 minutos en silencio, seguramente atascado.
- ⚪ **Inactivo** — Punto gris quieto. No pasa nada.
- 🟢 **Listo** — Anillo perfilado, tachado. Esta sí terminó de verdad.

## Todo lo que una terminal debería hacer, más lo de los agentes.

_Todas las funciones habituales de una terminal, más detección de estado y orquestación pensadas para una pantalla llena de agentes de IA._

### Agentes

- **Ve qué hace un agente** — En curso, esperando, atascado o listo — averiguado sin tocar tu config.
- **Detección de atascos + orden por urgencia** — ¿Pantalla llena de agentes? Los atascados y los que te esperan se van arriba.
- **Uso en vivo** — Contexto restante, cuota 5h/7d, ritmo de gasto, caché, coste — todo en una barra.
- **Estadísticas de uso** — Cifras de tokens y coste para Claude / Codex. Calculadas sin conexión, exportables.

### Terminal

- **Divisiones + worktrees** — Monta un worktree de git, un árbol de terminales por tarea.
- **Tablero Canvas** — Coloca las tareas como tarjetas, selección por marco, un comando enviado a varias terminales.
- **Ventanas flotantes** — Saca cualquier tarea a su propia ventana y síguela vigilando.
- **Renderizado GPU** — Acelerado por WebGL, y el CJK no pierde glifos ni se traba.

### Flujo

- **Paleta de comandos** — Atajos y acciones a tu medida. Todo con el teclado.
- **Plantillas de prompts** — Ajustes prácticos para claude / codex / shell, a una tecla.
- **Barra de estado configurable** — Arrastra los widgets para ordenarlos; cada tipo de agente tiene su propia configuración.
- **Notificaciones de escritorio** — 24 sonidos integrados + horas tranquilas, solo cuando cambia el estado de un agente.
- **Temas al instante** — 10 temas integrados, cámbialos cuando quieras, macOS y Windows.

## ¿Cómo sabe qué hace un agente sin tocar tu config?

Tres formas de observar, más vigilancia de archivos en solo lectura. Sin hooks, sin inicio de sesión, sin escribir nada.

1. **Secuencias OSC 133 / 633** — Marcadores de límite de comando de la integración del shell. La capa más fiable: sabe exactamente cuándo un comando empieza, termina o espera entrada.
2. **Leer la salida del agente** — Coteja los avisos de autorización de 11 agentes comunes para detectar cuándo uno te espera.
3. **Ese spinner del título** — Si el spinner braille del título de la ventana gira, el agente está trabajando.

> **La regla: no se toca lo tuyo** — Nunca escribe en ~/.claude ni ~/.codex, no instala ningún hook, no levanta ningún servicio en segundo plano. Cada estado se observa, nunca se inyecta.

## Ninguna de las grandes terminales de IA en inglés se toma el CJK en serio.

Casi todo repo de una gran terminal de IA tiene bugs de CJK abiertos, enterrados bajo los asuntos urgentes de los usuarios en inglés. Nadie ha hecho de verdad esta parte. VibeTerm la trata como trabajo real.

- Composición IME interceptada de principio a fin (isComposing / keyCode 229). Sin envíos falsos, sin lag.
- Anchos completos y ambiguos medidos bien, las tablas quedan alineadas.
- El ajuste de línea no se corta; el streaming nunca parte un glifo.
- La copia está protegida por Intl.Segmenter: no parte pares sustitutos ni secuencias ZWJ de emoji.
- El CJK no se cae ni se desplaza bajo el renderizado GPU.

## ¿Lo probamos?

macOS 11+ y Windows, en la misma página de descarga.

**[Descargar →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

O compílalo desde el código:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Sobre los hombros de estos.

Un agradecimiento especial a ccusage de ryoppippi (MIT). Las estadísticas de uso, los precios de modelos y los bloques de 5 horas vienen de ahí; los datos de precios provienen de LiteLLM y las cifras oficiales de Anthropic.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
