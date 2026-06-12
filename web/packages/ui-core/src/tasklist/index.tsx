// 任务列表
//
//   - 单击激活
//   - 双击重命名
//   - 右键菜单(关闭 / 置顶 / 浮窗中打开)
//   - 状态点(idle / running / waiting_input,呼吸动画)
//   - 拖拽排序

import { For, Show, createMemo, createSignal, onCleanup, onMount, type Component } from "solid-js";
import { Portal } from "solid-js/web";
import { GitBranch } from "lucide-solid";
import type { TaskDto, TaskId } from "@vibeterm/ipc-types";
import {
  closeTask,
  closeFloating,
  focusWindow,
  openFloating,
  onTaskFlash,
  renameTask,
  setActiveTask,
  setTaskNotifyMuted,
} from "../ipc";
import { t } from "../i18n";
import { modKeyLabel } from "../keybindings";
import { menuClampRef } from "../menu-clamp";
import { urgencyColorVar } from "../urgency";

export interface TaskListProps {
  tasks: TaskDto[];
  activeTaskId: number | null;
  onActivate: (id: number) => void;
  /** urgency 视图开启时传入 score map,行尾渲染分数 badge */
  urgencyScores?: Map<TaskId, number>;
  /** 右键 close 改回调上层,弹自定义模态(WKWebView 禁 confirm()) */
  onRequestClose?: (task: TaskDto) => void;
  /** 拖拽排序 — 列表回传新顺序 id[] */
  onReorder?: (newOrder: TaskId[]) => void;
}

export const TaskList: Component<TaskListProps> = (props) => {
  const [editingId, setEditingId] = createSignal<number | null>(null);
  const [editingName, setEditingName] = createSignal("");
  // 前台"非当前任务完成"→ 后端 emit task_flash,这一行闪一下高亮(配合轻提示音)。
  const [flashingId, setFlashingId] = createSignal<number | null>(null);
  // Esc 取消 flag — onBlur 在 unmount 时也会触发,有可能在 setEditingId(null)
  // 之后才跑(SolidJS 渲染 + DOM 移除时序),会把 ghost 名字提交。这里加显式标志拒绝。
  let cancelOnce = false;
  const [ctxMenu, setCtxMenu] = createSignal<{
    x: number;
    y: number;
    task: TaskDto;
  } | null>(null);

  // 稳定 id 数组,避免 backend tasks_changed 每秒推新 TaskDto 对象时
  // <For each={props.tasks}> 把每行当新元素 remount → 编辑中的 input 被销毁焦点丢失。
  const taskIds = createMemo<number[]>(
    () => props.tasks.map((t) => t.id),
    [],
    {
      equals: (a, b) =>
        a.length === b.length && a.every((v, i) => v === b[i]),
    },
  );

  // 拖拽 — 用 Pointer Events 自实现,绕开 HTML5 drag API 在 WKWebView 上的诸多坑
  // (自定义 MIME 不暴露 / dragend 坐标丢失 / 普通 div 默认不可拖)
  const [dropIndicator, setDropIndicator] = createSignal<
    { targetId: number; before: boolean } | null
  >(null);
  const DRAG_THRESHOLD = 5; // 移动 >5px 才进入 drag mode,小于视为 click
  let pointerStart: { id: number; y: number; sourceId: number } | null = null;
  let pointerActive = false; // 是否已超过 threshold 进入 drag
  // 拖动结束后下一个 tick 屏蔽 click — 防止释放时触发 row 的 onClick 激活任务
  let suppressClickUntil = 0;
  let listEl!: HTMLDivElement;

  const onRowPointerDown = (e: PointerEvent, task: TaskDto) => {
    if (e.button !== 0) return; // 只响应左键
    if (editingId() !== null) return; // 编辑态不拖
    // 阻止默认 mousedown 行为(否则拖动会带起文本选择高亮)。
    // row 本身没有需要 focus 的子元素,preventDefault 无副作用。
    e.preventDefault();
    pointerStart = { id: e.pointerId, y: e.clientY, sourceId: task.id };
    pointerActive = false;
  };

  const onWinPointerMove = (e: PointerEvent) => {
    if (!pointerStart || e.pointerId !== pointerStart.id) return;
    const dy = Math.abs(e.clientY - pointerStart.y);
    if (!pointerActive && dy > DRAG_THRESHOLD) {
      pointerActive = true;
    }
    if (!pointerActive) return;
    // 找指针下方的 row(elementsFromPoint 跨子元素拿到 row)
    const target = rowAt(e.clientX, e.clientY);
    if (target) {
      const rect = target.el.getBoundingClientRect();
      const before = e.clientY < rect.top + rect.height / 2;
      setDropIndicator({ targetId: target.taskId, before });
    } else {
      setDropIndicator(null);
    }
  };

  const onWinPointerUp = (e: PointerEvent) => {
    if (!pointerStart || e.pointerId !== pointerStart.id) return;
    const wasActive = pointerActive;
    const sourceId = pointerStart.sourceId;
    const upX = e.clientX;
    const upY = e.clientY;
    pointerStart = null;
    pointerActive = false;
    const indicator = dropIndicator();
    setDropIndicator(null);
    if (!wasActive) return; // 当作 click,onClick 会自然触发
    // 进入了 drag mode 释放 — 屏蔽紧接而来的 click(防止 row 被激活)
    suppressClickUntil = Date.now() + 250;
    // 释放在哪个 row 上?
    const target = rowAt(upX, upY);
    if (target && target.taskId !== sourceId) {
      const before = indicator?.targetId === target.taskId ? indicator.before : true;
      const order = props.tasks.map((t) => t.id);
      const srcIdx = order.indexOf(sourceId);
      if (srcIdx < 0) return;
      order.splice(srcIdx, 1);
      let tgtIdx = order.indexOf(target.taskId);
      if (tgtIdx < 0) return;
      if (!before) tgtIdx += 1;
      order.splice(tgtIdx, 0, sourceId);
      props.onReorder?.(order);
      return;
    }
    // 没命中 row — 判断是否在列表外:在列表 bbox 外 → 开浮窗
    const rect = listEl.getBoundingClientRect();
    const insideList =
      upX >= rect.left && upX <= rect.right && upY >= rect.top && upY <= rect.bottom;
    if (!insideList) {
      const src = props.tasks.find((t) => t.id === sourceId);
      if (src && src.location.kind !== "Floating") {
        openFloating(sourceId).catch(console.error);
      }
    }
  };

  // 给定屏幕坐标,查找其下方哪个 task row(用 data-task-id 标记找)
  const rowAt = (x: number, y: number): { el: HTMLElement; taskId: number } | null => {
    const els = document.elementsFromPoint(x, y);
    for (const el of els) {
      if (el instanceof HTMLElement && el.classList.contains("task-row")) {
        const idStr = el.getAttribute("data-task-id");
        if (idStr) return { el, taskId: parseInt(idStr, 10) };
      }
    }
    return null;
  };

  let unlistenFlash: (() => void) | undefined;
  onMount(() => {
    window.addEventListener("pointermove", onWinPointerMove);
    window.addEventListener("pointerup", onWinPointerUp);
    window.addEventListener("pointercancel", onWinPointerUp);
    onTaskFlash((id) => {
      setFlashingId(id);
      // 动画时长 0.7s,略留余量后清掉,避免再次渲染时残留 animation
      setTimeout(() => setFlashingId((cur) => (cur === id ? null : cur)), 800);
    })
      .then((un) => {
        unlistenFlash = un;
      })
      .catch(console.error);
  });
  onCleanup(() => {
    window.removeEventListener("pointermove", onWinPointerMove);
    window.removeEventListener("pointerup", onWinPointerUp);
    window.removeEventListener("pointercancel", onWinPointerUp);
    unlistenFlash?.();
  });


  const startEdit = (t: TaskDto) => {
    cancelOnce = false;
    setEditingId(t.id);
    setEditingName(t.name);
  };
  const cancelEdit = () => {
    cancelOnce = true;
    setEditingId(null);
  };
  const commitEdit = async () => {
    if (cancelOnce) {
      cancelOnce = false;
      return;
    }
    const id = editingId();
    const name = editingName().trim();
    setEditingId(null);
    if (id !== null && name) {
      await renameTask(id, name).catch(console.error);
    }
  };

  const onTaskClick = async (t: TaskDto) => {
    if (t.location.kind === "Floating") {
      await focusWindow(t.location.label).catch(console.error);
    } else {
      await setActiveTask(t.id).catch(console.error);
      props.onActivate(t.id);
    }
  };

  const onCtxMenu = (e: MouseEvent, t: TaskDto) => {
    e.preventDefault();
    setCtxMenu({ x: e.clientX, y: e.clientY, task: t });
  };

  return (
    <div
      data-testid="task-list"
      style={{
        height: "100%",
        display: "flex",
        "flex-direction": "column",
        "user-select": "none",
        "font-family": "-apple-system, SF Pro, sans-serif",
        "font-size": "13px",
      }}
      onClick={() => setCtxMenu(null)}
    >
      <style>{`
        .task-row:hover .task-wt-branch {
          max-width: 120px;
        }
        @keyframes task-flash {
          0% { background: var(--color-accent-subtle); }
          100% { background: transparent; }
        }
      `}</style>
      <div
        ref={listEl}
        style={{ flex: "1", "overflow-y": "auto" }}
      >
        <For each={taskIds()}>
          {(taskId) => {
            // Reactive task accessor — backend tasks_changed 推新对象时,
            // 行不再 remount,仅内部字段重渲;编辑中的 input 不会被销毁焦点也稳。
            const taskAccessor = () => props.tasks.find((x) => x.id === taskId);
            return (
              <Show when={taskAccessor()}>
                {(getTask) => {
                  const task = getTask;
                  const isActive = () => task().id === props.activeTaskId;
                  const isFloating = () => task().location.kind === "Floating";
                  const indicator = () => dropIndicator();
                  const showAbove = () =>
                    indicator()?.targetId === task().id && indicator()!.before;
                  const showBelow = () =>
                    indicator()?.targetId === task().id && !indicator()!.before;
                  // 序号(基于 taskIds 顺序;Cmd+1..9 跟这一致)
                  const orderNum = () => taskIds().indexOf(task().id) + 1;
                  // dot 视觉规则:
                  //   waiting_input → 2s 呼吸 + 强 glow(要你输入,最显眼)
                  //   running       → 静态点 + 中等 glow(跑着,不闹腾)
                  //   done          → 描边环 + 一次性 4s 弱呼吸(完成,提醒不烦)
                  //   idle          → 暗灰静点
                  const dotStyle = () => {
                    const s = task().status;
                    if (s === "waiting_input") {
                      return {
                        background: "var(--color-status-waiting)",
                        "box-shadow": "0 0 8px var(--color-status-waiting)",
                        animation: "vibeterm-breath 2s infinite",
                        border: "none",
                      } as const;
                    }
                    if (s === "running") {
                      return {
                        background: "var(--color-status-running)",
                        "box-shadow": "0 0 5px var(--color-status-running)",
                        animation: undefined,
                        border: "none",
                      } as const;
                    }
                    if (s === "done") {
                      return {
                        background: "transparent",
                        "box-shadow": "none",
                        animation: undefined,
                        border: "2px solid var(--color-status-done, var(--color-accent))",
                      } as const;
                    }
                    if (s === "stalled") {
                      // Stalled = agent 5 分钟无输出. 慢呼吸 + 红橙色 + 描边: 醒目但不慌张.
                      return {
                        background: "transparent",
                        "box-shadow": "0 0 6px var(--color-status-stalled, #d97757)",
                        animation: "vibeterm-breath 3s infinite",
                        border: "2px solid var(--color-status-stalled, #d97757)",
                      } as const;
                    }
                    return {
                      background: "var(--color-status-idle)",
                      "box-shadow": "none",
                      animation: undefined,
                      border: "none",
                    } as const;
                  };
                  return (
                    <div
                      class="task-row"
                      data-testid={`task-item-${task().id}`}
                      data-task-id={task().id}
                      data-task-name={task().name}
                      data-task-status={task().status}
                      data-task-order={orderNum()}
                      onPointerDown={(e) => onRowPointerDown(e, task())}
                      onClick={(e) => {
                        if (Date.now() < suppressClickUntil) {
                          e.stopPropagation();
                          return;
                        }
                        onTaskClick(task());
                      }}
                      onDblClick={() => startEdit(task())}
                      onContextMenu={(e) => onCtxMenu(e, task())}
                      style={{
                        position: "relative",
                        padding: "8px 12px",
                        cursor: "pointer",
                        display: "flex",
                        "align-items": "center",
                        gap: "8px",
                        animation:
                          flashingId() === task().id
                            ? "task-flash 0.7s ease-out"
                            : undefined,
                        "border-left": isActive()
                          ? "2px solid var(--color-accent)"
                          : isFloating()
                            ? "2px dashed var(--color-accent)"
                            : "2px solid transparent",
                        background: isActive()
                          ? "var(--color-accent-subtle)"
                          : "transparent",
                        opacity: task().location.kind === "Nowhere" ? 0.7 : 1,
                      }}
                    >
                      {/* 拖拽插入线 — 绝对定位,不撑 row 高度 */}
                      <Show when={showAbove()}>
                        <div
                          style={{
                            position: "absolute",
                            left: "0",
                            right: "0",
                            top: "0",
                            height: "2px",
                            background: "var(--color-accent)",
                            "pointer-events": "none",
                          }}
                        />
                      </Show>
                      <Show when={showBelow()}>
                        <div
                          style={{
                            position: "absolute",
                            left: "0",
                            right: "0",
                            bottom: "0",
                            height: "2px",
                            background: "var(--color-accent)",
                            "pointer-events": "none",
                          }}
                        />
                      </Show>
                      {/* Agent 标识:左侧 3px 彩色竖条 */}
                      <Show when={task().agent_kind}>
                        {(getKind) => {
                          const k = getKind;
                          return (
                            <span
                              data-testid={`task-agent-${task().id}`}
                              data-agent-kind={k()}
                              title={`agent: ${k()}`}
                              style={{
                                position: "absolute",
                                left: "0",
                                top: "4px",
                                bottom: "4px",
                                width: "3px",
                                background: agentColor(k()),
                                "border-radius": "0 2px 2px 0",
                                "pointer-events": "none",
                              }}
                            />
                          );
                        }}
                      </Show>
                      <span
                        data-testid={`task-status-dot-${task().id}`}
                        style={{
                          width: "10px",
                          height: "10px",
                          "border-radius": "50%",
                          "flex-shrink": 0,
                          "box-sizing": "border-box",
                          ...dotStyle(),
                        }}
                      />
                      <Show
                        when={editingId() === task().id}
                        fallback={
                          <div
                            style={{
                              flex: "1",
                              "min-width": "0",
                              display: "flex",
                              "flex-direction": "column",
                              gap: "1px",
                            }}
                          >
                            <span
                              style={{
                                "white-space": "nowrap",
                                overflow: "hidden",
                                "text-overflow": "ellipsis",
                              }}
                            >
                              {task().name}
                            </span>
                            {/* 第二行:最近一行输出预览 */}
                            <Show when={task().last_output}>
                              {(getLine) => (
                                <span
                                  data-testid={`task-last-output-${task().id}`}
                                  title={getLine()}
                                  style={{
                                    "font-size": "11px",
                                    color: "var(--color-text-2)",
                                    "white-space": "nowrap",
                                    overflow: "hidden",
                                    "text-overflow": "ellipsis",
                                    "font-family":
                                      "JetBrains Mono, SF Mono, Menlo, Consolas, monospace",
                                    opacity: 0.75,
                                  }}
                                >
                                  {getLine()}
                                </span>
                              )}
                            </Show>
                          </div>
                        }
                      >
                        <input
                          data-testid="task-rename-input"
                          autofocus
                          value={editingName()}
                          onInput={(e) => setEditingName(e.currentTarget.value)}
                          onBlur={commitEdit}
                          onKeyDown={(e) => {
                            // 🔴 红线4:IME 组合态下回车=确认候选、Esc=取消候选,不提交/取消重命名
                            if (e.isComposing || e.keyCode === 229) return;
                            if (e.key === "Enter") commitEdit();
                            if (e.key === "Escape") cancelEdit();
                          }}
                          onClick={(e) => e.stopPropagation()}
                          style={{
                            flex: "1",
                            "min-width": "0",
                            background: "var(--color-bg)",
                            color: "var(--color-text)",
                            border: "1px solid var(--color-border)",
                            padding: "2px 6px",
                            "font-size": "13px",
                          }}
                        />
                      </Show>
                      <Show when={task().worktree}>
                        {(getWt) => {
                          const wt = getWt;
                          return (
                            <span
                              data-testid={`task-wt-badge-${task().id}`}
                              data-task-wt-branch={wt().branch ?? ""}
                              class="task-wt-badge"
                              title={`${wt().branch ?? "(detached)"} · ${wt().worktree_path}`}
                              style={{
                                display: "flex",
                                "align-items": "center",
                                gap: "3px",
                                "font-size": "10px",
                                "font-variant-numeric": "tabular-nums",
                                color: "var(--color-text-2)",
                                "flex-shrink": 0,
                              }}
                            >
                              <GitBranch size={10} />
                              {/* branch 名默认折叠,hover 行展开 — 由 .task-row:hover .task-wt-branch 控制 */}
                              <span
                                class="task-wt-branch"
                                style={{
                                  "max-width": "0",
                                  overflow: "hidden",
                                  "text-overflow": "ellipsis",
                                  "white-space": "nowrap",
                                  transition: "max-width 200ms ease",
                                }}
                              >
                                {wt().branch ?? "(detached)"}
                              </span>
                              <Show when={wt().is_dirty}>
                                <span
                                  data-testid={`task-wt-dirty-${task().id}`}
                                  title="uncommitted changes"
                                  style={{
                                    width: "5px",
                                    height: "5px",
                                    "border-radius": "50%",
                                    background: "var(--color-status-waiting)",
                                  }}
                                />
                              </Show>
                              <Show when={(wt().ahead ?? 0) > 0}>
                                <span data-testid={`task-wt-ahead-${task().id}`}>↑{wt().ahead}</span>
                              </Show>
                              <Show when={(wt().behind ?? 0) > 0}>
                                <span data-testid={`task-wt-behind-${task().id}`}>↓{wt().behind}</span>
                              </Show>
                            </span>
                          );
                        }}
                      </Show>
                      <Show when={props.urgencyScores?.has(task().id)}>
                        {(() => {
                          const score = Math.round(
                            props.urgencyScores!.get(task().id)!,
                          );
                          return (
                            <span
                              data-testid={`task-urgency-badge-${task().id}`}
                              data-urgency-score={score}
                              style={{
                                "font-size": "10px",
                                "font-weight": "600",
                                color: urgencyColorVar(score),
                                "flex-shrink": 0,
                                "min-width": "20px",
                                "text-align": "right",
                                "font-variant-numeric": "tabular-nums",
                              }}
                            >
                              {score}
                            </span>
                          );
                        })()}
                      </Show>
                      {/* 行尾:#序号(替代原 terminal_count);1..9 与 Mod+1..9 一致 */}
                      <Show when={orderNum() <= 9}>
                        <span
                          data-testid={`task-order-${task().id}`}
                          title={`${modKeyLabel()}+${orderNum()}`}
                          style={{
                            "font-size": "10px",
                            color: "var(--color-text-2)",
                            "font-variant-numeric": "tabular-nums",
                            "flex-shrink": 0,
                            opacity: 0.7,
                          }}
                        >
                          #{orderNum()}
                        </span>
                      </Show>
                    </div>
                  );
                }}
              </Show>
            );
          }}
        </For>
      </div>

      {/* 右键菜单 */}
      {/* keyed:每次右键都重建菜单 DOM,ref 里的视口夹取才会对新坐标重跑 */}
      <Show when={ctxMenu()} keyed>
        {(menu) => (
          // Portal 到 body:侧栏自身 stacking context 层级低(z=5),菜单伸进工作区会被
          // canvas 卡片(z≥10)遮挡;Portal 后配合视口夹取保证菜单完整可见。
          <Portal>
            <div
              data-testid="task-ctx-menu"
              data-task-id={menu.task.id}
              ref={menuClampRef(menu.x, menu.y)}
              style={{
                position: "fixed",
                left: `${menu.x}px`,
                top: `${menu.y}px`,
                background: "var(--color-surface)",
                border: "1px solid var(--color-border)",
                "border-radius": "6px",
                "box-shadow": "0 4px 12px rgba(0,0,0,0.3)",
                padding: "4px 0",
                "z-index": 10001,
                "min-width": "180px",
              }}
              onClick={(e) => e.stopPropagation()}
            >
              <MenuItem
                testid="task-ctx-floating"
                label={menu.task.location.kind === "Floating" ? t("ctx.return_to_main") : t("ctx.open_in_floating")}
                onClick={async () => {
                  const tt = menu.task;
                  if (tt.location.kind === "Floating") {
                    await closeFloating(tt.location.label).catch(console.error);
                  } else {
                    await openFloating(tt.id).catch(console.error);
                  }
                  setCtxMenu(null);
                }}
              />
              <MenuItem
                testid="task-ctx-rename"
                label={t("ctx.rename")}
                onClick={() => {
                  startEdit(menu.task);
                  setCtxMenu(null);
                }}
              />
              <MenuItem
                testid="task-ctx-mute"
                label={menu.task.notify_muted ? t("ctx.unmute_notify") : t("ctx.mute_notify")}
                onClick={() => {
                  const tt = menu.task;
                  setCtxMenu(null);
                  setTaskNotifyMuted(tt.id, !tt.notify_muted).catch(console.error);
                }}
              />
              <MenuItem
                testid="task-ctx-close"
                label={t("ctx.close")}
                onClick={() => {
                  const tt = menu.task;
                  setCtxMenu(null);
                  if (props.onRequestClose) {
                    props.onRequestClose(tt);
                  } else {
                    // 没注入回调时退化为直接关闭(测试 / 老调用方)
                    closeTask(tt.id).catch(console.error);
                  }
                }}
              />
            </div>
          </Portal>
        )}
      </Show>
    </div>
  );
};

const MenuItem: Component<{ label: string; onClick: () => void; testid?: string }> = (props) => (
  <div
    data-testid={props.testid}
    onClick={props.onClick}
    style={{
      padding: "6px 12px",
      cursor: "pointer",
      color: "var(--color-text)",
      "font-size": "13px",
    }}
    onMouseEnter={(e) => (e.currentTarget.style.background = "var(--color-accent-subtle)")}
    onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
  >
    {props.label}
  </div>
);

// agent 视觉:每种 agent 用色块 — 行首 3px 彩色竖条,克制不抢眼。
function agentColor(k: string): string {
  switch (k) {
    case "claude": return "#cc785c";      // claude orange
    case "codex": return "#10a37f";       // openai green
    case "gemini": return "#8e75f0";      // google purple
    case "cursor": return "#000";
    case "cline": return "#3b82f6";
    case "opencode": return "#f59e0b";
    case "copilot": return "#24292e";
    case "kimi": return "#ff6b6b";
    case "droid": return "#6b7280";
    case "amp": return "#a855f7";
    case "aider": return "#ec4899";
    case "pi": return "#06b6d4";
    default: return "var(--color-text-2)";
  }
}
