// status-bar/popover/DetailPopover.tsx — 状态栏详情浮层容器.
//
// 单一全局触发 (StatusBar 内的 ⓘ 按钮), popover 内固定显示当前终端的:
//   - TerminalPanel (永远)
//   - ClaudePanel / CodexPanel (根据 ctx.agentKind 二选一; 普通终端不显)
// 不再有 tab 切换 — 一个终端只有一种 agent 角色.

import { Show, createSignal, onCleanup, onMount, type Component } from "solid-js";
import { t } from "../../i18n";
import type { RenderContext } from "../widgets";
import { computeAnchor } from "./anchor";
import { TerminalPanel } from "./TerminalPanel";
import { ClaudePanel, CodexPanel } from "./AgentPanel";

export interface DetailPopoverProps {
  ctx: RenderContext;
  anchor: HTMLElement | undefined;
  onClose: () => void;
}

export const DetailPopover: Component<DetailPopoverProps> = (props) => {
  const [pos, setPos] = createSignal(computeAnchor(props.anchor?.getBoundingClientRect()));
  onMount(() => {
    const recompute = () => setPos(computeAnchor(props.anchor?.getBoundingClientRect()));
    recompute();
    window.addEventListener("resize", recompute);
    onCleanup(() => window.removeEventListener("resize", recompute));
  });

  const kind = props.ctx.agentKind;

  return (
    <div
      data-status-popover="true"
      style={{
        position: "fixed",
        top: pos().top,
        bottom: pos().bottom,
        left: pos().left,
        "z-index": 9999,
        "min-width": "380px",
        "max-width": "min(520px, calc(100vw - 16px))",
        "max-height": pos().maxHeight,
        background: "var(--color-surface)",
        border: "1px solid var(--color-border)",
        "border-radius": "10px",
        "box-shadow": "0 12px 32px rgba(0,0,0,0.4)",
        display: "flex",
        "flex-direction": "column",
        "font-size": "12px",
        overflow: "hidden",
      }}
    >
      {/* Header — agent kind 标识 + 关闭按钮 */}
      <div
        style={{
          display: "flex",
          "align-items": "center",
          "justify-content": "space-between",
          padding: "8px 10px 8px 12px",
          "border-bottom": "1px solid var(--color-border)",
          "font-size": "11px",
          color: "var(--color-text-2)",
          "text-transform": "uppercase",
          "letter-spacing": "1px",
          "font-weight": 600,
        }}
      >
        <span>{kind() ?? t("statusbar.popover.tab.terminal")}</span>
        <button
          onClick={props.onClose}
          title={t("statusbar.popover.toggle_close")}
          style={{
            background: "transparent",
            border: "none",
            color: "var(--color-text-2)",
            cursor: "pointer",
            "font-size": "13px",
            padding: "2px 6px",
            "border-radius": "4px",
            "text-transform": "none",
            "letter-spacing": "normal",
          }}
        >
          ✕
        </button>
      </div>

      {/* Content (滚动) — Terminal panel + 当前 agent panel */}
      <div
        style={{
          "overflow-y": "auto",
          padding: "10px 12px 12px 12px",
          display: "flex",
          "flex-direction": "column",
          gap: "12px",
        }}
      >
        <TerminalPanel ctx={props.ctx} />
        <Show when={kind() === "claude"}>
          <ClaudePanel ctx={props.ctx} />
        </Show>
        <Show when={kind() === "codex"}>
          <CodexPanel ctx={props.ctx} />
        </Show>
      </div>
    </div>
  );
};
