// @vibeterm/ipc-types — Rust IPC schema 的 TS 镜像。
//
// 自 specta 接入起,镜像主体由 Rust 类型**自动生成**(./generated.ts,
// src-tauri 下 `VIBETERM_UPDATE_TS=1 cargo test --bin vibeterm ts_mirror` 重新生成;
// CI 上同名测试做逐字节比对,Rust 类型改了没再生成会直接红)。
// 本文件只保留:纯前端别名 / 事件 payload(非 Rust 类型)/ 历史命名的兼容别名。

export * from "./generated";

import type {
  ActiveBlock,
  BranchSpecDto,
  DailyStat,
  ExtraUsage,
  ModelStat,
  ProjectStat,
  QuotaWindow,
  RateLimit,
  SplitNode,
  Totals,
  UsageCache,
  WorktreeStatus,
} from "./generated";

// ---- 历史命名兼容别名(前端调用点零改动) ----
export type SplitTreeNode = SplitNode;
export type BranchSpec = BranchSpecDto;
export type ClaudeUsageCache = UsageCache;
export type ClaudeQuotaWindow = QuotaWindow;
export type ClaudeExtraUsage = ExtraUsage;
export type ClaudeActiveBlock = ActiveBlock;
export type CodexRateLimit = RateLimit;
export type GitStatusBrief = WorktreeStatus;
export type UsageTotals = Totals;
export type UsageDailyStat = DailyStat;
export type UsageModelStat = ModelStat;
export type UsageProjectStat = ProjectStat;

// ---- 纯手写部分(非 Rust 镜像) ----
// ---- IDs ----
export type TerminalId = number;

export type TaskId = number;

/** hook:claude/codex 的 permission_mode 字段值 */
export type PermissionMode =
  | "default"
  | "acceptEdits"
  | "plan"
  | "dontAsk"
  | "bypassPermissions";

/** 系统通知权限状态 — Tauri NotificationPermissionState 序列化为小写 string */
export type NotifyPermissionState = "granted" | "denied" | "default";

// ---- Status events ----
// Rust emit "agent_terminal_completed":某 task 的某终端 agent 刚完成一轮。
// 前端据此在切回该 task 时把焦点定位到最后完成的终端(一个 task 多 agent)。
export interface AgentTerminalCompleted {
  task_id: TaskId;
  terminal_id: TerminalId;
}

// ---- 辅助函数(非类型,specta 不生成) ----
import type { StatusLineItem, StatusLineItemDetail } from "./generated";

/** v1 字符串条目 / v2 对象条目 → 统一成对象形态 */
export function statusLineItemDetail(item: StatusLineItem): StatusLineItemDetail {
  return typeof item === "string" ? { type: item, metadata: {} } : item;
}
