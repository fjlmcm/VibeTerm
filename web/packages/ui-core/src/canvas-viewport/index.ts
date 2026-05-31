// Canvas viewport hook — pan / zoom 抽离, 借鉴 panzoom 库的算法.
//
// 设计:
//   - container 一个 ref, 内部 surface 用 transform: translate + scale 渲染
//   - 鼠标滚轮 / 触控板捏合 = 以光标为锚点缩放
//   - 触控板双指滑动 (deltaMode=PIXEL + 横向 delta) = 平移
//   - 拖动平移由调用方自行触发 startPan (典型: 右键或 Cmd+左键空白区)
//   - 光标在 "active 卡片" 内时 wheel 直通 (让 xterm 吃 scrollback);
//     active 标识通过 DOM 上的 data-canvas-card="true" + data-task-active="true" 识别
//
// 不持久化 — pan/zoom 是运行时视角, 不写 localStorage.

import { createSignal, type Accessor } from "solid-js";

export interface ViewportRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface ViewportPos {
  x: number;
  y: number;
}

export interface CanvasViewportOpts {
  /** 视口容器 (workspace div) — 用 getBoundingClientRect 算光标本地坐标 */
  container: () => HTMLElement | undefined;
  /** 仅在 canvas 模式响应; false 时所有 handler 短路 */
  enabled: () => boolean;
  /** zoom 范围, 默认 [0.1, 4] */
  minZoom?: number;
  maxZoom?: number;
  /** fit-to-view 留白 (px), 默认 48 */
  fitPadding?: number;
  /** 滚轮缩放系数; 默认 1.1 (每 notch 10%) */
  wheelZoomFactor?: number;
}

export interface CanvasViewport {
  pan: Accessor<ViewportPos>;
  zoom: Accessor<number>;
  isPanning: Accessor<boolean>;
  /** 挂到 container 的 onWheel; 含 over-active-card 直通逻辑 */
  onWheel: (e: WheelEvent) => void;
  /** 启动拖动平移 (调用方决定按键 / 修饰符) */
  startPan: (e: MouseEvent) => void;
  /** screen (clientX/Y) → content 坐标 (剔除 pan + zoom) */
  screenToContent: (clientX: number, clientY: number) => ViewportPos;
  /** 适配给定 rect 列表; 空列表 = reset */
  fit: (rects: ViewportRect[]) => void;
  /** 还原 pan=0,0 / zoom=1 */
  reset: () => void;
  /** 直接写 zoom (烘焙缩放后调回 1 用); 被 clamp 到 [minZoom, maxZoom] */
  setZoom: (z: number) => void;
  /** 直接写 pan (烘焙时一般 pan 不变, 提供以备拓展) */
  setPan: (p: ViewportPos) => void;
}

/** 光标锚点缩放公式 (panzoom 标准):
 *  新 pan 让 cursor 下的 content 点保持在 cursor 下.
 *    newPan = cursor - (cursor - pan) * (newZoom / oldZoom)
 */
function computePanAfterZoom(
  cursorLocalX: number,
  cursorLocalY: number,
  pan: ViewportPos,
  oldZoom: number,
  newZoom: number,
): ViewportPos {
  const realFactor = newZoom / oldZoom;
  return {
    x: cursorLocalX - (cursorLocalX - pan.x) * realFactor,
    y: cursorLocalY - (cursorLocalY - pan.y) * realFactor,
  };
}

function isOverActiveCard(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const card = target.closest<HTMLElement>("[data-canvas-card='true']");
  if (!card) return false;
  return card.dataset.taskActive === "true";
}

export function createCanvasViewport(opts: CanvasViewportOpts): CanvasViewport {
  const minZoom = opts.minZoom ?? 0.1;
  const maxZoom = opts.maxZoom ?? 4;
  const fitPadding = opts.fitPadding ?? 48;
  const wheelFactor = opts.wheelZoomFactor ?? 1.1;

  const [pan, setPan] = createSignal<ViewportPos>({ x: 0, y: 0 });
  const [zoom, setZoom] = createSignal(1);
  const [isPanning, setPanning] = createSignal(false);

  const containerRect = (): DOMRect | null => {
    const el = opts.container();
    return el ? el.getBoundingClientRect() : null;
  };

  const screenToContent = (clientX: number, clientY: number): ViewportPos => {
    const rect = containerRect();
    const ox = rect?.left ?? 0;
    const oy = rect?.top ?? 0;
    const p = pan();
    const z = zoom();
    return { x: (clientX - ox - p.x) / z, y: (clientY - oy - p.y) / z };
  };

  const zoomAtCursor = (clientX: number, clientY: number, factor: number) => {
    const rect = containerRect();
    if (!rect) return;
    const z = zoom();
    const nz = Math.max(minZoom, Math.min(maxZoom, z * factor));
    if (nz === z) return;
    const cursorLocalX = clientX - rect.left;
    const cursorLocalY = clientY - rect.top;
    setPan(computePanAfterZoom(cursorLocalX, cursorLocalY, pan(), z, nz));
    setZoom(nz);
  };

  const onWheel = (e: WheelEvent) => {
    if (!opts.enabled()) return;
    // active 卡片内 — 让 xterm 吃 scrollback, 不缩放也不 preventDefault
    if (isOverActiveCard(e.target)) return;

    // 判定缩放 vs 平移:
    //   - Ctrl/Cmd (含触控板捏合, 浏览器合成 ctrlKey) → zoom
    //   - 鼠标滚轮 (deltaMode=LINE/PAGE) → zoom (用户要求"直接滚轮缩放")
    //   - 触控板双指 (deltaMode=PIXEL 且有横向 delta) → pan
    //   - 触控板纯纵向滑 → zoom (无法跟鼠标滚轮区分, 偏向用户主意图)
    const trackpadPan =
      e.deltaMode === 0 &&
      !e.ctrlKey &&
      !e.metaKey &&
      Math.abs(e.deltaX) > 0.5;

    e.preventDefault();
    if (trackpadPan) {
      setPan((p) => ({ x: p.x - e.deltaX, y: p.y - e.deltaY }));
      return;
    }
    const factor = e.deltaY > 0 ? 1 / wheelFactor : wheelFactor;
    zoomAtCursor(e.clientX, e.clientY, factor);
  };

  const startPan = (e: MouseEvent) => {
    if (!opts.enabled()) return;
    e.preventDefault();
    const sx = e.clientX;
    const sy = e.clientY;
    const start = pan();
    setPanning(true);
    const move = (mv: MouseEvent) => {
      setPan({ x: start.x + (mv.clientX - sx), y: start.y + (mv.clientY - sy) });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setPanning(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const fit = (rects: ViewportRect[]) => {
    const rect = containerRect();
    if (!rect || rects.length === 0) {
      setZoom(1);
      setPan({ x: 0, y: 0 });
      return;
    }
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const r of rects) {
      if (r.x < minX) minX = r.x;
      if (r.y < minY) minY = r.y;
      if (r.x + r.w > maxX) maxX = r.x + r.w;
      if (r.y + r.h > maxY) maxY = r.y + r.h;
    }
    const bw = Math.max(1, maxX - minX);
    const bh = Math.max(1, maxY - minY);
    const aw = Math.max(1, rect.width - fitPadding * 2);
    const ah = Math.max(1, rect.height - fitPadding * 2);
    const z = Math.max(minZoom, Math.min(maxZoom, Math.min(aw / bw, ah / bh)));
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    setZoom(z);
    setPan({ x: rect.width / 2 - cx * z, y: rect.height / 2 - cy * z });
  };

  const reset = () => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  };

  const setZoomClamped = (z: number) => {
    setZoom(Math.max(minZoom, Math.min(maxZoom, z)));
  };

  return {
    pan,
    zoom,
    isPanning,
    onWheel,
    startPan,
    screenToContent,
    fit,
    reset,
    setZoom: setZoomClamped,
    setPan,
  };
}
