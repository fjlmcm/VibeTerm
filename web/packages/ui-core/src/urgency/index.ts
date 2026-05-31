// Agent Urgency 评分(完整加权)
//
// 信号权重:
//   - waiting_input: 100
//   - waiting 持续时长: +10/min,封顶 +60(用 web 端 sticky waitingSince 估算)
//   - pinned: +30
//   - 最近 5 分钟内交互: +20
//   - 异常退出: 50
//   - running: 5
//   - idle: 0
//
// waitingSince:每次 task 状态 → waiting_input 时记一次 timestamp;变出去时清掉。
// 由 main.tsx 维护此 Map<TaskId, sinceMs> 并传入 rankByUrgency。

import type { TaskDto, TaskId } from "@vibeterm/ipc-types";

export interface UrgencyContext {
  /** TaskId → 首次进入 waiting_input 的 unix ms */
  waitingSince?: Map<TaskId, number>;
  /** 用户最近交互的 TaskId → unix ms(目前未传)*/
  lastInteractAt?: Map<TaskId, number>;
  /** 当前时间(测试用) */
  now?: number;
}

export function urgencyScore(t: TaskDto, ctx: UrgencyContext = {}): number {
  const now = ctx.now ?? Date.now();
  let score = 0;
  if (t.status === "waiting_input") {
    score += 100;
    const since = ctx.waitingSince?.get(t.id);
    if (since !== undefined) {
      const minutes = (now - since) / 60_000;
      score += Math.min(60, minutes * 10); // +10/min,封顶 60
    }
  } else if (t.status === "stalled") {
    score += 50; // 卡住/无响应(权重对齐"异常退出: 50")
  } else if (t.status === "running") {
    score += 5;
  }
  if (t.pinned) score += 30;
  const lastInteract = ctx.lastInteractAt?.get(t.id);
  if (lastInteract !== undefined && now - lastInteract < 5 * 60_000) {
    score += 20;
  }
  return score;
}

export function rankByUrgency(tasks: TaskDto[], ctx: UrgencyContext = {}): TaskDto[] {
  return [...tasks]
    .map((t) => ({ t, s: urgencyScore(t, ctx) }))
    .sort((a, b) => b.s - a.s)
    .map(({ t }) => t);
}

/** 返回 TaskId → score map,供 TaskList 渲染 badge */
export function urgencyScoreMap(
  tasks: TaskDto[],
  ctx: UrgencyContext = {},
): Map<TaskId, number> {
  const m = new Map<TaskId, number>();
  for (const t of tasks) m.set(t.id, urgencyScore(t, ctx));
  return m;
}

/** 按 score 给 CSS 颜色 token(三档) */
export function urgencyColorVar(score: number): string {
  if (score >= 100) return "var(--color-status-waiting)";
  if (score >= 50) return "var(--color-accent)";
  return "var(--color-text-2)";
}

/**
 * 维护 waitingSince Map 的辅助函数:
 *   - tasks 中谁现在 waiting_input 但 map 没记 → 记 now
 *   - tasks 中谁不再 waiting_input 但 map 记了 → 删
 */
export function updateWaitingSince(
  current: Map<TaskId, number>,
  tasks: TaskDto[],
  now: number = Date.now(),
): Map<TaskId, number> {
  const next = new Map(current);
  for (const t of tasks) {
    if (t.status === "waiting_input") {
      if (!next.has(t.id)) next.set(t.id, now);
    } else {
      next.delete(t.id);
    }
  }
  // 删 tasks 中不存在的
  for (const id of next.keys()) {
    if (!tasks.find((t) => t.id === id)) next.delete(id);
  }
  return next;
}
