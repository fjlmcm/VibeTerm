// 设置 · 通知 tab
//
// 控件:
//   1. 系统权限横幅 — denied/default 时引导用户开启
//   2. 全局开关
//   3. 三类事件 (waiting_input / done / stalled) 分别 enable + sound 选择
//   4. 免打扰时段 (enabled + start + end, HH:MM)
//
// 自定义 sound: macOS 系统声音名下拉 + "其它(自定义文件名)" 文本框.
// 自定义文件名需是 ~/Library/Sounds/<name>.aiff 形式; 不支持任意 wav 路径
// (走系统通知子系统的限制, 见 notify_prefs.rs 注释).

import { For, Show, createMemo, createSignal, onCleanup, onMount, type Component } from "solid-js";
import { Bell, BellOff, AlertCircle, Play, FolderOpen, X } from "lucide-solid";
import { ipc, t, playNotifySound, stopNotifySound, isMacPlatform } from "@vibeterm/ui-core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  NotifyFile,
  EventNotifyPrefs,
  NotifyPermissionState,
  BuiltinSound,
} from "@vibeterm/ipc-types";

// 系统声音名按平台列表:
//   macOS → /System/Library/Sounds/*.aiff;Windows → %SystemRoot%\Media\*.wav
// (Rust 侧 resolve_sound_to_path 同步支持两套 fallback)。其它平台只留 default。
const MAC_SOUNDS = [
  "default",
  "Glass",
  "Tink",
  "Sosumi",
  "Hero",
  "Pop",
  "Ping",
  "Funk",
  "Submarine",
  "Bottle",
  "Frog",
  "Morse",
  "Purr",
] as const;
const WIN_SOUNDS = [
  "default",
  "chimes",
  "chord",
  "ding",
  "notify",
  "tada",
  "Windows Notify",
  "Windows Ding",
  "Windows Background",
] as const;
const BUILTIN_SOUNDS: readonly string[] = isMacPlatform()
  ? MAC_SOUNDS
  : /Win/.test(typeof navigator !== "undefined" ? navigator.userAgent : "")
    ? WIN_SOUNDS
    : ["default"];

const CUSTOM_OPT = "__custom__";

/** sound 字段是否为本地文件路径(Unix 绝对 / ~ / Windows 盘符 / UNC)— 与 Rust 侧 sound_is_file_path 一致 */
function isFilePath(s: string): boolean {
  const t = s.trim();
  return (
    t.startsWith("/") ||
    t.startsWith("~/") ||
    t.startsWith("~\\") ||
    t.startsWith("\\\\") ||
    /^[A-Za-z]:[/\\]/.test(t)
  );
}

/** 文件路径取 basename 显示, 系统名原样返回 */
function displaySound(s: string): string {
  if (!s) return "default";
  if (isFilePath(s)) {
    const parts = s.split(/[/\\]/);
    return parts[parts.length - 1] || s;
  }
  return s;
}

const EVENT_KEYS = ["waiting_input", "done"] as const;
type EventKey = (typeof EVENT_KEYS)[number];

function eventLabel(k: EventKey): string {
  switch (k) {
    case "waiting_input":
      return t("notify.event.waiting_input");
    case "done":
      return t("notify.event.done");
  }
}

function eventHint(k: EventKey): string {
  switch (k) {
    case "waiting_input":
      return t("notify.event.waiting_input.hint");
    case "done":
      return t("notify.event.done.hint");
  }
}

function isBuiltinSound(s: string): boolean {
  return (BUILTIN_SOUNDS as readonly string[]).includes(s);
}

/** 按 category 分组 (display name 顺序内稳定). category 顺序固定. */
const CATEGORY_ORDER = ["notification", "tone", "voice", "ui", "ringtone", "other"] as const;
function categoryLabel(cat: string): string {
  switch (cat) {
    case "notification": return t("notify.sound.cat.notification");
    case "tone": return t("notify.sound.cat.tone");
    case "voice": return t("notify.sound.cat.voice");
    case "ui": return t("notify.sound.cat.ui");
    case "ringtone": return t("notify.sound.cat.ringtone");
    case "other": return t("notify.sound.cat.other");
    default: return cat;
  }
}
function groupSounds(list: BuiltinSound[]): { cat: string; items: BuiltinSound[] }[] {
  const map = new Map<string, BuiltinSound[]>();
  for (const s of list) {
    if (!map.has(s.category)) map.set(s.category, []);
    map.get(s.category)!.push(s);
  }
  const out: { cat: string; items: BuiltinSound[] }[] = [];
  for (const c of CATEGORY_ORDER) {
    const items = map.get(c);
    if (items && items.length) out.push({ cat: c, items });
  }
  // 不在 CATEGORY_ORDER 里的兜底分类
  for (const [c, items] of map) {
    if (!(CATEGORY_ORDER as readonly string[]).includes(c)) {
      out.push({ cat: c, items });
    }
  }
  return out;
}

const HH_MM_RE = /^([01]\d|2[0-3]):[0-5]\d$/;

// Tauri dialog 接受的音频扩展名
const AUDIO_EXTS = ["aiff", "aif", "aifc", "wav", "mp3", "ogg", "oga", "m4a", "aac", "flac"];

export const NotifyTab: Component = () => {
  const [prefs, setPrefs] = createSignal<NotifyFile | null>(null);
  const [perm, setPerm] = createSignal<NotifyPermissionState>("default");
  const [bundledSounds, setBundledSounds] = createSignal<BuiltinSound[]>([]);
  const [savedAt, setSavedAt] = createSignal(0);

  const bundledGrouped = createMemo(() => groupSounds(bundledSounds()));
  const bundledIds = createMemo(() => new Set(bundledSounds().map((s) => s.id)));

  const reload = async () => {
    try {
      setPrefs(await ipc.getNotifyPrefs());
    } catch (e) {
      console.error("[notify] load failed", e);
    }
    try {
      setPerm(await ipc.notifyPermission());
    } catch (e) {
      console.error("[notify] permission query failed", e);
    }
    try {
      setBundledSounds(await ipc.listBuiltinSounds());
    } catch (e) {
      console.error("[notify] list builtin sounds failed", e);
    }
  };

  onMount(reload);
  // 离开 tab 时立刻停掉预览, 防止设置关闭后还在响
  onCleanup(stopNotifySound);

  // 任何字段变更后立即落盘. atomic_write 安全, 频次低于人手速度.
  const persist = async (next: NotifyFile) => {
    setPrefs(next);
    try {
      await ipc.saveNotifyPrefs(next);
      setSavedAt(Date.now());
    } catch (e) {
      console.error("[notify] save failed", e);
    }
  };

  const requestPerm = async () => {
    try {
      const r = await ipc.requestNotifyPermission();
      setPerm(r);
    } catch (e) {
      console.error("[notify] request permission failed", e);
    }
  };

  const setEvent = (key: EventKey, patch: Partial<EventNotifyPrefs>) => {
    const f = prefs();
    if (!f) return;
    persist({
      ...f,
      events: {
        ...f.events,
        [key]: { ...f.events[key], ...patch },
      },
    });
  };

  return (
    <Show when={prefs()} fallback={<div style={{ "font-size": "12px", color: "var(--color-text-2)" }}>{t("notify.loading")}</div>}>
      {(p) => (
        <div data-testid="settings-notify" style={{ display: "flex", "flex-direction": "column", gap: "16px" }}>
          <Show when={perm() !== "granted"}>
            <PermissionBanner state={perm()} onRequest={requestPerm} />
          </Show>

          {/* 全局开关 */}
          <div style={sectionStyle()}>
            <div style={rowStyle()}>
              <div style={{ flex: 1 }}>
                <div style={{ "font-size": "13px", "font-weight": 600, color: "var(--color-text)" }}>
                  {p().enabled ? <Bell size={13} style={{ "vertical-align": "-2px", "margin-right": "6px" }} /> : <BellOff size={13} style={{ "vertical-align": "-2px", "margin-right": "6px" }} />}
                  {t("notify.global.title")}
                </div>
                <div style={hintStyle()}>{t("notify.global.hint")}</div>
              </div>
              <Toggle
                checked={p().enabled}
                onChange={(v) => persist({ ...p(), enabled: v })}
                testid="notify-global-toggle"
              />
            </div>
          </div>

          {/* 三类事件 */}
          <div style={sectionStyle()}>
            <div style={sectionHeaderStyle()}>{t("notify.events.title")}</div>
            <For each={EVENT_KEYS}>
              {(key) => (
                <EventRow
                  label={eventLabel(key)}
                  hint={eventHint(key)}
                  prefs={p().events[key]}
                  disabled={!p().enabled}
                  bundledSounds={bundledGrouped()}
                  bundledIds={bundledIds()}
                  onToggle={(v) => setEvent(key, { enabled: v })}
                  onSound={(s) => setEvent(key, { sound: s })}
                  testid={`notify-event-${key}`}
                />
              )}
            </For>
          </div>

          {/* 多 agent 提醒方式 */}
          <div style={sectionStyle()}>
            <div style={sectionHeaderStyle()}>{t("notify.multiagent.title")}</div>
            <ToggleRow
              title={t("notify.focused_other.title")}
              hint={t("notify.focused_other.hint")}
              checked={p().notify_focused_other_task}
              disabled={!p().enabled}
              onChange={(v) => persist({ ...p(), notify_focused_other_task: v })}
              testid="notify-focused-other-toggle"
            />
            <ToggleRow
              title={t("notify.dock_badge.title")}
              hint={t("notify.dock_badge.hint")}
              checked={p().dock_badge_unseen}
              disabled={!p().enabled}
              onChange={(v) => persist({ ...p(), dock_badge_unseen: v })}
              testid="notify-dock-badge-toggle"
            />
            <ToggleRow
              title={t("notify.persistent.title")}
              hint={t("notify.persistent.hint")}
              checked={p().persistent_unseen_sound}
              disabled={!p().enabled}
              onChange={(v) => persist({ ...p(), persistent_unseen_sound: v })}
              testid="notify-persistent-toggle"
            />
            <Show when={p().persistent_unseen_sound}>
              <div style={{ ...rowStyle(), "padding-left": "0", gap: "8px", "align-items": "center" }}>
                <span style={hintStyle()}>{t("notify.persistent.interval")}</span>
                <input
                  type="number"
                  min="5"
                  max="3600"
                  step="5"
                  value={p().persistent_remind_secs}
                  disabled={!p().enabled}
                  onChange={(e) => {
                    const v = Math.min(
                      3600,
                      Math.max(5, Math.round(Number(e.currentTarget.value) || 30)),
                    );
                    persist({ ...p(), persistent_remind_secs: v });
                  }}
                  style={{
                    width: "70px",
                    padding: "4px 8px",
                    "font-size": "13px",
                    color: "var(--color-text)",
                    background: "var(--color-bg)",
                    border: "1px solid var(--color-border)",
                    "border-radius": "6px",
                  }}
                  data-testid="notify-persistent-interval"
                />
                <span style={hintStyle()}>{t("notify.persistent.interval_unit")}</span>
              </div>
            </Show>
          </div>

          {/* 免打扰时段 */}
          <div style={sectionStyle()}>
            <div style={rowStyle()}>
              <div style={{ flex: 1 }}>
                <div style={{ "font-size": "13px", "font-weight": 600, color: "var(--color-text)" }}>
                  {t("notify.quiet_hours.title")}
                </div>
                <div style={hintStyle()}>{t("notify.quiet_hours.hint")}</div>
              </div>
              <Toggle
                checked={p().quiet_hours.enabled}
                onChange={(v) => persist({ ...p(), quiet_hours: { ...p().quiet_hours, enabled: v } })}
                testid="notify-quiet-toggle"
              />
            </div>
            <Show when={p().quiet_hours.enabled}>
              <div style={{ ...rowStyle(), "padding-left": "0", gap: "8px" }}>
                <TimeInput
                  label={t("notify.quiet_hours.start")}
                  value={p().quiet_hours.start}
                  onCommit={(v) => persist({ ...p(), quiet_hours: { ...p().quiet_hours, start: v } })}
                  testid="notify-quiet-start"
                />
                <span style={{ color: "var(--color-text-2)", "font-size": "12px" }}>→</span>
                <TimeInput
                  label={t("notify.quiet_hours.end")}
                  value={p().quiet_hours.end}
                  onCommit={(v) => persist({ ...p(), quiet_hours: { ...p().quiet_hours, end: v } })}
                  testid="notify-quiet-end"
                />
              </div>
            </Show>
          </div>

          <Show when={savedAt() > 0}>
            <div style={{ "font-size": "11px", color: "var(--color-text-2)", "text-align": "right" }}>
              {t("notify.saved")} · {new Date(savedAt()).toLocaleTimeString()}
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
};

const PermissionBanner: Component<{
  state: NotifyPermissionState;
  onRequest: () => void;
}> = (p) => (
  <div
    data-testid="notify-permission-banner"
    style={{
      display: "flex",
      "align-items": "center",
      gap: "10px",
      padding: "10px 12px",
      background: "var(--color-status-warning-bg, rgba(245,166,35,0.12))",
      color: "var(--color-status-warning-fg, var(--color-text))",
      border: "1px solid var(--color-border)",
      "border-radius": "6px",
      "font-size": "12px",
    }}
  >
    <AlertCircle size={14} />
    <div style={{ flex: 1 }}>
      {p.state === "denied"
        ? t("notify.perm.denied")
        : t("notify.perm.default")}
    </div>
    <Show when={p.state !== "denied"}>
      <button
        data-testid="notify-perm-request"
        onClick={p.onRequest}
        style={primaryBtnStyle()}
      >
        {t("notify.perm.request")}
      </button>
    </Show>
  </div>
);

const EventRow: Component<{
  label: string;
  hint: string;
  prefs: EventNotifyPrefs;
  disabled: boolean;
  bundledSounds: { cat: string; items: BuiltinSound[] }[];
  bundledIds: Set<string>;
  onToggle: (v: boolean) => void;
  onSound: (s: string) => void;
  testid: string;
}> = (p) => {
  const currentSound = () => p.prefs.sound ?? "";
  const isCustomFile = () => isFilePath(currentSound());
  const [previewing, setPreviewing] = createSignal(false);
  const [previewFail, setPreviewFail] = createSignal(false);

  const onDropdown = async (v: string) => {
    if (v === CUSTOM_OPT) {
      try {
        const picked = await openDialog({
          multiple: false,
          filters: [{ name: "Audio", extensions: AUDIO_EXTS }],
        });
        if (typeof picked === "string" && picked) {
          p.onSound(picked);
        }
      } catch (e) {
        console.error("[notify] file picker failed", e);
      }
    } else {
      p.onSound(v);
    }
  };

  const preview = async () => {
    const s = currentSound() || "default";
    setPreviewing(true);
    setPreviewFail(false);
    try {
      const ok = await playNotifySound(s);
      if (!ok) setPreviewFail(true);
    } finally {
      // 大多数提示音 < 2s, 给个保底重置
      setTimeout(() => setPreviewing(false), 2000);
    }
  };

  const dropdownValue = () => {
    if (isCustomFile()) return CUSTOM_OPT;
    const s = currentSound();
    if (!s) return "default";
    // 自带库 id 或 macOS 系统名都直接返回; 其它视为不合法回退 default
    if (p.bundledIds.has(s) || isBuiltinSound(s)) return s;
    return "default";
  };

  return (
    <div
      data-testid={p.testid}
      style={{
        display: "flex",
        "flex-direction": "column",
        gap: "4px",
        padding: "8px 0",
        "border-bottom": "1px solid var(--color-border)",
        opacity: p.disabled ? 0.5 : 1,
      }}
    >
      <div style={rowStyle()}>
        <div style={{ flex: 1 }}>
          <div style={{ "font-size": "12px", "font-weight": 500, color: "var(--color-text)" }}>{p.label}</div>
          <div style={hintStyle()}>{p.hint}</div>
        </div>
        <Toggle
          checked={p.prefs.enabled && !p.disabled}
          onChange={p.onToggle}
          disabled={p.disabled}
          testid={`${p.testid}-toggle`}
        />
      </div>
      <Show when={p.prefs.enabled && !p.disabled}>
        <div style={{ ...rowStyle(), "padding-left": "0" }}>
          <label style={{ "min-width": "60px", "font-size": "11px", color: "var(--color-text-2)" }}>
            {t("notify.sound")}
          </label>
          <select
            data-testid={`${p.testid}-sound`}
            value={dropdownValue()}
            onChange={(e) => onDropdown((e.target as HTMLSelectElement).value)}
            style={selectStyle()}
          >
            <optgroup label={t("notify.sound.group.system")}>
              <For each={BUILTIN_SOUNDS}>
                {(s) => <option value={s}>{s}</option>}
              </For>
            </optgroup>
            <For each={p.bundledSounds}>
              {(grp) => (
                <optgroup
                  label={`VibeTerm · ${categoryLabel(grp.cat)}`}
                >
                  <For each={grp.items}>
                    {(s) => <option value={s.id}>{s.name}</option>}
                  </For>
                </optgroup>
              )}
            </For>
            <option value={CUSTOM_OPT}>{t("notify.sound.custom")}</option>
          </select>
          <Show when={isCustomFile()}>
            <span
              data-testid={`${p.testid}-sound-file`}
              title={currentSound()}
              style={{
                "font-family": "monospace",
                "font-size": "11px",
                color: "var(--color-text)",
                "max-width": "180px",
                overflow: "hidden",
                "text-overflow": "ellipsis",
                "white-space": "nowrap",
                background: "var(--color-bg)",
                border: "1px solid var(--color-border)",
                "border-radius": "4px",
                padding: "3px 6px",
              }}
            >
              {displaySound(currentSound())}
            </span>
            <button
              data-testid={`${p.testid}-sound-clear`}
              onClick={() => p.onSound("default")}
              title={t("notify.sound.clear")}
              style={iconBtnStyle()}
            >
              <X size={12} />
            </button>
          </Show>
          <button
            data-testid={`${p.testid}-sound-preview`}
            onClick={preview}
            disabled={previewing()}
            title={t("notify.sound.preview")}
            style={{
              ...iconBtnStyle(),
              color: previewFail() ? "var(--color-status-danger-fg, red)" : "var(--color-text)",
            }}
          >
            <Play size={12} />
          </button>
          <Show when={isCustomFile()}>
            <button
              data-testid={`${p.testid}-sound-rechoose`}
              onClick={() => onDropdown(CUSTOM_OPT)}
              title={t("notify.sound.rechoose")}
              style={iconBtnStyle()}
            >
              <FolderOpen size={12} />
            </button>
          </Show>
          <Show when={previewFail()}>
            <span style={{ "font-size": "11px", color: "var(--color-status-danger-fg, red)" }}>
              {t("notify.sound.preview_failed")}
            </span>
          </Show>
        </div>
      </Show>
    </div>
  );
};

const Toggle: Component<{
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  testid?: string;
}> = (p) => (
  <button
    data-testid={p.testid}
    role="switch"
    aria-checked={p.checked}
    disabled={p.disabled}
    onClick={() => !p.disabled && p.onChange(!p.checked)}
    style={{
      width: "36px",
      height: "20px",
      "border-radius": "10px",
      border: "1px solid var(--color-border)",
      background: p.checked ? "var(--color-accent)" : "var(--color-bg)",
      cursor: p.disabled ? "not-allowed" : "pointer",
      position: "relative",
      transition: "background 150ms",
      padding: 0,
      "flex-shrink": 0,
    }}
  >
    <span
      style={{
        position: "absolute",
        top: "1px",
        left: p.checked ? "17px" : "1px",
        width: "16px",
        height: "16px",
        background: "var(--color-surface)",
        "border-radius": "50%",
        transition: "left 150ms",
        "box-shadow": "0 1px 2px rgba(0,0,0,0.2)",
      }}
    />
  </button>
);

/** 标题 + 提示 + 右侧开关的一行(多 agent 提醒区块复用)。 */
const ToggleRow: Component<{
  title: string;
  hint: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
  testid?: string;
}> = (p) => (
  <div style={{ ...rowStyle(), opacity: p.disabled ? 0.5 : 1 }}>
    <div style={{ flex: 1 }}>
      <div style={{ "font-size": "12px", "font-weight": 500, color: "var(--color-text)" }}>
        {p.title}
      </div>
      <div style={hintStyle()}>{p.hint}</div>
    </div>
    <Toggle checked={p.checked} onChange={p.onChange} disabled={p.disabled} testid={p.testid} />
  </div>
);

const TimeInput: Component<{
  label: string;
  value: string;
  onCommit: (v: string) => void;
  testid?: string;
}> = (p) => {
  const [draft, setDraft] = createSignal(p.value);
  const [invalid, setInvalid] = createSignal(false);
  const commit = () => {
    const v = draft().trim();
    if (HH_MM_RE.test(v)) {
      setInvalid(false);
      if (v !== p.value) p.onCommit(v);
    } else {
      setInvalid(true);
      setDraft(p.value);
    }
  };
  return (
    <div style={{ display: "flex", "align-items": "center", gap: "4px" }}>
      <span style={{ "font-size": "11px", color: "var(--color-text-2)" }}>{p.label}</span>
      <input
        data-testid={p.testid}
        value={draft()}
        onInput={(e) => setDraft((e.target as HTMLInputElement).value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.isComposing || e.keyCode === 229) return; // 红线4:IME 组合态回车不触发 blur
          if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
        }}
        placeholder="HH:MM"
        style={{
          width: "70px",
          background: "var(--color-bg)",
          color: invalid() ? "var(--color-status-danger-fg, red)" : "var(--color-text)",
          border: `1px solid ${invalid() ? "var(--color-status-danger-fg, red)" : "var(--color-border)"}`,
          "border-radius": "4px",
          padding: "4px 6px",
          "font-size": "12px",
          "font-family": "monospace",
          "text-align": "center",
        }}
      />
    </div>
  );
};

// ---- styles (复用 settings.tsx 同款 token, 避免跨文件引用私有函数) ----
function sectionStyle() {
  return {
    background: "var(--color-bg)",
    border: "1px solid var(--color-border)",
    "border-radius": "6px",
    padding: "12px",
  };
}

function sectionHeaderStyle() {
  return {
    "font-size": "12px",
    "font-weight": 600,
    "margin-bottom": "8px",
    color: "var(--color-text)",
  };
}

function rowStyle() {
  return {
    display: "flex",
    "align-items": "center",
    gap: "8px",
    padding: "3px 0",
  };
}

function hintStyle() {
  return {
    "font-size": "11px",
    color: "var(--color-text-2)",
    "line-height": 1.5,
    "margin-top": "2px",
  };
}

function selectStyle() {
  return {
    background: "var(--color-bg)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    padding: "3px 6px",
    "font-size": "12px",
    cursor: "pointer",
  };
}

function iconBtnStyle() {
  return {
    display: "inline-flex",
    "align-items": "center",
    "justify-content": "center",
    width: "24px",
    height: "24px",
    background: "transparent",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    cursor: "pointer",
    padding: "0",
  };
}

function primaryBtnStyle() {
  return {
    background: "var(--color-accent)",
    color: "var(--color-bg)",
    border: "none",
    "border-radius": "4px",
    padding: "5px 12px",
    cursor: "pointer",
    "font-size": "12px",
    "font-weight": 500,
  };
}
