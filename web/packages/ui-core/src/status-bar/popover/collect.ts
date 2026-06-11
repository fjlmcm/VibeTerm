// status-bar/popover/collect.ts — Claude / Codex → AgentPanelData 映射.
//
// 目的: AgentPanel 是个不挑 agent 的通用模板, collect 函数负责把各 agent 的
// 异构数据 (ClaudeSession + UsageCache + ActiveBlock + plan; CodexSnapshot + 自算 block)
// 投影到同一个 AgentPanelData shape, 保证 panel 排版骨架对齐.
//
// quotas[] 固定 [5h, 7d] 两槽位 — 数据缺则 pct=null (QuotaRow 显示 "—"), 永远不
// skip 整行,以免 panel 长度随 agent 跳动.

import type { RenderContext } from "../widgets";
import { t } from "../../i18n";
import { formatLocalHM, formatLocalDateHM } from "./anchor";
import {
  formatReset,
  formatResetUnix,
  formatCodexWindow,
  pickCodexShortWindow,
  pickCodexLongWindow,
} from "../widgets";

/// 通用 agent 详情数据 — Claude 跟 Codex 都映射到这套字段, AgentPanel 渲染一致.
export interface AgentPanelData {
  hasSession: boolean;
  /** Session 段 */
  model?: string | null;
  provider?: string | null;
  plan?: string | null;
  effort?: string | null;
  sessionId?: string | null;
  contextTokens?: number | null;
  contextWindow?: number | null;
  /** 显示用的 ctx 百分比 — 优先用 agent 自带 (Codex CLI 扣 baseline), null 时面板按 tokens/window fallback */
  contextUsedPct?: number | null;
  cliVersion?: string | null;
  /** Quota 段 — 每条 (label, pct, reset 文案) */
  quotas: Array<{
    key: string;
    label: string;
    pct: number | null;
    resetLabel?: string | null;
    resetAt?: string | null;
  }>;
  /** Quota 段尾巴: extra credits 文案 */
  extraCredits?: string | null;
  /** Usage 段 — 5h block / burn rate / elapsed / 24h tokens */
  tokensUsed?: number | null;
  burnRate?: { rate: number; level: string } | null;
  elapsed?: { pct: number; remainMs: number } | null;
  tokens24h?: number | null;
  /** Prompt cache TTL (Claude only) — unix ms */
  cache5mUntilMs?: number | null;
  cache1hUntilMs?: number | null;
}

/// Claude → AgentPanelData. quotas 永远固定 [5h, 7d] 两槽位 (即使数据缺也保留).
export function collectClaudeData(ctx: RenderContext): AgentPanelData {
  const s = ctx.claudeSession();
  const cache = ctx.claudeUsage();
  const block = ctx.claudeBlock();
  const plan = ctx.claudePlan();
  const tokens24h = ctx.claudeTokensToday();

  const quotas: AgentPanelData["quotas"] = [
    {
      key: "5h",
      label: t("statusbar.popover.5h_block"),
      pct: cache?.five_hour?.utilization ?? null,
      resetLabel: cache?.five_hour ? formatReset(cache.five_hour.resets_at) : null,
    },
    {
      key: "7d",
      label: t("statusbar.popover.7d_window"),
      pct: cache?.seven_day?.utilization ?? null,
      resetLabel: cache?.seven_day ? formatReset(cache.seven_day.resets_at) : null,
    },
  ];
  if (cache?.seven_day_sonnet && (cache.seven_day_sonnet.utilization ?? 0) > 0) {
    quotas.push({
      key: "7d_sonnet",
      label: t("statusbar.popover.7d_sonnet"),
      pct: cache.seven_day_sonnet.utilization,
      resetLabel: formatReset(cache.seven_day_sonnet.resets_at),
    });
  }
  if (cache?.seven_day_opus && (cache.seven_day_opus.utilization ?? 0) > 0) {
    quotas.push({
      key: "7d_opus",
      label: t("statusbar.popover.7d_opus"),
      pct: cache.seven_day_opus.utilization,
      resetLabel: formatReset(cache.seven_day_opus.resets_at),
    });
  }

  let extraCredits: string | null = null;
  if (cache?.extra_usage?.is_enabled) {
    const e = cache.extra_usage;
    extraCredits =
      e.used_credits != null
        ? `${e.used_credits.toFixed(2)} ${e.currency ?? ""}${e.monthly_limit != null ? ` / ${e.monthly_limit}` : ""}`
        : "enabled";
  }

  return {
    hasSession: s != null,
    model: s?.model ?? null,
    provider: null,
    plan,
    effort: null,
    sessionId: s?.session_id ?? null,
    contextTokens: s?.context_tokens ?? null,
    contextWindow: s?.context_window ?? null,
    contextUsedPct: null, // Claude 没专用算法, 用 tokens/window
    cliVersion: null,
    quotas,
    extraCredits,
    tokensUsed: block?.tokens_used ?? null,
    burnRate:
      block && block.tokens_per_min_recent > 0
        ? { rate: block.tokens_per_min_recent, level: block.burn_rate_level }
        : null,
    elapsed: block ? { pct: block.elapsed_pct, remainMs: block.remaining_ms } : null,
    tokens24h: tokens24h > 0 ? tokens24h : null,
    cache5mUntilMs: s?.cache_5m_until_ms ?? null,
    cache1hUntilMs: s?.cache_1h_until_ms ?? null,
  };
}

/// Codex → AgentPanelData. 5h block 优先用本地 ccusage 移植算法 (`ctx.codexBlock`),
/// 没算到才回退到 rate_limits.primary/secondary 按 window_minutes 选.
/// 7d 仍走 rate_limits (服务端权威).
export function collectCodexData(ctx: RenderContext): AgentPanelData {
  const c = ctx.codexSnap();
  const block = ctx.codexBlock?.() ?? null;

  // 短窗(5h 之类)优先本地 block (含 tokens_used + cost), 没有再看 rate_limits.
  // 长窗(>=1天)走 rate_limits —— 周(10080)/月(43200) 按 window_minutes 自动选, 不锁死分钟数
  // (free 计划 2026-06 起从 7d 改月度). 用 widgets 里同一套 picker, 分类逻辑单一来源.
  const shortFromLimits = pickCodexShortWindow(c);
  const longFromLimits = pickCodexLongWindow(c);

  // 优先 block (本地算的), fallback rate_limits 短窗
  const fiveHPct = block != null ? block.elapsed_pct : (shortFromLimits?.used_percent ?? null);
  const fiveHResetUnix =
    block != null ? Math.floor(block.end_at_ms / 1000) : (shortFromLimits?.resets_at ?? null);

  const quotas: AgentPanelData["quotas"] = [
    {
      key: "5h",
      label: t("statusbar.popover.5h_block"),
      pct: fiveHPct,
      resetLabel: fiveHResetUnix != null ? formatResetUnix(fiveHResetUnix) : null,
      resetAt: fiveHResetUnix != null ? formatLocalHM(fiveHResetUnix) : null,
    },
    {
      key: "7d",
      // 标签跟随实际窗口: 周→"7d", 月→"30d"; 无数据时回退到 i18n 占位.
      label:
        longFromLimits?.window_minutes != null
          ? formatCodexWindow(longFromLimits.window_minutes)
          : t("statusbar.popover.7d_window"),
      pct: longFromLimits?.used_percent ?? null,
      resetLabel: longFromLimits ? formatResetUnix(longFromLimits.resets_at ?? null) : null,
      resetAt:
        longFromLimits && longFromLimits.resets_at != null
          ? formatLocalDateHM(longFromLimits.resets_at * 1000)
          : null,
    },
  ];

  return {
    hasSession: c != null,
    model: c?.model ?? null,
    provider: c?.model_provider ?? null,
    plan: c?.plan_type ?? null,
    effort: c?.effort ?? null,
    sessionId: c?.session_id ?? null,
    contextTokens: c?.context_tokens ?? null,
    contextWindow: c?.context_window ?? null,
    contextUsedPct: c?.context_used_pct ?? null,
    cliVersion: c?.cli_version ?? null,
    quotas,
    extraCredits: null,
    tokensUsed: block?.tokens_used ?? null,
    burnRate:
      block && block.tokens_per_min_recent > 0
        ? { rate: block.tokens_per_min_recent, level: block.burn_rate_level }
        : c && c.tokens_per_min_recent > 0
          ? { rate: c.tokens_per_min_recent, level: c.burn_rate_level }
          : null,
    elapsed: block ? { pct: block.elapsed_pct, remainMs: block.remaining_ms } : null,
    tokens24h: null,
    cache5mUntilMs: null, // Codex 没 prompt cache 概念
    cache1hUntilMs: null,
  };
}
