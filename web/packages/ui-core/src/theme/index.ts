// 主题应用机制
//   - shell:写 CSS 变量到 :root
//   - terminal:返回一个 xterm options.theme 对象,由 Terminal 组件 apply
//
// 注意:不引入响应式 — 由 consumer 在 theme 变化时调 applyShellTheme(theme)。

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Theme, ThemeTerminal } from "@vibeterm/ipc-types";

const SHELL_VAR_MAP: Record<string, keyof Theme["shell"]> = {
  "--color-bg": "background",
  "--color-surface": "surface",
  "--color-border": "border",
  "--color-text": "text_primary",
  "--color-text-2": "text_secondary",
  "--color-accent": "accent",
  "--color-accent-subtle": "accent_subtle",
  "--color-status-running": "status_running",
  "--color-status-waiting": "status_waiting",
  "--color-status-idle": "status_idle",
};

export function applyShellTheme(theme: Theme) {
  const root = document.documentElement;
  for (const [cssVar, themeKey] of Object.entries(SHELL_VAR_MAP)) {
    root.style.setProperty(cssVar, theme.shell[themeKey]);
  }
  // 启动闪屏修复:缓存 bg + appearance 到 localStorage,下次启动 index.html
  // 内联脚本读出来先上色,跳过"白 → 紫 → 真色"三段闪烁。
  try {
    localStorage.setItem("vibeterm.theme.bg", theme.shell.background);
    localStorage.setItem("vibeterm.theme.appearance", theme.appearance);
  } catch {
    /* private mode / quota — 忽略 */
  }
  // 同步窗口 NSAppearance(macOS)→ traffic lights 切到对应深 / 浅风格;
  // Windows / Linux 不接此 API,catch 吞掉
  try {
    const next = theme.appearance === "dark" ? "dark" : "light";
    void getCurrentWindow().setTheme(next);
  } catch {
    /* not in Tauri context or unsupported — 忽略 */
  }
}

// 把 ThemeTerminal 转成 xterm.js options.theme 字段名(snake → camel)
export function toXtermTheme(t: ThemeTerminal): Record<string, string> {
  return {
    background: t.background,
    foreground: t.foreground,
    cursor: t.cursor,
    selectionBackground: t.selection_bg,
    black: t.black,
    red: t.red,
    green: t.green,
    yellow: t.yellow,
    blue: t.blue,
    magenta: t.magenta,
    cyan: t.cyan,
    white: t.white,
    brightBlack: t.bright_black,
    brightRed: t.bright_red,
    brightGreen: t.bright_green,
    brightYellow: t.bright_yellow,
    brightBlue: t.bright_blue,
    brightMagenta: t.bright_magenta,
    brightCyan: t.bright_cyan,
    brightWhite: t.bright_white,
  };
}
