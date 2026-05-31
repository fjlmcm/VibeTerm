// 设置 · 终端 tab
//
// 外观:字体族 / 字号 / 行高 / 光标样式 / 光标闪烁 / 内边距
// 行为:关闭含终端的任务前是否确认
//
// 全部经 @vibeterm/ui-core 的 terminal/prefs (localStorage) 持久化 —— 纯前端、
// 零侵入,改动经 createEffect 实时应用到所有已挂载终端。

import { For, createSignal, onMount, type Component, type JSX } from "solid-js";
import { RotateCcw } from "lucide-solid";
import {
  ipc,
  t,
  getTerminalFontSize,
  setTerminalFontSize,
  terminalFontFamily,
  setTerminalFontFamily,
  terminalLineHeight,
  setTerminalLineHeight,
  terminalCursorStyle,
  setTerminalCursorStyle,
  terminalCursorBlink,
  setTerminalCursorBlink,
  terminalPaddingX,
  setTerminalPaddingX,
  terminalPaddingY,
  setTerminalPaddingY,
  shouldConfirmCloseTask,
  setConfirmCloseTask,
  resetTerminalPrefs,
  type CursorStyle,
} from "@vibeterm/ui-core";

const CURSOR_STYLES: readonly CursorStyle[] = ["block", "bar", "underline"];

export const TerminalTab: Component = () => {
  // shell 集成开关来自后端 config.toml(spawn 时读),非 localStorage
  const [shellIntegration, setShellIntegrationSig] = createSignal(true);
  onMount(async () => {
    try {
      const cfg = await ipc.getConfig();
      setShellIntegrationSig(cfg.shell_integration);
    } catch {
      /* 取不到就保持默认 true */
    }
  });
  const toggleShellIntegration = (v: boolean) => {
    setShellIntegrationSig(v);
    ipc.setShellIntegration(v).catch((e) => console.warn("[settings] setShellIntegration failed", e));
  };

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "18px", "max-width": "520px" }}>
      {/* ---- 外观 ---- */}
      <section style={{ display: "flex", "flex-direction": "column", gap: "10px" }}>
        <SectionTitle
          title={t("settings.terminal.appearance")}
          action={
            <button onClick={resetTerminalPrefs} title={t("settings.terminal.reset")} style={resetBtn()}>
              <RotateCcw size={11} /> {t("settings.terminal.reset")}
            </button>
          }
        />

        <Field label={t("settings.terminal.font_family")} hint={t("settings.terminal.font_family_hint")}>
          <input
            type="text"
            value={terminalFontFamily()}
            spellcheck={false}
            onChange={(e) => setTerminalFontFamily(e.currentTarget.value)}
            placeholder="JetBrains Mono, ..."
            style={{ ...inputStyle(), flex: 1, "min-width": "240px" }}
          />
        </Field>

        <Field label={t("settings.terminal.font_size")}>
          <input
            type="number"
            min={8}
            max={32}
            value={getTerminalFontSize()}
            onInput={(e) => {
              const n = parseInt(e.currentTarget.value, 10);
              if (Number.isFinite(n)) setTerminalFontSize(n);
            }}
            style={{ ...inputStyle(), width: "72px" }}
          />
          <Unit>px</Unit>
        </Field>

        <Field label={t("settings.terminal.line_height")}>
          <input
            type="number"
            min={1}
            max={2}
            step={0.05}
            value={terminalLineHeight()}
            onInput={(e) => {
              const n = Number(e.currentTarget.value);
              if (Number.isFinite(n)) setTerminalLineHeight(n);
            }}
            style={{ ...inputStyle(), width: "72px" }}
          />
        </Field>

        <Field label={t("settings.terminal.cursor_style")}>
          <select
            value={terminalCursorStyle()}
            onChange={(e) => setTerminalCursorStyle(e.currentTarget.value as CursorStyle)}
            style={{ ...inputStyle(), width: "140px", cursor: "pointer" }}
          >
            <For each={CURSOR_STYLES}>
              {(s) => <option value={s}>{t(`settings.terminal.cursor.${s}`)}</option>}
            </For>
          </select>
        </Field>

        <Field label={t("settings.terminal.cursor_blink")}>
          <input
            type="checkbox"
            checked={terminalCursorBlink()}
            onChange={(e) => setTerminalCursorBlink(e.currentTarget.checked)}
          />
        </Field>

        <Field label={t("settings.terminal.padding")}>
          <Unit>X</Unit>
          <input
            type="number"
            min={0}
            max={40}
            value={terminalPaddingX()}
            onInput={(e) => {
              const n = parseInt(e.currentTarget.value, 10);
              if (Number.isFinite(n)) setTerminalPaddingX(n);
            }}
            style={{ ...inputStyle(), width: "64px" }}
          />
          <Unit>Y</Unit>
          <input
            type="number"
            min={0}
            max={40}
            value={terminalPaddingY()}
            onInput={(e) => {
              const n = parseInt(e.currentTarget.value, 10);
              if (Number.isFinite(n)) setTerminalPaddingY(n);
            }}
            style={{ ...inputStyle(), width: "64px" }}
          />
          <Unit>px</Unit>
        </Field>
      </section>

      {/* ---- 行为 ---- */}
      <section style={{ display: "flex", "flex-direction": "column", gap: "10px" }}>
        <SectionTitle title={t("settings.terminal.behavior")} />
        <label style={{ display: "flex", "align-items": "center", gap: "8px", cursor: "pointer", "font-size": "12px" }}>
          <input
            type="checkbox"
            checked={shouldConfirmCloseTask()}
            onChange={(e) => setConfirmCloseTask(e.currentTarget.checked)}
          />
          {t("settings.terminal.confirm_close")}
        </label>
        <div style={{ "font-size": "11px", color: "var(--color-text-2)", "line-height": 1.5, "padding-left": "24px" }}>
          {t("settings.terminal.confirm_close_hint")}
        </div>

        <label style={{ display: "flex", "align-items": "center", gap: "8px", cursor: "pointer", "font-size": "12px", "margin-top": "4px" }}>
          <input
            type="checkbox"
            checked={shellIntegration()}
            onChange={(e) => toggleShellIntegration(e.currentTarget.checked)}
          />
          {t("settings.terminal.shell_integration")}
        </label>
        <div style={{ "font-size": "11px", color: "var(--color-text-2)", "line-height": 1.5, "padding-left": "24px" }}>
          {t("settings.terminal.shell_integration_hint")}
        </div>
      </section>
    </div>
  );
};

// ---- 小组件 ----

const SectionTitle: Component<{ title: string; action?: JSX.Element }> = (props) => (
  <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
    <h3 style={{ margin: 0, "font-size": "13px", "font-weight": 600 }}>{props.title}</h3>
    <div style={{ flex: 1 }} />
    {props.action}
  </div>
);

const Field: Component<{ label: string; hint?: string; children: JSX.Element }> = (props) => (
  <div style={{ display: "flex", "flex-direction": "column", gap: "3px" }}>
    <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
      <span style={{ "min-width": "88px", "font-size": "12px", color: "var(--color-text)" }}>{props.label}</span>
      {props.children}
    </div>
    {props.hint && (
      <span style={{ "font-size": "11px", color: "var(--color-text-2)", "padding-left": "96px" }}>{props.hint}</span>
    )}
  </div>
);

const Unit: Component<{ children: JSX.Element }> = (props) => (
  <span style={{ "font-size": "11px", color: "var(--color-text-2)" }}>{props.children}</span>
);

function inputStyle(): JSX.CSSProperties {
  return {
    background: "var(--color-surface)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    color: "var(--color-text)",
    padding: "4px 8px",
    "font-size": "12px",
  };
}

function resetBtn(): JSX.CSSProperties {
  return {
    display: "inline-flex",
    "align-items": "center",
    gap: "4px",
    padding: "4px 10px",
    background: "transparent",
    color: "var(--color-text-2)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    cursor: "pointer",
    "font-size": "11px",
  };
}
