// menu-clamp.ts — 右键菜单视口定位:实测尺寸后翻转/夹取,保证整个菜单留在窗口内。
//
// 背景:裸 `position: fixed; left/top = clientX/Y` 有两类边缘病——
//   1. 靠右/下缘时菜单伸出窗口被裁切;
//   2. canvas 模式的 transform 祖先会把 fixed 退化成相对容器定位,菜单陷进卡片
//      stacking context 被侧栏/其他卡片遮挡。
// 解法:菜单一律 <Portal> 到 body(逃出 transform/stacking)+ 本 helper 夹取坐标。

/// 实测 `el` 尺寸后定位:越过右/下缘先翻转到光标另一侧(原生菜单行为),
/// 仍放不下(菜单比剩余空间大)再贴边。直接写 el.style.left/top。
export function clampMenuToViewport(el: HTMLElement, x: number, y: number, margin = 8): void {
  const { width: w, height: h } = el.getBoundingClientRect();
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  let left = x + w > vw - margin ? x - w : x;
  let top = y + h > vh - margin ? y - h : y;
  left = Math.min(Math.max(left, margin), Math.max(margin, vw - w - margin));
  top = Math.min(Math.max(top, margin), Math.max(margin, vh - h - margin));
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
}

/// ref 回调工厂。Solid 的 ref 在元素插入文档前触发(此时 rect 全 0),
/// 故推迟到 queueMicrotask —— 插入后、首帧绘制前,无闪跳。
export function menuClampRef(x: number, y: number, margin = 8): (el: HTMLElement) => void {
  return (el) => queueMicrotask(() => clampMenuToViewport(el, x, y, margin));
}
