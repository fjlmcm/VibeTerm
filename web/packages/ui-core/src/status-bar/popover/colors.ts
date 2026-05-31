// status-bar/popover/colors.ts — popover 专用配色.
//
// quotaBarColor 跟 widgets.tsx 的 pctColor 区分:
//   - pctColor 是"状态指示"用的 (低=text-2 灰, 跟 widget 文字一致, 不抢眼)
//   - quotaBarColor 是"进度条填充"用的, 低段必须跟 --color-border 槽底有对比,
//     否则用户看到的是"33% 文字但条不见"(灰填充 + 灰底 = 看不出来)
// 高水位 (≥80%) 仍走警示色, 中段橙黄, 低段用主文字色保证视觉存在.

export function quotaBarColor(pct: number): string {
  if (pct >= 80) return "var(--color-status-stalled, #d97757)";
  if (pct >= 50) return "var(--color-status-waiting, #f5a623)";
  return "var(--color-text)";
}
