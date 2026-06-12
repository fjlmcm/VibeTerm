// 命令面板
//
// 任务 + 内置 4 个命令(新建任务 / 切主题 / 关闭当前任务 / 刷新主题列表)
// 模糊搜索:简化版 substring(引入 fuzzysort + pinyin-pro)

import { For, Show, createMemo, createSignal, onCleanup, onMount, type Component } from "solid-js";
import { ipc, modKeyLabel, requestRenderRepair, t } from "@vibeterm/ui-core";
import type { ActionEntry, LayoutTemplate, TaskDto, TerminalId } from "@vibeterm/ipc-types";

export interface CommandPaletteProps {
  tasks: TaskDto[];
  onClose: () => void;
  onActivateTask: (id: number) => void;
  onOpenSettings?: () => void;
  /** 打开使用统计面板 */
  onOpenStats?: () => void;
  /** 打开当前任务的 diff 查看器 */
  onOpenDiff?: () => void;
  /** 应用布局模板(创建预设任务) */
  onApplyLayout?: (template: LayoutTemplate) => void;
  /** 恢复当前任务的 agent 会话(只读嗅探 session_id → 新 pane 跑 resume 命令) */
  onResumeAgent?: () => void;
  /** 新建任务 → 交给上层弹 NewTaskDialog 模态(macOS WKWebView 禁用 prompt()) */
  onCreateTask?: () => void;
  /** A4:当前聚焦的 terminal,用于 current_terminal / insert 模式 action */
  currentTerminalId?: TerminalId | null;
}

interface CmdItem {
  id: string;
  label: string;
  hint?: string;
  action: () => void | Promise<void>;
}

export const CommandPalette: Component<CommandPaletteProps> = (props) => {
  const [q, setQ] = createSignal("");
  const [highlighted, setHighlighted] = createSignal(0);
  const [themes, setThemes] = createSignal<{ id: string; name: string }[]>([]);
  const [actions, setActions] = createSignal<ActionEntry[]>([]);
  const [layouts, setLayouts] = createSignal<LayoutTemplate[]>([]);

  const refreshActions = async () => {
    try {
      const f = await ipc.getActions();
      setActions(f.actions);
    } catch (e) {
      console.error("[palette] getActions failed", e);
    }
  };

  // Tauri 事件监听器在 Rust 侧注册,不受 JS GC 影响 —— 必须显式注销,
  // 否则每次打开命令面板都泄漏一个 actions_changed 监听器。
  let off: (() => void) | null = null;
  onMount(async () => {
    try {
      const list = await ipc.listThemes();
      setThemes(list.map((t) => ({ id: t.id, name: t.name })));
    } catch (e) {
      console.error(e);
    }
    await refreshActions();
    if (props.onApplyLayout) {
      try {
        setLayouts(await ipc.listLayouts());
      } catch (e) {
        console.error("[palette] listLayouts failed", e);
      }
    }
    off = await ipc.onActionsChanged(refreshActions);
  });
  // onCleanup 在组件同步上下文注册;off 在 onMount 的 await 之后才被赋值,
  // 卸载时若监听器已注册则注销。
  onCleanup(() => {
    off?.();
    off = null;
  });

  const allItems = createMemo<CmdItem[]>(() => {
    const items: CmdItem[] = [];

    // 任务
    for (const task of props.tasks) {
      items.push({
        id: `task:${task.id}`,
        label: t("palette.cmd.jump_task", { name: task.name }),
        hint: task.status,
        action: () => props.onActivateTask(task.id),
      });
    }

    // Custom Actions(A4)— 顶部,因为执行频率最高
    for (const a of actions()) {
      items.push({
        id: `action:${a.id}`,
        label: `▶ ${a.title}`,
        hint: a.shortcut ?? a.mode,
        action: async () => {
          try {
            await ipc.executeAction(a.id, props.currentTerminalId ?? null);
          } catch (e) {
            console.error(`[palette] executeAction(${a.id}) failed`, e);
          }
          props.onClose();
        },
      });
    }

    // 布局模板(任务预设)
    for (const lay of layouts()) {
      items.push({
        id: `layout:${lay.name}`,
        label: t("palette.cmd.apply_layout", { name: lay.name }),
        hint: lay.keywords.join(" "),
        action: () => {
          props.onClose();
          props.onApplyLayout?.(lay);
        },
      });
    }

    // 主题切换
    for (const th of themes()) {
      items.push({
        id: `theme:${th.id}`,
        label: t("palette.cmd.switch_theme", { name: th.name }),
        action: async () => {
          await ipc.setActiveTheme(th.id);
          props.onClose();
        },
      });
    }

    // 内置命令 — 新建任务交给上层弹 NewTaskDialog 模态;
    // macOS WKWebView 禁用 prompt(),不能在此直接调原生对话框(会静默失败)。
    items.push({
      id: "cmd:new-task",
      label: t("palette.cmd.new_task"),
      hint: `${modKeyLabel()}+N`,
      action: () => {
        props.onClose();
        props.onCreateTask?.();
      },
    });

    if (props.onOpenStats) {
      items.push({
        id: "cmd:open-stats",
        label: t("palette.cmd.open_stats"),
        action: () => props.onOpenStats?.(),
      });
    }

    if (props.onOpenDiff) {
      items.push({
        id: "cmd:open-diff",
        label: t("palette.cmd.open_diff"),
        action: () => props.onOpenDiff?.(),
      });
    }

    if (props.onResumeAgent) {
      items.push({
        id: "cmd:resume-agent",
        label: t("palette.cmd.resume_agent"),
        action: () => {
          props.onClose();
          props.onResumeAgent?.();
        },
      });
    }

    // WebGL 纹理图集偶发损坏(睡眠唤醒/GPU 波动)会让字形画错 —— 让所有
    // Terminal 实例 clearTextureAtlas 立即重绘,详见 ui-core terminal 顶部注释。
    items.push({
      id: "cmd:repair-render",
      label: t("palette.cmd.repair_render"),
      action: () => {
        props.onClose();
        requestRenderRepair();
      },
    });

    if (props.onOpenSettings) {
      items.push({
        id: "cmd:open-settings",
        label: t("kb.command.open_settings"),
        hint: `${modKeyLabel()}+,`,
        action: () => props.onOpenSettings?.(),
      });
    }

    return items;
  });

  const filtered = createMemo(() => {
    const query = q().trim().toLowerCase();
    if (!query) return allItems();
    return allItems().filter((it) => it.label.toLowerCase().includes(query));
  });

  const onKey = (e: KeyboardEvent) => {
    // 🔴 红线4:IME 组合态(中文/日文等候选)下,方向键/回车/Esc 交给输入法选词,不驱动面板。
    if (e.isComposing || e.keyCode === 229) return;
    const items = filtered();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlighted((h) => Math.min(items.length - 1, h + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlighted((h) => Math.max(0, h - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const it = items[highlighted()];
      if (it) it.action();
    } else if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.4)",
        display: "flex",
        "justify-content": "center",
        "align-items": "flex-start",
        "padding-top": "10vh",
        "z-index": 2000,
      }}
      onClick={props.onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          "border-radius": "8px",
          width: "600px",
          "max-width": "90vw",
          "max-height": "60vh",
          display: "flex",
          "flex-direction": "column",
          overflow: "hidden",
          "box-shadow": "0 8px 24px rgba(0,0,0,0.5)",
        }}
      >
        <input
          data-testid="palette-input"
          autofocus
          value={q()}
          onInput={(e) => {
            setQ(e.currentTarget.value);
            setHighlighted(0);
          }}
          onKeyDown={onKey}
          placeholder={t("palette.placeholder")}
          style={{
            background: "var(--color-bg)",
            color: "var(--color-text)",
            border: "none",
            "border-bottom": "1px solid var(--color-border)",
            padding: "12px 16px",
            "font-size": "14px",
            outline: "none",
          }}
        />
        <div data-testid="palette-list" data-count={filtered().length} style={{ "overflow-y": "auto", flex: "1" }}>
          <For each={filtered()}>
            {(it, i) => (
              <div
                data-testid={`palette-item-${it.id}`}
                data-highlighted={i() === highlighted() ? "true" : "false"}
                onClick={() => it.action()}
                onMouseEnter={() => setHighlighted(i())}
                style={{
                  padding: "8px 16px",
                  cursor: "pointer",
                  display: "flex",
                  "justify-content": "space-between",
                  "align-items": "center",
                  background:
                    i() === highlighted() ? "var(--color-accent-subtle)" : "transparent",
                  color: "var(--color-text)",
                  "font-size": "13px",
                }}
              >
                <span>{it.label}</span>
                <Show when={it.hint}>
                  <span style={{ "font-size": "11px", color: "var(--color-text-2)" }}>
                    {it.hint}
                  </span>
                </Show>
              </div>
            )}
          </For>
          <Show when={filtered().length === 0}>
            <div style={{ padding: "16px", color: "var(--color-text-2)", "font-size": "13px" }}>
              {t("palette.no_match")}
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
};
