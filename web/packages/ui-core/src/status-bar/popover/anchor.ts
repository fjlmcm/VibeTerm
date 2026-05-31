// status-bar/popover/anchor.ts — 定位 + 时间格式化 helpers.
//
// computeAnchor: trigger getBoundingClientRect → fixed top/bottom/left/maxHeight.
//   - 下方空间够 → 向下展开; 否则向上.
//   - 横向 left 夹到视口内 (left >= 8, left+width <= vw-8).
// format*: 把不同时间表示转成 popover 显示用文字.

/// trigger 元素的 bounding rect → popover 位置 (position: fixed 用).
export function computeAnchor(rect: DOMRect | undefined, popoverHeight = 400, popoverWidth = 420) {
  if (!rect) {
    return {
      top: "auto" as const,
      bottom: "auto" as const,
      left: "8px",
      maxHeight: "80vh",
    };
  }
  const vh = window.innerHeight;
  const vw = window.innerWidth;
  const spaceBelow = vh - rect.bottom - 8;
  const spaceAbove = rect.top - 8;
  // 优先下方; 不够时改向上, 取两者较大的可用高度做 max-height.
  const placeBelow = spaceBelow >= Math.min(popoverHeight, 300) || spaceBelow >= spaceAbove;
  const maxH = Math.max(180, Math.min(popoverHeight, placeBelow ? spaceBelow : spaceAbove));
  const left = Math.max(8, Math.min(rect.left, vw - popoverWidth - 8));
  if (placeBelow) {
    return {
      top: `${rect.bottom + 6}px`,
      bottom: "auto" as const,
      left: `${left}px`,
      maxHeight: `${maxH}px`,
    };
  }
  return {
    top: "auto" as const,
    bottom: `${vh - rect.top + 6}px`,
    left: `${left}px`,
    maxHeight: `${maxH}px`,
  };
}

/// remain ms → "Nh Nm" / "Nm" / "expired"
export function formatRemainMs(ms: number): string {
  if (ms <= 0) return "expired";
  const mins = Math.round(ms / 60000);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const rem = mins % 60;
  return rem > 0 ? `${hours}h ${rem}m` : `${hours}h`;
}

/// unix SEC (秒) → 本地 "HH:MM". 注意: 与 formatLocalDateHM (收毫秒) 单位相反, 勿混用.
export function formatLocalHM(epochSec: number | null | undefined): string | null {
  if (epochSec == null || epochSec <= 0) return null;
  const d = new Date(epochSec * 1000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/// unix MS (毫秒) → 本地 "MM-DD HH:MM". 注意: 与 formatLocalHM (收秒) 单位相反, 勿混用.
export function formatLocalDateHM(epochMs: number | null | undefined): string | null {
  if (epochMs == null || epochMs <= 0) return null;
  const d = new Date(epochMs);
  const mo = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${mo}-${dd} ${hh}:${mm}`;
}
