export * as ipc from "./ipc";
export { playNotifySound, stopNotifySound, disposeNotifyAudio } from "./notify-audio";
export * as theme from "./theme";
export * as urgency from "./urgency";
export * as i18n from "./i18n";
export * as split from "./split";
export { t, tOr, promptDisplayName, promptDisplayContent, setLang, currentLang, LANGS, LANG_NAMES } from "./i18n";
export type { Lang } from "./i18n";
export { Terminal, getTerminalFontSize, setTerminalFontSize, requestRenderRepair } from "./terminal";
export { loadSavedScrollback, startScrollbackAutosave } from "./scrollback";
export {
  terminalFontFamily,
  terminalLineHeight,
  terminalCursorStyle,
  terminalCursorBlink,
  terminalPaddingX,
  terminalPaddingY,
  shouldConfirmCloseTask,
  setTerminalFontFamily,
  setTerminalLineHeight,
  setTerminalCursorStyle,
  setTerminalCursorBlink,
  setTerminalPaddingX,
  setTerminalPaddingY,
  setConfirmCloseTask,
  resetTerminalPrefs,
  TERMINAL_PREF_DEFAULTS,
  type CursorStyle,
} from "./terminal/prefs";
export { TaskList } from "./tasklist";
export { Titlebar } from "./titlebar";
export { SplitView, splitLeaf, removeLeaf, normalize, collectSlots, singleLeaf, newSlotId, bumpSlotIdAtLeast, setRatiosAt, rightmostBottomSlot, leftmostBottomSlot } from "./split";
export type { SplitNode, Orientation } from "./split";
export { createCanvasViewport } from "./canvas-viewport";
export { StatusBar } from "./status-bar";
export { WIDGETS, WIDGET_LIST, type WidgetMeta } from "./status-bar/widgets";
export type { CanvasViewport, CanvasViewportOpts, ViewportPos, ViewportRect } from "./canvas-viewport";
export {
  initKeybindings,
  keybindings,
  keysFor,
  matchChord,
  createDoubleTapMatcher,
  createKeybindingDispatcher,
  registerTerminalFocus,
  focusTerminal,
  IMPLEMENTED_COMMANDS,
  isMacPlatform,
  isWindowsPlatform,
  modKeyLabel,
} from "./keybindings";
export type { ActionHandler, ActionMap } from "./keybindings";
