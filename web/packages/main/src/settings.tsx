// 设置页(modal)
//
// 4 tabs:
//   - 主题(Theme):网格 + 当前激活 + 点击切换
//   - 环境变量(Env):env.toml [env] / [proxy] form,value 默认掩码
//   - 快捷键(Keys):只读列表(加录入)
//   - AI CLI:detect + 状态 + PATH 修复一键复制

import { For, Show, createSignal, createMemo, onMount, type Component } from "solid-js";
import { ArrowLeft, Check, Eye, EyeOff, RotateCcw, RefreshCw, Trash2, Plus } from "lucide-solid";
import { ipc, t, Titlebar, IMPLEMENTED_COMMANDS, promptDisplayName, isMacPlatform } from "@vibeterm/ui-core";
import type { Theme, EnvFile, KeybindingsFile, CliStatus, PromptsFile, PromptEntry, PromptKind } from "@vibeterm/ipc-types";
import { StatuslineTab } from "./settings-statusline";
import { NotifyTab } from "./settings-notify";
import { TerminalTab } from "./settings-terminal";
import { AboutTab } from "./settings-about";
import { UpdateTab } from "./settings-update";
import { LanguageTab } from "./settings-language";

type Tab = "language" | "theme" | "terminal" | "env" | "keys" | "prompts" | "cli" | "privacy" | "statusline" | "notify" | "update" | "about";

export interface SettingsProps {
  onClose: () => void;
  activeThemeId: string;
  /** 当前激活的 terminal id, 给状态栏标签 PREVIEW 用 */
  activeTerminalId?: number | null;
  /** 初始打开的 tab(菜单"检查更新"→ "update")。默认 theme。 */
  initialTab?: Tab;
}

export const Settings: Component<SettingsProps> = (props) => {
  const [tab, setTab] = createSignal<Tab>(props.initialTab ?? "theme");
  return (
    <div
      data-testid="settings-panel"
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--color-surface)",
        "z-index": 2000,
        display: "grid",
        "grid-template-rows": "auto 1fr",
        "min-height": 0,
      }}
    >
      {/* 顶部标题栏 — 返回按钮在右侧, 标题居中 */}
      <Titlebar
        center={
          <span style={{ "font-weight": 600, color: "var(--color-text)", "font-size": "12px" }}>
            {t("settings.title")}
          </span>
        }
        right={
          <button
            data-testid="settings-back"
            onClick={props.onClose}
            title={t("settings.back")}
            style={{
              display: "inline-flex",
              "align-items": "center",
              gap: "4px",
              padding: "3px 9px",
              background: "transparent",
              color: "var(--color-text-2)",
              border: "1px solid var(--color-border)",
              "border-radius": "5px",
              cursor: "pointer",
              "font-size": "11px",
              "font-weight": 500,
            }}
          >
            {t("settings.back")} <ArrowLeft size={12} style={{ transform: "rotate(180deg)" }} />
          </button>
        }
      />

      <div
        style={{
          display: "grid",
          "grid-template-columns": "200px 1fr",
          overflow: "hidden",
          "min-height": 0,
        }}
      >
        {/* Sidebar */}
        <div
          style={{
            background: "var(--color-bg)",
            "border-right": "1px solid var(--color-border)",
            padding: "12px 0",
            "overflow-y": "auto",
          }}
        >
          <SidebarItem label={t("settings.tab.language")} tab="language" current={tab()} onSelect={setTab} testid="settings-tab-language" />
          <SidebarItem label={t("settings.tab.theme")} tab="theme" current={tab()} onSelect={setTab} testid="settings-tab-theme" />
          <SidebarItem label={t("settings.tab.terminal")} tab="terminal" current={tab()} onSelect={setTab} testid="settings-tab-terminal" />
          <SidebarItem label={t("settings.tab.env")} tab="env" current={tab()} onSelect={setTab} testid="settings-tab-env" />
          <SidebarItem label={t("settings.tab.keys")} tab="keys" current={tab()} onSelect={setTab} testid="settings-tab-keys" />
          <SidebarItem label={t("settings.tab.prompts")} tab="prompts" current={tab()} onSelect={setTab} testid="settings-tab-prompts" />
          <SidebarItem label={t("settings.tab.statusline")} tab="statusline" current={tab()} onSelect={setTab} testid="settings-tab-statusline" />
          <SidebarItem label={t("settings.tab.notify")} tab="notify" current={tab()} onSelect={setTab} testid="settings-tab-notify" />
          <SidebarItem label={t("settings.tab.cli")} tab="cli" current={tab()} onSelect={setTab} testid="settings-tab-cli" />
          <SidebarItem label={t("settings.tab.privacy")} tab="privacy" current={tab()} onSelect={setTab} testid="settings-tab-privacy" />
          <SidebarItem label={t("settings.tab.update")} tab="update" current={tab()} onSelect={setTab} testid="settings-tab-update" />
          <SidebarItem label={t("settings.tab.about")} tab="about" current={tab()} onSelect={setTab} testid="settings-tab-about" />
          {/* hidden marker for E2E to detect active tab */}
          <div data-testid="settings-active-tab" data-tab={tab()} style={{ display: "none" }} />
        </div>

        {/* Content */}
        <div style={{ "overflow-y": "auto", padding: "20px 24px", "min-height": 0 }}>
          <Show when={tab() === "language"}>
            <LanguageTab />
          </Show>
          <Show when={tab() === "theme"}>
            <ThemeTab activeThemeId={props.activeThemeId} />
          </Show>
          <Show when={tab() === "terminal"}>
            <TerminalTab />
          </Show>
          <Show when={tab() === "env"}>
            <EnvTab />
          </Show>
          <Show when={tab() === "keys"}>
            <KeysTab />
          </Show>
          <Show when={tab() === "prompts"}>
            <PromptsTab />
          </Show>
          <Show when={tab() === "statusline"}>
            <StatuslineTab activeTerminalId={props.activeTerminalId ?? null} />
          </Show>
          <Show when={tab() === "notify"}>
            <NotifyTab />
          </Show>
          <Show when={tab() === "cli"}>
            <CliTab />
          </Show>
          <Show when={tab() === "privacy"}>
            <PrivacyTab />
          </Show>
          <Show when={tab() === "update"}>
            <UpdateTab />
          </Show>
          <Show when={tab() === "about"}>
            <AboutTab />
          </Show>
        </div>
      </div>
    </div>
  );
};

const SidebarItem: Component<{
  label: string;
  tab: Tab;
  current: Tab;
  onSelect: (t: Tab) => void;
  testid?: string;
}> = (p) => (
  <div
    data-testid={p.testid}
    onClick={() => p.onSelect(p.tab)}
    style={{
      padding: "8px 16px",
      cursor: "pointer",
      "font-size": "13px",
      "border-left": p.current === p.tab ? "3px solid var(--color-accent)" : "3px solid transparent",
      color: "var(--color-text)",
      background: p.current === p.tab ? "var(--color-accent-subtle)" : "transparent",
    }}
  >
    {p.label}
  </div>
);

// ---- Theme tab ----
const ThemeTab: Component<{ activeThemeId: string }> = (p) => {
  const [themes, setThemes] = createSignal<Theme[]>([]);
  onMount(async () => {
    setThemes(await ipc.listThemes().catch(() => []));
  });
  return (
    <div>
      <h3 style={{ margin: "0 0 12px 0", "font-size": "14px" }}>{t("settings.tab.theme")}</h3>
      <div
        style={{
          display: "grid",
          "grid-template-columns": "repeat(auto-fill, minmax(220px, 1fr))",
          gap: "8px",
        }}
      >
        <For each={themes()}>
          {(th) => (
            <div
              data-testid={`theme-card-${th.id}`}
              data-theme-id={th.id}
              data-active={p.activeThemeId === th.id ? "true" : "false"}
              onClick={() => ipc.setActiveTheme(th.id).catch(console.error)}
              style={{
                background: th.shell.background,
                color: th.shell.text_primary,
                border:
                  p.activeThemeId === th.id
                    ? "2px solid var(--color-accent)"
                    : "1px solid " + th.shell.border,
                "border-radius": "6px",
                padding: "8px",
                cursor: "pointer",
                "font-size": "11px",
              }}
            >
              <div style={{ "font-size": "13px", "font-weight": 600 }}>{th.name}</div>
              <div style={{ color: th.shell.text_secondary, "margin-top": "4px" }}>
                {th.appearance} · {th.id}
              </div>
              <div style={{ display: "flex", gap: "3px", "margin-top": "6px" }}>
                {[
                  th.terminal.red,
                  th.terminal.green,
                  th.terminal.yellow,
                  th.terminal.blue,
                  th.terminal.magenta,
                  th.terminal.cyan,
                ].map((c) => (
                  <span
                    style={{
                      display: "inline-block",
                      width: "16px",
                      height: "16px",
                      background: c,
                      "border-radius": "3px",
                    }}
                  />
                ))}
              </div>
              <div
                style={{
                  background: th.terminal.background,
                  color: th.terminal.foreground,
                  "border-radius": "3px",
                  padding: "4px 6px",
                  "margin-top": "8px",
                  "font-family": "JetBrains Mono, monospace",
                }}
              >
                $ echo {th.id}
              </div>
            </div>
          )}
        </For>
      </div>
    </div>
  );
};

// ---- Env tab ----
const EnvTab: Component = () => {
  const [file, setFile] = createSignal<EnvFile | null>(null);
  const [revealed, setRevealed] = createSignal<Set<string>>(new Set());
  const [savedAt, setSavedAt] = createSignal(0);
  const [saveErr, setSaveErr] = createSignal(false);

  const reload = async () => {
    setFile(await ipc.getEnvFile().catch(() => null));
  };
  onMount(reload);

  const toggleReveal = (k: string) => {
    const s = new Set(revealed());
    s.has(k) ? s.delete(k) : s.add(k);
    setRevealed(s);
  };

  const updateEnv = (k: string, v: string) => {
    const f = file();
    if (!f) return;
    const env = { ...f.env, [k]: v };
    setFile({ ...f, env });
  };

  const deleteEnv = (k: string) => {
    const f = file();
    if (!f) return;
    const env = { ...f.env };
    delete env[k];
    setFile({ ...f, env });
  };

  const addEnv = () => {
    const k = prompt(t("settings.env.add_prompt"));
    if (!k) return;
    const f = file();
    if (!f) return;
    setFile({ ...f, env: { ...f.env, [k]: "" } });
  };

  const updateProxy = (patch: Partial<NonNullable<EnvFile["proxy"]>>) => {
    const f = file();
    if (!f) return;
    const proxy = {
      enabled: false,
      http: null,
      https: null,
      no_proxy: null,
      ...(f.proxy ?? {}),
      ...patch,
    };
    setFile({ ...f, proxy });
  };

  const save = async () => {
    const f = file();
    if (!f) return;
    try {
      await ipc.saveEnvFile(f);
      setSaveErr(false);
      setSavedAt(Date.now());
      setTimeout(() => setSavedAt(0), 3000);
    } catch (e) {
      console.error("[settings] save env failed", e);
      setSaveErr(true);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", "margin-bottom": "12px" }}>
        <h3 style={{ margin: 0, "font-size": "14px" }}>{t("settings.tab.env")}</h3>
        <button data-testid="env-save-btn" onClick={save} style={btnStyle()}>
          <span style={{ display: "flex", "align-items": "center", gap: "4px" }}>
            {t("settings.env.save")} {savedAt() > 0 ? <Check size={11} /> : null}
            {saveErr() ? <span style={{ color: "var(--color-danger, #e5484d)" }}>{t("settings.save_failed")}</span> : null}
          </span>
        </button>
      </div>
      <Show when={file()}>
        {(f) => (
          <>
            <div style={{ "margin-bottom": "16px", color: "var(--color-text-2)", "font-size": "11px" }}>
              {t("settings.env.note")}
            </div>

            {/* [env] */}
            <div style={sectionStyle()}>
              <div style={sectionHeaderStyle()}>
                <span>{t("settings.env.section_env")}</span>
                <button data-testid="env-add-btn" onClick={addEnv} style={btnStyle()}>{t("settings.env.add")}</button>
              </div>
              <For each={Object.entries(f().env)}>
                {([k, v]) => {
                  const isRevealed = createMemo(() => revealed().has(k));
                  return (
                    <div data-testid={`env-row-${k}`} style={rowStyle()}>
                      <span style={{ "font-family": "monospace", "min-width": "180px", "font-size": "12px" }}>{k}</span>
                      <input
                        data-testid={`env-input-${k}`}
                        type={isRevealed() ? "text" : "password"}
                        value={v}
                        onInput={(e) => updateEnv(k, e.currentTarget.value)}
                        style={inputStyle()}
                      />
                      <button data-testid={`env-reveal-${k}`} onClick={() => toggleReveal(k)} style={btnStyle()} title={isRevealed() ? t("settings.env.hide") : t("settings.env.reveal")}>
                        {isRevealed() ? <EyeOff size={12} /> : <Eye size={12} />}
                      </button>
                      <button data-testid={`env-delete-${k}`} onClick={() => deleteEnv(k)} style={btnStyle()} title={t("settings.env.delete")}>
                        <Trash2 size={12} />
                      </button>
                    </div>
                  );
                }}
              </For>
              <Show when={Object.keys(f().env).length === 0}>
                <div style={{ "font-size": "12px", color: "var(--color-text-2)", padding: "8px" }}>
                  {t("settings.env.empty")}
                </div>
              </Show>
            </div>

            {/* [proxy] */}
            <div style={sectionStyle()}>
              <div style={sectionHeaderStyle()}>{t("settings.env.section_proxy")}</div>
              <label style={{ display: "flex", "align-items": "center", gap: "8px", padding: "4px 0" }}>
                <input
                  data-testid="env-proxy-enabled"
                  type="checkbox"
                  checked={f().proxy?.enabled ?? false}
                  onChange={(e) => updateProxy({ enabled: e.currentTarget.checked })}
                />
                <span>{t("settings.env.enable")}</span>
              </label>
              <ProxyRow label="http" value={f().proxy?.http} onChange={(v) => updateProxy({ http: v || null })} />
              <ProxyRow label="https" value={f().proxy?.https} onChange={(v) => updateProxy({ https: v || null })} />
              <ProxyRow label="no_proxy" value={f().proxy?.no_proxy} onChange={(v) => updateProxy({ no_proxy: v || null })} />
            </div>
          </>
        )}
      </Show>
    </div>
  );
};

const ProxyRow: Component<{ label: string; value: string | null | undefined; onChange: (v: string) => void }> = (p) => (
  <div style={rowStyle()}>
    <span style={{ "min-width": "80px", "font-size": "12px", "font-family": "monospace" }}>{p.label}</span>
    <input
      data-testid={`env-proxy-${p.label}`}
      value={p.value ?? ""}
      onInput={(e) => p.onChange(e.currentTarget.value)}
      style={inputStyle()}
      placeholder="http://127.0.0.1:7890"
    />
  </div>
);

// ---- Keys tab(可录入)----
const KeysTab: Component = () => {
  const [file, setFile] = createSignal<KeybindingsFile | null>(null);
  const [recording, setRecording] = createSignal<number | null>(null); // 录入中的行 idx
  const [savedAt, setSavedAt] = createSignal(0);
  const [saveErr, setSaveErr] = createSignal(false);
  // 双击修饰键检测 — 300ms 内连按两次同一修饰键识别为 DoubleTap
  const DOUBLE_TAP_MS = 300;
  let lastModTap: { key: string; t: number } | null = null;

  onMount(async () => {
    setFile(await ipc.getKeybindings().catch(() => null));
  });

  const startRecord = (i: number) => {
    setRecording(i);
    lastModTap = null;
  };

  const writeKeys = (i: number, newKeys: string) => {
    const f = file();
    if (!f) return;
    const bindings = [...f.bindings];
    bindings[i] = { ...bindings[i], keys: newKeys };
    setFile({ ...f, bindings });
    setRecording(null);
  };

  const captureKey = (e: KeyboardEvent, i: number) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      setRecording(null);
      return;
    }
    // 双击修饰键识别 (DoubleTap+Mod / +Control / +Shift / +Alt)
    if (["Meta", "Control", "Shift", "Alt"].includes(e.key) && !e.repeat) {
      const now = Date.now();
      if (lastModTap && lastModTap.key === e.key && now - lastModTap.t < DOUBLE_TAP_MS) {
        const isMac = isMacPlatform();
        const tag =
          e.key === "Meta" ? (isMac ? "Mod" : "Meta") :
          e.key === "Control" ? (isMac ? "Control" : "Mod") :
          e.key;
        writeKeys(i, `DoubleTap+${tag}`);
        lastModTap = null;
        return;
      }
      lastModTap = { key: e.key, t: now };
      return;
    }
    // 组装 "Mod+Shift+K" 风格 chord
    const parts: string[] = [];
    if (e.metaKey || e.ctrlKey) parts.push("Mod");
    if (e.shiftKey) parts.push("Shift");
    if (e.altKey) parts.push("Alt");
    let key = e.key;
    if (key.length === 1) key = key.toUpperCase();
    parts.push(key);
    writeKeys(i, parts.join("+"));
  };

  const reset = (i: number) => {
    // 简单 reset:重新加载,丢未保存改动
    setRecording(null);
    ipc.getKeybindings().then((f) => {
      const fresh = file();
      if (!fresh) return;
      const bindings = [...fresh.bindings];
      bindings[i] = f.bindings[i] ?? bindings[i];
      setFile({ ...fresh, bindings });
    }).catch((e) => console.error("[settings] reset keybinding failed", e));
  };

  const save = async () => {
    const f = file();
    if (!f) return;
    try {
      await ipc.saveKeybindings(f);
      setSaveErr(false);
      setSavedAt(Date.now());
      setTimeout(() => setSavedAt(0), 3000);
    } catch (e) {
      console.error("[settings] save keybindings failed", e);
      setSaveErr(true);
    }
  };

  // 删 keybindings.toml 让默认值生效, 主要兜底 "用户之前 save 过自定义,
  // 后续默认值改了拿不到新默认" 的场景.
  const resetAll = async () => {
    if (!confirm(t("settings.keys.reset_all_confirm"))) return;
    try {
      const fresh = await ipc.resetKeybindings();
      setFile(fresh);
      setSavedAt(Date.now());
    } catch (e) {
      console.error("[settings] reset keybindings failed", e);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", "margin-bottom": "12px" }}>
        <h3 style={{ margin: 0, "font-size": "14px" }}>{t("settings.keys.title")}</h3>
        <div style={{ display: "flex", gap: "6px" }}>
          <button data-testid="keys-reset-all-btn" onClick={resetAll} style={btnStyle()} title={t("settings.keys.reset_all")}>
            <span style={{ display: "flex", "align-items": "center", gap: "4px" }}>
              <RotateCcw size={11} />
              {t("settings.keys.reset_all")}
            </span>
          </button>
          <button data-testid="keys-save-btn" onClick={save} style={btnStyle()}>
            <span style={{ display: "flex", "align-items": "center", gap: "4px" }}>
              {t("settings.keys.save")} {savedAt() > 0 ? <Check size={11} /> : null}
              {saveErr() ? <span style={{ color: "var(--color-danger, #e5484d)" }}>{t("settings.save_failed")}</span> : null}
            </span>
          </button>
        </div>
      </div>
      <div style={{ color: "var(--color-text-2)", "font-size": "11px", "margin-bottom": "12px" }}>
        {t("settings.keys.hint")}
      </div>
      <Show when={file()}>
        {(f) => (
          <For each={f().bindings}>
            {(b, i) => (
              <div
                data-testid={`keys-row-${b.command}`}
                data-keys={b.keys}
                style={{
                  display: "grid",
                  "grid-template-columns": "180px 1fr auto auto",
                  gap: "8px",
                  padding: "6px 8px",
                  "border-bottom": "1px solid var(--color-border)",
                  "font-size": "12px",
                  "align-items": "center",
                }}
              >
                <div style={{ display: "flex", "align-items": "center", gap: "6px" }}>
                  <span style={{ "font-family": "inherit", color: "var(--color-text)", "font-size": "12px" }}>
                    {t(`kb.command.${b.command}`)}
                  </span>
                  <Show when={!IMPLEMENTED_COMMANDS.has(b.command)}>
                    <span style={{
                      "font-size": "9px",
                      color: "var(--color-status-stalled, #d97757)",
                      "padding": "1px 4px",
                      border: "1px solid var(--color-status-stalled, #d97757)",
                      "border-radius": "3px",
                    }}>
                      {t("settings.keys.status_pending")}
                    </span>
                  </Show>
                </div>
                <Show
                  when={recording() === i()}
                  fallback={
                    <code style={{ "font-family": "monospace" }}>{b.keys}</code>
                  }
                >
                  <input
                    data-testid={`keys-capture-${b.command}`}
                    autofocus
                    readonly
                    value={t("settings.keys.recording")}
                    onKeyDown={(e) => captureKey(e, i())}
                    style={{
                      background: "var(--color-accent)",
                      color: "var(--color-bg)",
                      border: "none",
                      "border-radius": "3px",
                      padding: "3px 6px",
                      "font-size": "12px",
                      outline: "none",
                    }}
                  />
                </Show>
                <button data-testid={`keys-record-${b.command}`} onClick={() => startRecord(i())} style={btnStyle()} title={t("settings.keys.record")}>
                  {t("settings.keys.record")}
                </button>
                <button data-testid={`keys-reset-${b.command}`} onClick={() => reset(i())} style={btnStyle()} title={t("settings.keys.reset")}>
                  <RotateCcw size={11} />
                </button>
              </div>
            )}
          </For>
        )}
      </Show>
    </div>
  );
};

// ---- Prompts tab ----
// 区分 agent 类 (LLM 提问) / terminal 类 (shell 命令片段). 新增/编辑/删除.
const PromptsTab: Component = () => {
  const [file, setFile] = createSignal<PromptsFile | null>(null);
  const [activeKind, setActiveKind] = createSignal<PromptKind>("agent");
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [savedAt, setSavedAt] = createSignal(0);
  const [saveErr, setSaveErr] = createSignal(false);

  onMount(async () => {
    setFile(await ipc.getPrompts().catch(() => ({ schema_version: 1, prompts: [] })));
  });

  const promptsOfKind = (kind: PromptKind) =>
    (file()?.prompts ?? []).filter((p) => (p.kind ?? "agent") === kind);

  const updatePrompt = (id: string, patch: Partial<PromptEntry>) => {
    const f = file();
    if (!f) return;
    setFile({ ...f, prompts: f.prompts.map((p) => (p.id === id ? { ...p, ...patch } : p)) });
  };

  const addPrompt = () => {
    const f = file();
    if (!f) return;
    // 生成唯一 id
    let i = 1;
    while (f.prompts.find((p) => p.id === `new-${i}`)) i++;
    const id = `new-${i}`;
    const next: PromptEntry = {
      id,
      name: id,
      content: "{{cursor}}",
      kind: activeKind(),
      shortcut: null,
    };
    setFile({ ...f, prompts: [...f.prompts, next] });
    setEditingId(id);
  };

  const deletePrompt = (p: PromptEntry) => {
    if (!confirm(t("prompts.confirm_delete", { name: p.name }))) return;
    const f = file();
    if (!f) return;
    setFile({ ...f, prompts: f.prompts.filter((x) => x.id !== p.id) });
  };

  const save = async () => {
    const f = file();
    if (!f) return;
    try {
      await ipc.savePrompts(f);
      setSaveErr(false);
      setSavedAt(Date.now());
      setTimeout(() => setSavedAt(0), 3000);
      setEditingId(null);
    } catch (e) {
      console.error("[settings] save prompts failed", e);
      setSaveErr(true);
    }
  };

  const resetAll = async () => {
    if (!confirm(t("prompts.reset_all_confirm"))) return;
    try {
      const fresh = await ipc.resetPrompts();
      setFile(fresh);
      setSavedAt(Date.now());
      setEditingId(null);
    } catch (e) {
      console.error("[settings] reset prompts failed", e);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", "margin-bottom": "12px" }}>
        <h3 style={{ margin: 0, "font-size": "14px" }}>{t("prompts.title")}</h3>
        <div style={{ display: "flex", gap: "6px" }}>
          <button data-testid="prompts-reset-all-btn" onClick={resetAll} style={btnStyle()} title={t("prompts.reset_all")}>
            <span style={{ display: "flex", "align-items": "center", gap: "4px" }}>
              <RotateCcw size={11} />
              {t("prompts.reset_all")}
            </span>
          </button>
          <button data-testid="prompts-save-btn" onClick={save} style={btnStyle()}>
            <span style={{ display: "flex", "align-items": "center", gap: "4px" }}>
              {t("prompts.save")}
              {savedAt() > 0 ? <Check size={11} /> : null}
              {saveErr() ? <span style={{ color: "var(--color-danger, #e5484d)" }}>{t("settings.save_failed")}</span> : null}
            </span>
          </button>
        </div>
      </div>

      {/* kind 切换 */}
      <div style={{ display: "flex", gap: "4px", "margin-bottom": "6px" }}>
        <KindToggle kind="agent" active={activeKind()} onSelect={setActiveKind} />
        <KindToggle kind="terminal" active={activeKind()} onSelect={setActiveKind} />
      </div>
      <div style={{ color: "var(--color-text-2)", "font-size": "11px", "margin-bottom": "12px" }}>
        {activeKind() === "agent" ? t("prompts.kind.agent_hint") : t("prompts.kind.terminal_hint")}
      </div>

      <Show when={file()}>
        <For each={promptsOfKind(activeKind())}>
          {(p) => (
            <PromptRow
              prompt={p}
              editing={editingId() === p.id}
              onEdit={() => setEditingId(p.id)}
              onUpdate={(patch) => updatePrompt(p.id, patch)}
              onDelete={() => deletePrompt(p)}
              onCommitEdit={() => setEditingId(null)}
            />
          )}
        </For>

        <button
          data-testid="prompts-add-btn"
          onClick={addPrompt}
          style={{
            ...btnStyle(),
            "margin-top": "8px",
            display: "flex",
            "align-items": "center",
            gap: "4px",
          }}
        >
          <Plus size={11} />
          {t("prompts.add")}
        </button>
      </Show>
    </div>
  );
};

const KindToggle: Component<{ kind: PromptKind; active: PromptKind; onSelect: (k: PromptKind) => void }> = (p) => {
  const isActive = () => p.active === p.kind;
  const label = () => p.kind === "agent" ? t("prompts.kind.agent") : t("prompts.kind.terminal");
  return (
    <button
      data-testid={`prompts-kind-${p.kind}`}
      onClick={() => p.onSelect(p.kind)}
      style={{
        background: isActive() ? "var(--color-accent)" : "transparent",
        color: isActive() ? "var(--color-bg)" : "var(--color-text-2)",
        border: "1px solid var(--color-border)",
        "border-radius": "4px",
        padding: "4px 10px",
        cursor: "pointer",
        "font-size": "12px",
      }}
    >
      <span style={{ "margin-right": "4px" }}>{p.kind === "agent" ? "✨" : "⌨"}</span>
      {label()}
    </button>
  );
};

const PromptRow: Component<{
  prompt: PromptEntry;
  editing: boolean;
  onEdit: () => void;
  onUpdate: (patch: Partial<PromptEntry>) => void;
  onDelete: () => void;
  onCommitEdit: () => void;
}> = (p) => {
  return (
    <div
      data-testid={`prompt-row-${p.prompt.id}`}
      style={{
        "border": "1px solid var(--color-border)",
        "border-radius": "4px",
        padding: "8px 10px",
        "margin-bottom": "6px",
        background: "var(--color-bg)",
      }}
    >
      <Show
        when={p.editing}
        fallback={
          <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", gap: "8px" }}>
            <div style={{ flex: 1, overflow: "hidden" }}>
              <div style={{ "font-weight": 600, "font-size": "12px", color: "var(--color-text)" }}>{promptDisplayName(p.prompt)}</div>
              <div style={{ "font-family": "monospace", "font-size": "10px", color: "var(--color-text-2)", "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis", "margin-top": "2px" }}>
                {p.prompt.content.replace(/\n/g, " ⏎ ").slice(0, 100)}
              </div>
            </div>
            <button onClick={p.onEdit} style={btnStyle()} title={t("prompts.edit")}>
              {t("prompts.edit")}
            </button>
            <button onClick={p.onDelete} style={btnStyle()} title={t("prompts.delete")}>
              <Trash2 size={11} />
            </button>
          </div>
        }
      >
        <div style={{ display: "grid", "grid-template-columns": "80px 1fr", gap: "6px", "align-items": "center" }}>
          <label style={{ "font-size": "11px", color: "var(--color-text-2)" }}>{t("prompts.id")}</label>
          <input
            value={p.prompt.id}
            onInput={(e) => p.onUpdate({ id: e.currentTarget.value })}
            style={inputStyle()}
          />
          <label style={{ "font-size": "11px", color: "var(--color-text-2)" }}>{t("prompts.name")}</label>
          <input
            value={p.prompt.name}
            onInput={(e) => p.onUpdate({ name: e.currentTarget.value })}
            style={inputStyle()}
          />
          <label style={{ "font-size": "11px", color: "var(--color-text-2)", "align-self": "flex-start", "padding-top": "4px" }}>{t("prompts.content")}</label>
          <textarea
            value={p.prompt.content}
            onInput={(e) => p.onUpdate({ content: e.currentTarget.value })}
            rows={4}
            style={{ ...inputStyle(), "font-family": "monospace", resize: "vertical" }}
          />
          <div></div>
          <div style={{ "font-size": "10px", color: "var(--color-text-2)" }}>{t("prompts.cursor_hint")}</div>
        </div>
        <div style={{ display: "flex", "justify-content": "flex-end", gap: "4px", "margin-top": "6px" }}>
          <button onClick={p.onCommitEdit} style={btnStyle()}>
            {t("dialog.cancel")}
          </button>
        </div>
      </Show>
    </div>
  );
};

// ---- CLI tab ----
const CliTab: Component = () => {
  const [clis, setClis] = createSignal<CliStatus[]>([]);
  const reload = async () => {
    setClis(await ipc.detectAiClis().catch(() => []));
  };
  onMount(reload);

  return (
    <div>
      <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between" }}>
        <h3 style={{ margin: 0, "font-size": "14px" }}>{t("settings.cli.title")}</h3>
        <button data-testid="cli-rescan-btn" onClick={reload} style={{ ...btnStyle(), display: "flex", "align-items": "center", gap: "4px" }}>
          <RefreshCw size={11} /> Rescan
        </button>
      </div>

      <table style={{ width: "100%", "border-collapse": "collapse", "margin-top": "12px", "font-size": "12px" }}>
        <thead>
          <tr style={{ "border-bottom": "1px solid var(--color-border)" }}>
            <th style={thStyle()}>CLI</th>
            <th style={thStyle()}>{t("settings.cli.col_status")}</th>
            <th style={thStyle()}>{t("settings.cli.col_path")}</th>
          </tr>
        </thead>
        <tbody>
          <For each={clis()}>
            {(c) => (
              <tr
                data-testid={`cli-row-${c.name}`}
                data-installed={c.installed ? "true" : "false"}
                style={{ "border-bottom": "1px solid var(--color-border)" }}
              >
                <td style={tdStyle()}><code>{c.name}</code></td>
                <td style={tdStyle()}>
                  {c.installed ? (
                    <span style={{ color: "var(--color-status-running)", display: "inline-flex", "align-items": "center", gap: "4px" }}>
                      <Check size={11} /> {t("settings.cli.installed")}
                    </span>
                  ) : (
                    <span style={{ color: "var(--color-status-waiting)" }}>{t("settings.cli.uninstalled")}</span>
                  )}
                </td>
                <td style={{ ...tdStyle(), "font-family": "monospace", "font-size": "11px", color: "var(--color-text-2)" }}>
                  {c.path ?? "—"}
                </td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
};

// ---- 隐私 / 临时图 ----
const PrivacyTab: Component = () => {
  const [dir, setDir] = createSignal<string>("");
  const [draftDir, setDraftDir] = createSignal<string>("");
  const [maxCount, setMaxCount] = createSignal<string>("");
  const [maxMb, setMaxMb] = createSignal<string>("");
  const [saving, setSaving] = createSignal(false);
  const [cleared, setCleared] = createSignal<number | null>(null);
  const [resolved, setResolved] = createSignal<string>("");

  const refresh = async () => {
    try {
      const f = await ipc.getEnvFile();
      const sec = f.clipboard_images;
      setDraftDir(sec?.dir ?? "");
      setMaxCount(sec?.max_count != null ? String(sec.max_count) : "");
      setMaxMb(sec?.max_mb != null ? String(sec.max_mb) : "");
      setDir(sec?.dir ?? "");
    } catch (e) {
      console.error("load env failed", e);
    }
    try {
      setResolved(await ipc.getClipboardImagesDir());
    } catch (e) {
      console.error("getClipboardImagesDir failed", e);
    }
  };

  onMount(refresh);

  const save = async () => {
    setSaving(true);
    setCleared(null);
    try {
      const f = await ipc.getEnvFile();
      const trimmed = draftDir().trim();
      const cnt = maxCount().trim();
      const mb = maxMb().trim();
      const section =
        trimmed || cnt || mb
          ? {
              dir: trimmed || null,
              max_count: cnt ? Math.max(1, parseInt(cnt, 10) || 0) : null,
              max_mb: mb ? Math.max(1, parseInt(mb, 10) || 0) : null,
            }
          : null;
      await ipc.saveEnvFile({ ...f, clipboard_images: section });
      await refresh();
    } catch (e) {
      console.error("saveEnvFile failed", e);
    } finally {
      setSaving(false);
    }
  };

  const reset = async () => {
    setDraftDir("");
    setMaxCount("");
    setMaxMb("");
    await save();
  };

  return (
    <div data-testid="settings-privacy" style={{ display: "flex", "flex-direction": "column", gap: "16px" }}>
      <div style={sectionStyle()}>
        <div style={sectionHeaderStyle()}>{t("privacy.clipboard_images.title")}</div>
        <p style={{ "font-size": "12px", color: "var(--color-text-2)", margin: "0 0 12px 0", "line-height": 1.5 }}>
          {t("settings.privacy.desc")}
        </p>

        <div style={rowStyle()}>
          <label style={{ "min-width": "120px", "font-size": "12px", color: "var(--color-text-2)" }}>{t("settings.privacy.current")}</label>
          <code
            data-testid="privacy-current-dir"
            style={{
              flex: 1,
              "font-family": "JetBrains Mono, SF Mono, Menlo, monospace",
              "font-size": "11px",
              color: "var(--color-text)",
              "word-break": "break-all",
            }}
          >
            {resolved() || t("settings.privacy.unresolved")}
          </code>
          <button
            data-testid="privacy-open-dir"
            style={btnStyle()}
            onClick={() => ipc.openClipboardImagesDir().catch(console.error)}
            title={t("settings.privacy.open_hint")}
          >
            {t("settings.privacy.open_dir")}
          </button>
        </div>

        <div style={rowStyle()}>
          <label style={{ "min-width": "120px", "font-size": "12px", color: "var(--color-text-2)" }}>{t("settings.privacy.custom_dir")}</label>
          <input
            data-testid="privacy-dir-input"
            value={draftDir()}
            onInput={(e) => setDraftDir(e.currentTarget.value)}
            placeholder={t("settings.privacy.dir_ph")}
            style={inputStyle()}
          />
        </div>

        <div style={rowStyle()}>
          <label style={{ "min-width": "120px", "font-size": "12px", color: "var(--color-text-2)" }}>{t("settings.privacy.max_count")}</label>
          <input
            data-testid="privacy-max-count"
            value={maxCount()}
            onInput={(e) => setMaxCount(e.currentTarget.value.replace(/[^0-9]/g, ""))}
            placeholder={t("settings.privacy.default_ph")}
            style={{ ...inputStyle(), "max-width": "120px" }}
          />
          <label style={{ "min-width": "80px", "font-size": "12px", color: "var(--color-text-2)" }}>{t("settings.privacy.max_mb")}</label>
          <input
            data-testid="privacy-max-mb"
            value={maxMb()}
            onInput={(e) => setMaxMb(e.currentTarget.value.replace(/[^0-9]/g, ""))}
            placeholder={t("settings.privacy.default_ph")}
            style={{ ...inputStyle(), "max-width": "120px" }}
          />
        </div>

        <div style={{ display: "flex", gap: "8px", "margin-top": "12px" }}>
          <button data-testid="privacy-save" style={btnStyle()} onClick={save} disabled={saving()}>
            {saving() ? t("settings.privacy.saving") : t("settings.privacy.save")}
          </button>
          <button data-testid="privacy-reset" style={btnStyle()} onClick={reset} disabled={saving()}>
            {t("settings.privacy.reset")}
          </button>
          <div style={{ flex: 1 }} />
          <button
            data-testid="privacy-clear"
            style={btnStyle()}
            onClick={async () => {
              if (!confirm(t("settings.privacy.clear_confirm"))) return;
              try {
                setCleared(await ipc.clearClipboardImages());
              } catch (e) {
                console.error("clearClipboardImages failed", e);
              }
            }}
          >
            <Trash2 size={12} /> {t("settings.privacy.clear_all")}
          </button>
        </div>
        <Show when={cleared() !== null}>
          <div
            data-testid="privacy-cleared-toast"
            style={{ "font-size": "11px", color: "var(--color-text-2)", "margin-top": "8px" }}
          >
            {t("settings.privacy.cleared", { count: cleared() ?? 0 })}
          </div>
        </Show>
        <Show when={dir() && dir() !== resolved()}>
          <div style={{ "font-size": "11px", color: "var(--color-text-2)", "margin-top": "8px" }}>
            {t("settings.privacy.no_migrate")}
          </div>
        </Show>
      </div>
    </div>
  );
};

// ---- 共享样式 ----
function btnStyle() {
  return {
    background: "var(--color-accent-subtle)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    padding: "3px 10px",
    cursor: "pointer",
    "font-size": "11px",
  };
}
function inputStyle() {
  return {
    flex: "1",
    background: "var(--color-bg)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    padding: "4px 8px",
    "font-size": "12px",
    "font-family": "monospace",
  };
}
function sectionStyle() {
  return {
    background: "var(--color-bg)",
    border: "1px solid var(--color-border)",
    "border-radius": "6px",
    padding: "12px",
    "margin-bottom": "12px",
  };
}
function sectionHeaderStyle() {
  return {
    "font-size": "12px",
    "font-weight": 600,
    "margin-bottom": "8px",
    display: "flex",
    "justify-content": "space-between",
    "align-items": "center",
  };
}
function rowStyle() {
  return {
    display: "flex",
    "align-items": "center",
    gap: "6px",
    padding: "3px 0",
  };
}
function thStyle() {
  return {
    "text-align": "left" as const,
    padding: "6px 8px",
    "font-weight": 600,
    color: "var(--color-text-2)",
  };
}
function tdStyle() {
  return { padding: "6px 8px" };
}

void t; // i18n 占位 — 加翻译
