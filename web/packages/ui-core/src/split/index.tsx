// 分屏嵌套树 — 扁平渲染版
//
// 关键思想:**所有 leaf 都是 SplitView 根 div 下的 absolute-positioned 子元素**,
// 按 slotId 作为 <For> 主键。当树从 {leaf:A} 重塑成 {split, [leaf:A, leaf:B]} 时:
//   - collectSlots 由 [A] 变为 [A, B] → <For> 增量挂上 B 的 wrapper
//   - A 的 wrapper 不动 → 里面的 Terminal / PTY 不重 mount → 运行任务保留
//
// Splitter 拖动:从树的 split 节点路径回写 ratios。
//
// 借鉴 tabby `splitTab.component.ts` 的 n-ary 递归;normalize / removeLeaf /
// splitLeaf 树操作与之前版本兼容,API 不变。

import { For, Show, type Component, type JSX } from "solid-js";

export type Orientation = "h" | "v"; // h: 水平拆 = 左右;v: 垂直拆 = 上下

export type SplitNode =
  | { kind: "leaf"; slot_id: number }
  | { kind: "split"; orientation: Orientation; children: SplitNode[]; ratios?: number[] };

let nextSlotId = 0;
export function newSlotId(): number {
  return nextSlotId++;
}

/** 把 nextSlotId 推进到 ≥ 给定值(从后端 tree 加载后调用,防 id 冲突) */
export function bumpSlotIdAtLeast(n: number): void {
  if (n >= nextSlotId) nextSlotId = n + 1;
}

/** 把叶子 leaf 在指定方向上一拆为二 → 新增 slotId 返回 */
export function splitLeaf(root: SplitNode, target: number, orientation: Orientation): { root: SplitNode; newSlot: number } {
  const newSlot = newSlotId();
  function go(n: SplitNode): SplitNode {
    if (n.kind === "leaf") {
      if (n.slot_id !== target) return n;
      return {
        kind: "split",
        orientation,
        children: [n, { kind: "leaf", slot_id: newSlot }],
      };
    }
    return { ...n, children: n.children.map(go) };
  }
  return { root: normalize(go(root)), newSlot };
}

/** 删除指定 slotId 叶子 → normalize 收敛;若整树空了返回 null */
export function removeLeaf(root: SplitNode, target: number): SplitNode | null {
  function go(n: SplitNode): SplitNode | null {
    if (n.kind === "leaf") return n.slot_id === target ? null : n;
    const kept: SplitNode[] = [];
    const keptRatios: number[] = [];
    const cnt = n.children.length;
    const ratios =
      n.ratios && n.ratios.length === cnt ? n.ratios : undefined;
    n.children.forEach((c, i) => {
      const res = go(c);
      if (res !== null) {
        kept.push(res);
        if (ratios) keptRatios.push(ratios[i]);
      }
    });
    if (kept.length === 0) return null;
    // 删节点后按保留项的原始比例重新归一化,避免回退等比布局
    if (ratios && keptRatios.length === kept.length) {
      const sum = keptRatios.reduce((a, b) => a + b, 0) || 1;
      return { ...n, children: kept, ratios: keptRatios.map((r) => r / sum) };
    }
    return { ...n, children: kept };
  }
  const r = go(root);
  return r === null ? null : normalize(r);
}

/** 把同向嵌套 split 折叠到 parent + 单子节点提升 */
export function normalize(node: SplitNode): SplitNode {
  if (node.kind === "leaf") return node;
  const cnt = node.children.length;
  // 父层各 child 的有效比例(无则等比),用于扁平化时按比例合并子层 ratios
  const parentRatios =
    node.ratios && node.ratios.length === cnt
      ? node.ratios
      : new Array(cnt).fill(1 / cnt);
  const flattened: SplitNode[] = [];
  const mergedRatios: number[] = [];
  node.children.forEach((c, i) => {
    const nc = normalize(c);
    const parentFrac = parentRatios[i];
    if (nc.kind === "split" && nc.orientation === node.orientation) {
      // 同向子树被提升:把父占比按子层比例拆分给各子节点
      const subCnt = nc.children.length;
      const subRatios =
        nc.ratios && nc.ratios.length === subCnt
          ? nc.ratios
          : new Array(subCnt).fill(1 / subCnt);
      const subTotal = subRatios.reduce((a, b) => a + b, 0) || 1;
      nc.children.forEach((sub, j) => {
        flattened.push(sub);
        mergedRatios.push(parentFrac * (subRatios[j] / subTotal));
      });
    } else {
      flattened.push(nc);
      mergedRatios.push(parentFrac);
    }
  });
  if (flattened.length === 1) return flattened[0];
  // 仅当父原本带有效 ratios,或发生了同向扁平合并(长度变化)时才写回 ratios,
  // 否则保持无 ratios 让 layoutTree 走等比,行为不变
  const didFlatten = flattened.length !== cnt;
  if (node.ratios?.length === cnt || didFlatten) {
    const sum = mergedRatios.reduce((a, b) => a + b, 0) || 1;
    return {
      ...node,
      children: flattened,
      ratios: mergedRatios.map((r) => r / sum),
    };
  }
  return { ...node, children: flattened };
}

/** 收集所有 leaf 的 slotId(顺序遍历,用于持久化 / 重连)*/
export function collectSlots(node: SplitNode): number[] {
  if (node.kind === "leaf") return [node.slot_id];
  return node.children.flatMap(collectSlots);
}

/** 找 split tree 的"右下叶子" — 每层 split 都取最后一个 child, 递归到 leaf.
 *  用于:多分屏场景里,只有视觉最右下的 slot 才需要跟窗口圆角对齐的 border-radius. */
export function rightmostBottomSlot(node: SplitNode): number {
  if (node.kind === "leaf") return node.slot_id;
  return rightmostBottomSlot(node.children[node.children.length - 1]);
}

/** 找"左下叶子" — h-split 取第一个 child (最左), v-split 取最后一个 child (最下).
 *  与 rightmostBottomSlot 配对, 用于决定 slot 哪个底部圆角与外框对齐.
 *  典型:单叶 → 同时是左下/右下; 左右 h-split → 左叶左下, 右叶右下;
 *  上下 v-split → 下叶同时是左下/右下. */
export function leftmostBottomSlot(node: SplitNode): number {
  if (node.kind === "leaf") return node.slot_id;
  // h-split: 底边由 children 横向均分, 左叶最左
  // v-split: 底边只属于最下面的 child
  const idx = node.orientation === "h" ? 0 : node.children.length - 1;
  return leftmostBottomSlot(node.children[idx]);
}

/** 按 path(child index 链)定位到一个 split 节点,写新 ratios;返回新树。 */
export function setRatiosAt(root: SplitNode, path: number[], ratios: number[]): SplitNode {
  if (path.length === 0) {
    if (root.kind !== "split") return root;
    return { ...root, ratios };
  }
  if (root.kind === "leaf") return root;
  const [head, ...rest] = path;
  return {
    ...root,
    children: root.children.map((c, i) =>
      i === head ? setRatiosAt(c, rest, ratios) : c,
    ),
  };
}

// ---- 布局计算 ----
type Rect = { x: number; y: number; w: number; h: number };
type SplitterInfo = {
  /** splitter 中心位置 + 跨越的另一轴范围(% 0..100) */
  rect: Rect;
  /** 父 split 节点的方向(决定 splitter 是垂直线 h 还是水平线 v) */
  orientation: Orientation;
  /** 从根到父 split 节点的 child-index 路径 */
  nodePath: number[];
  /** 拖动影响 ratios[childIdx] 与 ratios[childIdx+1] */
  childIdx: number;
};

function layoutTree(
  node: SplitNode,
  rect: Rect = { x: 0, y: 0, w: 100, h: 100 },
  path: number[] = [],
): { leaves: Map<number, Rect>; splitters: SplitterInfo[] } {
  const leaves = new Map<number, Rect>();
  const splitters: SplitterInfo[] = [];
  if (node.kind === "leaf") {
    leaves.set(node.slot_id, rect);
    return { leaves, splitters };
  }
  const n = node.children.length;
  const ratios =
    node.ratios && node.ratios.length === n
      ? node.ratios
      : new Array(n).fill(1 / n);
  const total = ratios.reduce((a, b) => a + b, 0) || 1;
  const isH = node.orientation === "h";
  let offset = 0;
  for (let i = 0; i < n; i++) {
    const frac = ratios[i] / total;
    const childRect: Rect = isH
      ? { x: rect.x + offset, y: rect.y, w: rect.w * frac, h: rect.h }
      : { x: rect.x, y: rect.y + offset, w: rect.w, h: rect.h * frac };
    const sub = layoutTree(node.children[i], childRect, [...path, i]);
    sub.leaves.forEach((v, k) => leaves.set(k, v));
    splitters.push(...sub.splitters);
    if (i < n - 1) {
      const splitterRect: Rect = isH
        ? { x: childRect.x + childRect.w, y: childRect.y, w: 0, h: childRect.h }
        : { x: childRect.x, y: childRect.y + childRect.h, w: childRect.w, h: 0 };
      splitters.push({
        rect: splitterRect,
        orientation: node.orientation,
        nodePath: path,
        childIdx: i,
      });
    }
    offset += isH ? childRect.w : childRect.h;
  }
  return { leaves, splitters };
}

function getSplitNodeAt(root: SplitNode, path: number[]): SplitNode | null {
  let cur: SplitNode = root;
  for (const i of path) {
    if (cur.kind !== "split") return null;
    if (i < 0 || i >= cur.children.length) return null;
    cur = cur.children[i];
  }
  return cur;
}

// ---- 渲染 ----
export interface SplitViewProps {
  node: SplitNode;
  renderLeaf: (slotId: number) => JSX.Element;
  /** 拖动 splitter 时回调,父组件用 setRatiosAt 写回 splitTrees 信号 */
  onRatiosChange?: (path: number[], ratios: number[]) => void;
}

export const SplitView: Component<SplitViewProps> = (props) => {
  let containerEl!: HTMLDivElement;
  const layout = () => layoutTree(props.node);
  const slotIds = () => collectSlots(props.node);

  const startDrag = (s: SplitterInfo, e: MouseEvent) => {
    e.preventDefault();
    const containerRect = containerEl.getBoundingClientRect();
    const totalPx = s.orientation === "h" ? containerRect.width : containerRect.height;
    if (totalPx <= 0) return;
    const startPos = s.orientation === "h" ? e.clientX : e.clientY;

    const parent = getSplitNodeAt(props.node, s.nodePath);
    if (!parent || parent.kind !== "split") return;
    const n = parent.children.length;
    const originalRatios =
      parent.ratios && parent.ratios.length === n
        ? [...parent.ratios]
        : new Array(n).fill(1 / n);
    const total = originalRatios.reduce((a, b) => a + b, 0) || 1;
    const normalized = originalRatios.map((r) => r / total);
    const i = s.childIdx;
    const sumLR = normalized[i] + normalized[i + 1];

    const onMove = (mv: MouseEvent) => {
      const cur = s.orientation === "h" ? mv.clientX : mv.clientY;
      const deltaFrac = (cur - startPos) / totalPx;
      const newLeft = normalized[i] + deltaFrac;
      const newRight = sumLR - newLeft;
      const minFrac = 0.05;
      if (newLeft < minFrac || newRight < minFrac) return;
      const updated = [...normalized];
      updated[i] = newLeft;
      updated[i + 1] = newRight;
      props.onRatiosChange?.(s.nodePath, updated);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <div
      ref={containerEl}
      data-testid="split-view"
      style={{
        position: "relative",
        width: "100%",
        height: "100%",
        background: "var(--color-bg)",
      }}
    >
      {/* 所有 leaf 按 slotId key 扁平渲染 — 树重塑不引起 unmount */}
      <For each={slotIds()}>
        {(slotId) => (
          <Show when={layout().leaves.get(slotId)}>
            {(rect) => (
              <div
                style={{
                  position: "absolute",
                  left: `${rect().x}%`,
                  top: `${rect().y}%`,
                  width: `${rect().w}%`,
                  height: `${rect().h}%`,
                  "min-width": 0,
                  "min-height": 0,
                }}
              >
                {props.renderLeaf(slotId)}
              </div>
            )}
          </Show>
        )}
      </For>

      {/* Splitter handles — 4px 透明命中区 + 1px 居中视觉线 (absolute 内层, 顶满父宽/高) */}
      <For each={layout().splitters}>
        {(s) => (
          <div
            data-testid="split-divider"
            onMouseDown={(e) => startDrag(s, e)}
            style={
              s.orientation === "h"
                ? {
                    position: "absolute",
                    left: `calc(${s.rect.x}% - 2px)`,
                    top: `${s.rect.y}%`,
                    width: "4px",
                    height: `${s.rect.h}%`,
                    cursor: "col-resize",
                    background: "transparent",
                    "z-index": 1,
                  }
                : {
                    position: "absolute",
                    left: `${s.rect.x}%`,
                    top: `calc(${s.rect.y}% - 2px)`,
                    width: `${s.rect.w}%`,
                    height: "4px",
                    cursor: "row-resize",
                    background: "transparent",
                    "z-index": 1,
                  }
            }
          >
            <div
              style={
                s.orientation === "h"
                  ? {
                      position: "absolute",
                      left: "50%",
                      top: 0,
                      bottom: 0,
                      width: "1px",
                      transform: "translateX(-0.5px)",
                      background: "var(--color-border)",
                      "pointer-events": "none",
                    }
                  : {
                      position: "absolute",
                      top: "50%",
                      left: 0,
                      right: 0,
                      height: "1px",
                      transform: "translateY(-0.5px)",
                      background: "var(--color-border)",
                      "pointer-events": "none",
                    }
              }
            />
          </div>
        )}
      </For>
    </div>
  );
};

// 便利:从单叶子根树(初始状态)开始
export function singleLeaf(slotId: number): SplitNode {
  return { kind: "leaf", slot_id: slotId };
}
