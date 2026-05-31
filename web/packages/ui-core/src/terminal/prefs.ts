// 终端外观 / 行为偏好 —— 纯前端 localStorage 持久化。
//
// 零侵入:只写浏览器 localStorage(VibeTerm 自身渲染状态),绝不碰用户
// dotfiles。镜像 terminal/index.tsx 既有 fontSize 的 signal + localStorage 模式。
// 改动经 terminal/index.tsx 里的 createEffect 实时应用到所有已挂载终端。
//
// 默认值 = 改造前的硬编码值,因此不设置任何项时行为零变化。

import { createSignal } from "solid-js";

export type CursorStyle = "block" | "bar" | "underline";

// ---- 默认值(= 改造前硬编码值) ----
const DEFAULT_FONT_FAMILY = "JetBrains Mono, SF Mono, Menlo, Consolas, monospace";
const DEFAULT_LINE_HEIGHT = 1.2;
const DEFAULT_CURSOR_STYLE: CursorStyle = "block"; // xterm.js 原生默认
const DEFAULT_CURSOR_BLINK = true; // = 改造前 cursorBlink: true
const DEFAULT_PAD_X = 6; // = 改造前 padding "4px 6px" 的横向 6
const DEFAULT_PAD_Y = 4; // = 改造前 padding "4px 6px" 的纵向 4
const DEFAULT_CONFIRM_CLOSE = true;

const LINE_HEIGHT_MIN = 1.0;
const LINE_HEIGHT_MAX = 2.0;
const PAD_MAX = 40;
const CURSOR_STYLES: readonly CursorStyle[] = ["block", "bar", "underline"];

const K_FONT_FAMILY = "vibeterm.terminal.fontFamily";
const K_LINE_HEIGHT = "vibeterm.terminal.lineHeight";
const K_CURSOR_STYLE = "vibeterm.terminal.cursorStyle";
const K_CURSOR_BLINK = "vibeterm.terminal.cursorBlink";
const K_PAD_X = "vibeterm.terminal.paddingX";
const K_PAD_Y = "vibeterm.terminal.paddingY";
const K_CONFIRM_CLOSE = "vibeterm.confirmCloseTask";

// ---- localStorage 读写守门(private mode / 损坏值 一律回退默认) ----
function readStr(key: string, def: string): string {
  try {
    const raw = localStorage.getItem(key);
    return raw && raw.trim() ? raw : def;
  } catch {
    return def;
  }
}

function readNum(key: string, def: number, min: number, max: number): number {
  try {
    const raw = localStorage.getItem(key);
    const n = raw != null ? Number(raw) : def;
    if (!Number.isFinite(n) || n < min || n > max) return def;
    return n;
  } catch {
    return def;
  }
}

function readBool(key: string, def: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return def;
    return raw === "true";
  } catch {
    return def;
  }
}

function readCursorStyle(): CursorStyle {
  const raw = readStr(K_CURSOR_STYLE, DEFAULT_CURSOR_STYLE);
  return CURSOR_STYLES.includes(raw as CursorStyle) ? (raw as CursorStyle) : DEFAULT_CURSOR_STYLE;
}

function write(key: string, val: string) {
  try {
    localStorage.setItem(key, val);
  } catch {
    /* private mode — 忽略 */
  }
}

// ---- signals(全局共享,改一处所有终端联动) ----
const [fontFamily, setFontFamilySig] = createSignal<string>(readStr(K_FONT_FAMILY, DEFAULT_FONT_FAMILY));
const [lineHeight, setLineHeightSig] = createSignal<number>(
  readNum(K_LINE_HEIGHT, DEFAULT_LINE_HEIGHT, LINE_HEIGHT_MIN, LINE_HEIGHT_MAX),
);
const [cursorStyle, setCursorStyleSig] = createSignal<CursorStyle>(readCursorStyle());
const [cursorBlink, setCursorBlinkSig] = createSignal<boolean>(readBool(K_CURSOR_BLINK, DEFAULT_CURSOR_BLINK));
const [paddingX, setPaddingXSig] = createSignal<number>(readNum(K_PAD_X, DEFAULT_PAD_X, 0, PAD_MAX));
const [paddingY, setPaddingYSig] = createSignal<number>(readNum(K_PAD_Y, DEFAULT_PAD_Y, 0, PAD_MAX));
const [confirmCloseTask, setConfirmCloseTaskSig] = createSignal<boolean>(
  readBool(K_CONFIRM_CLOSE, DEFAULT_CONFIRM_CLOSE),
);

// ---- 只读 accessor(供终端组件 createEffect 订阅) ----
export const terminalFontFamily = fontFamily;
export const terminalLineHeight = lineHeight;
export const terminalCursorStyle = cursorStyle;
export const terminalCursorBlink = cursorBlink;
export const terminalPaddingX = paddingX;
export const terminalPaddingY = paddingY;
export const shouldConfirmCloseTask = confirmCloseTask;

// ---- setter(写 signal + 持久化) ----
export function setTerminalFontFamily(v: string) {
  const next = v.trim() || DEFAULT_FONT_FAMILY;
  setFontFamilySig(next);
  write(K_FONT_FAMILY, next);
}

export function setTerminalLineHeight(v: number) {
  const next = Math.max(LINE_HEIGHT_MIN, Math.min(LINE_HEIGHT_MAX, v));
  setLineHeightSig(next);
  write(K_LINE_HEIGHT, String(next));
}

export function setTerminalCursorStyle(v: CursorStyle) {
  setCursorStyleSig(v);
  write(K_CURSOR_STYLE, v);
}

export function setTerminalCursorBlink(v: boolean) {
  setCursorBlinkSig(v);
  write(K_CURSOR_BLINK, String(v));
}

export function setTerminalPaddingX(v: number) {
  const next = Math.max(0, Math.min(PAD_MAX, Math.round(v)));
  setPaddingXSig(next);
  write(K_PAD_X, String(next));
}

export function setTerminalPaddingY(v: number) {
  const next = Math.max(0, Math.min(PAD_MAX, Math.round(v)));
  setPaddingYSig(next);
  write(K_PAD_Y, String(next));
}

export function setConfirmCloseTask(v: boolean) {
  setConfirmCloseTaskSig(v);
  write(K_CONFIRM_CLOSE, String(v));
}

// ---- 恢复默认(供设置面板 Reset) ----
export function resetTerminalPrefs() {
  setTerminalFontFamily(DEFAULT_FONT_FAMILY);
  setTerminalLineHeight(DEFAULT_LINE_HEIGHT);
  setTerminalCursorStyle(DEFAULT_CURSOR_STYLE);
  setTerminalCursorBlink(DEFAULT_CURSOR_BLINK);
  setTerminalPaddingX(DEFAULT_PAD_X);
  setTerminalPaddingY(DEFAULT_PAD_Y);
}

export const TERMINAL_PREF_DEFAULTS = {
  fontFamily: DEFAULT_FONT_FAMILY,
  lineHeight: DEFAULT_LINE_HEIGHT,
  cursorStyle: DEFAULT_CURSOR_STYLE,
  cursorBlink: DEFAULT_CURSOR_BLINK,
  paddingX: DEFAULT_PAD_X,
  paddingY: DEFAULT_PAD_Y,
} as const;
