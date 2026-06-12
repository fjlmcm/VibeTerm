// 全局 keybindings store + dispatcher.
//
// 之前 keybindings.toml 是个"展示性 manifest" — 20 项里只有 4 项真生效,
// 其他 16 项要么硬编码在 terminal/main 不可改, 要么完全没接通.
// 此模块把 keybindings.toml 升级为唯一权威 source:
//   - 启动时拉一次 + 监听 keybindings_changed 事件 hot reload
//   - matchChord / matchDoubleTap 工具
//   - createKeybindingDispatcher 工厂: caller 注册 actions = { cmd: handler },
//     dispatcher 是一个 keydown listener, 拿到 event → 查当前 keybindings →
//     match 命中就调 handler.
//
// 设计原则:
//   - 全局命令 (command_palette / new_task / open_settings 等) 在 main.tsx 注册
//   - per-terminal 命令 (find_in_terminal / font_size_up 等) 在 Terminal 组件注册
//   - 同一个 KeybindingsFile 被多个 dispatcher 复用,每个只看自己关心的 commands

import { createSignal } from "solid-js";
import { getKeybindings, onKeybindingsChanged } from "../ipc";
import type { KeybindingsFile } from "@vibeterm/ipc-types";

const [keybindings, setKeybindings] = createSignal<KeybindingsFile | null>(null);

export { keybindings };

let initStarted = false;
let unlisten: (() => void) | null = null;

/** 拉一次 keybindings + 监听 keybindings_changed 自动 reload. 幂等. */
export async function initKeybindings(): Promise<void> {
  if (initStarted) return;
  initStarted = true;
  try {
    setKeybindings(await getKeybindings());
  } catch (e) {
    console.error("[keybindings] initial load failed", e);
  }
  try {
    unlisten = await onKeybindingsChanged(async () => {
      try {
        setKeybindings(await getKeybindings());
      } catch (e) {
        console.error("[keybindings] reload failed", e);
      }
    });
  } catch (e) {
    console.error("[keybindings] listener failed", e);
  }
}

export function disposeKeybindings(): void {
  unlisten?.();
  unlisten = null;
  initStarted = false;
}

/** 给定 command 名取当前绑定 keys; 未加载/未绑定返回 undefined. */
export function keysFor(command: string): string | undefined {
  return keybindings()?.bindings.find((b) => b.command === command)?.keys;
}

// 平台检测统一出口:用 navigator.userAgent(navigator.platform 已废弃)。
// titlebar / settings 等处不要自行复制检测逻辑,从这里 import。
export const isMacPlatform = () =>
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPod|iPad/.test(navigator.userAgent);

/** 快捷键提示文案用的修饰键名:macOS → Cmd,其它平台 → Ctrl。 */
export const modKeyLabel = () => (isMacPlatform() ? "Cmd" : "Ctrl");

/** Windows 平台检测(shell quoting / 路径拼接等按平台分支用)。 */
export const isWindowsPlatform = () =>
  typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent);

/**
 * 匹配 chord 字符串 (e.g. "Mod+Shift+P").
 *   - "Mod" 在 macOS 解释为 Cmd, 其他平台为 Ctrl
 *   - "Control" / "Ctrl" 强制 Ctrl, "Meta" 强制 Cmd
 *   - 大小写不敏感
 *   - 仅 keydown 阶段调用, e.repeat 由 caller 过滤
 */
export function matchChord(e: KeyboardEvent, chord: string): boolean {
  const parts = chord.split("+").map((s) => s.trim());
  if (parts.length === 0) return false;
  const isMac = isMacPlatform();
  const keyToken = parts.pop();
  if (!keyToken) return false;
  const wantMod = parts.includes("Mod");
  const wantShift = parts.includes("Shift");
  const wantAlt = parts.includes("Alt");
  const wantCtrl = parts.includes("Control") || parts.includes("Ctrl");
  const wantMeta = parts.includes("Meta");

  // 修饰键匹配
  const modOk = wantMod ? (isMac ? e.metaKey : e.ctrlKey) : true;
  if (!modOk) return false;
  if (e.shiftKey !== wantShift) return false;
  if (e.altKey !== wantAlt) return false;
  // 显式 Control / Meta 处理
  if (wantCtrl && !e.ctrlKey) return false;
  if (wantMeta && !e.metaKey) return false;
  // 没要求 Mod / Control 时, 别让 ctrl 误命中
  if (!wantMod && !wantCtrl && isMac && e.ctrlKey) return false;
  if (!wantMod && !wantMeta && !isMac && e.metaKey) return false;
  // macOS 上 Mod 绑定 (Cmd) 不应被多余的 Ctrl 误命中 — 让 Ctrl 组合键透传给 PTY
  if (wantMod && !wantCtrl && isMac && e.ctrlKey) return false;

  // 键名匹配 — 单字符大小写不敏感, F1-F12 / Arrow* / End / Home 等按 key 名比较
  const k = keyToken.toLowerCase();
  return e.key.toLowerCase() === k;
}

/**
 * createDoubleTapMatcher: 工厂方法, 维护一个 per-instance 的 lastTap 状态.
 * 返回的函数应该作为 keydown listener 调用, 返回 true 表示命中并已 preventDefault.
 *
 * 阈值 300ms (JetBrains Search Everywhere 默认).
 */
export function createDoubleTapMatcher(triggerTag: string): (e: KeyboardEvent) => boolean {
  const DOUBLE_TAP_MS = 300;
  let lastTap: { key: string; t: number } | null = null;
  const isMac = isMacPlatform();
  const expectedKey =
    triggerTag === "Mod" ? (isMac ? "Meta" : "Control") :
    triggerTag === "Control" ? "Control" :
    triggerTag === "Shift" ? "Shift" :
    triggerTag === "Alt" ? "Alt" :
    triggerTag === "Meta" ? "Meta" :
    triggerTag;
  return (e: KeyboardEvent) => {
    if (e.key !== expectedKey || e.repeat) return false;
    const now = Date.now();
    if (lastTap && lastTap.key === e.key && now - lastTap.t < DOUBLE_TAP_MS) {
      lastTap = null;
      return true;
    }
    lastTap = { key: e.key, t: now };
    return false;
  };
}

export type ActionHandler = (e: KeyboardEvent) => void;
export type ActionMap = Record<string, ActionHandler>;

/**
 * 创建一个 keydown dispatcher. caller 调用返回值传给 keydown listener.
 *
 * actions 是 { command: handler } 的 map. dispatcher 遍历当前 keybindings,
 * 对每个 actions 里有 handler 的 command 检查 keys:
 *   - "DoubleTap+X" → 用 doubleTap matcher
 *   - 其他 → 用 matchChord
 *
 * 命中即 preventDefault + stopImmediatePropagation + 调 handler, 返回 true.
 * 没命中返回 false, caller 可继续处理.
 *
 * 注意: doubleTap matcher 是 stateful, dispatcher 在 closure 里维护每个
 * command 一个 matcher.
 */
export function createKeybindingDispatcher(actions: ActionMap): (e: KeyboardEvent) => boolean {
  const doubleTapMatchers: Record<string, (e: KeyboardEvent) => boolean> = {};
  return (e: KeyboardEvent): boolean => {
    if (e.isComposing || e.keyCode === 229) return false;
    const kb = keybindings();
    if (!kb) return false;
    for (const b of kb.bindings) {
      const handler = actions[b.command];
      if (!handler) continue;
      const keys = b.keys;
      let hit = false;
      if (keys.startsWith("DoubleTap+")) {
        const tag = keys.slice("DoubleTap+".length);
        let m = doubleTapMatchers[b.command];
        if (!m) {
          m = createDoubleTapMatcher(tag);
          doubleTapMatchers[b.command] = m;
        }
        hit = m(e);
      } else {
        if (e.repeat) continue;
        hit = matchChord(e, keys);
      }
      if (hit) {
        e.preventDefault();
        e.stopImmediatePropagation();
        handler(e);
        return true;
      }
    }
    return false;
  };
}

/**
 * 标记哪些 command 已实现 (settings UI 用这个显示徽章).
 * 未在此 Set 中的 command 在 KeysTab 上会显示 "未实现" 提示.
 *
 * 真实生效的判定由 caller 维护 — 一旦某个 dispatcher actions 里有它就算"已实现".
 * 这里只是一个静态白名单 (硬编码为已知集合, 后续可改为 runtime 收集).
 */
// Terminal focus registry — picker / dialog 关闭后调 focusTerminal(id)
// 让 xterm 拿回焦点, 用户立刻能按 Enter 提交插入的命令.
const terminalFocusers = new Map<number, () => void>();

export function registerTerminalFocus(id: number, fn: () => void): () => void {
  terminalFocusers.set(id, fn);
  return () => {
    if (terminalFocusers.get(id) === fn) terminalFocusers.delete(id);
  };
}

export function focusTerminal(id: number): void {
  terminalFocusers.get(id)?.();
}

export const IMPLEMENTED_COMMANDS: ReadonlySet<string> = new Set([
  "command_palette",
  "new_task",
  "open_settings",
  "prompt_picker",
  "new_terminal",
  "close_terminal",
  "next_task",
  "prev_task",
  "split_horizontal",
  "split_vertical",
  "close_split",
  "font_size_up",
  "font_size_down",
  "font_size_reset",
  "find_in_terminal",
  "scroll_to_bottom",
]);
