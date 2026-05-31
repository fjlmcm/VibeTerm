// status-bar/popover/AgentPanel.tsx — Claude / Codex 通用面板.
//
// 排版骨架 (三个 Section, 字段顺序固定):
//   Session: model / provider / plan / effort / session / context / cli version
//   Quota:   5h / 7d (+ 7d_sonnet / 7d_opus / extra_credits 追加)
//   Usage:   tokens used / burn rate / block cost / elapsed / 24h tokens
//
// agent 独有的字段用 Show 隐藏行, agent 缺失数据的 quota 槽位由 collect 函数填 pct=null
// (QuotaRow 显示 "—"). 视觉上 Claude / Codex panel 永远对齐.

import { For, Show, type Component } from "solid-js";
import { t } from "../../i18n";
import type { RenderContext } from "../widgets";
import { formatTokens } from "../widgets";
import { Row, QuotaRow, Section, EmptyState } from "./atoms";
import { formatRemainMs } from "./anchor";
import {
  type AgentPanelData,
  collectClaudeData,
  collectCodexData,
} from "./collect";

const AgentPanel: Component<{ data: AgentPanelData }> = (props) => {
  const d = () => props.data;
  const hasQuota = () => d().quotas.length > 0 || d().extraCredits != null;
  const hasUsage = () =>
    d().tokensUsed != null ||
    d().burnRate != null ||
    d().elapsed != null ||
    d().tokens24h != null ||
    d().cache5mUntilMs != null ||
    d().cache1hUntilMs != null;

  return (
    <Show when={d().hasSession} fallback={<EmptyState />}>
      <Section title={t("statusbar.popover.section.session")}>
        <Show when={d().model}>
          <Row label={t("statusbar.popover.model")} value={d().model!} mono />
        </Show>
        <Show when={d().provider}>
          <Row label={t("statusbar.popover.provider")} value={d().provider!} />
        </Show>
        <Show when={d().plan}>
          <Row label={t("statusbar.popover.plan")} value={d().plan!} />
        </Show>
        <Show when={d().effort}>
          <Row label={t("statusbar.popover.effort")} value={d().effort!} />
        </Show>
        <Show when={d().sessionId}>
          <Row label={t("statusbar.popover.session")} value={d().sessionId!} mono />
        </Show>
        <Show when={d().contextTokens != null && d().contextWindow != null}>
          <Row
            label={t("statusbar.popover.context_full")}
            value={`${formatTokens(d().contextTokens!)} / ${formatTokens(d().contextWindow!)} (${Math.round(
              d().contextUsedPct != null
                ? d().contextUsedPct!
                : (d().contextTokens! / d().contextWindow!) * 100
            )}%)`}
          />
        </Show>
        <Show when={d().cliVersion}>
          <Row label={t("statusbar.popover.cli_version")} value={d().cliVersion!} mono />
        </Show>
      </Section>

      <Show when={hasQuota()}>
        <Section title={t("statusbar.popover.section.quota")}>
          <For each={d().quotas}>
            {(q) => (
              <QuotaRow
                label={q.label}
                pct={q.pct}
                resetLabel={q.resetLabel ?? null}
                resetAt={q.resetAt ?? null}
              />
            )}
          </For>
          <Show when={d().extraCredits}>
            <Row label={t("statusbar.popover.extra_credits")} value={d().extraCredits!} />
          </Show>
        </Section>
      </Show>

      <Show when={hasUsage()}>
        <Section title={t("statusbar.popover.section.usage")}>
          <Show when={d().tokensUsed != null}>
            <Row
              label={t("statusbar.popover.tokens_used")}
              value={formatTokens(d().tokensUsed!)}
            />
          </Show>
          <Show when={d().burnRate}>
            <Row
              label={t("statusbar.popover.burn_rate")}
              value={`${Math.round(d().burnRate!.rate)} tok/min · ${d().burnRate!.level}`}
            />
          </Show>
          <Show when={d().elapsed}>
            <Row
              label={t("statusbar.popover.elapsed")}
              value={`${Math.round(d().elapsed!.pct)}% · ${formatRemainMs(d().elapsed!.remainMs)}`}
            />
          </Show>
          <Show when={d().tokens24h != null}>
            <Row
              label={t("statusbar.popover.today")}
              value={`${formatTokens(d().tokens24h!)} tokens`}
            />
          </Show>
          <Show when={d().cache5mUntilMs != null || d().cache1hUntilMs != null}>
            <Row
              label={t("statusbar.popover.cache_ttl")}
              value={formatCacheTtl(d().cache5mUntilMs, d().cache1hUntilMs)}
            />
          </Show>
        </Section>
      </Show>
    </Show>
  );
};

/// 把 5m/1h cache 到期时刻 → 显示文案. 两个都有: "5m 3m12s · 1h 42m"; 单个: "5m 3m12s"; 都过期: "expired".
function formatCacheTtl(c5: number | null | undefined, c1: number | null | undefined): string {
  const now = Date.now();
  const parts: string[] = [];
  if (c5 != null) {
    const r = c5 - now;
    parts.push(`5m ${r > 0 ? fmtMs(r) : "expired"}`);
  }
  if (c1 != null) {
    const r = c1 - now;
    parts.push(`1h ${r > 0 ? fmtMs(r) : "expired"}`);
  }
  return parts.join(" · ") || "—";
}

function fmtMs(ms: number): string {
  const sec = Math.max(0, Math.floor(ms / 1000));
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m${sec % 60 > 0 && min < 10 ? `${sec % 60}s` : ""}`;
  return `${Math.floor(min / 60)}h${min % 60}m`;
}

/// Claude panel — 通过 collect 函数把 ctx 信号投到统一 shape, 再用 AgentPanel 渲染.
export const ClaudePanel: Component<{ ctx: RenderContext }> = (props) => (
  <AgentPanel data={collectClaudeData(props.ctx)} />
);

/// Codex panel — 同上.
export const CodexPanel: Component<{ ctx: RenderContext }> = (props) => (
  <AgentPanel data={collectCodexData(props.ctx)} />
);
