// 浮窗 entry
//
// 设计要点:
//   - 有:键盘 / 终端工作 / 新建终端 / 分屏(h+v 嵌套)/ 切 tab / 右键菜单
//   - 无:命令面板 / 任务列表 / 新建-删除-重命名任务 / 设置页 / 状态栏
//
// 实现:
//   - 浮窗内有自己的 splitTree(SolidJS signal)
//   - 第一片 leaf attach 到主窗口已 spawn 的 task.terminal_ids[0]
//   - 后续分屏新 leaf 走 spawn_terminal_in_task(后端把 PTY 关联到同一 task)
//   - Cmd+T 新建终端 = 在当前 active slot 水平分屏新 leaf
//   - Cmd+W 关闭当前 slot;Cmd+D / Cmd+Shift+D 水平/垂直分屏
//   - 全局快捷键(Cmd+K / Cmd+N / Cmd+, / Cmd+Shift+]/[)拉主窗口前台

import { createSignal, createEffect, onMount, onCleanup, Show } from "solid-js";
import { render } from "solid-js/web";
import {
  Terminal,
  Titlebar,
  ipc,
  t,
  theme as themeMod,
  SplitView,
  singleLeaf,
  splitLeaf,
  removeLeaf,
  newSlotId,
  bumpSlotIdAtLeast,
  collectSlots,
  setRatiosAt,
  rightmostBottomSlot,
  leftmostBottomSlot,
  type SplitNode,
} from "@vibeterm/ui-core";
import { SplitSquareHorizontal, SplitSquareVertical, X } from "lucide-solid";
import type { TaskDto, Theme } from "@vibeterm/ipc-types";

// 同主:HMR 整页 reload,避免 Terminal 重 mount 重 spawn
if (import.meta.hot) {
  import.meta.hot.accept(() => location.reload());
}

function FloatingApp() {
  const params = new URLSearchParams(location.search);
  const taskId = Number(params.get("taskId") ?? "0");
  const [task, setTask] = createSignal<TaskDto | null>(null);
  const [theme, setTheme] = createSignal<Theme | null>(null);
  const [ctxMenu, setCtxMenu] = createSignal<{ x: number; y: number } | null>(null);

  // 分屏树读自 task.split_tree(后端 source of truth)。
  // 主窗 + 浮窗都从同一处读写,自然同步。
  const tree = (): SplitNode | null => task()?.split_tree ?? null;
  const writeTree = (next: SplitNode) => {
    ipc.setTaskSplitTree(taskId, next).catch((e) =>
      console.error("[floating] setTaskSplitTree failed", e),
    );
  };
  const [activeSlot, setActiveSlot] = createSignal<number | null>(null);
  // slot → terminalId 映射:本窗口本地缓存(浮窗启动后,首槽 = 首个 terminal_id;
  // 后续新分屏 leaf spawn 时回填)。主窗也有自己的 slotToTerm — 同 slot 在不同
  // 窗口可能映射到不同终端,但任务至多一处显示,实际同一时间只有一窗活跃。
  const [slotToTerm, setSlotToTerm] = createSignal<Map<number, number>>(new Map());

  const resolveSlot = (): number | null => {
    const t = tree();
    if (!t) return null;
    const ids = collectSlots(t);
    const ex = activeSlot();
    if (ex !== null && ids.includes(ex)) return ex;
    return ids[0] ?? null;
  };

  const splitCurrent = (orientation: "h" | "v") => {
    const t = tree();
    if (!t) return;
    const slot = resolveSlot();
    if (slot === null) return;
    const { root, newSlot } = splitLeaf(t, slot, orientation);
    writeTree(root);
    setActiveSlot(newSlot);
  };

  const closeCurrentSlot = () => {
    const t = tree();
    if (!t) return;
    const slot = resolveSlot();
    if (slot === null) return;
    const next = removeLeaf(t, slot);
    writeTree(next ?? singleLeaf(newSlotId()));
    const term = slotToTerm().get(slot);
    if (term !== undefined) {
      ipc.closePty(term).catch(console.error);
      setSlotToTerm((m) => {
        const nx = new Map(m);
        nx.delete(slot);
        return nx;
      });
    }
    setActiveSlot(null);
  };

  function ctxItemStyle() {
    return {
      padding: "6px 12px",
      cursor: "pointer",
      color: "var(--color-text)",
      "font-size": "13px",
    };
  }

  onMount(async () => {
    try {
      const cfg = await ipc.getConfig();
      const th = await ipc.getTheme(cfg.active_theme);
      themeMod.applyShellTheme(th);
      setTheme(th);
    } catch (e) {
      console.error("[floating] config/theme load failed", e);
    }

    // 全局快捷键 → 拉主窗口前台 + 触发 action
    // 本地快捷键:Cmd+T 新建 / Cmd+W 关闭 / Cmd+D 水平 / Cmd+Shift+D 垂直
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      if (e.key === "k") {
        e.preventDefault();
        ipc.invokeGlobalAction("command_palette").catch(console.error);
      } else if (e.key === "n") {
        e.preventDefault();
        ipc.invokeGlobalAction("new_task").catch(console.error);
      } else if (e.shiftKey && (e.key === "]" || e.key === "[")) {
        e.preventDefault();
        ipc.invokeGlobalAction(e.key === "]" ? "next_task" : "prev_task").catch(console.error);
      } else if (e.key === ",") {
        e.preventDefault();
        ipc.invokeGlobalAction("open_settings").catch(console.error);
      } else if (e.key === "t" || e.key === "T") {
        e.preventDefault();
        splitCurrent("h"); // Cmd+T = 在当前 slot 水平分屏新终端
      } else if (e.key === "w" || e.key === "W") {
        e.preventDefault();
        closeCurrentSlot();
      } else if (e.key === "d" || e.key === "D") {
        e.preventDefault();
        splitCurrent(e.shiftKey ? "v" : "h");
      }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));

    const applyTask = (t: TaskDto) => {
      setTask(t);
      // 同步 frontend slotId 计数器,避免新 slot 与后端 tree 已有 id 冲突
      const slots = collectSlots(t.split_tree);
      const maxSlot = slots.reduce((a, b) => (b > a ? b : a), 0);
      bumpSlotIdAtLeast(maxSlot);
      // 首槽 = tree 中第一个 leaf,绑到 task.terminal_ids[0](attach 模式复用主窗 PTY)
      const firstSlot = slots[0];
      if (firstSlot === undefined || t.terminal_ids.length === 0) return;
      setSlotToTerm((m) => {
        if (m.has(firstSlot)) return m;
        const nx = new Map(m);
        nx.set(firstSlot, t.terminal_ids[0]);
        return nx;
      });
    };

    const refresh = async () => {
      const list = await ipc.listTasks();
      const t = list.find((x) => x.id === taskId);
      if (t) applyTask(t);
    };
    await refresh();

    const off = await ipc.onTasksChanged((list) => {
      const t = list.find((x) => x.id === taskId);
      if (t) applyTask(t);
    });
    const offTheme = await ipc.onThemeChanged((th) => {
      themeMod.applyShellTheme(th);
      setTheme(th);
    });
    onCleanup(() => {
      off();
      offTheme();
    });
  });

  createEffect(() => {
    document.title = `VibeTerm — ${task()?.name ?? "Floating"}`;
  });

  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        height: "100vh",
        background: "var(--color-bg)",
        color: "var(--color-text)",
        "font-family":
          "-apple-system, SF Pro, BlinkMacSystemFont, Helvetica, Arial, sans-serif",
      }}
    >
      <Titlebar
        left={
          <strong style={{ "font-size": "12px" }}>{task()?.name ?? `Task ${taskId}`}</strong>
        }
      />

      {/* 浮窗工具条 — 任务名 + 状态点 + 新建终端 / 分屏按钮 */}
      <header
        data-testid="floating-header"
        data-task-id={taskId}
        data-task-status={task()?.status ?? "unknown"}
        style={{
          padding: "4px 10px",
          background: "var(--color-surface)",
          "border-bottom": "1px solid var(--color-border)",
          display: "flex",
          "align-items": "center",
          gap: "8px",
          "font-size": "12px",
        }}
      >
        <span
          data-testid="floating-status-dot"
          style={{
            width: "8px",
            height: "8px",
            "border-radius": "50%",
            background:
              task()?.status === "waiting_input"
                ? "var(--color-status-waiting)"
                : task()?.status === "running"
                  ? "var(--color-status-running)"
                  : "var(--color-status-idle)",
            animation:
              task()?.status === "waiting_input" ? "vibeterm-breath 2s infinite" : undefined,
          }}
        />
        <strong>{task()?.name ?? `Task ${taskId}`}</strong>
        <div style={{ flex: 1 }} />
        <button
          data-testid="floating-split-h"
          title={t("tooltip.split_h")}
          onClick={() => splitCurrent("h")}
          style={toolBtn()}
        >
          <SplitSquareHorizontal size={12} />
        </button>
        <button
          data-testid="floating-split-v"
          title={t("tooltip.split_v")}
          onClick={() => splitCurrent("v")}
          style={toolBtn()}
        >
          <SplitSquareVertical size={12} />
        </button>
        <button
          data-testid="floating-close-slot"
          title={t("tooltip.close_term")}
          onClick={closeCurrentSlot}
          style={toolBtn()}
        >
          <X size={12} />
        </button>
      </header>

      {/* SplitView — 与主窗口同款扁平渲染,leaf attach / spawn 自适应 */}
      <div
        style={{ flex: 1, "min-height": 0, position: "relative" }}
        onContextMenu={(e: MouseEvent) => {
          e.preventDefault();
          setCtxMenu({ x: e.clientX, y: e.clientY });
        }}
        onClick={() => setCtxMenu(null)}
      >
        {/* race fix:等 task + tree 取到,首槽已绑(若有 terminal),再 mount SplitView。
           否则 Terminal 在 attachId 还 undefined 时进 spawn 分支,新 PTY 顶替已有的 attach 流。 */}
        <Show
          when={(() => {
            const t = task();
            if (!t || !t.split_tree) return false;
            const firstSlot = collectSlots(t.split_tree)[0];
            if (firstSlot === undefined) return false;
            return t.terminal_ids.length === 0 || slotToTerm().has(firstSlot);
          })()}
          fallback={
            <div style={{ padding: "16px", color: "var(--color-text-2)" }}>
              {t("floating.loading")}
            </div>
          }
        >
        <SplitView
          node={tree()!}
          onRatiosChange={(path, ratios) => {
            const t = tree();
            if (t) writeTree(setRatiosAt(t, path, ratios));
          }}
          renderLeaf={(slotId) => {
            // 触底叶子 → 跟浮窗外框圆角对齐. macOS 浮窗系统圆角 ≈14px,
            // overlay 至 slot 边缘无 inset (slot padding 已去), 直接用同值.
            const isBottomRight = () => {
              const t = tree();
              return t ? rightmostBottomSlot(t) === slotId : false;
            };
            const isBottomLeft = () => {
              const t = tree();
              return t ? leftmostBottomSlot(t) === slotId : false;
            };
            const overlayRadius = () => {
              const br = isBottomRight() ? "17px" : "0";
              const bl = isBottomLeft() ? "17px" : "0";
              return `0 0 ${br} ${bl}`;
            };
            return (
              <div
                data-testid={`floating-slot-${slotId}`}
                data-active={activeSlot() === slotId ? "true" : "false"}
                onClick={() => setActiveSlot(slotId)}
                style={{
                  width: "100%",
                  height: "100%",
                  "box-sizing": "border-box",
                  background: "var(--color-bg)",
                  position: "relative",
                  "z-index": activeSlot() === slotId ? 2 : "auto",
                }}
              >
                {/* 后端按 (task, slot) 幂等:主窗已 spawn → 这里自动 attach 共享 PTY */}
                <Terminal
                  taskId={taskId}
                  slotId={slotId}
                  theme={theme() ?? undefined}
                  onReady={(termId) => {
                    setSlotToTerm((m) => {
                      const nx = new Map(m);
                      nx.set(slotId, termId);
                      return nx;
                    });
                    if (activeSlot() === null) setActiveSlot(slotId);
                  }}
                />
                {/* 边框 + 内发光 overlay — 压在 xterm canvas 上,不阻塞点击 */}
                <div
                  aria-hidden="true"
                  style={{
                    position: "absolute",
                    inset: 0,
                    "pointer-events": "none",
                    "box-sizing": "border-box",
                    border:
                      activeSlot() === slotId
                        ? "1px solid var(--color-accent)"
                        : "1px solid var(--color-border)",
                    "border-radius": overlayRadius(),
                    "box-shadow":
                      activeSlot() === slotId
                        ? "inset 0 0 8px -3px var(--color-accent)"
                        : "none",
                    transition: "box-shadow 120ms ease, border-color 120ms ease",
                  }}
                />
              </div>
            );
          }}
        />
        </Show>
      </div>

      {/* 右键菜单 — 命令面板 / 回主窗口 */}
      <Show when={ctxMenu()}>
        {(menu) => (
          <div
            data-testid="floating-ctx-menu"
            onClick={(e) => e.stopPropagation()}
            style={{
              position: "fixed",
              left: `${menu().x}px`,
              top: `${menu().y}px`,
              background: "var(--color-surface)",
              border: "1px solid var(--color-border)",
              "border-radius": "6px",
              "box-shadow": "0 4px 12px rgba(0,0,0,0.3)",
              padding: "4px 0",
              "z-index": 1000,
              "min-width": "180px",
            }}
          >
            <div
              data-testid="floating-ctx-palette"
              onClick={() => {
                ipc.invokeGlobalAction("command_palette").catch(console.error);
                setCtxMenu(null);
              }}
              style={ctxItemStyle()}
            >
              {t("floating.open_palette")}
            </div>
            <div
              data-testid="floating-ctx-return-main"
              onClick={() => {
                const loc = task()?.location;
                if (loc && loc.kind === "Floating") {
                  ipc.closeFloating(loc.label).catch(console.error);
                }
                setCtxMenu(null);
              }}
              style={ctxItemStyle()}
            >
              {t("ctx.return_to_main")}
            </div>
          </div>
        )}
      </Show>

      <style>{`
        @keyframes vibeterm-breath { 0%,100% { opacity:1; } 50% { opacity:0.5; } }
        @media (prefers-reduced-motion: reduce) {
          @keyframes vibeterm-breath { 0%,100%,50% { opacity:1; } }
        }
      `}</style>
    </div>
  );
}

function toolBtn() {
  return {
    background: "transparent",
    color: "var(--color-text-2)",
    border: "1px solid var(--color-border)",
    padding: "3px 6px",
    "border-radius": "4px",
    cursor: "pointer",
    display: "flex",
    "align-items": "center",
  };
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
render(() => <FloatingApp />, root);
