// Widget 注册表 — 状态栏可自定义 v1.
//
// 设计原则 (借鉴 ccstatusline / Codex /statusline):
//   - 每个 widget = 一个纯函数 (item, ctx) → JSX | null
//   - widget 自决定能否 render: 数据不适用 (e.g. claude widget 在 Codex 终端) 返回 null
//   - 配置层只是 ordered list of widget id, 不写 if/else 分支
//   - item.color / bold / max_width / metadata 由 widget 自己解释
//
// 当前 v1 范围: cwd / git / claude-* / codex-* — 9 个 widget.
// v2 加: 5h block / burn rate / session cost / today cost (移植 ccusage 算法).

import { Show, type Accessor, type Component, type JSX } from "solid-js";
import type {
  ClaudeActiveBlock,
  ClaudeQuotaWindow,
  ClaudeSession,
  ClaudeUsageCache,
  CodexRateLimit,
  CodexSnapshot,
  GitStatusBrief,
  StatusLineItemDetail,
  TaskDto,
} from "@vibeterm/ipc-types";
import { formatRemainMs } from "./popover/anchor";

// ---- 公共 helpers ----

// burn rate 数值格式化:≥1000 → "N.Nk",否则取整(claude/codex 两个 burn-rate widget 共用)。
function formatBurnRate(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)}k` : Math.round(value).toString();
}

export function pctColor(pct: number): string {
  if (pct >= 80) return "var(--color-status-stalled, #d97757)";
  if (pct >= 50) return "var(--color-status-waiting, #f5a623)";
  return "var(--color-text-2)";
}

export function formatReset(iso: string | null): string {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return "";
  const diff = t - Date.now();
  if (diff <= 0) return "now";
  const mins = Math.round(diff / 60000);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const rem = mins % 60;
  if (hours < 24) return rem > 0 ? `${hours}h ${rem}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  const remHr = hours % 24;
  return remHr > 0 ? `${days}d ${remHr}h` : `${days}d`;
}

export function formatResetUnix(sec: number | null): string {
  if (sec == null) return "—";
  const diff = sec * 1000 - Date.now();
  if (diff <= 0) return "now";
  const mins = Math.round(diff / 60000);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const rem = mins % 60;
  if (hours < 24) return rem > 0 ? `${hours}h ${rem}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  const remHr = hours % 24;
  return remHr > 0 ? `${days}d ${remHr}h` : `${days}d`;
}

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
  return `${n}`;
}

export function formatCodexWindow(mins: number | null): string {
  if (mins == null) return "?";
  if (mins >= 1440) return `${Math.round(mins / 1440)}d`;
  if (mins >= 60) return `${Math.round(mins / 60)}h`;
  return `${mins}m`;
}

export function shortenModel(model: string | null): string {
  if (!model) return "";
  const m = model.match(/^claude-(opus|sonnet|haiku)-(\d+)-(\d+)/i);
  if (m) return `${m[1]} ${m[2]}.${m[3]}`;
  return model.replace(/^claude-/, "");
}

export function shortenCodexModel(model: string | null): string {
  if (!model) return "";
  return model.replace(/^gpt-/i, "gpt ");
}

export function shortenCwd(cwd: string, max: number = 200): string {
  // 各平台 home 根前缀 (macOS / Linux / Windows). 命中后把 "<前缀><user>" 段缩成 "~".
  const HOME_PREFIXES = ["/Users/", "/home/", "C:\\Users\\"];
  let out = cwd;
  for (const prefix of HOME_PREFIXES) {
    if (cwd.startsWith(prefix)) {
      const rest = cwd.slice(prefix.length);
      // 同时认 "/" 与 "\" 作为 user 段之后的分隔符 (Windows 用反斜杠).
      const slashIdx = rest.indexOf("/");
      const backslashIdx = rest.indexOf("\\");
      const sep =
        slashIdx < 0
          ? backslashIdx
          : backslashIdx < 0
            ? slashIdx
            : Math.min(slashIdx, backslashIdx);
      if (sep > 0) out = "~" + rest.slice(sep);
      break;
    }
  }
  if (out.length > max) out = `…${out.slice(-(max - 1))}`;
  return out;
}

// ---- RenderContext: widget 共享的响应式数据 ----

export interface RenderContext {
  cwd: Accessor<string | null>;
  git: Accessor<GitStatusBrief | null>;
  /** 当前 cwd 的 stash 数, 没 git 仓库为 0 */
  gitStashCount: Accessor<number>;
  /** 跨所有 Claude project 今天累计 token 用量 (过去 24h) */
  claudeTokensToday: Accessor<number>;
  /** Claude 订阅 plan ("Max 20x" / "Pro" / "Free" / ...) */
  claudePlan: Accessor<string | null>;
  /** 当前 cwd 的 PR 状态 ("open" / "draft" / "merged" / "closed" / null) */
  prStatus: Accessor<string | null>;
  agentKind: Accessor<string | null>;
  claudeSession: Accessor<ClaudeSession | null>;
  claudeUsage: Accessor<ClaudeUsageCache | null>;
  claudeBlock: Accessor<ClaudeActiveBlock | null>;
  codexSnap: Accessor<CodexSnapshot | null>;
  /** Codex 5h 滑动块 (ccusage 算法本地移植到 codex rollout); optional 给老代码兜底 */
  codexBlock?: Accessor<ClaudeActiveBlock | null>;
  /** 当前活跃 task — task-status / task-name / worktree-name widget 用 */
  task: Accessor<TaskDto | null>;
}

// widget 渲染签名 — 返回 null 即不显示 (条件隐藏靠这个)
export type WidgetRenderer = (
  item: StatusLineItemDetail,
  ctx: RenderContext,
) => JSX.Element | null;

// ---- 共用可视化子组件 ----

/** 紧凑 progress bar — 用作 metadata.style="bar" 渲染. */
const MiniBar: Component<{ pct: number; color?: string; label?: string }> = (props) => (
  <span style={{ display: "inline-flex", "align-items": "center", gap: "5px", "font-variant-numeric": "tabular-nums" }}>
    <Show when={props.label}>
      <span style={{ opacity: 0.7, "font-size": "10px" }}>{props.label}</span>
    </Show>
    <span
      style={{
        display: "inline-block",
        width: "44px",
        height: "5px",
        "border-radius": "3px",
        background: "var(--color-border)",
        overflow: "hidden",
        position: "relative",
      }}
    >
      <span
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: `${Math.min(100, Math.max(0, props.pct))}%`,
          background: props.color ?? pctColor(props.pct),
        }}
      />
    </span>
    <span style={{ color: props.color ?? pctColor(props.pct), "font-weight": 500 }}>
      {Math.round(props.pct)}%
    </span>
  </span>
);

/** 半圆 gauge — SVG, ccstatusline 同款风格. */
const Gauge: Component<{ pct: number; color?: string; label?: string }> = (props) => {
  const radius = 8;
  const circ = Math.PI * radius;
  const dash = (Math.min(100, Math.max(0, props.pct)) / 100) * circ;
  return (
    <span style={{ display: "inline-flex", "align-items": "center", gap: "5px" }}>
      <Show when={props.label}>
        <span style={{ opacity: 0.7, "font-size": "10px" }}>{props.label}</span>
      </Show>
      <svg width="20" height="12" viewBox="0 0 20 12">
        <path
          d={`M 2 10 A ${radius} ${radius} 0 0 1 18 10`}
          fill="none"
          stroke="var(--color-border)"
          stroke-width="2"
          stroke-linecap="round"
        />
        <path
          d={`M 2 10 A ${radius} ${radius} 0 0 1 18 10`}
          fill="none"
          stroke={props.color ?? pctColor(props.pct)}
          stroke-width="2"
          stroke-linecap="round"
          stroke-dasharray={`${dash} ${circ}`}
        />
      </svg>
      <span
        style={{
          color: props.color ?? pctColor(props.pct),
          "font-weight": 500,
          "font-variant-numeric": "tabular-nums",
        }}
      >
        {Math.round(props.pct)}%
      </span>
    </span>
  );
};

/** 取 item.metadata.style — 默认 "text". 大部分 % widget 都支持 text/bar/gauge. */
function styleMode(item: StatusLineItemDetail): "text" | "bar" | "gauge" {
  const m = item.metadata?.style;
  if (m === "bar" || m === "gauge") return m;
  return "text";
}

// ---- 单 widget 渲染函数 ----

const cwdWidget: WidgetRenderer = (item, ctx) => {
  const v = ctx.cwd();
  if (!v) return null;
  const max = item.max_width ?? 200;
  return (
    <span
      title={v}
      style={{
        color: item.color ?? "var(--color-text)",
        "font-weight": item.bold ? 500 : undefined,
        "white-space": "nowrap",
        overflow: "hidden",
        "text-overflow": "ellipsis",
        "max-width": `${max}px`,
      }}
    >
      {shortenCwd(v, max)}
    </span>
  );
};

const gitBranchWidget: WidgetRenderer = (item, ctx) => {
  const g = ctx.git();
  if (!g || !g.branch) return null;
  const color = item.color
    ?? (g.is_dirty ? "var(--color-status-waiting, #f5a623)" : "var(--color-text-2)");
  return (
    <span
      title={`branch: ${g.branch}${g.is_dirty ? " (dirty)" : ""}${g.ahead ? ` · ↑${g.ahead}` : ""}${g.behind ? ` · ↓${g.behind}` : ""}`}
      style={{
        "font-variant-numeric": "tabular-nums",
        color,
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      ⎇ {g.branch}
      <Show when={g.is_dirty}>
        <span style={{ "margin-left": "2px" }}>●</span>
      </Show>
      <Show when={g.ahead > 0}>
        <span style={{ "margin-left": "4px", opacity: 0.7 }}>↑{g.ahead}</span>
      </Show>
      <Show when={g.behind > 0}>
        <span style={{ "margin-left": "4px", opacity: 0.7 }}>↓{g.behind}</span>
      </Show>
    </span>
  );
};

function gitCountChip(label: string, count: number, color: string, item: StatusLineItemDetail, hideZero = true): JSX.Element | null {
  if (count === 0 && hideZero) return null;
  return (
    <span
      title={`${label}: ${count}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? color,
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>{label}</span>
      <span style={{ "font-weight": 500 }}>{count}</span>
    </span>
  );
}

const gitStagedWidget: WidgetRenderer = (item, ctx) => {
  const g = ctx.git();
  if (!g) return null;
  return gitCountChip("●staged", g.staged ?? 0, "var(--color-status-running, #10a37f)", item);
};

const gitUnstagedWidget: WidgetRenderer = (item, ctx) => {
  const g = ctx.git();
  if (!g) return null;
  return gitCountChip("●unstaged", g.unstaged ?? 0, "var(--color-status-waiting, #f5a623)", item);
};

const gitUntrackedWidget: WidgetRenderer = (item, ctx) => {
  const g = ctx.git();
  if (!g) return null;
  return gitCountChip("?untracked", g.untracked ?? 0, "var(--color-text-2)", item);
};

const claudeTokensTodayWidget: WidgetRenderer = (item, ctx) => {
  const n = ctx.claudeTokensToday();
  if (n === 0 && item.metadata?.showZero !== "true") return null;
  return (
    <span
      title={`Claude tokens (last 24h): ${n}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? "var(--color-text-2)",
        "font-variant-numeric": "tabular-nums",
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>24h</span>
      <span style={{ "font-weight": 500 }}>{formatTokens(n)}</span>
    </span>
  );
};

const prStatusWidget: WidgetRenderer = (item, ctx) => {
  const s = ctx.prStatus();
  if (!s) return null;
  const colors: Record<string, string> = {
    open: "var(--color-status-running, #10a37f)",
    draft: "var(--color-text-2)",
    merged: "var(--color-accent)",
    closed: "var(--color-status-stalled, #d97757)",
  };
  return (
    <span
      title={`PR: ${s}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? colors[s] ?? "var(--color-text-2)",
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>PR</span>
      <span>{s}</span>
    </span>
  );
};

const gitStashCountWidget: WidgetRenderer = (item, ctx) => {
  const n = ctx.gitStashCount();
  if (n === 0 && item.metadata?.showZero !== "true") return null;
  return (
    <span
      title={`stash: ${n}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? "var(--color-text-2)",
        "font-variant-numeric": "tabular-nums",
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>stash</span>
      <span style={{ "font-weight": 500 }}>{n}</span>
    </span>
  );
};

const claudeModelWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const s = ctx.claudeSession();
  if (!s?.model) return null;
  return (
    <span
      title={`model: ${s.model}`}
      style={{
        color: item.color ?? "var(--color-text)",
        "font-weight": (item.bold ?? true) ? 500 : undefined,
      }}
    >
      {shortenModel(s.model)}
    </span>
  );
};

const claudeCtxWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const s = ctx.claudeSession();
  // context_window <= 0 会让 pct 变成 Infinity, 一并守卫掉.
  if (!s || s.context_tokens == null || s.context_window == null || s.context_window <= 0)
    return null;
  const pct = (s.context_tokens / s.context_window) * 100;
  const mode = styleMode(item);
  const title = `context: ${s.context_tokens} / ${s.context_window}`;
  if (mode === "bar") return <span title={title}><MiniBar pct={pct} color={item.color ?? undefined} label="ctx" /></span>;
  if (mode === "gauge") return <span title={title}><Gauge pct={pct} color={item.color ?? undefined} label="ctx" /></span>;
  return (
    <span
      title={title}
      style={{
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? pctColor(pct),
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7 }}>ctx</span> {Math.round(pct)}%
    </span>
  );
};

const claudeQuotaPill = (label: string, w: ClaudeQuotaWindow | null, item: StatusLineItemDetail) => {
  if (!w) return null;
  const pct = Math.round(w.utilization);
  const threshold = parseInt(item.metadata?.hideUnderThreshold ?? "0", 10);
  if (Number.isFinite(threshold) && pct < threshold) return null;
  const reset = formatReset(w.resets_at);
  const showReset = item.metadata?.showReset !== "false";
  const mode = styleMode(item);
  const title = `${label}: ${pct}% used${reset ? ` · resets in ${reset}` : ""}`;
  if (mode === "bar") return <span title={title}><MiniBar pct={w.utilization} color={item.color ?? undefined} label={label} /></span>;
  if (mode === "gauge") return <span title={title}><Gauge pct={w.utilization} color={item.color ?? undefined} label={label} /></span>;
  return (
    <span
      title={title}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? pctColor(w.utilization),
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7 }}>{label}</span>
      <span style={{ "font-weight": 500 }}>{pct}%</span>
      <Show when={showReset && reset}>
        <span style={{ opacity: 0.5, "font-size": "10px" }}>({reset})</span>
      </Show>
    </span>
  );
};

const claude5hWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  return claudeQuotaPill("5h", ctx.claudeUsage()?.five_hour ?? null, item);
};

const claude7dWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  return claudeQuotaPill("7d", ctx.claudeUsage()?.seven_day ?? null, item);
};

const claude7dSonnetWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  return claudeQuotaPill("7d-sonnet", ctx.claudeUsage()?.seven_day_sonnet ?? null, item);
};

const claude7dOpusWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  return claudeQuotaPill("7d-opus", ctx.claudeUsage()?.seven_day_opus ?? null, item);
};

// ---- Claude 5h block (移植 ccusage `blocks.rs`) ----
// formatRemainMs 从 ./popover/anchor 引入(同一实现,去重)。

const claudeBlockPctWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const b = ctx.claudeBlock();
  if (!b) return null;
  const pct = b.elapsed_pct;
  const reset = formatRemainMs(b.remaining_ms);
  const showReset = item.metadata?.showReset !== "false";
  const mode = styleMode(item);
  const title = `5h block: ${Math.round(pct)}% elapsed · ${reset} left · ${b.tokens_used} tokens`;
  if (mode === "bar") return <span title={title}><MiniBar pct={pct} color={item.color ?? undefined} label="block" /></span>;
  if (mode === "gauge") return <span title={title}><Gauge pct={pct} color={item.color ?? undefined} label="block" /></span>;
  return (
    <span
      title={title}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? pctColor(pct),
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7 }}>block</span>
      <span style={{ "font-weight": 500 }}>{Math.round(pct)}%</span>
      <Show when={showReset}>
        <span style={{ opacity: 0.5, "font-size": "10px" }}>({reset})</span>
      </Show>
    </span>
  );
};

const claudeBlockTokensWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const b = ctx.claudeBlock();
  if (!b) return null;
  return (
    <span
      title={`5h block tokens: ${b.tokens_used}`}
      style={{
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? "var(--color-text-2)",
      }}
    >
      <span style={{ opacity: 0.7 }}>block</span> {formatTokens(b.tokens_used)}
    </span>
  );
};

const claudeBlockRemainingWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const b = ctx.claudeBlock();
  if (!b) return null;
  return (
    <span
      title={`block resets in ${formatRemainMs(b.remaining_ms)}`}
      style={{
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? "var(--color-text-2)",
      }}
    >
      <span style={{ opacity: 0.7 }}>resets</span> {formatRemainMs(b.remaining_ms)}
    </span>
  );
};

function burnRateColor(level: string): string {
  if (level === "high") return "var(--color-status-stalled, #d97757)";
  if (level === "moderate") return "var(--color-status-waiting, #f5a623)";
  return "var(--color-text-2)";
}

/// claude-cache-ttl widget — prompt cache 倒计时.
/// Anthropic 5m / 1h 两档 TTL 各独立, 从最后一次写入新 cache 起算.
/// 取两者较小的"还剩多久"显示, title 给完整明细. 都过期或都没用 → 隐藏.
const claudeCacheTtlWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const s = ctx.claudeSession();
  if (!s) return null;
  const now = Date.now();
  const r5 = s.cache_5m_until_ms != null ? s.cache_5m_until_ms - now : null;
  const r1 = s.cache_1h_until_ms != null ? s.cache_1h_until_ms - now : null;
  // 至少一个未过期才显示
  const active: Array<{ label: string; remainMs: number }> = [];
  if (r5 != null && r5 > 0) active.push({ label: "5m", remainMs: r5 });
  if (r1 != null && r1 > 0) active.push({ label: "1h", remainMs: r1 });
  if (active.length === 0) return null;
  // 主显示: 取最早到期的那个 (用户最需要关注)
  active.sort((a, b) => a.remainMs - b.remainMs);
  const primary = active[0];
  const fmt = (ms: number) => {
    const sec = Math.max(0, Math.floor(ms / 1000));
    if (sec < 60) return `${sec}s`;
    const min = Math.floor(sec / 60);
    if (min < 60) return `${min}m${sec % 60 > 0 && min < 10 ? `${sec % 60}s` : ""}`;
    return `${Math.floor(min / 60)}h${min % 60}m`;
  };
  const titleParts: string[] = [];
  if (r5 != null) titleParts.push(`5m cache: ${r5 > 0 ? fmt(r5) : "expired"}`);
  if (r1 != null) titleParts.push(`1h cache: ${r1 > 0 ? fmt(r1) : "expired"}`);
  return (
    <span
      title={titleParts.join(" · ")}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? "var(--color-text-2)",
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>cache</span>
      <span style={{ "font-weight": 500 }}>{primary.label}</span>
      <span>{fmt(primary.remainMs)}</span>
    </span>
  );
};

const claudeBurnRateWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const b = ctx.claudeBlock();
  if (!b || b.tokens_per_min_recent <= 0) return null;
  const showAvg = item.metadata?.showAvg === "true";
  const value = showAvg ? b.tokens_per_min_avg : b.tokens_per_min_recent;
  const formatted = formatBurnRate(value);
  return (
    <span
      title={`burn rate: recent ${Math.round(b.tokens_per_min_recent)} tok/min · ${b.burn_rate_level}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? burnRateColor(b.burn_rate_level),
      }}
    >
      <span style={{ opacity: 0.7 }}>burn</span>
      <span style={{ "font-weight": 500 }}>{formatted}</span>
      <span style={{ opacity: 0.5, "font-size": "10px" }}>t/m</span>
    </span>
  );
};

const codexModelWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  const c = ctx.codexSnap();
  if (!c?.model) return null;
  return (
    <span
      title={`model: ${c.model} (${c.model_provider ?? "?"})`}
      style={{
        color: item.color ?? "var(--color-text)",
        "font-weight": 500,
      }}
    >
      {shortenCodexModel(c.model)}
    </span>
  );
};

const codexCtxWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  const c = ctx.codexSnap();
  // context_window <= 0 会让 fallback ratio 变成 Infinity, 一并守卫掉.
  if (!c || c.context_tokens == null || c.context_window == null || c.context_window <= 0)
    return null;
  // 优先用后端 context_used_pct (按 Codex CLI 算法, 扣 12000 baseline).
  // fallback 到原始 ratio 仅为防字段缺失.
  const pct =
    c.context_used_pct != null
      ? c.context_used_pct
      : (c.context_tokens / c.context_window) * 100;
  const mode = styleMode(item);
  const title = `context: ${c.context_tokens} / ${c.context_window} (${Math.round(pct)}%)`;
  if (mode === "bar") return <span title={title}><MiniBar pct={pct} color={item.color ?? undefined} label="ctx" /></span>;
  if (mode === "gauge") return <span title={title}><Gauge pct={pct} color={item.color ?? undefined} label="ctx" /></span>;
  return (
    <span
      title={title}
      style={{
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? pctColor(pct),
      }}
    >
      <span style={{ opacity: 0.7 }}>ctx</span> {Math.round(pct)}%
    </span>
  );
};

const codexLimitPill = (label: string | null, l: CodexRateLimit | null, item: StatusLineItemDetail) => {
  if (!l) return null;
  const pct = Math.round(l.used_percent);
  const threshold = parseInt(item.metadata?.hideUnderThreshold ?? "0", 10);
  if (Number.isFinite(threshold) && pct < threshold) return null;
  const win = label ?? formatCodexWindow(l.window_minutes ?? null);
  const reset = formatResetUnix(l.resets_at ?? null);
  const showReset = item.metadata?.showReset !== "false";
  const mode = styleMode(item);
  const title = `${win} window · ${pct}% used · resets in ${reset}`;
  if (mode === "bar") return <span title={title}><MiniBar pct={l.used_percent} color={item.color ?? undefined} label={win} /></span>;
  if (mode === "gauge") return <span title={title}><Gauge pct={l.used_percent} color={item.color ?? undefined} label={win} /></span>;
  return (
    <span
      title={title}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? pctColor(l.used_percent),
      }}
    >
      <span style={{ opacity: 0.7 }}>{win}</span>
      <span style={{ "font-weight": 500 }}>{pct}%</span>
      <Show when={showReset}>
        <span style={{ opacity: 0.5, "font-size": "10px" }}>({reset})</span>
      </Show>
    </span>
  );
};

// Codex 额度窗口按"时长类别"挑, 不锁死具体分钟数 —— 服务端会改: free 计划 2026-06 起
// 把长窗从周(7d=10080)改成月度(30d=43200), primary/secondary 语义也随 plan 反转
// (free: primary=长窗 secondary=null; pro: primary=5h secondary=长窗). 用 1 天为界分
// 短/长窗, 对未来再变窗(如 14d)也鲁棒. window_minutes / used_percent / resets_at 均服务端权威.
const CODEX_LONG_WINDOW_MIN_MINUTES = 1440; // >= 1 天算长窗

type CodexLimitWithWindow = CodexRateLimit & { window_minutes: number };

function codexLimitsWithWindow(snap: CodexSnapshot | null): CodexLimitWithWindow[] {
  if (!snap) return [];
  // window_minutes 可能为 null(free 计划/新模型) → 无法归类, 过滤掉.
  return [snap.primary_limit, snap.secondary_limit].filter(
    (x): x is CodexLimitWithWindow => x != null && x.window_minutes != null,
  );
}

/** 短窗 (< 1 天, 如 5h=300) —— pro 计划才有; free 计划无短窗 → null. 多个取最短. */
export function pickCodexShortWindow(snap: CodexSnapshot | null): CodexRateLimit | null {
  return (
    codexLimitsWithWindow(snap)
      .filter((l) => l.window_minutes < CODEX_LONG_WINDOW_MIN_MINUTES)
      .sort((a, b) => a.window_minutes - b.window_minutes)[0] ?? null
  );
}

/** 长窗 (>= 1 天) —— 周(10080)或月(43200), 多个取最长. free 计划唯一的窗口. */
export function pickCodexLongWindow(snap: CodexSnapshot | null): CodexRateLimit | null {
  return (
    codexLimitsWithWindow(snap)
      .filter((l) => l.window_minutes >= CODEX_LONG_WINDOW_MIN_MINUTES)
      .sort((a, b) => b.window_minutes - a.window_minutes)[0] ?? null
  );
}

const codex5hWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  // label=null → 由 window_minutes 动态渲染真实窗口 (5h / 1h …), 不硬编码.
  return codexLimitPill(null, pickCodexShortWindow(ctx.codexSnap()), item);
};

const codex7dWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  // label=null → 周显示 "7d", 月显示 "30d", 跟随服务端实际窗口.
  return codexLimitPill(null, pickCodexLongWindow(ctx.codexSnap()), item);
};

const codexEffortWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  const c = ctx.codexSnap();
  if (!c?.effort) return null;
  const effortColor: Record<string, string> = {
    xhigh: "#d97757",
    high: "#f5a623",
    normal: "var(--color-text-2)",
    low: "var(--color-text-2)",
  };
  return (
    <span
      title={`reasoning effort: ${c.effort}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? effortColor[c.effort] ?? "var(--color-text-2)",
        "font-weight": (item.bold ?? true) ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>effort</span>
      {c.effort}
    </span>
  );
};

/** claude-effort: 显示 Claude reasoning effort 等级 (low/medium/high/xhigh/max/ultracode).
 *  数据源优先 transcript session.effort, 回退 live task.effort. 没取到 → 不显 (不臆造). */
const claudeEffortWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  // 优先 transcript session.effort: 后端解析最近一次 /effort 命令回显, 是唯一能拿到
  // ultracode 的源(ultracode 底层=xhigh). 回退 live task.effort —— 嗅探层从 claude
  // 工作动画 "thinking with <effort> effort" 抠出(零侵入, 不依赖已删的 hook).
  const eff = ctx.claudeSession()?.effort ?? ctx.task()?.effort;
  if (!eff) return null;
  // effort 阶梯(由低到高): low < medium < high < xhigh < max < ultracode.
  // 颜色逐级更醒目: 灰 → 琥珀 → 赭红 → 鲜红 → 紫(跳出暖色系,ultracode 最高最显眼).
  // high/xhigh/max 来自工作动画嗅探; ultracode 来自 /effort 命令回显解析.
  const effortColor: Record<string, string> = {
    ultracode: "#a855f7",
    max: "#e5484d",
    xhigh: "#d97757",
    high: "#f5a623",
    medium: "var(--color-text-2)",
    normal: "var(--color-text-2)",
    low: "var(--color-text-2)",
  };
  return (
    <span
      title={`reasoning effort: ${eff}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? effortColor[eff] ?? "var(--color-text-2)",
        "font-weight": (item.bold ?? true) ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.7, "font-size": "10px" }}>effort</span>
      {eff}
    </span>
  );
};

const codexBurnRateWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  const c = ctx.codexSnap();
  if (!c || c.tokens_per_min_recent <= 0) return null;
  const value = c.tokens_per_min_recent;
  const formatted = formatBurnRate(value);
  return (
    <span
      title={`burn rate: ${Math.round(c.tokens_per_min_recent)} tok/min · ${c.burn_rate_level}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        "font-variant-numeric": "tabular-nums",
        color: item.color ?? burnRateColor(c.burn_rate_level),
      }}
    >
      <span style={{ opacity: 0.7 }}>burn</span>
      <span style={{ "font-weight": 500 }}>{formatted}</span>
      <span style={{ opacity: 0.5, "font-size": "10px" }}>t/m</span>
    </span>
  );
};

const claudePlanWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "claude") return null;
  const p = ctx.claudePlan();
  if (!p) return null;
  return (
    <span
      title={`Claude plan: ${p}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? "var(--color-text-2)",
        "font-size": "11px",
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      {p}
    </span>
  );
};

const codexPlanWidget: WidgetRenderer = (item, ctx) => {
  if (ctx.agentKind() !== "codex") return null;
  const c = ctx.codexSnap();
  if (!c?.plan_type) return null;
  return (
    <span style={{ opacity: 0.6, "font-size": "10px", color: item.color ?? undefined }}>
      {c.plan_type}
    </span>
  );
};

// ---- Task widgets ----

function taskStatusColor(status?: string): string {
  switch (status) {
    case "waiting_input":
      return "var(--color-status-waiting, #f5a623)";
    case "running":
      return "var(--color-status-running, #10a37f)";
    case "stalled":
      return "var(--color-status-stalled, #d97757)";
    case "done":
      return "var(--color-text-2)";
    default:
      return "var(--color-text-2)";
  }
}

const taskStatusWidget: WidgetRenderer = (item, ctx) => {
  const t = ctx.task();
  if (!t) return null;
  const color = item.color ?? taskStatusColor(t.status);
  const breathing = t.status === "waiting_input" || t.status === "stalled";
  return (
    <span
      title={`task status: ${t.status}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "4px",
        color,
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span
        style={{
          width: "8px",
          height: "8px",
          "border-radius": "50%",
          background: color,
          animation: breathing ? "vibeterm-breath 2s infinite" : undefined,
        }}
      />
      <Show when={item.metadata?.showLabel !== "false"}>
        <span style={{ "font-size": "10px", opacity: 0.7 }}>{t.status}</span>
      </Show>
    </span>
  );
};

const taskNameWidget: WidgetRenderer = (item, ctx) => {
  const t = ctx.task();
  if (!t) return null;
  const max = item.max_width ?? 160;
  return (
    <span
      title={t.name}
      style={{
        color: item.color ?? "var(--color-text)",
        "font-weight": (item.bold ?? true) ? 500 : undefined,
        "white-space": "nowrap",
        overflow: "hidden",
        "text-overflow": "ellipsis",
        "max-width": `${max}px`,
      }}
    >
      {t.name}
    </span>
  );
};

const worktreeNameWidget: WidgetRenderer = (item, ctx) => {
  const t = ctx.task();
  const wt = t?.worktree;
  if (!wt) return null;
  const max = item.max_width ?? 160;
  // worktree.worktree_path 最后一段(分隔符兼容 Windows 反斜杠)
  const path = wt.worktree_path ?? "";
  const name = path.split(/[/\\]/).filter(Boolean).pop() ?? path;
  return (
    <span
      title={`worktree: ${path}${wt.branch ? ` · ${wt.branch}` : ""}`}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "3px",
        color: item.color ?? "var(--color-text-2)",
        "white-space": "nowrap",
        overflow: "hidden",
        "text-overflow": "ellipsis",
        "max-width": `${max}px`,
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      <span style={{ opacity: 0.6, "font-size": "10px" }}>wt</span>
      <span>{name}</span>
    </span>
  );
};

const separatorWidget: WidgetRenderer = (item, _ctx) => (
  <span style={{ opacity: 0.3, "font-size": "10px", color: item.color ?? undefined }}>
    {item.metadata?.char ?? "│"}
  </span>
);

const flexSeparatorWidget: WidgetRenderer = (_item, _ctx) => (
  // flex:1 占据剩余宽度, 把后续 widget 推到右边
  <span style={{ flex: 1, "min-width": "12px" }} />
);

/** gauge-ctx — 当前活跃 agent 的 context % 用半圆 gauge 显示, 自动适配 Claude/Codex. */
const gaugeCtxWidget: WidgetRenderer = (item, ctx) => {
  const kind = ctx.agentKind();
  let pct: number | null = null;
  let title = "";
  if (kind === "claude") {
    const s = ctx.claudeSession();
    if (s?.context_tokens != null && s.context_window != null && s.context_window > 0) {
      pct = (s.context_tokens / s.context_window) * 100;
      title = `Claude context: ${s.context_tokens} / ${s.context_window}`;
    }
  } else if (kind === "codex") {
    const c = ctx.codexSnap();
    if (c?.context_tokens != null && c.context_window != null && c.context_window > 0) {
      // Codex CLI 算法 (扣 baseline) — 跟 codex-ctx widget 一致
      pct =
        c.context_used_pct != null
          ? c.context_used_pct
          : (c.context_tokens / c.context_window) * 100;
      title = `Codex context: ${c.context_tokens} / ${c.context_window} (${Math.round(pct)}%)`;
    }
  }
  if (pct == null) return null;
  return <span title={title}><Gauge pct={pct} color={item.color ?? undefined} label="ctx" /></span>;
};

const customTextWidget: WidgetRenderer = (item, _ctx) => {
  const text = item.metadata?.text;
  if (!text) return null;
  return (
    <span
      style={{
        color: item.color ?? "var(--color-text-2)",
        "font-weight": item.bold ? 500 : undefined,
      }}
    >
      {text}
    </span>
  );
};

// ---- 注册表 ----

export const WIDGETS: Record<string, WidgetRenderer> = {
  "current-dir": cwdWidget,
  "git-branch": gitBranchWidget,
  "git-staged": gitStagedWidget,
  "git-unstaged": gitUnstagedWidget,
  "git-untracked": gitUntrackedWidget,
  "git-stash-count": gitStashCountWidget,
  "pr-status": prStatusWidget,
  "claude-model": claudeModelWidget,
  "claude-ctx": claudeCtxWidget,
  "claude-5h": claude5hWidget,
  "claude-7d": claude7dWidget,
  "claude-block-pct": claudeBlockPctWidget,
  "claude-block-tokens": claudeBlockTokensWidget,
  "claude-block-remaining": claudeBlockRemainingWidget,
  "claude-burn-rate": claudeBurnRateWidget,
  "claude-cache-ttl": claudeCacheTtlWidget,
  "claude-7d-sonnet": claude7dSonnetWidget,
  "claude-7d-opus": claude7dOpusWidget,
  "codex-model": codexModelWidget,
  "codex-ctx": codexCtxWidget,
  "codex-5h": codex5hWidget,
  "codex-7d": codex7dWidget,
  "codex-burn-rate": codexBurnRateWidget,
  "codex-effort": codexEffortWidget,
  "claude-effort": claudeEffortWidget,
  "claude-tokens-today": claudeTokensTodayWidget,
  "claude-plan": claudePlanWidget,
  "codex-plan": codexPlanWidget,
  "task-status": taskStatusWidget,
  "task-name": taskNameWidget,
  "worktree-name": worktreeNameWidget,
  separator: separatorWidget,
  "flex-separator": flexSeparatorWidget,
  "gauge-ctx": gaugeCtxWidget,
  "custom-text": customTextWidget,
};

export interface WidgetMeta {
  id: string;
  display_name: string;
  description: string;
  category: "core" | "git" | "claude" | "codex" | "layout";
}

/** 所有可用 widget 元数据 — 给配置 UI / docs 用 */
export const WIDGET_LIST: WidgetMeta[] = [
  { id: "current-dir", display_name: "Current Dir", description: "当前工作目录 (短路径)", category: "core" },
  { id: "git-branch", display_name: "Git Branch", description: "分支 / dirty / ahead / behind", category: "git" },
  { id: "git-staged", display_name: "Git Staged", description: "已 stage 文件数 (隐藏 0)", category: "git" },
  { id: "git-unstaged", display_name: "Git Unstaged", description: "已修改未 stage 文件数 (隐藏 0)", category: "git" },
  { id: "git-untracked", display_name: "Git Untracked", description: "未跟踪文件数 (隐藏 0)", category: "git" },
  { id: "git-stash-count", display_name: "Git Stash", description: "stash 数 (隐藏 0)", category: "git" },
  { id: "pr-status", display_name: "PR Status", description: "当前分支 PR 状态 (需 gh CLI)", category: "git" },
  { id: "claude-model", display_name: "Claude Model", description: "Claude 模型简写 (opus 4.7 / sonnet 4.5)", category: "claude" },
  { id: "claude-ctx", display_name: "Claude Context %", description: "当前上下文百分比", category: "claude" },
  { id: "claude-5h", display_name: "Claude 5h", description: "5 小时块配额 (服务端)", category: "claude" },
  { id: "claude-7d", display_name: "Claude 7d", description: "7 天总配额 (服务端)", category: "claude" },
  { id: "claude-block-pct", display_name: "Claude Block %", description: "本地 5h 块已用时间百分比 (ccusage 算法)", category: "claude" },
  { id: "claude-block-tokens", display_name: "Claude Block Tokens", description: "5h 块累计 token 数", category: "claude" },
  { id: "claude-block-remaining", display_name: "Claude Block Remaining", description: "5h 块剩余时间倒计时", category: "claude" },
  { id: "claude-burn-rate", display_name: "Claude Burn Rate", description: "最近 tokens/min (normal/moderate/high)", category: "claude" },
  { id: "claude-cache-ttl", display_name: "Claude Cache TTL", description: "prompt cache 5m/1h 倒计时 (距过期还有多久)", category: "claude" },
  { id: "claude-7d-sonnet", display_name: "Claude 7d Sonnet", description: "Sonnet 单独 7d 配额", category: "claude" },
  { id: "claude-7d-opus", display_name: "Claude 7d Opus", description: "Opus 单独 7d 配额", category: "claude" },
  { id: "codex-model", display_name: "Codex Model", description: "Codex 模型简写", category: "codex" },
  { id: "codex-ctx", display_name: "Codex Context %", description: "当前上下文百分比", category: "codex" },
  { id: "codex-5h", display_name: "Codex 5h", description: "短周期配额 (5h 窗口, 按 window_minutes 自动选)", category: "codex" },
  { id: "codex-7d", display_name: "Codex 7d", description: "长周期配额 (周/月窗口自动选; free 计划现为月度 30d)", category: "codex" },
  { id: "codex-burn-rate", display_name: "Codex Burn Rate", description: "token_count 事件累计 tokens/min", category: "codex" },
  { id: "codex-effort", display_name: "Codex Effort", description: "reasoning effort (xhigh/high/normal/low)", category: "codex" },
  { id: "claude-effort", display_name: "Claude Effort", description: "reasoning effort (low/medium/high/xhigh/max,需 hook 已装)", category: "claude" },
  { id: "claude-tokens-today", display_name: "Claude Tokens 24h", description: "跨所有 project 过去 24h 累计 token", category: "claude" },
  { id: "claude-plan", display_name: "Claude Plan", description: "订阅级别 (Free/Pro/Max 5x/Max 20x)", category: "claude" },
  { id: "codex-plan", display_name: "Codex Plan", description: "订阅级别 (free/paid)", category: "codex" },
  { id: "task-status", display_name: "Task Status", description: "状态点 (waiting_input 呼吸 / running / stalled)", category: "core" },
  { id: "task-name", display_name: "Task Name", description: "当前 task 名", category: "core" },
  { id: "worktree-name", display_name: "Worktree", description: "挂载的 git worktree 简名", category: "git" },
  { id: "separator", display_name: "Separator", description: "竖线分隔 (metadata.char 自定义)", category: "layout" },
  { id: "flex-separator", display_name: "Flex Separator", description: "占据剩余宽度,把后续 widget 推到右边", category: "layout" },
  { id: "gauge-ctx", display_name: "Gauge Context", description: "当前 agent 上下文 % 用半圆 gauge 显示", category: "layout" },
  { id: "custom-text", display_name: "Custom Text", description: "任意文本 (metadata.text 设)", category: "layout" },
];
