// 状态栏自定义 — v6 final: 单列 + 每 profile 自带 "+" 弹 catalog.
//
// 布局:
//   状态栏(标题 + 说明)
//   [终端 default] · 当前
//   PREVIEW: ...
//   [chip] [chip] ... [+] ← 点 + 弹 catalog popover
//
//   [Claude claude]
//   ...
//
//   [Codex codex]
//   ...
//
//   [+ 自定义 profile]  [↺ 重置]
//
//   Item editor (选 chip 后显示)
//
// 每个 profile card 自管 add-popover 状态. 没有右侧固定 catalog 列.

import { For, Show, createMemo, createSignal, onCleanup, onMount, type Component } from "solid-js";
import { GripVertical, Plus, RotateCcw, Search, Trash2, X } from "lucide-solid";
import {
  DragDropProvider,
  DragDropSensors,
  SortableProvider,
  createSortable,
  closestCenter,
  type Draggable,
  type Droppable,
} from "@thisbeyond/solid-dnd";
import { WIDGETS, WIDGET_LIST, ipc, t, tOr, type WidgetMeta } from "@vibeterm/ui-core";
import type {
  ClaudeActiveBlock,
  ClaudeSession,
  ClaudeUsageCache,
  CodexSnapshot,
  GitStatusBrief,
  ProfileConfig,
  StatusLineFile,
  StatusLineItem,
  StatusLineItemDetail,
  TaskDto,
} from "@vibeterm/ipc-types";
import { statusLineItemDetail } from "@vibeterm/ipc-types";

// solid-dnd 用 `use:sortable` directive, TS 需此声明保证类型识别
declare module "solid-js" {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace JSX {
    interface Directives {
      sortable: ReturnType<typeof createSortable>;
    }
  }
}

function normalizeItem(raw: StatusLineItem): StatusLineItemDetail {
  return statusLineItemDetail(raw);
}

const CATEGORY_LABEL: Record<WidgetMeta["category"], string> = {
  core: "Core",
  git: "Git",
  claude: "Claude",
  codex: "Codex",
  layout: "Layout",
};

const CATEGORY_COLOR: Record<WidgetMeta["category"], string> = {
  core: "var(--color-text-2)",
  git: "#f5a623",
  claude: "#d97757",
  codex: "#10a37f",
  layout: "var(--color-text-2)",
};

// ---- mock 数据给每个 profile 预览用 ----

const MOCK_GIT: GitStatusBrief = {
  branch: "main",
  head: "abc1234",
  is_dirty: true,
  ahead: 1,
  behind: 0,
  staged: 2,
  unstaged: 3,
  untracked: 1,
};
const MOCK_CLAUDE_SESSION: ClaudeSession = {
  session_id: "preview",
  project_path: "/Users/example/dev",
  model: "claude-opus-4-7",
  context_tokens: 142000,
  context_window: 1_000_000,
  session_cost_usd: 4.21,
  cache_5m_until_ms: Date.now() + 3.5 * 60_000,
  cache_1h_until_ms: Date.now() + 42 * 60_000,
  effort: "xhigh",
};
const MOCK_CLAUDE_USAGE: ClaudeUsageCache = {
  five_hour: { utilization: 38, resets_at: new Date(Date.now() + 1.5 * 3600_000).toISOString() },
  seven_day: { utilization: 19, resets_at: new Date(Date.now() + 4.5 * 86400_000).toISOString() },
  seven_day_sonnet: { utilization: 12, resets_at: null },
  seven_day_opus: { utilization: 28, resets_at: null },
  seven_day_oauth_apps: null,
  extra_usage: null,
};
const MOCK_CLAUDE_BLOCK: ClaudeActiveBlock = {
  start_at_ms: Date.now() - 1.5 * 3600_000,
  end_at_ms: Date.now() + 3.5 * 3600_000,
  last_entry_at_ms: Date.now() - 60_000,
  tokens_used: 280000,
  elapsed_ms: 1.5 * 3600_000,
  remaining_ms: 3.5 * 3600_000,
  elapsed_pct: 30,
  tokens_per_min_avg: 1500,
  tokens_per_min_recent: 1800,
  burn_rate_level: "normal",
  cost_usd: 4.21,
};
const MOCK_CODEX: CodexSnapshot = {
  session_id: "preview",
  cwd: "/Users/example/dev",
  model: "gpt-5.5",
  model_provider: "openai",
  cli_version: "0.134.0",
  context_tokens: 22000,
  context_window: 258400,
  // (22000 - 12000) / (258400 - 12000) * 100 ≈ 4.06
  context_used_pct: 4.06,
  // free 计划 2026-06 起为月度窗口 (30d=43200), 预览即所见.
  primary_limit: { used_percent: 32, window_minutes: 43200, resets_at: Math.floor(Date.now() / 1000) + 18 * 86400 },
  secondary_limit: null,
  plan_type: "free",
  updated_at_ms: Date.now(),
  tokens_per_min_recent: 800,
  burn_rate_level: "normal",
  effort: "xhigh",
};

type MockCtx = Parameters<(typeof WIDGETS)[string]>[1];

const MOCK_TASK: TaskDto = {
  id: 1,
  name: "Example",
  cwd: "/Users/example/dev",
  pinned: false,
  status: "running",
  terminal_ids: [],
  location: { kind: "MainWorkspace" },
  split_tree: { kind: "leaf", slot_id: 0 } as any,
  worktree: { repo_path: "/Users/example/dev", worktree_path: "/Users/example/dev/wt-feature", branch: "feature-branch", head: "" } as any,
};

function mockCtxFor(profileKey: string): MockCtx {
  const baseGit = () => MOCK_GIT;
  const baseStash = () => 2;
  const baseCwd = () => "/Users/example/dev";
  const baseTask = () => MOCK_TASK;
  const baseTokensToday = () => 1_240_000;
  const basePr = () => "open" as string | null;
  const basePlan = () => "Max 20x" as string | null;
  if (profileKey === "claude") {
    return {
      cwd: baseCwd,
      git: baseGit,
      gitStashCount: baseStash,
      claudeTokensToday: baseTokensToday,
      claudePlan: basePlan,
      prStatus: basePr,
      agentKind: () => "claude",
      claudeSession: () => MOCK_CLAUDE_SESSION,
      claudeUsage: () => MOCK_CLAUDE_USAGE,
      claudeBlock: () => MOCK_CLAUDE_BLOCK,
      codexSnap: () => null,
      task: baseTask,
    };
  }
  if (profileKey === "codex") {
    return {
      cwd: baseCwd,
      git: baseGit,
      gitStashCount: baseStash,
      claudeTokensToday: baseTokensToday,
      claudePlan: basePlan,
      prStatus: basePr,
      agentKind: () => "codex",
      claudeSession: () => null,
      claudeUsage: () => null,
      claudeBlock: () => null,
      codexSnap: () => MOCK_CODEX,
      task: baseTask,
    };
  }
  return {
    cwd: baseCwd,
    git: baseGit,
    gitStashCount: baseStash,
    claudeTokensToday: baseTokensToday,
    claudePlan: basePlan,
    prStatus: basePr,
    agentKind: () => (profileKey === "default" ? null : profileKey),
    claudeSession: () => null,
    claudeUsage: () => null,
    claudeBlock: () => null,
    codexSnap: () => null,
    task: baseTask,
  };
}

// ---- Props ----

export interface StatuslineTabProps {
  activeTerminalId: number | null;
}

const PROFILE_ORDER_HINT = ["default", "claude", "codex"];

// ---- 主组件 ----

export const StatuslineTab: Component<StatuslineTabProps> = (props) => {
  const [config, setConfig] = createSignal<StatusLineFile | null>(null);
  const [selected, setSelected] = createSignal<{ profile: string; idx: number } | null>(null);
  const [currentAgent, setCurrentAgent] = createSignal<string | null>(null);

  const reload = async () => {
    try {
      setConfig(await ipc.getStatusLineConfig());
    } catch (e) {
      console.warn("[statusline] load failed", e);
    }
  };

  const refreshAgent = async () => {
    const tid = props.activeTerminalId;
    if (tid == null) {
      setCurrentAgent(null);
      return;
    }
    try {
      const d = await ipc.detectAgentForTerminal(tid);
      setCurrentAgent(d.agent_kind ?? null);
    } catch {
      setCurrentAgent(null);
    }
  };

  onMount(() => {
    void reload();
    void refreshAgent();
  });

  const save = async (next: StatusLineFile) => {
    setConfig(next);
    try {
      await ipc.saveStatusLineConfig(next);
    } catch (e) {
      console.warn("[statusline] save failed", e);
    }
  };

  const profiles = createMemo<Array<[string, ProfileConfig]>>(() => {
    const cfg = config();
    if (!cfg) return [];
    const known = new Set(PROFILE_ORDER_HINT);
    const entries: Array<[string, ProfileConfig]> = [];
    for (const k of PROFILE_ORDER_HINT) {
      if (cfg.profiles[k]) entries.push([k, cfg.profiles[k]]);
    }
    const custom = Object.keys(cfg.profiles).filter((k) => !known.has(k)).sort();
    for (const k of custom) entries.push([k, cfg.profiles[k]]);
    return entries;
  });

  const updateProfile = (key: string, profile: ProfileConfig) => {
    const cfg = config();
    if (!cfg) return;
    const next = { ...cfg, profiles: { ...cfg.profiles, [key]: profile } };
    void save(next);
  };

  const addItem = (profileKey: string, type: string) => {
    const cfg = config();
    if (!cfg) return;
    const profile = cfg.profiles[profileKey] ?? { items: [] };
    const items = [...profile.items, type as StatusLineItem];
    updateProfile(profileKey, { ...profile, items });
  };

  const removeItem = (profileKey: string, idx: number) => {
    const cfg = config();
    if (!cfg) return;
    const profile = cfg.profiles[profileKey];
    if (!profile) return;
    const items = profile.items.filter((_, i) => i !== idx);
    updateProfile(profileKey, { ...profile, items });
    setSelected(null);
  };

  const reorderItem = (profileKey: string, fromIdx: number, toIdx: number) => {
    const cfg = config();
    if (!cfg) return;
    const profile = cfg.profiles[profileKey];
    if (!profile || fromIdx === toIdx) return;
    const items = [...profile.items];
    const [moved] = items.splice(fromIdx, 1);
    const adjusted = toIdx > fromIdx ? toIdx - 1 : toIdx;
    items.splice(adjusted, 0, moved);
    updateProfile(profileKey, { ...profile, items });
    setSelected({ profile: profileKey, idx: adjusted });
  };

  const updateItemDetail = (profileKey: string, idx: number, patch: Partial<StatusLineItemDetail>) => {
    const cfg = config();
    if (!cfg) return;
    const profile = cfg.profiles[profileKey];
    if (!profile) return;
    const cur = normalizeItem(profile.items[idx]);
    const next: StatusLineItemDetail = { ...cur, ...patch };
    const items = [...profile.items];
    items[idx] = next;
    updateProfile(profileKey, { ...profile, items });
  };

  const addCustomProfile = () => {
    const key = prompt(t("statusbar.profile_prompt"));
    if (!key) return;
    const cfg = config();
    if (!cfg) return;
    if (cfg.profiles[key]) {
      alert(t("statusbar.profile_exists", { key }));
      return;
    }
    const next = {
      ...cfg,
      profiles: {
        ...cfg.profiles,
        [key]: { display_name: key, items: ["current-dir", "git-branch"] as StatusLineItem[] },
      },
    };
    void save(next);
  };

  const deleteProfile = (key: string) => {
    if (key === "default") {
      alert(t("statusbar.cannot_delete_default"));
      return;
    }
    if (!confirm(t("statusbar.delete_profile_confirm", { key }))) return;
    const cfg = config();
    if (!cfg) return;
    const profiles = { ...cfg.profiles };
    delete profiles[key];
    void save({ ...cfg, profiles });
  };

  const resetDefault = async () => {
    if (!confirm(t("statusbar.reset_confirm"))) return;
    try {
      await ipc.saveStatusLineConfig({
        schema_version: 2,
        use_theme_colors: true,
        profiles: {
          default: {
            display_name: t("statusbar.profile.default"),
            items: [
              "current-dir",
              "worktree-name",
              "git-branch",
              "git-stash-count",
              "pr-status",
            ],
          },
          claude: {
            display_name: "Claude",
            items: [
              "current-dir",
              "git-branch",
              "separator",
              "claude-plan",
              "claude-model",
              "claude-effort",
              "claude-ctx",
              "claude-5h",
              "claude-7d",
            ],
          },
          codex: {
            display_name: "Codex",
            items: [
              "current-dir",
              "git-branch",
              "separator",
              "codex-plan",
              "codex-model",
              "codex-effort",
              "codex-ctx",
              "codex-5h",
              "codex-7d",
            ],
          },
        },
      });
      await reload();
      setSelected(null);
    } catch (e) {
      console.warn("[statusline] reset failed", e);
    }
  };

  const selectedDetail = createMemo<StatusLineItemDetail | null>(() => {
    const s = selected();
    const cfg = config();
    if (!s || !cfg) return null;
    const profile = cfg.profiles[s.profile];
    if (!profile || s.idx >= profile.items.length) return null;
    return normalizeItem(profile.items[s.idx]);
  });

  const selectedMeta = (): WidgetMeta | undefined => {
    const d = selectedDetail();
    return d ? WIDGET_LIST.find((w) => w.id === d.type) : undefined;
  };

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "10px", height: "100%" }}>
      <div>
        <h3 style={{ margin: "0 0 4px 0", "font-size": "14px" }}>{t("statusbar.title")}</h3>
        <div style={{ "font-size": "11px", color: "var(--color-text-2)", "line-height": 1.5 }}>
          {t("statusbar.description")}
        </div>
      </div>

      {/* 单列: profile stack + 底部按钮 + 选中 editor */}
      <div
        style={{
          display: "flex",
          "flex-direction": "column",
          gap: "10px",
          flex: 1,
          "min-height": 0,
          "overflow-y": "auto",
          padding: "2px",
        }}
      >
        <For each={profiles()}>
          {([key, profile]) => (
            <ProfileCard
              profileKey={key}
              profile={profile}
              isCurrentTerminal={
                currentAgent() === key || (!currentAgent() && key === "default")
              }
              onDelete={key === "default" ? undefined : () => deleteProfile(key)}
              selectedIdx={selected()?.profile === key ? selected()!.idx : null}
              onSelect={(idx) => setSelected({ profile: key, idx })}
              onReorder={(from, to) => reorderItem(key, from, to)}
              onRemove={(idx) => removeItem(key, idx)}
              onAddWidget={(type) => addItem(key, type)}
            />
          )}
        </For>

        <div style={{ display: "flex", gap: "8px", "padding-top": "4px" }}>
          <button onClick={addCustomProfile} style={secondaryBtn()}>
            <Plus size={11} /> {t("statusbar.add_profile")}
          </button>
          <button onClick={resetDefault} title={t("statusbar.reset_default")} style={secondaryBtn()}>
            <RotateCcw size={11} /> {t("statusbar.reset_default")}
          </button>
        </div>

        <Show when={selectedDetail()}>
          <div
            ref={(el) => {
              // 选中时滚动到 editor
              requestAnimationFrame(() => {
                el?.scrollIntoView({ behavior: "smooth", block: "nearest" });
              });
            }}
          >
            <ItemEditor
              item={selectedDetail()!}
              meta={selectedMeta()}
              profileKey={selected()!.profile}
              onClose={() => setSelected(null)}
              onChange={(patch) => updateItemDetail(selected()!.profile, selected()!.idx, patch)}
            />
          </div>
        </Show>
      </div>
    </div>
  );
};

// ---- ProfileCard ----

const ProfileCard: Component<{
  profileKey: string;
  profile: ProfileConfig;
  isCurrentTerminal: boolean;
  onDelete?: () => void;
  selectedIdx: number | null;
  onSelect: (idx: number) => void;
  onReorder: (from: number, to: number) => void;
  onRemove: (idx: number) => void;
  onAddWidget: (type: string) => void;
}> = (props) => {
  const [addOpen, setAddOpen] = createSignal(false);

  const mockCtx = mockCtxFor(props.profileKey);

  // 给每个 item 分配稳定 id (index based — 拖动期间数组不变, drop 时一次性 reorder)
  const ids = () => props.profile.items.map((_, i) => `chip-${i}`);

  const handleDragEnd = (event: { draggable: Draggable | null; droppable?: Droppable | null }) => {
    const { draggable, droppable } = event;
    if (!draggable || !droppable || draggable.id === droppable.id) return;
    const from = parseInt(String(draggable.id).split("-")[1], 10);
    const to = parseInt(String(droppable.id).split("-")[1], 10);
    if (Number.isFinite(from) && Number.isFinite(to)) {
      props.onReorder(from, to);
    }
  };

  const renderedPreview = () => {
    return props.profile.items
      .map((raw) => {
        const item = normalizeItem(raw);
        if (item.hide) return null;
        const renderer = WIDGETS[item.type];
        if (!renderer) return null;
        return renderer(item, mockCtx);
      })
      .filter((x) => x != null);
  };

  return (
    <div
      style={{
        background: "var(--color-bg)",
        border: "1px solid var(--color-border)",
        "border-left": props.isCurrentTerminal ? "3px solid var(--color-accent)" : "1px solid var(--color-border)",
        "border-radius": "8px",
        padding: "12px 14px",
        transition: "border-color 150ms ease",
      }}
    >
      {/* 标题行 */}
      <div style={{ display: "flex", "align-items": "center", gap: "8px", "margin-bottom": "10px" }}>
        <span style={{ "font-weight": 600, color: "var(--color-text)", "font-size": "13px" }}>
          {props.profile.display_name ?? props.profileKey}
        </span>
        <code style={profileChipStyle()}>{props.profileKey}</code>
        <Show when={props.isCurrentTerminal}>
          <span style={currentBadgeStyle()}>{t("statusbar.current_terminal")}</span>
        </Show>
        <div style={{ flex: 1 }} />
        <span style={{ "font-size": "10px", color: "var(--color-text-2)", opacity: 0.6 }}>
          {t("statusbar.widget_count", { count: props.profile.items.length })}
        </span>
        <Show when={props.onDelete}>
          <button
            onClick={(e) => {
              e.stopPropagation();
              props.onDelete?.();
            }}
            title={t("statusbar.delete_profile")}
            style={miniBtn()}
          >
            <Trash2 size={11} />
          </button>
        </Show>
      </div>

      {/* PREVIEW (mock) */}
      <div
        style={{
          background: "var(--color-surface)",
          "border-radius": "5px",
          padding: "8px 10px",
          "margin-bottom": "10px",
          "min-height": "30px",
          display: "flex",
          "align-items": "center",
          gap: "10px",
          "font-size": "11px",
          color: "var(--color-text-2)",
          overflow: "hidden",
        }}
      >
        <span
          style={{
            "font-size": "9px",
            "text-transform": "uppercase",
            "letter-spacing": "0.8px",
            opacity: 0.5,
            "font-weight": 600,
            "flex-shrink": 0,
          }}
        >
          {t("statusbar.preview")}
        </span>
        <div style={{ display: "flex", "align-items": "center", gap: "10px", "flex-wrap": "wrap" }}>
          <Show
            when={renderedPreview().length > 0}
            fallback={<span style={{ opacity: 0.4 }}>{t("statusbar.empty_preview")}</span>}
          >
            <For each={renderedPreview()}>{(node) => node as any}</For>
          </Show>
        </div>
      </div>

      {/* Chip row + 末尾 + 按钮 (弹 catalog popover) — solid-dnd 排序 */}
      <div style={{ position: "relative" }}>
        <div style={{ display: "flex", "flex-wrap": "wrap", gap: "4px", "align-items": "center" }}>
          <DragDropProvider onDragEnd={handleDragEnd} collisionDetector={closestCenter}>
            <DragDropSensors />
            <SortableProvider ids={ids()}>
              <For each={props.profile.items}>
                {(raw, i) => (
                  <SortableChip
                    id={`chip-${i()}`}
                    raw={raw}
                    isSelected={props.selectedIdx === i()}
                    onClick={() => props.onSelect(i())}
                    onRemove={() => props.onRemove(i())}
                  />
                )}
              </For>
            </SortableProvider>
          </DragDropProvider>

          {/* + 按钮 */}
          <button
            onClick={(e) => {
              e.stopPropagation();
              setAddOpen(true);
            }}
            title={t("statusbar.add_widget")}
            style={{
              display: "inline-flex",
              "align-items": "center",
              gap: "4px",
              padding: "4px 10px 4px 8px",
              "border-radius": "5px",
              background: addOpen() ? "var(--color-accent-subtle)" : "transparent",
              color: "var(--color-accent)",
              border: "1px dashed var(--color-accent)",
              cursor: "pointer",
              "font-size": "11px",
              "font-weight": 500,
              transition: "background 120ms",
            }}
            onMouseOver={(e) => {
              if (!addOpen()) (e.currentTarget as HTMLElement).style.background = "var(--color-accent-subtle)";
            }}
            onMouseOut={(e) => {
              if (!addOpen()) (e.currentTarget as HTMLElement).style.background = "transparent";
            }}
          >
            <Plus size={12} /> {t("statusbar.add_widget")}
          </button>
        </div>

        {/* widget catalog popover */}
        <Show when={addOpen()}>
          <CatalogPopover
            onClose={() => setAddOpen(false)}
            onPick={(id) => {
              props.onAddWidget(id);
              setAddOpen(false);
            }}
            existingIds={new Set(props.profile.items.map((i) => normalizeItem(i).type))}
          />
        </Show>
      </div>
    </div>
  );
};

// ---- SortableChip (用 solid-dnd) ----

const SortableChip: Component<{
  id: string;
  raw: StatusLineItem;
  isSelected: boolean;
  onClick: () => void;
  onRemove: () => void;
}> = (props) => {
  const sortable = createSortable(props.id);
  const detail = () => normalizeItem(props.raw);
  const meta = () => WIDGET_LIST.find((w) => w.id === detail().type);
  const [hovered, setHovered] = createSignal(false);

  return (
    <div
      // @ts-expect-error use:sortable directive
      use:sortable
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onClick={(e) => {
        e.stopPropagation();
        props.onClick();
      }}
      style={{
        display: "inline-flex",
        "align-items": "center",
        gap: "5px",
        padding: "4px 9px 4px 6px",
        "border-radius": "5px",
        background: props.isSelected ? "var(--color-accent)" : "var(--color-surface)",
        color: props.isSelected ? "var(--color-bg)" : "var(--color-text)",
        border: props.isSelected ? "1px solid var(--color-accent)" : "1px solid var(--color-border)",
        cursor: sortable.isActiveDraggable ? "grabbing" : "grab",
        "font-size": "11px",
        "user-select": "none",
        opacity: sortable.isActiveDraggable ? 0.3 : detail().hide ? 0.45 : 1,
        "touch-action": "none",
        transition: sortable.isActiveDraggable
          ? "none"
          : "transform 200ms cubic-bezier(0.16, 1, 0.3, 1), background 120ms ease, border-color 120ms ease, opacity 120ms",
      }}
      title={`${meta() ? tOr(`statusbar.widget_desc.${meta()!.id}`, meta()!.description) : detail().type}\nid: ${detail().type}`}
    >
      <GripVertical size={10} style={{ opacity: hovered() || props.isSelected ? 0.7 : 0.3, transition: "opacity 120ms" }} />
      <span style={{ "font-weight": 500 }}>{meta() ? tOr(`statusbar.widget.${meta()!.id}`, meta()!.display_name) : detail().type}</span>
      <Show when={detail().hide}>
        <span style={{ "font-size": "9px", opacity: 0.7, "font-style": "italic" }}>(hidden)</span>
      </Show>
      <Show when={hovered() || props.isSelected}>
        <button
          onClick={(e) => {
            e.stopPropagation();
            props.onRemove();
          }}
          title={t("statusbar.remove_item")}
          style={chipXBtn()}
        >
          <X size={11} />
        </button>
      </Show>
    </div>
  );
};

// ---- CatalogPopover ----

const CatalogPopover: Component<{
  onClose: () => void;
  onPick: (id: string) => void;
  existingIds: Set<string>;
}> = (props) => {
  const [search, setSearch] = createSignal("");

  // 点外部关闭
  onMount(() => {
    const handler = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest("[data-catalog-popover='true']")) return;
      props.onClose();
    };
    // 延 1 tick 注册避免触发当前 click
    setTimeout(() => document.addEventListener("mousedown", handler), 0);
    onCleanup(() => document.removeEventListener("mousedown", handler));
  });

  const groups = createMemo<Array<[WidgetMeta["category"], WidgetMeta[]]>>(() => {
    const q = search().toLowerCase().trim();
    const m = new Map<WidgetMeta["category"], WidgetMeta[]>();
    for (const w of WIDGET_LIST) {
      const tname = tOr(`statusbar.widget.${w.id}`, w.display_name);
      if (q && !w.id.toLowerCase().includes(q) && !tname.toLowerCase().includes(q)) continue;
      const arr = m.get(w.category) ?? [];
      arr.push(w);
      m.set(w.category, arr);
    }
    return Array.from(m.entries());
  });

  return (
    <div
      data-catalog-popover="true"
      onClick={(e) => e.stopPropagation()}
      style={{
        position: "absolute",
        top: "100%",
        "margin-top": "8px",
        left: 0,
        right: 0,
        background: "var(--color-surface)",
        border: "1px solid var(--color-border)",
        "border-radius": "10px",
        "box-shadow": "0 12px 36px rgba(0,0,0,0.55)",
        "max-height": "400px",
        display: "flex",
        "flex-direction": "column",
        "z-index": 100,
      }}
    >
      <div
        style={{
          padding: "10px 12px",
          "border-bottom": "1px solid var(--color-border)",
          display: "flex",
          "align-items": "center",
          gap: "8px",
        }}
      >
        <Search size={12} style={{ color: "var(--color-text-2)" }} />
        <input
          type="text"
          value={search()}
          autofocus
          onInput={(e) => setSearch(e.currentTarget.value)}
          placeholder={t("statusbar.search_widget")}
          onKeyDown={(e) => {
            if (e.key === "Escape") props.onClose();
          }}
          style={{
            flex: 1,
            background: "transparent",
            border: "none",
            outline: "none",
            color: "var(--color-text)",
            "font-size": "12px",
          }}
        />
        <button onClick={props.onClose} style={miniBtn()} title={t("statusbar.esc_close")}>
          <X size={11} />
        </button>
      </div>

      <div style={{ flex: 1, "overflow-y": "auto", padding: "4px 0" }}>
        <For each={groups()}>
          {([cat, widgets]) => (
            <>
              <div
                style={{
                  "font-size": "9px",
                  "text-transform": "uppercase",
                  "letter-spacing": "0.8px",
                  color: CATEGORY_COLOR[cat],
                  "font-weight": 700,
                  padding: "6px 12px 2px",
                }}
              >
                {CATEGORY_LABEL[cat]} ({widgets.length})
              </div>
              <div style={{ display: "grid", "grid-template-columns": "1fr 1fr", gap: "2px", padding: "0 6px 4px" }}>
                <For each={widgets}>
                  {(w) => {
                    const already = props.existingIds.has(w.id);
                    return (
                      <div
                        onClick={() => props.onPick(w.id)}
                        title={tOr(`statusbar.widget_desc.${w.id}`, w.description)}
                        style={{
                          padding: "5px 8px",
                          cursor: "pointer",
                          "font-size": "11px",
                          "border-radius": "3px",
                        }}
                        onMouseOver={(e) => (e.currentTarget.style.background = "var(--color-accent-subtle)")}
                        onMouseOut={(e) => (e.currentTarget.style.background = "transparent")}
                      >
                        <div
                          style={{
                            "font-weight": 500,
                            color: "var(--color-text)",
                            display: "flex",
                            "align-items": "center",
                            gap: "4px",
                          }}
                        >
                          <Show when={already}>
                            <span style={{ "font-size": "9px", opacity: 0.6 }}>✓</span>
                          </Show>
                          {tOr(`statusbar.widget.${w.id}`, w.display_name)}
                        </div>
                        <div
                          style={{
                            "font-size": "10px",
                            color: "var(--color-text-2)",
                            "white-space": "nowrap",
                            overflow: "hidden",
                            "text-overflow": "ellipsis",
                          }}
                        >
                          {tOr(`statusbar.widget_desc.${w.id}`, w.description)}
                        </div>
                      </div>
                    );
                  }}
                </For>
              </div>
            </>
          )}
        </For>
        <Show when={groups().length === 0}>
          <div style={{ padding: "20px", "text-align": "center", color: "var(--color-text-2)", "font-size": "11px" }}>
            {t("statusbar.no_match")}
          </div>
        </Show>
      </div>
    </div>
  );
};

// ---- Item editor ----

const ItemEditor: Component<{
  item: StatusLineItemDetail;
  meta?: WidgetMeta;
  profileKey: string;
  onClose: () => void;
  onChange: (patch: Partial<StatusLineItemDetail>) => void;
}> = (props) => {
  const [newKey, setNewKey] = createSignal("");
  const [newValue, setNewValue] = createSignal("");

  const updateMetadata = (k: string, v: string) => {
    const next = { ...(props.item.metadata ?? {}) };
    if (v === "") delete next[k];
    else next[k] = v;
    props.onChange({ metadata: next });
  };

  const removeMetadata = (k: string) => {
    const next = { ...(props.item.metadata ?? {}) };
    delete next[k];
    props.onChange({ metadata: next });
  };

  const submitNewMetadata = () => {
    const k = newKey().trim();
    if (!k) return;
    updateMetadata(k, newValue());
    setNewKey("");
    setNewValue("");
  };

  return (
    <div
      style={{
        background: "var(--color-bg)",
        border: "1px solid var(--color-accent)",
        "border-radius": "8px",
        padding: "12px",
        display: "flex",
        "flex-direction": "column",
        gap: "8px",
        "font-size": "12px",
      }}
    >
      <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
        <div style={{ "font-weight": 600, "font-size": "13px" }}>
          {props.meta ? tOr(`statusbar.widget.${props.meta.id}`, props.meta.display_name) : props.item.type}
        </div>
        <code style={{ "font-size": "10px", color: "var(--color-text-2)" }}>
          [{props.profileKey}].{props.item.type}
        </code>
        <div style={{ flex: 1 }} />
        <button onClick={props.onClose} title={t("statusbar.close")} style={miniBtn()}>
          <X size={11} />
        </button>
      </div>
      <Show when={props.meta}>
        <div style={{ "font-size": "11px", color: "var(--color-text-2)", "line-height": 1.5 }}>
          {tOr(`statusbar.widget_desc.${props.meta!.id}`, props.meta!.description)}
        </div>
      </Show>

      <div style={{ display: "grid", "grid-template-columns": "1fr 1fr", gap: "8px" }}>
        <Field label={t("statusbar.color")}>
          <div style={{ display: "flex", gap: "4px", "align-items": "center", flex: 1 }}>
            <input
              type="color"
              value={hexFor(props.item.color)}
              onInput={(e) => props.onChange({ color: e.currentTarget.value })}
              style={{
                width: "26px",
                height: "20px",
                padding: 0,
                border: "1px solid var(--color-border)",
                "border-radius": "3px",
                background: "transparent",
                cursor: "pointer",
              }}
            />
            <input
              type="text"
              value={props.item.color ?? ""}
              placeholder="default / var(--..) / #hex"
              onInput={(e) => props.onChange({ color: e.currentTarget.value || undefined })}
              style={{ ...inputStyle(), flex: 1 }}
            />
            <Show when={props.item.color}>
              <button onClick={() => props.onChange({ color: undefined })} title="×" style={miniBtn()}>
                ×
              </button>
            </Show>
          </div>
        </Field>

        <Field label={t("statusbar.max_width")}>
          <input
            type="number"
            value={props.item.max_width ?? ""}
            placeholder={t("statusbar.unlimited")}
            onInput={(e) => {
              const n = parseInt(e.currentTarget.value, 10);
              props.onChange({ max_width: Number.isFinite(n) ? n : undefined });
            }}
            style={{ ...inputStyle(), width: "80px" }}
          />
          <span style={{ "font-size": "10px", color: "var(--color-text-2)", "margin-left": "4px" }}>px</span>
        </Field>

        <Field label={t("statusbar.bold")}>
          <input
            type="checkbox"
            checked={!!props.item.bold}
            onChange={(e) => props.onChange({ bold: e.currentTarget.checked || undefined })}
          />
        </Field>

        <Field label={t("statusbar.hide")}>
          <input
            type="checkbox"
            checked={!!props.item.hide}
            onChange={(e) => props.onChange({ hide: e.currentTarget.checked || undefined })}
          />
        </Field>
      </div>

      <div>
        <div
          style={{
            "font-size": "10px",
            color: "var(--color-text-2)",
            "text-transform": "uppercase",
            "letter-spacing": "0.5px",
            "font-weight": 600,
            "margin-bottom": "4px",
          }}
        >
          {t("statusbar.metadata")}
        </div>
        <For each={Object.entries(props.item.metadata ?? {})}>
          {([k, v]) => (
            <div style={{ display: "flex", gap: "4px", "margin-bottom": "3px", "align-items": "center" }}>
              <input
                type="text"
                value={k}
                disabled
                style={{ ...inputStyle(), "min-width": "100px", flex: "0 0 100px", background: "var(--color-surface)" }}
              />
              <input
                type="text"
                value={v}
                onInput={(e) => updateMetadata(k, e.currentTarget.value)}
                style={{ ...inputStyle(), flex: 1 }}
              />
              <button onClick={() => removeMetadata(k)} style={miniBtn()}>
                <Trash2 size={10} />
              </button>
            </div>
          )}
        </For>
        <div style={{ display: "flex", gap: "4px", "margin-top": "4px", "align-items": "center" }}>
          <input
            type="text"
            value={newKey()}
            onInput={(e) => setNewKey(e.currentTarget.value)}
            placeholder="key"
            onKeyDown={(e) => {
              if (e.key === "Enter") submitNewMetadata();
            }}
            style={{ ...inputStyle(), "min-width": "100px", flex: "0 0 100px" }}
          />
          <input
            type="text"
            value={newValue()}
            onInput={(e) => setNewValue(e.currentTarget.value)}
            placeholder="value"
            onKeyDown={(e) => {
              if (e.key === "Enter") submitNewMetadata();
            }}
            style={{ ...inputStyle(), flex: 1 }}
          />
          <button
            onClick={submitNewMetadata}
            disabled={!newKey().trim()}
            style={{ ...miniBtn(), opacity: newKey().trim() ? 1 : 0.4 }}
          >
            +
          </button>
        </div>
      </div>
    </div>
  );
};

const Field: Component<{ label: string; children: any }> = (props) => (
  <div style={{ display: "flex", "align-items": "center", gap: "6px" }}>
    <span style={{ "min-width": "60px", "font-size": "10px", color: "var(--color-text-2)" }}>
      {props.label}
    </span>
    {props.children}
  </div>
);

// ---- styles ----

function hexFor(color: string | undefined): string {
  if (!color) return "#888888";
  if (/^#[0-9a-f]{6}$/i.test(color)) return color;
  if (/^#[0-9a-f]{3}$/i.test(color)) {
    const m = color.slice(1);
    return `#${m[0]}${m[0]}${m[1]}${m[1]}${m[2]}${m[2]}`;
  }
  return "#888888";
}

function inputStyle() {
  return {
    background: "var(--color-surface)",
    border: "1px solid var(--color-border)",
    "border-radius": "3px",
    color: "var(--color-text)",
    padding: "3px 6px",
    "font-size": "11px",
    "font-family": "ui-monospace, SFMono-Regular, monospace",
  };
}

function secondaryBtn() {
  return {
    display: "inline-flex" as const,
    "align-items": "center" as const,
    gap: "4px",
    padding: "5px 10px",
    background: "transparent",
    color: "var(--color-text-2)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    cursor: "pointer" as const,
    "font-size": "11px",
  };
}

function miniBtn() {
  return {
    padding: "3px 6px",
    background: "transparent",
    border: "1px solid var(--color-border)",
    "border-radius": "3px",
    color: "var(--color-text-2)",
    "font-size": "10px",
    cursor: "pointer" as const,
    display: "inline-flex" as const,
    "align-items": "center" as const,
  };
}

function chipXBtn() {
  return {
    padding: "0",
    background: "transparent",
    border: "none",
    color: "inherit",
    cursor: "pointer" as const,
    opacity: 0.6,
    display: "inline-flex" as const,
    "align-items": "center" as const,
  };
}

function profileChipStyle() {
  return {
    "font-size": "10px",
    padding: "1px 6px",
    "border-radius": "10px",
    background: "var(--color-border)",
    color: "var(--color-text-2)",
  };
}

function currentBadgeStyle() {
  return {
    "font-size": "10px",
    padding: "1px 6px",
    "border-radius": "10px",
    background: "var(--color-accent)",
    color: "var(--color-bg)",
    "font-weight": 600,
  };
}
