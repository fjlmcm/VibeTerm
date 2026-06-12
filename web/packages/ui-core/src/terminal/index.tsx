// SolidJS xterm.js 封装
//
// 支持指定 taskId(在该任务下 spawn);可选 startInTask
// 支持 theme prop(xterm options 立即应用)

import { onCleanup, onMount, createEffect, createSignal, Show } from "solid-js";

import { Terminal as XTerm } from "@xterm/xterm";
// xterm 自带样式随依赖图打包(此前 index.html/floating.html 用
// `/node_modules/@xterm/xterm/css/xterm.css` 裸链接,生产构建解析不到该路径时
// 原样保留 → 运行时 404 → 终端无样式渲染成空白)。
import "@xterm/xterm/css/xterm.css";
import { WebglAddon } from "@xterm/addon-webgl";
import { FitAddon } from "@xterm/addon-fit";
import { UnicodeGraphemesAddon } from "@xterm/addon-unicode-graphemes";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { WebLinksAddon } from "@xterm/addon-web-links";
import {
  commitScrollback,
  peekScrollback,
  registerScrollbackSnapshot,
  RESTORE_SEPARATOR,
} from "../scrollback";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";

import type { Theme, TaskId } from "@vibeterm/ipc-types";
import { toXtermTheme } from "../theme";
import { t } from "../i18n";
import {
  Channel,
  closePty,
  resizePty,
  terminalSize,
  spawnTerminalInTask,
  startPty,
  writePty,
  detachTerminal,
  pasteClipboard,
  writeClipboardText,
  openExternal,
} from "../ipc";
import { createKeybindingDispatcher as kbCreateDispatcher, registerTerminalFocus } from "../keybindings";
import { shellQuotePaths } from "./shell-quote";
import {
  terminalFontFamily,
  terminalLineHeight,
  terminalCursorStyle,
  terminalCursorBlink,
  terminalPaddingX,
  terminalPaddingY,
} from "./prefs";

// 复用 TextEncoder 单例 — onData/drag-drop/粘贴热路径每次构造产生无谓 GC 压力
const ENCODER = new TextEncoder();

// 字号配置(全局共享 + localStorage 持久化)
// 范围 8-32,默认 13;Cmd+ / Cmd- / Cmd+0 跨所有终端实例一致改
const FONT_MIN = 8;
const FONT_MAX = 32;
const FONT_DEFAULT = 13;
const FONT_KEY = "vibeterm.terminal.fontSize";

function readFontSize(): number {
  try {
    const raw = localStorage.getItem(FONT_KEY);
    const n = raw ? parseInt(raw, 10) : FONT_DEFAULT;
    if (!Number.isFinite(n) || n < FONT_MIN || n > FONT_MAX) return FONT_DEFAULT;
    return n;
  } catch {
    return FONT_DEFAULT;
  }
}

const [fontSize, setFontSize] = createSignal<number>(readFontSize());

/**
 * CJK 复制守门: 用 Intl.Segmenter 按 grapheme cluster 切片重组,
 * 自动丢弃孤立的 lone surrogate / 半截 emoji ZWJ sequence. 适用于 xterm.js
 * 选区在极端 case 跨过半个宽字符时的兜底.
 *
 * 浏览器不支持 Intl.Segmenter (老 WebView) 时退化为原样返回.
 */
function normalizeGraphemes(input: string): string {
  if (typeof (Intl as { Segmenter?: unknown }).Segmenter !== "function") {
    return input;
  }
  try {
    const seg = new Intl.Segmenter(undefined, { granularity: "grapheme" });
    let out = "";
    for (const s of seg.segment(input)) {
      out += s.segment;
    }
    return out;
  } catch {
    return input;
  }
}

function persistFontSize(n: number) {
  try {
    localStorage.setItem(FONT_KEY, String(n));
  } catch {
    /* private mode — 忽略 */
  }
}

export function getTerminalFontSize(): number {
  return fontSize();
}

export function setTerminalFontSize(n: number) {
  const clamped = Math.max(FONT_MIN, Math.min(FONT_MAX, Math.round(n)));
  setFontSize(clamped);
  persistFontSize(clamped);
}

// 文件路径 link matcher — 绝对路径 / ~ 开头 / 相对 ./../
// 仅识别明显是 fs path 的形态,避免误匹配普通单词
// CJK 一等公民:路径主体用 Unicode 属性转义 \p{L}\p{N} + /u flag,
// 以匹配中/日/韩等目录名;\w 在非 Unicode 模式下只命中 ASCII,会截断 CJK 路径。
const FILE_PATH_REGEX =
  /(?:^|[\s'"`(])((?:~|\.{1,2})?\/[\p{L}\p{N}._\-/]+(?::\d+(?::\d+)?)?)/gu;

// WebGL 渲染层偶发损坏自愈。
// 现象:部分字形画错/丢失(W、h、CJK 等画成错误方块或空白)但底层数据无损——选中
// 复制出的文本是对的。典型触发:OS 睡眠唤醒 / GPU 进程波动;macOS 26.x WebKit 另有
// 纹理对象级 WebGL bug(xterm.js#5816),CJK 大字符集填图集远快于拉丁文,概率更高。
//
// 实测教训(2026-06-12,带血):clearTextureAtlas() 只把字形**重传进同一个纹理对象**,
// 修不了 WebKit 纹理对象/多页绑定层面的损坏 —— focus 清图集无效,而分屏增减(行列变化
// → renderer 完整重建,拿到**全新纹理**)立刻恢复。因此 repair 动作升级为 dispose 再
// loadAddon 整个 WebglAddon:全新 WebGL context + 纹理页,与分屏 resize 等价但可随时触发。
//
// 触发源收敛到一个 module 级 signal(同 fontSize 模式),各 Terminal 实例
// createEffect 订阅,随组件自动释放:
//   1. 窗口聚焦 / 文档恢复可见(下方 DOM 监听,覆盖唤醒/切回场景);
//   2. Tauri 原生 onFocusChanged —— WKWebView 在终端 textarea 持焦时窗口切换
//      不一定派发 window focus 事件,DOM 监听会整条漏掉,原生事件兜底;
//   3. 命令面板「修复文字渲染」调用 requestRenderRepair() —— 乱码在眼前时一键重建。
// 成本:context 创建 + 可见区重栅格化(毫秒级,触发频度低);多源同时触发用 1.5s 窗口合并。
const [repairTick, setRepairTick] = createSignal(0);

let lastRepairAt = 0;
/** 让所有 Terminal 实例重建 WebGL 渲染层(修复偶发字形乱码) */
export function requestRenderRepair() {
  const now = Date.now();
  if (now - lastRepairAt < 1500) return; // 合并 DOM focus / visibility / Tauri focus 同时触发
  lastRepairAt = now;
  setRepairTick((n) => n + 1);
}

// app 生命周期常驻,只挂一次;浮窗是独立 webview,各自实例化本 module 同样生效。
window.addEventListener("focus", requestRenderRepair);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") requestRenderRepair();
});
// Tauri 原生窗口焦点事件兜底(触发源 2)。非 Tauri 环境(Playwright 直连 vite)注册失败即跳过。
try {
  getCurrentWindow()
    .onFocusChanged(({ payload: focused }) => {
      if (focused) requestRenderRepair();
    })
    .catch(() => {});
} catch {
  /* 非 Tauri 环境 */
}

export interface TerminalProps {
  /** 在指定 task 下 spawn;不给则用 start_pty(独立) */
  taskId?: TaskId;
  /** (task, slot) 幂等键:后端按它判断 spawn vs attach,前端无需自己判断
   *  Normal 和 Canvas 视图都传同一 slotId,后端保证只 spawn 一个 PTY,另一次自动 attach */
  slotId?: number;
  /** 字号覆盖(per-instance):Canvas 卡片缩放时用,不影响全局 Cmd+= 设置 */
  fontSizeOverride?: number;
  /** 主题(改变时立即重新 apply xterm.options.theme) */
  theme?: Theme;
  onReady?: (id: number) => void;
  onError?: (e: unknown) => void;
}

export function Terminal(props: TerminalProps) {
  let hostEl!: HTMLDivElement;
  let term: XTerm | null = null;
  let webgl: WebglAddon | null = null;
  // WebGL 不可用(初始化失败 / context loss)→ 永久回 DOM 渲染器,repair 不再重试
  let webglDisabled = false;
  // repair 到达时本实例不可见(display:none 的非激活任务)→ 0 尺寸下建 context 不可靠,
  // 记账推迟,变可见(onBecameVisible)时补建
  let pendingWebglRepair = false;
  let fit: FitAddon | null = null;
  let search: SearchAddon | null = null;
  let serialize: SerializeAddon | null = null;
  // G5 scrollback 快照注销函数(组件卸载时注销序列化注册)
  let unregisterSnapshot: (() => void) | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let intersectionObserver: IntersectionObserver | null = null;
  let terminalId: number | null = null;
  // slot 幂等命中(本视图 attach 到共享 PTY)时后端返回的订阅 id;卸载时据此 detach,
  // 否则浮窗每次开关都给共享 PTY 永久多挂一个 sink。全新 spawn 为 null(主嗅探 sink 不可摘)。
  let sinkId: number | null = null;
  let onDataDispose: { dispose(): void } | null = null;
  let onResizeDispose: { dispose(): void } | null = null;
  let dragDropUnlisten: UnlistenFn | null = null;
  // focus registry 注销函数 — 必须在同步 onCleanup 里调用
  //   (在异步 rAF 回调里调 onCleanup 时 owner 已丢失,注册会被静默忽略)
  let unregisterFocus: (() => void) | null = null;
  // 卸载守卫:异步 spawn 链(rAF + 50ms + spawn await)期间组件可能先卸载.
  //   onCleanup 同步置 true;每个 await 后检查,确保 PTY / 监听器无论何时卸载都被清理.
  let disposed = false;

  // Cmd+F 搜索浮层(每个终端独立 UI)
  const [searchOpen, setSearchOpen] = createSignal(false);
  const [searchTerm, setSearchTerm] = createSignal("");
  const [searchCounter, setSearchCounter] = createSignal<{ idx: number; total: number } | null>(null);
  let searchInputEl: HTMLInputElement | undefined;

  // 右键菜单:位置 + 当前是否有选区(决定 Copy 是否可用)
  const [ctxMenu, setCtxMenu] = createSignal<{ x: number; y: number; hasSel: boolean } | null>(null);
  const closeCtxMenu = () => setCtxMenu(null);
  const openSearchOverlay = () => {
    setSearchOpen(true);
    requestAnimationFrame(() => {
      searchInputEl?.focus();
      searchInputEl?.select();
    });
  };

  // 搜索结果高亮配色 — 主题感知:用 var(--color-accent) 当前 / 半透明当背景
  const SEARCH_OPTS = {
    decorations: {
      matchBackground: "rgba(255, 200, 0, 0.25)",
      activeMatchBackground: "#ffcc00",
      matchOverviewRuler: "#ffcc00",
      activeMatchColorOverviewRuler: "#ffaa00",
    },
  } as const;

  const closeSearch = () => {
    setSearchOpen(false);
    setSearchCounter(null);
    search?.clearDecorations();
    // 把焦点还给终端
    requestAnimationFrame(() => term?.focus());
  };

  // hostEl 有可见尺寸时才 fit,避免 0×0 容器(display:none / 首帧未布局)报错
  const tryFit = () => {
    if (!fit || !hostEl || !term) return;
    if (hostEl.offsetWidth <= 0 || hostEl.offsetHeight <= 0) return;
    // follow-bottom 守门:fit 触发的 resize 会按行数增减扰动 xterm 视口(ydisp/ybase),
    // 用户正向上看历史时会被冲走。记录 fit 前是否贴底:仅当原本在底部才跟随到底,
    // 否则不打断用户正在看的历史(agent 提问/界面变动引起的频繁 resize 不再冲乱滚动)。
    const buf = term.buffer.active;
    const wasAtBottom = buf.viewportY >= buf.baseY;
    try {
      fit.fit();
    } catch (e) {
      console.warn("[terminal] fit failed", e);
    }
    if (wasAtBottom) term.scrollToBottom();
  };

  // 返回终端(display:none → block)时强制 xterm 重新同步滚动区。
  // 根因:后台 agent 的输出会写进隐藏(display:none)的本终端(多 sink 共享 PTY)。隐藏期间
  // xterm Viewport 缓存的滚动高度会失真;返回后若窗口尺寸未变,fit() 是 no-op、不重算缓存,
  // 导致滚轮往下"触底差一截"(需按方向键走 scrollToBottom 才回到底)。这里发一次"真实滚动
  // 往返"触发 Viewport.syncScrollArea 校正缓存——同帧内完成、还原原位置,无可见闪烁。
  const resyncScroll = () => {
    if (!term) return;
    const buf = term.buffer.active;
    if (buf.baseY <= 0) return; // 无 scrollback → 不存在触底问题
    const y = buf.viewportY;
    if (y >= buf.baseY) {
      // 贴底:上滚 1 行(必触发 scroll 事件 → 校正)再回到底部
      term.scrollLines(-1);
      term.scrollToBottom();
    } else {
      // 非贴底:滚到底部触发校正,再还原用户原本所在行
      term.scrollToBottom();
      term.scrollToLine(y);
    }
  };

  // 把 PTY 尺寸断言为本视图(xterm)的当前尺寸。视图变可见时调用:
  // 浮窗在它生命周期里把共享 PTY 改成了浮窗尺寸,而本主窗视图隐藏期 fit 是 no-op、
  // onResize 不触发 → PTY 不会自己回到主窗尺寸,app 继续按浮窗宽度绘制 → 主窗顶部错乱。
  // 这里无条件下发本视图尺寸(后端 last_size 幂等:真没变则 no-op,不会多余 SIGWINCH)。
  const assertPtySize = () => {
    if (disposed || terminalId === null || !term) return;
    resizePty(terminalId, term.rows, term.cols).catch((err) => {
      console.error("[terminal] resizePty(assert) failed", err);
      props.onError?.(err);
    });
  };

  // 挂载 WebGL 渲染层。mount 与 rebuildWebgl 共用;失败(环境不支持)即永久回 DOM。
  const attachWebgl = () => {
    if (!term || webglDisabled) return;
    try {
      webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        console.warn("[terminal] WebGL context lost — fallback DOM");
        webglDisabled = true;
        webgl?.dispose();
        webgl = null;
      });
      term.loadAddon(webgl);
    } catch (e) {
      console.warn("[terminal] WebGL addon unavailable", e);
      webglDisabled = true;
      webgl = null;
    }
  };

  // 整层重建:dispose 旧 addon(旧 context/纹理一并释放)→ 全新挂载。
  // 不可见实例推迟(0 尺寸 canvas 上建 context 不可靠),变可见时由 onBecameVisible 补做。
  const rebuildWebgl = () => {
    if (disposed || !term || !webgl) return;
    if (hostEl && hostEl.offsetParent === null) {
      pendingWebglRepair = true;
      return;
    }
    pendingWebglRepair = false;
    try {
      webgl.dispose();
    } catch {
      /* renderer 已失效时 dispose 可能抛 —— 忽略,直接重建 */
    }
    webgl = null;
    attachWebgl();
  };

  // 视图从 display:none → 可见时的重排 + 反污染处理。
  //
  // 根因:一个 PTY 多 sink(主窗 + 浮窗)共享同一字节流,但各自是独立、可不同尺寸的 xterm。
  // 浮窗调尺寸会把共享 PTY 改成浮窗宽度,Claude Code 等全屏 TUI 据此发出「光标上移 N 行 +
  // 清行 + 重绘」序列;这些序列也写进隐藏的本主窗模拟器,而它 grid 还停在主窗宽度 → 行数
  // 计算错位,把 banner 等内容重叠进 buffer。返回可见时 fit 是 no-op、app 又按浮窗宽度残留 →
  // 主窗顶部出现重复 banner。
  //
  // 修法(不碰 reflow —— 用户确认拉伸窗口本身重排是正确的):变可见时先比对 PTY 当前尺寸与本
  // 视图尺寸;若不一致(= 隐藏期被别的视图改过、buffer 已污染)则清当前屏(ESC[2J + 光标归位,
  // 保留 scrollback、不 reset 以免丢 app 的 cursor-keys / bracketed-paste 等模式),再 assertPtySize
  // 把 PTY 断言回本视图尺寸 → app 收 SIGWINCH 在干净屏上重绘,消除重叠。尺寸一致(普通切任务)
  // 则跳过清屏,零副作用。
  // 已知竞态(可接受):隐藏期主窗被拉伸时,tryFit() 触发 xterm onResize → 异步 resizePty,
  // 与下面的 terminalSize() 读取无顺序保证;读到旧尺寸会误判污染、多清一次屏。后果只是
  // TUI 多重绘一帧(普通 shell 可能丢可见区几行),概率低且收 SIGWINCH 后即恢复,不加锁串行化。
  const onBecameVisible = async () => {
    const xt = term;
    if (!xt || terminalId === null) return;
    tryFit();
    // 隐藏期间错过的 WebGL 重建在此补做(fit 之后,尺寸已就绪)
    if (pendingWebglRepair) rebuildWebgl();
    try {
      const [ptyRows, ptyCols] = await terminalSize(terminalId);
      const contaminated =
        ptyRows > 0 &&
        ptyCols > 0 &&
        (ptyRows !== xt.rows || ptyCols !== xt.cols);
      if (!disposed && term === xt && contaminated) {
        // 清可见屏 + 光标归位;不动 scrollback、不 reset 模式
        xt.write("\x1b[2J\x1b[H");
      }
    } catch (e) {
      console.warn("[terminal] visible-resync size check failed", e);
    }
    if (disposed || term !== xt) return;
    assertPtySize();
    resyncScroll();
  };

  const findNext = () => {
    const q = searchTerm();
    if (q && search) search.findNext(q, SEARCH_OPTS);
  };
  const findPrev = () => {
    const q = searchTerm();
    if (q && search) search.findPrevious(q, SEARCH_OPTS);
  };

  // 粘贴图片/文本注入。
  //
  // 走 Rust 侧 tauri-plugin-clipboard-manager(包 arboard),绕开 WebView
  // 对纯 image 剪贴板内容的 paste 事件兼容性问题。
  //   - paste_clipboard_image:Some(path) → 注入路径
  //   - 否则 readText:非空 → 走 xterm.paste(保留 bracketed paste 行为)
  //   - 都没有 → noop
  //
  // 双入口拦截:Cmd+V/Ctrl+V keydown(键盘)+ paste 事件(右键菜单)。
  // keydown 先 preventDefault → 后续 paste 事件不再触发,两者互斥。
  const focusInHost = () =>
    !!hostEl &&
    (hostEl === document.activeElement ||
      hostEl.contains(document.activeElement));

  const doPaste = async (source: string) => {
    const t0 = performance.now();
    try {
      const r = await pasteClipboard();
      // await 期间组件可能已卸载(onCleanup 置 term=null、关 PTY)——
      // 不守门会向已关闭的 PTY 写入 / 在 null 上调 paste。
      if (disposed || !term || terminalId === null) return;
      const ms = (performance.now() - t0).toFixed(1);
      console.debug(`[terminal:${source}] paste_clipboard →`, r.kind, `${ms}ms`);
      if (r.kind === "files") {
        await writePty(
          terminalId,
          ENCODER.encode(shellQuotePaths(r.paths) + " "),
        );
      } else if (r.kind === "image") {
        // claude code 只把 bracketed paste(粘贴/拖拽)里的图片路径转成图片附件,裸键入的路径
        // 当普通文本;且路径【加引号会阻止转图片】。故走 term.paste 注入【裸路径】—— xterm 会按
        // PTY 的 bracketed paste 模式自动包 ESC[200~/201~(claude/codex/shell 都会开启),
        // 等价于"拖拽文件入终端"。见 anthropics/claude-code#4705 / #27904 / #62208。
        term.paste(r.path);
      } else if (r.kind === "text") {
        term.paste(r.text);
      }
      // 粘贴后把键盘焦点交回 xterm —— 右键菜单粘贴会让 activeElement 漂到菜单,
      // 窗口刚激活时 textarea 也可能没焦点;不回焦用户得再点一次终端才能继续输入。
      term?.focus();
    } catch (err) {
      console.error(`[terminal:${source}] paste failed`, err);
    }
  };

  // per-terminal 命令通过 keybindings dispatcher 路由 (用户改 toml 立即生效).
  //   find_in_terminal / font_size_up/down/reset / scroll_to_bottom
  // 之前这几个硬编码在 onWinKeydown 里, 用户改 keybindings.toml 不会生效 — 现在统一走 store.
  const perTerminalDispatcher = kbCreateDispatcher({
    find_in_terminal: () => {
      if (terminalId === null || !term) return;
      setSearchOpen(true);
      requestAnimationFrame(() => {
        searchInputEl?.focus();
        searchInputEl?.select();
      });
    },
    font_size_up: () => setTerminalFontSize(fontSize() + 1),
    font_size_down: () => setTerminalFontSize(fontSize() - 1),
    font_size_reset: () => setTerminalFontSize(FONT_DEFAULT),
    scroll_to_bottom: () => term?.scrollToBottom(),
  });

  // CJK IME 合成态:由 compositionstart/end 维护,比 keydown 的 isComposing/keyCode===229 可靠。
  // WKWebView 下拼音选词「提交候选的数字键 keydown」常不带这俩标记 → 漏进 PTY(打中文成 2112)。
  // 合成期间用本标志在 customKeyEventHandler 里一律拦下。
  let composing = false;
  const onCompositionStart = () => {
    composing = true;
  };
  const onCompositionEnd = () => {
    composing = false;
  };

  const onWinKeydown = (e: KeyboardEvent) => {
    // CJK IME 合成期间不拦截快捷键 — 让 IME 自己消费 Enter / Esc / 上下选词.
    if (e.isComposing || e.keyCode === 229) return;

    if (!focusInHost()) return;
    if (terminalId === null || !term) return;

    // per-terminal 命令路由 (字号 / 搜索 / 滚动)
    if (perTerminalDispatcher(e)) return;

    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;

    // Cmd+V 粘贴 — 不走 keybindings 因为浏览器 paste 事件本身需要处理
    if (e.key === "v" || e.key === "V") {
      e.preventDefault();
      e.stopImmediatePropagation();
      void doPaste("keydown");
    }
  };

  const onWinPaste = (e: ClipboardEvent) => {
    if (terminalId === null || !term) return;
    if (!focusInHost()) {
      // 也尝试看 target 是否在我们 hostEl 子树里(右键菜单可能让 activeElement 漂)
      const tgt = e.target as Node | null;
      if (!tgt || !hostEl.contains(tgt)) return;
      console.debug("[terminal:paste] focus drifted, but target in host — proceed");
    }
    e.preventDefault();
    e.stopImmediatePropagation();
    void doPaste("paste");
  };

  // Cmd+C / 浏览器原生复制路径的 grapheme 守门(右键菜单 Copy 已单独走 normalizeGraphemes)
  const onHostCopy = (e: ClipboardEvent) => {
    const sel = term?.getSelection() ?? "";
    if (!sel || !e.clipboardData) return;
    e.preventDefault();
    e.clipboardData.setData("text/plain", normalizeGraphemes(sel));
  };

  // 终端区右键 — 阻止系统默认菜单(macOS Cut/Copy/Spelling 等),弹自定义菜单
  const onHostContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const sel = term?.getSelection() ?? "";
    setCtxMenu({ x: e.clientX, y: e.clientY, hasSel: sel.length > 0 });
  };
  // E2E:暴露 terminal_id + term 实例 给测试用
  const setHostAttrs = (id: number | null) => {
    if (hostEl) {
      if (id !== null) hostEl.setAttribute("data-terminal-id", String(id));
      hostEl.setAttribute(
        "data-mode",
        props.taskId !== undefined ? "task-spawn" : "standalone",
      );
      // E2E 直读 xterm buffer(WebglAddon 渲染到 canvas,DOM 不带文本)
      (hostEl as unknown as { __vibeterm_term__: XTerm | null }).__vibeterm_term__ = term;
    }
  };

  // 三态 PTY 清理 — onCleanup 与异步 spawn 续体(竞态发现已卸载时)共用:
  //   1. slot attach 命中(sinkId 非 null)→ 只 detach 本视图的订阅,不杀 PTY(主视图还在用)
  //   2. slot 全新 spawn → 不动 PTY(任务切走只是 display:none;关 pane 走 closeActiveSlot)
  //   3. 独立 spawn → 杀 PTY
  const teardownPty = () => {
    if (terminalId === null) return;
    if (sinkId !== null) {
      detachTerminal(terminalId, sinkId).catch(console.error);
    } else if (props.slotId !== undefined && props.taskId !== undefined) {
      // slot 全新 spawn:不调任何 close/detach,PTY 保留
    } else {
      closePty(terminalId).catch(console.error);
    }
  };

  onMount(() => {
    const initialXtermTheme = props.theme ? toXtermTheme(props.theme.terminal) : undefined;

    term = new XTerm({
      fontFamily: terminalFontFamily(),
      fontSize: fontSize(),
      lineHeight: terminalLineHeight(),
      scrollback: 10000,
      cursorStyle: terminalCursorStyle(),
      cursorBlink: terminalCursorBlink(),
      allowProposedApi: true,
      theme: initialXtermTheme,
    });

    fit = new FitAddon();
    term.loadAddon(fit);
    const unicode = new UnicodeGraphemesAddon();
    term.loadAddon(unicode);
    term.unicode.activeVersion = "15-graphemes";

    // 搜索(浮层 UI 自渲,见 return)
    search = new SearchAddon();
    term.loadAddon(search);
    // G5:序列化 addon(会话 scrollback 快照),纯读缓冲,不影响渲染 / CJK / fit。
    serialize = new SerializeAddon();
    term.loadAddon(serialize);
    // n / total 计数:addon 每次定位后 emit resultIndex / resultCount
    search.onDidChangeResults(({ resultIndex, resultCount }) => {
      setSearchCounter(resultCount > 0 ? { idx: resultIndex + 1, total: resultCount } : null);
    });

    // URL Cmd+Click 打开 — handler 仅在按住 modifier 时触发
    // 否则单击不响应,避免误触
    const webLinks = new WebLinksAddon((event, uri) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      openExternal(uri).catch((err) => console.warn("[terminal] openExternal failed", err));
    });
    term.loadAddon(webLinks);

    // 文件路径 link provider — 同样要求 Cmd/Ctrl + Click
    term.registerLinkProvider({
      provideLinks(bufferLineNumber, callback) {
        if (!term) return callback(undefined);
        const line = term.buffer.active.getLine(bufferLineNumber - 1);
        if (!line) return callback(undefined);
        const text = line.translateToString(true);
        // CJK 一等公民:translateToString 对每个 CJK 宽字符只产 1 个 JS char,
        // 但该字符在缓冲里占 2 个 cell;link provider 的 range.x 是 1-based 显示列(cell),
        // 不是字符串偏移。逐 cell 累加宽度,把"字符串字符索引"映射到"1-based 显示列"。
        // colStarts[charIndex] = 该字符首 cell 的 1-based 显示列。
        const colStarts: number[] = [];
        const colCells: number[] = []; // 同 index 字符占用的显示列宽(1 或 2)
        for (let cell = 0; cell < line.length; ) {
          const c = line.getCell(cell);
          const width = c ? c.getWidth() : 1;
          // width=0 是宽字符的占位 cell,不对应任何 JS char,跳过
          if (width === 0) {
            cell += 1;
            continue;
          }
          colStarts.push(cell + 1); // 1-based 显示列
          colCells.push(width);
          cell += width;
        }
        const links: {
          range: { start: { x: number; y: number }; end: { x: number; y: number } };
          text: string;
          activate: (e: MouseEvent, t: string) => void;
        }[] = [];
        FILE_PATH_REGEX.lastIndex = 0;
        for (let m = FILE_PATH_REGEX.exec(text); m !== null; m = FILE_PATH_REGEX.exec(text)) {
          const path = m[1];
          // path 在字符串中的起始/结束字符索引
          const startCharIdx = (m.index ?? 0) + (m[0].length - path.length);
          const endCharIdx = startCharIdx + path.length - 1;
          // 映射到显示列:start 取该字符首 cell;end 取最后字符末 cell(含宽字符的第 2 列)
          const startCol = colStarts[startCharIdx] ?? startCharIdx + 1;
          const endCol =
            (colStarts[endCharIdx] ?? endCharIdx + 1) +
            ((colCells[endCharIdx] ?? 1) - 1);
          // 去掉 `:line:col` 后缀传给 OS open(open 不识别这种语法)
          const fsPath = path.replace(/:\d+(?::\d+)?$/, "");
          links.push({
            range: {
              start: { x: startCol, y: bufferLineNumber },
              end: { x: endCol, y: bufferLineNumber },
            },
            text: path,
            activate: (e) => {
              if (!(e.metaKey || e.ctrlKey)) return;
              openExternal(fsPath).catch((err) =>
                console.warn("[terminal] open path failed", err),
              );
            },
          });
        }
        callback(links);
      },
    });

    term.open(hostEl);

    // CJK IME composition 守门.
    //   claude-code#1547 (241👍) / #8405 (95👍) 都是 IME 合成期间 Enter
    //   被误当作"提交命令"送 PTY. 根因: xterm.js 默认在 keydown 里直接
    //   送数据, 不区分 isComposing.
    //   修法: customKeyEventHandler 返回 false 时 xterm.js 跳过该 keydown,
    //   把字符交给 IME 完成合成. compositionend 由 xterm.js textarea 自身
    //   产生完整 input event 走 onData 路径 -> 一次性原子推到 PTY.
    //   keyCode 229 是浏览器在 IME 合成期间历史值, 双保险.
    term.attachCustomKeyEventHandler((e) => {
      if (composing || e.isComposing || e.keyCode === 229) return false;
      return true;
    });
    // compositionstart/end 在 xterm 自己的隐藏 textarea 上触发 — 在此订阅维护合成态。
    const ta = term.textarea;
    if (ta) {
      ta.addEventListener("compositionstart", onCompositionStart);
      ta.addEventListener("compositionend", onCompositionEnd);
    }

    attachWebgl();

    requestAnimationFrame(async () => {
      if (disposed) return;
      fit?.fit();
      // 修 race:首次 mount 时 Tauri Channel bridge 可能未完全就绪,
      // shell 启动写 prompt 时 channel.send 静默丢弃。让 microtask 跑一轮再 spawn。
      await new Promise((r) => setTimeout(r, 50));
      // 50ms 窗口内组件可能已卸载;此刻 onCleanup 已执行(terminalId 仍为 null,
      // 它清理不到尚未分配的 PTY),直接退出,避免注册永不释放的资源。
      if (disposed || !term) return;
      const channel = new Channel<number[] | Uint8Array>();
      channel.onmessage = (data: number[] | Uint8Array) => {
        const bytes = data instanceof Uint8Array ? data : Uint8Array.from(data);
        term?.write(bytes);
      };

      try {
        // 两种模式
        //   1. taskId 给定 → spawn 新 terminal 关联到该 task(slot 幂等:后端已绑则 attach)
        //   2. 不给 → start_pty(独立)
        // 捕获当前实例:new Channel() 等调用会重置 TS 对闭包 let 的 narrowing
        const xt = term;
        // G5:重启后回放该 task/slot 的旧 scrollback。peek 不消费——spawn 成功且组件
        // 仍存活后才 commit,避免「spawn await 期间被卸载 → 内容已消费却没人看到」永久丢失。
        // 在新 PTY 输出前写:旧历史在上、新 shell prompt 在下。
        const snapKey =
          props.taskId !== undefined && props.slotId !== undefined
            ? `${props.taskId}:${props.slotId}`
            : null;
        const restored = snapKey ? peekScrollback(snapKey) : null;
        if (restored) {
          xt.write(restored);
          xt.write(RESTORE_SEPARATOR);
        }
        const opts = {
          rows: xt.rows,
          cols: xt.cols,
          cwd: null,
          command: null,
          args: null,
          env: null,
        };
        const r =
          props.taskId !== undefined
            ? await spawnTerminalInTask(props.taskId, props.slotId ?? null, opts, channel)
            : await startPty(opts, channel);
        terminalId = r.terminal_id;
        sinkId = r.sink_id ?? null;
        if (sinkId !== null) {
          // slot 幂等命中 = PTY 已存活(webview reload / 浮窗同 slot 挂载),后端 attach 会
          // 经 channel 回放整段 scrollback ring。清掉刚写的本地磁盘快照(内容重复且更旧),
          // 否则同一段历史出现两遍、分隔线错插中间。ring 回放 chunk 在本续体之后到达,不受影响。
          xt.reset();
        } else if (restored && snapKey && !disposed) {
          // 真·全新 spawn 且本组件存活:本地快照已成功展示,此时才消费
          commitScrollback(snapKey);
        }
        // spawn/attach await 期间组件可能已卸载:onCleanup 此前以 terminalId===null
        // 退出,清理不到刚分配的 PTY/sink — 这里补做 teardown 并停止注册监听器。
        if (disposed || !term) {
          teardownPty();
          return;
        }
        // 捕获当前实例:后续函数调用会重置 TS 对闭包 let 变量的 narrowing,
        // 且 onCleanup 可能把 term 置 null;用 const 捕获既类型安全又锁定当前实例。
        const xterm = term;
        setHostAttrs(terminalId);
        props.onReady?.(terminalId);

        // 注册到 focus registry, picker / dialog 关闭后能让 xterm 拿回焦点.
        //   仅赋值给同步声明的 unregisterFocus;注销由组件级 onCleanup 调用
        //   (在异步上下文里调 onCleanup owner 已丢失,会被静默忽略)。
        unregisterFocus = registerTerminalFocus(terminalId, () => {
          term?.focus();
        });

        // G5:注册 scrollback 序列化(仅 task 终端 + 主 spawn 视图;attach 副本不重复存)。
        if (sinkId === null && props.taskId !== undefined && props.slotId !== undefined) {
          unregisterSnapshot = registerScrollbackSnapshot(
            `${props.taskId}:${props.slotId}`,
            () => serialize?.serialize({ scrollback: 1000 }) ?? "",
          );
        }

        onDataDispose = xterm.onData((data: string) => {
          if (terminalId === null) return;
          writePty(terminalId, ENCODER.encode(data)).catch((err) => {
            console.error("[terminal] writePty failed", err);
            props.onError?.(err);
          });
        });
        onResizeDispose = xterm.onResize(
          ({ rows, cols }: { rows: number; cols: number }) => {
            if (terminalId === null) return;
            resizePty(terminalId, rows, cols).catch((err) => {
              console.error("[terminal] resizePty failed", err);
              props.onError?.(err);
            });
          },
        );

        // Tauri 2 native drag-drop — 仅处理 drop 事件;事件全 webview 广播,
        // 按 hostEl 的 viewport 矩形过滤,确保多终端布局下命中正确实例
        try {
          dragDropUnlisten = await getCurrentWebview().onDragDropEvent((event) => {
            if (event.payload.type !== "drop") return;
            if (terminalId === null) return;
            const paths = event.payload.paths ?? [];
            if (paths.length === 0) return;
            const r = hostEl.getBoundingClientRect();
            const { x, y } = event.payload.position;
            if (x < r.left || x > r.right || y < r.top || y > r.bottom) return;
            // 命中点最顶层的 terminal host 才接收:canvas 模式卡片可重叠,纯几何过滤会让
            // 上下两个终端同时收到 drop,被遮住的终端被误注入路径文本。
            const topHost = document
              .elementsFromPoint(x, y)
              .find((el) => el.hasAttribute("data-terminal-id"));
            if (topHost && topHost !== hostEl) return;
            const text = shellQuotePaths(paths);
            writePty(terminalId, ENCODER.encode(text)).catch(console.error);
          });
          // 订阅返回前组件可能已卸载:onCleanup 调过的 dragDropUnlisten?.() 此时还是 null,
          // 监听器永不释放 — 这里立即注销并退出,不再注册 window 监听器。
          if (disposed) {
            dragDropUnlisten?.();
            dragDropUnlisten = null;
            return;
          }
        } catch (e) {
          console.warn("[terminal] drag-drop subscribe failed", e);
        }

        // 粘贴图片/文本 — 双入口 window 级 capture(键盘 + 右键菜单)
        window.addEventListener("keydown", onWinKeydown, true);
        window.addEventListener("paste", onWinPaste, true);
        // 拦截 WKWebView 默认右键(Cut/Copy/Paste/Spelling/AutoFill…),
        // 改用自定义 i18n ContextMenu(仅终端区生效)
        hostEl.addEventListener("contextmenu", onHostContextMenu);
        // Cmd+C / 自动复制与右键菜单 Copy 走同一 grapheme 守门:xterm 选区可能切在宽字符
        // /代理对/ZWJ 中间,normalizeGraphemes 防撕裂(CJK 一等公民红线)。
        hostEl.addEventListener("copy", onHostCopy, true);
      } catch (e) {
        props.onError?.(e);
        console.error("[terminal] spawn failed", e);
      }
    });

    resizeObserver = new ResizeObserver(() => {
      requestAnimationFrame(tryFit);
    });
    resizeObserver.observe(hostEl);

    // 兜底:多次延迟 fit,覆盖
    //   - 首帧 hostEl 还是 0 高度(被 display:none 父级遮)
    //   - parent layout 在后续 frame 才 settle(grid + flex 链)
    //   - 切换任务从 display:none → block 后 ResizeObserver 偶有不触发
    for (const ms of [0, 50, 200, 600, 1500]) {
      setTimeout(() => requestAnimationFrame(tryFit), ms);
    }

    // 父级 display:none ↔ block 切换时,IntersectionObserver 触发 fit
    // (ResizeObserver 对该转换不可靠)
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            // 变可见:fit + 反污染(尺寸不一致则清屏重绘)+ 重同步滚动区,详见 onBecameVisible。
            requestAnimationFrame(() => {
              void onBecameVisible();
            });
            // 首帧 hostEl 可能 0 高度致 syncScrollArea 读到错尺寸,稍后再校正一次兜底。
            setTimeout(() => {
              if (disposed) return;
              requestAnimationFrame(resyncScroll);
            }, 120);
          }
        }
      },
      { threshold: 0 },
    );
    io.observe(hostEl);
    intersectionObserver = io;
  });

  // 主题变化时立即应用到 xterm
  createEffect(() => {
    if (term && props.theme) {
      term.options.theme = toXtermTheme(props.theme.terminal);
    }
  });

  // 字号变化时立即应用 + refit(字号变了 row/col 跟着变)
  //  fontSizeOverride 优先(Canvas 卡片缩放传入),否则用全局 fontSize signal
  //  字号变化常伴随容器尺寸变化(Canvas ↔ Normal 切换):reflow + glyph metrics
  //  重新测量跨多 frame,单次 RAF fit 会拿到陈旧 char 宽度,导致文字挤在左侧.
  //  多次延迟 fit 覆盖 reflow 不同阶段的稳定时点.
  createEffect(() => {
    const px = props.fontSizeOverride ?? fontSize();
    if (!term) return;
    term.options.fontSize = Math.max(4, Math.round(px));
    for (const ms of [0, 50, 200]) {
      setTimeout(() => requestAnimationFrame(tryFit), ms);
    }
  });

  // 字体族 / 行高变化:改 glyph 度量,需 refit(同字号逻辑)
  createEffect(() => {
    const family = terminalFontFamily();
    const lh = terminalLineHeight();
    if (!term) return;
    term.options.fontFamily = family;
    term.options.lineHeight = lh;
    for (const ms of [0, 50, 200]) {
      setTimeout(() => requestAnimationFrame(tryFit), ms);
    }
  });

  // 光标样式 / 闪烁变化:不影响布局,直接改 options
  createEffect(() => {
    const style = terminalCursorStyle();
    const blink = terminalCursorBlink();
    if (!term) return;
    term.options.cursorStyle = style;
    term.options.cursorBlink = blink;
  });

  // WebGL 渲染层自愈:订阅 module 级 repairTick(触发源与"为什么是整层重建而非
  // clearTextureAtlas"见文件顶部注释)。首跑 = mount 当下(addon 刚挂),跳过;
  // 之后每次 tick 重建。DOM fallback(webglDisabled)时 rebuildWebgl 自然 no-op。
  createEffect<number>((prev) => {
    const tick = repairTick();
    if (tick !== prev) rebuildWebgl();
    return tick;
  }, repairTick());

  onCleanup(() => {
    // 卸载守卫:让仍在排队/await 中的异步 spawn 续体知道组件已销毁,
    // 续体会自行 teardownPty 并跳过 window 监听器注册。
    disposed = true;
    onDataDispose?.dispose();
    onResizeDispose?.dispose();
    dragDropUnlisten?.();
    unregisterFocus?.();
    unregisterSnapshot?.();
    window.removeEventListener("keydown", onWinKeydown, true);
    window.removeEventListener("paste", onWinPaste, true);
    hostEl?.removeEventListener("contextmenu", onHostContextMenu);
    hostEl?.removeEventListener("copy", onHostCopy, true);
    term?.textarea?.removeEventListener("compositionstart", onCompositionStart);
    term?.textarea?.removeEventListener("compositionend", onCompositionEnd);
    resizeObserver?.disconnect();
    intersectionObserver?.disconnect();
    // 三态 PTY cleanup(见 teardownPty 注释):
    //   1. attach → 只 detach 不杀 PTY
    //   2. slot 幂等 → 不动 PTY(可能另一视图还在用 / 用户切回来时复用)
    //   3. 独立 spawn → unmount 即杀 PTY
    teardownPty();
    term?.dispose();
    term = null;
  });

  // 搜索浮层 — 输入框 + 上/下/关闭;Enter=findNext,Shift+Enter=findPrev,Esc=close
  const onSearchKeydown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      closeSearch();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey) findPrev();
      else findNext();
    }
  };

  return (
    <div
      style={{
        position: "relative",
        width: "100%",
        height: "100%",
        padding: `${terminalPaddingY()}px ${terminalPaddingX()}px`,
        "box-sizing": "border-box",
        background: "var(--color-bg)",
      }}
    >
      <div
        ref={hostEl}
        data-testid="terminal-host"
        style={{ width: "100%", height: "100%" }}
      />
      <Show when={searchOpen()}>
        <div
          data-testid="terminal-search"
          style={{
            position: "absolute",
            top: "8px",
            right: "16px",
            background: "var(--color-surface)",
            border: "1px solid var(--color-border)",
            "border-radius": "6px",
            padding: "4px 6px",
            display: "flex",
            "align-items": "center",
            gap: "4px",
            "box-shadow": "0 4px 12px rgba(0,0,0,0.4)",
            "z-index": 5,
          }}
        >
          <input
            ref={searchInputEl}
            data-testid="terminal-search-input"
            value={searchTerm()}
            onInput={(e) => {
              const v = e.currentTarget.value;
              setSearchTerm(v);
              // 增量搜索:每次按键即时找一次(addon 内部 debounced)
              if (v && search) search.findNext(v, SEARCH_OPTS);
              else {
                search?.clearDecorations();
                setSearchCounter(null);
              }
            }}
            onKeyDown={onSearchKeydown}
            placeholder={t("terminal.search.placeholder")}
            style={{
              background: "transparent",
              border: "none",
              outline: "none",
              color: "var(--color-text)",
              "font-size": "12px",
              width: "180px",
            }}
          />
          <Show when={searchCounter()}>
            {(c) => (
              <span
                data-testid="terminal-search-count"
                style={{
                  "font-size": "11px",
                  color: "var(--color-text-2)",
                  "font-variant-numeric": "tabular-nums",
                  "white-space": "nowrap",
                  "padding-right": "2px",
                }}
              >
                {c().idx}/{c().total}
              </span>
            )}
          </Show>
          <button
            type="button"
            title={t("terminal.search.prev")}
            onClick={findPrev}
            style={searchBtnStyle()}
          >
            ↑
          </button>
          <button
            type="button"
            title={t("terminal.search.next")}
            onClick={findNext}
            style={searchBtnStyle()}
          >
            ↓
          </button>
          <button
            type="button"
            title={t("terminal.search.close")}
            onClick={closeSearch}
            style={searchBtnStyle()}
          >
            ×
          </button>
        </div>
      </Show>
      <Show when={ctxMenu()}>
        {(m) => (
          <>
            {/* 全屏隐形 backdrop:点空白处或滚轮关闭 */}
            <div
              onClick={closeCtxMenu}
              onContextMenu={(e) => {
                e.preventDefault();
                closeCtxMenu();
              }}
              onWheel={closeCtxMenu}
              style={{
                position: "fixed",
                inset: 0,
                "z-index": 9998,
              }}
            />
            <div
              data-testid="terminal-ctxmenu"
              style={{
                position: "fixed",
                left: `${m().x}px`,
                top: `${m().y}px`,
                background: "var(--color-surface)",
                border: "1px solid var(--color-border)",
                "border-radius": "6px",
                padding: "4px",
                "box-shadow": "0 8px 24px rgba(0,0,0,0.35)",
                "z-index": 9999,
                "min-width": "160px",
                "font-size": "13px",
                color: "var(--color-text)",
                "user-select": "none",
              }}
            >
              <CtxItem
                label={t("ctxmenu.terminal.copy")}
                disabled={!m().hasSel}
                onClick={() => {
                  const sel = term?.getSelection() ?? "";
                  if (sel) {
                    // Rust 侧 arboard 写剪贴板 —— navigator.clipboard 在右键菜单
                    // 焦点漂移后会静默失败(尤其 Windows WebView2)。
                    writeClipboardText(normalizeGraphemes(sel)).catch(console.error);
                  }
                  closeCtxMenu();
                }}
              />
              <CtxItem
                label={t("ctxmenu.terminal.paste")}
                onClick={() => {
                  closeCtxMenu();
                  void doPaste("ctxmenu");
                }}
              />
              <CtxItem
                label={t("ctxmenu.terminal.select_all")}
                onClick={() => {
                  term?.selectAll();
                  closeCtxMenu();
                }}
              />
              <CtxSep />
              <CtxItem
                label={t("ctxmenu.terminal.find")}
                onClick={() => {
                  closeCtxMenu();
                  openSearchOverlay();
                }}
              />
              <CtxItem
                label={t("ctxmenu.terminal.clear")}
                onClick={() => {
                  term?.clear();
                  closeCtxMenu();
                }}
              />
            </div>
          </>
        )}
      </Show>
    </div>
  );
}

function CtxItem(props: { label: string; onClick: () => void; disabled?: boolean }) {
  return (
    <div
      onClick={() => {
        if (props.disabled) return;
        props.onClick();
      }}
      style={{
        padding: "5px 10px",
        "border-radius": "4px",
        cursor: props.disabled ? "default" : "pointer",
        opacity: props.disabled ? "0.4" : "1",
      }}
      onMouseEnter={(e) => {
        if (!props.disabled) e.currentTarget.style.background = "var(--color-hover, rgba(255,255,255,0.06))";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
      }}
    >
      {props.label}
    </div>
  );
}

function CtxSep() {
  return (
    <div
      style={{
        height: "1px",
        background: "var(--color-border)",
        margin: "4px 2px",
      }}
    />
  );
}

function searchBtnStyle() {
  return {
    background: "transparent",
    border: "none",
    color: "var(--color-text-2)",
    cursor: "pointer",
    padding: "2px 6px",
    "font-size": "12px",
    "line-height": "1",
  } as const;
}
