// status-bar/popover 原子组件 — Row / QuotaRow / Section / EmptyState.
//
// 设计原则:
//   - 不接 RenderContext, 接纯数据 props, 方便复用.
//   - Row 用 grid 布局, label/value 自适应, 防止 i18n 翻译后 label 被截断.
//   - QuotaRow 配进度条 + 可选 reset 时间文案.
//   - Section 内容分组, top border + uppercase title.
//   - EmptyState 占位 "no active session".

import { Show, type Component, type ParentComponent } from "solid-js";
import { t } from "../../i18n";
import { quotaBarColor } from "./colors";

/// 单行 label/value — label 不截断, value 长时 ellipsis + 标 title.
export const Row: Component<{ label: string; value: string; mono?: boolean }> = (props) => (
  <div
    style={{
      display: "grid",
      "grid-template-columns": "minmax(0, max-content) 1fr",
      "column-gap": "12px",
      "align-items": "baseline",
    }}
  >
    <span style={{ color: "var(--color-text-2)", "white-space": "nowrap" }}>{props.label}</span>
    <span
      style={{
        color: "var(--color-text)",
        "font-family": props.mono ? "ui-monospace, SFMono-Regular, monospace" : undefined,
        "font-size": props.mono ? "11px" : undefined,
        "font-weight": props.mono ? undefined : 500,
        "min-width": 0,
        overflow: "hidden",
        "text-overflow": "ellipsis",
        "white-space": "nowrap",
        "text-align": "right",
      }}
      title={props.value}
    >
      {props.value}
    </span>
  </div>
);

/// 配额行 — label · pct · 进度条 · 可选 reset 信息. pct null 时显示 "—" 不画条.
export const QuotaRow: Component<{
  label: string;
  pct: number | null;
  resetLabel?: string | null;
  resetAt?: string | null;
}> = (props) => (
  <div style={{ display: "flex", "flex-direction": "column", gap: "3px" }}>
    <div
      style={{
        display: "flex",
        "align-items": "baseline",
        "justify-content": "space-between",
        gap: "8px",
        "font-size": "11px",
      }}
    >
      <span style={{ color: "var(--color-text-2)" }}>{props.label}</span>
      <span
        style={{
          color: props.pct != null ? quotaBarColor(props.pct) : "var(--color-text-2)",
          "font-variant-numeric": "tabular-nums",
          "font-weight": 500,
        }}
      >
        {props.pct != null ? `${Math.round(props.pct)}%` : "—"}
      </span>
    </div>
    <Show when={props.pct != null}>
      <div
        style={{
          position: "relative",
          width: "100%",
          height: "4px",
          background: "var(--color-border)",
          "border-radius": "2px",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            bottom: 0,
            width: `${Math.min(100, Math.max(0, props.pct ?? 0))}%`,
            background: quotaBarColor(props.pct ?? 0),
            transition: "width 200ms ease",
          }}
        />
      </div>
    </Show>
    <Show when={props.resetLabel || props.resetAt}>
      <div
        style={{
          "font-size": "10px",
          color: "var(--color-text-2)",
          opacity: 0.7,
          display: "flex",
          "justify-content": "space-between",
          gap: "8px",
        }}
      >
        <Show when={props.resetLabel}>
          <span>{t("statusbar.popover.resets_in", { time: props.resetLabel! })}</span>
        </Show>
        <Show when={props.resetAt}>
          <span>
            {t("statusbar.popover.reset_at")} {props.resetAt}
          </span>
        </Show>
      </div>
    </Show>
  </div>
);

/// 段标题 + top border + 内容 column.
export const Section: ParentComponent<{ title: string }> = (props) => (
  <section style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
    <header
      style={{
        "font-size": "10px",
        "text-transform": "uppercase",
        "letter-spacing": "1px",
        color: "var(--color-text-2)",
        "font-weight": 600,
        "padding-bottom": "4px",
        "border-bottom": "1px solid var(--color-border)",
      }}
    >
      {props.title}
    </header>
    <div style={{ display: "flex", "flex-direction": "column", gap: "5px" }}>{props.children}</div>
  </section>
);

/// 该 agent 当前无活跃 session 的占位.
export const EmptyState: Component = () => (
  <div
    style={{
      padding: "24px 12px",
      "text-align": "center",
      color: "var(--color-text-2)",
      opacity: 0.6,
      "font-size": "11px",
    }}
  >
    {t("statusbar.popover.no_session")}
  </div>
);
