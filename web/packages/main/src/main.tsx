// VibeTerm 主窗口 — 完整 layout
//
// 左侧:任务列表 + 顶部"新建"按钮
// 右侧:激活任务的终端工作区(多 tab + 当前 tab xterm)

import { For, Show, createEffect, createMemo, createSignal, onMount, onCleanup } from "solid-js";
import { render } from "solid-js/web";

import { Terminal, TaskList, Titlebar, theme as themeMod, ipc, t, SplitView, singleLeaf, splitLeaf, removeLeaf, newSlotId, bumpSlotIdAtLeast, collectSlots, setRatiosAt, initKeybindings, createKeybindingDispatcher, focusTerminal, rightmostBottomSlot, StatusBar, playNotifySound, shouldConfirmCloseTask, loadSavedScrollback, startScrollbackAutosave, modKeyLabel, isWindowsPlatform, type SplitNode } from "@vibeterm/ui-core";
import { Plus, X, Settings as SettingsIcon, SplitSquareHorizontal, SplitSquareVertical } from "lucide-solid";
import { check as checkUpdate } from "@tauri-apps/plugin-updater";
import type { TaskDto, Theme, LayoutTemplate } from "@vibeterm/ipc-types";
import { CommandPalette } from "./command-palette";
import { DiffViewer } from "./diff-viewer";
import { Settings } from "./settings";
import { PromptPicker } from "./prompt-picker";
import { NewTaskDialog, ConfirmCloseDialog } from "./dialogs";
// Terminal 组件持有真 PTY 句柄;HMR 不能仅重 mount(会导致旧 PTY 孤儿 + 新 PTY 重复 spawn)
// 解决:接收 HMR 信号但 reload 整页,确保 Web 重连 + Rust 端唯一一份 PTY
// 生产构建无 HMR,此 guard 无影响。
if (import.meta.hot) {
  import.meta.hot.accept(() => location.reload());
}

// 复用单例 encoder(避免每次发布局/resume 命令都 new 一个,对齐 terminal 里的 ENCODER 约定)
const PTY_ENCODER = new TextEncoder();

function App() {
  const [tasks, setTasks] = createSignal<TaskDto[]>([]);
  const [activeTaskId, setActiveTaskId] = createSignal<number | null>(null);
  // tab 改为 split tree per task(替代简化 2 路)
  //   - 每 task 1 棵 SplitNode
  //   - 切 tab 模式被分屏取代;UI 上仅一个工作区(若需要多 tab 可复用 task)
  // splitTree 现在是后端 source of truth(TaskDto.split_tree),主 + 浮窗都从此读写
  // 当前聚焦的 slotId(用于 split / close 等操作)
  // activeSlot:按 task 区分(slot_id 不同 task 间可能重复 — 后端默认每 task 第一个 leaf 都是 slot_id=0)
  const [activeSlotByTask, setActiveSlotByTask] = createSignal<Map<number, number | null>>(new Map());
  const activeSlotOf = (tid: number): number | null => activeSlotByTask().get(tid) ?? null;
  const setActiveSlotFor = (tid: number, slot: number | null) => {
    setActiveSlotByTask((m) => {
      const nx = new Map(m);
      if (slot === null) nx.delete(tid);
      else nx.set(tid, slot);
      return nx;
    });
  };
  // 兼容旧 callsite:不带 task id 时操作当前活跃 task
  const activeSlot = (): number | null => {
    const tid = activeTaskId();
    return tid === null ? null : activeSlotOf(tid);
  };
  const setActiveSlot = (slot: number | null) => {
    const tid = activeTaskId();
    if (tid !== null) setActiveSlotFor(tid, slot);
  };

  // 左侧任务列表宽度,可拖动 + localStorage 持久化(纯 UI preference,不入 env.toml)
  const SIDEBAR_KEY = "vibeterm.sidebar.width";
  const SIDEBAR_MIN = 140;
  const SIDEBAR_MAX = 480;
  const readStoredWidth = (): number => {
    const raw = localStorage.getItem(SIDEBAR_KEY);
    const n = raw ? parseInt(raw, 10) : NaN;
    return Number.isFinite(n) && n >= SIDEBAR_MIN && n <= SIDEBAR_MAX ? n : 220;
  };
  const [sidebarWidth, setSidebarWidth] = createSignal<number>(readStoredWidth());
  const persistSidebarWidth = (w: number) => {
    try {
      localStorage.setItem(SIDEBAR_KEY, String(Math.round(w)));
    } catch {
      // private mode / quota — 忽略
    }
  };
  const startSidebarDrag = (e: MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = sidebarWidth();
    const onMove = (mv: MouseEvent) => {
      const next = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startW + (mv.clientX - startX)));
      setSidebarWidth(next);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      persistSidebarWidth(sidebarWidth());
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };
  // (taskId, slotId) → terminal_id 映射(Terminal 组件 spawn 完成时回填)。
  // 注意 slot_id 是 per-task 命名空间(不同 task 都有 slot 0),所以必须复合 key。
  const slotKey = (taskId: number, slotId: number) => `${taskId}:${slotId}`;
  const [slotToTerm, setSlotToTerm] = createSignal<Map<string, number>>(new Map());
  const [currentTheme, setCurrentTheme] = createSignal<Theme | null>(null);
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [missingClis, setMissingClis] = createSignal<string[]>([]);
  const [cliBannerDismissed, setCliBannerDismissed] = createSignal(false);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  // 菜单"检查更新"→ 打开设置并定位到更新页
  const [settingsInitialTab, setSettingsInitialTab] = createSignal<"about" | "update" | undefined>(undefined);
  const [promptPickerOpen, setPromptPickerOpen] = createSignal(false);
  // Diff 查看器:打开时存目标 cwd(null = 关闭)
  const [diffCwd, setDiffCwd] = createSignal<string | null>(null);
  // 启动自动检查更新发现新版 → 设置按钮显示角标(仅提示,安装仍手动)
  const [updateAvailable, setUpdateAvailable] = createSignal(false);
  // G6 布局模板:slotKey → 待发送启动命令。终端首次 onReady 时消费一次。
  const [pendingCommands, setPendingCommands] = createSignal<Map<string, string>>(new Map());
  // 布局/resume 命令的延迟发送计时器 —— 卸载时统一清理,避免悬空回调。
  const pendingTimers = new Set<ReturnType<typeof setTimeout>>();
  onCleanup(() => {
    for (const t of pendingTimers) clearTimeout(t);
    pendingTimers.clear();
  });
  const [activeTerminalId, setActiveTerminalId] = createSignal<number | null>(null);

  // "离开期间某 agent 终端刚完成" 的待消费记录:taskId → terminalId(最近一次完成覆盖)。
  // agent_terminal_completed 事件在该 task 非当前激活时写入;切回该 task 时一次性消费,
  // 把焦点定位到那个终端(一个 task 多 agent 场景)。
  const [pendingFocusTermByTask, setPendingFocusTermByTask] = createSignal<Map<number, number>>(
    new Map(),
  );
  // 反查:terminalId 在 task tid 下属于哪个 slot(无则 null,如终端已关闭)。
  const slotOfTerminalInTask = (tid: number, termId: number): number | null => {
    const tk = tasks().find((t) => t.id === tid);
    if (!tk) return null;
    const map = slotToTerm();
    for (const s of collectSlots(tk.split_tree)) {
      if (map.get(slotKey(tid, s)) === termId) return s;
    }
    return null;
  };

  // 切换 task (点 sidebar) 时, activeTerminalId 不会自动跟随 —
  // 这里同步: task → 它的 active slot (或第一个有 terminal 的 slot) → terminal_id
  // 状态栏 / executeAction / 浮窗等都依赖 activeTerminalId 跟随当前可见终端
  createEffect(() => {
    const tid = activeTaskId();
    if (tid === null) {
      setActiveTerminalId(null);
      return;
    }
    const slot = activeSlotOf(tid);
    let termId: number | undefined;
    if (slot !== null) {
      termId = slotToTerm().get(slotKey(tid, slot));
    }
    if (termId === undefined) {
      // fallback: 任务下第一个已就绪的 slot
      const tk = tasks().find((t) => t.id === tid);
      if (tk) {
        for (const s of collectSlots(tk.split_tree)) {
          const t2 = slotToTerm().get(slotKey(tid, s));
          if (t2 !== undefined) { termId = t2; break; }
        }
      }
    }
    // 切到新任务但其 slot→terminal 映射尚未就绪时,先清空避免下游误操作上一个
    // 任务的终端;Terminal onReady 回填后此 effect 重跑会写入正确值。
    if (termId !== undefined) setActiveTerminalId(termId);
    else setActiveTerminalId(null);
  });

  // 切回某 task 时:若它在"离开期间"有 agent 终端刚完成(未消费),把高亮外框 + 焦点
  // 都定位到那个终端。一次性消费(无论是否解析到 slot 都清掉,避免悬挂)。
  //   设 active slot + focus 都放到 rAF 里:切任务的 onActivate 会同步 setActiveSlot(null)
  //   清空高亮,而 Solid 在每个顶层 setter 后就 flush 本 effect —— 同步设会被随后的清空
  //   覆盖(光标走 rAF 不受影响,故曾出现"光标落了外框却没亮")。挪到下一帧既晚于那次
  //   清空,又等 display:none→block 布局就绪。
  createEffect(() => {
    const tid = activeTaskId();
    if (tid === null) return;
    const termId = pendingFocusTermByTask().get(tid);
    if (termId === undefined) return;
    const slot = slotOfTerminalInTask(tid, termId);
    setPendingFocusTermByTask((m) => {
      const n = new Map(m);
      n.delete(tid);
      return n;
    });
    if (slot === null) return; // 终端已关闭 / 映射未就绪 → 消费但不强切
    requestAnimationFrame(() => {
      if (activeTaskId() !== tid) return; // 这一帧内又切走了 → 放弃,别抢当前任务焦点
      setActiveSlotFor(tid, slot);
      focusTerminal(termId);
    });
  });

  // onMount 内 await 之后注册的 onCleanup 会被 Solid 丢弃(owner 已失效),
  // 故在同步上下文注册,监听句柄在 await 后逐个推入。
  const unlisteners: (() => void)[] = [];
  onCleanup(() => {
    for (const off of unlisteners.splice(0)) off();
  });

  // 启动:加载主题 + 任务 + 监听变化
  onMount(async () => {
    // G5:先载入 scrollback 快照(必须早于任何终端 mount 调 takeScrollback),再起自动保存。
    await loadSavedScrollback();
    startScrollbackAutosave();

    try {
      const cfg = await ipc.getConfig();
      const th = await ipc.getTheme(cfg.active_theme);
      themeMod.applyShellTheme(th);
      setCurrentTheme(th);

      // 自动检查更新(可在设置关闭)。updater 插件 check() 只拉 latest.json 比对版本:
      // 不上传、零遥测、不自动下载安装。发现新版只在设置按钮显示角标;安装永远是用户手动点。
      if (cfg.auto_check_updates) {
        checkUpdate()
          .then((u) => setUpdateAvailable(u !== null))
          .catch((e) => console.error("[main] auto update check failed", e));
      }
    } catch (e) {
      console.error("[main] config/theme load failed", e);
    }

    // 启动主动申请通知权限:macOS 不会自动申请,用户从不进「设置→通知」点按钮就永远收不到横幅。
    // 仅在尚未授权时申请(default 弹系统框;已 denied 则 request 直接返回、不打扰)。
    try {
      if ((await ipc.notifyPermission()) !== "granted") {
        await ipc.requestNotifyPermission();
      }
    } catch (e) {
      console.error("[main] notify permission request failed", e);
    }

    try {
      const list = await ipc.listTasks();
      setTasks(list);
      // 后端 task 已带 split_tree 字段(老 tasks.json 也走 #[serde(default)]);
      // 同步 frontend 的 newSlotId 计数器,避免新 slot id 冲突
      let maxSlot = 0;
      for (const tk of list) {
        for (const s of collectSlots(tk.split_tree)) {
          if (s > maxSlot) maxSlot = s;
        }
      }
      bumpSlotIdAtLeast(maxSlot);
      // 延迟 setActiveTaskId 让 Tauri Channel bridge 完全就绪 — 否则首次
      // 自动 mount Terminal 会与 Channel 注册时序竞争,导致 PTY 输出永远不到 xterm。
      // 手动切换任务时无此问题(那时 bridge 已暖)
      if (list.length > 0 && activeTaskId() === null) {
        // 启动时恢复上次激活的 task;后端没记或 id 已删 → 用 order[0]
        let restored: number | null = null;
        try {
          restored = await ipc.getActiveTask();
        } catch (e) {
          console.warn("[main] get_active_task failed", e);
        }
        const valid = restored !== null && list.some((t) => t.id === restored);
        setActiveTaskId(valid ? restored! : list[0].id);
      }
    } catch (e) {
      console.error("[main] list_tasks failed", e);
    }

    // AI CLI 检测(首启动 banner)
    try {
      const clis = await ipc.detectAiClis();
      const missing = clis.filter((c) => !c.installed).map((c) => c.name);
      // 仅当全部缺失时显示 banner
      if (missing.length === clis.length) {
        setMissingClis(missing);
      }
    } catch (e) {
      console.error("[main] detect_ai_clis failed", e);
    }

    const offTasks = await ipc.onTasksChanged((list) => {
      const prev = tasks();
      setTasks(list);
      // slot 计数器跟随后端树:浮窗里分屏产生的新 slot 写回后,主窗计数器若不同步,
      // 主窗再分屏会撞 id —— 两个 pane 叠同一矩形、绑同一 PTY、删一个连删俩。
      let maxSlot = 0;
      for (const tk of list) {
        for (const s of collectSlots(tk.split_tree)) {
          if (s > maxSlot) maxSlot = s;
        }
      }
      bumpSlotIdAtLeast(maxSlot);
      // 清扫已关闭任务的本地 Map 残留(task id 不复用,残留无错误表现但缓慢泄漏)
      const aliveIds = new Set(list.map((t) => t.id));
      const sweepByTaskId = <V,>(m: Map<number, V>): Map<number, V> => {
        if (![...m.keys()].some((k) => !aliveIds.has(k))) return m;
        const nx = new Map(m);
        for (const k of nx.keys()) if (!aliveIds.has(k)) nx.delete(k);
        return nx;
      };
      const sweepByTaskKey = <V,>(m: Map<string, V>): Map<string, V> => {
        const dead = [...m.keys()].filter(
          (k) => !aliveIds.has(parseInt(k.split(":")[0], 10)),
        );
        if (dead.length === 0) return m;
        const nx = new Map(m);
        for (const k of dead) nx.delete(k);
        return nx;
      };
      setActiveSlotByTask(sweepByTaskId);
      setPendingFocusTermByTask(sweepByTaskId);
      setSlotToTerm(sweepByTaskKey);
      setPendingCommands(sweepByTaskKey);
      // 用户反馈:关闭浮窗后该任务应回到主右侧 — 探测任意 task
      // 由 Floating 转出(关浮窗)→ 自动 setActive 那个 task。
      // (与 "工作区保持原状" 微妙冲突,以用户需求为准)
      const justReturned = prev.find((p) => {
        if (p.location.kind !== "Floating") return false;
        const now = list.find((t) => t.id === p.id);
        return !!now && now.location.kind !== "Floating";
      });
      if (justReturned) {
        setActiveTaskId(justReturned.id);
      }
      // 若激活任务不在了,fallback 到第一个
      const aid = activeTaskId();
      if (aid !== null && !list.find((t) => t.id === aid)) {
        setActiveTaskId(list.length > 0 ? list[0].id : null);
      }
    });
    const offTheme = await ipc.onThemeChanged((th) => {
      themeMod.applyShellTheme(th);
      setCurrentTheme(th);
    });

    // 收到浮窗触发的全局 action
    const offGlobal = await ipc.onGlobalAction((action) => {
      switch (action) {
        case "command_palette": setPaletteOpen(true); break;
        case "new_task": handleCreateTask(); break;
        case "split_horizontal": splitActive("h"); break;
        case "split_vertical": splitActive("v"); break;
        case "close_terminal": closeActiveSlot(); break;
        case "new_terminal": splitActive("h"); break; // 简化:新建终端 = 水平分屏
        case "next_task": {
          const order = tasks();
          const idx = order.findIndex((t) => t.id === activeTaskId());
          if (idx >= 0 && idx + 1 < order.length) setActiveTaskId(order[idx + 1].id);
          break;
        }
        case "prev_task": {
          const order = tasks();
          const idx = order.findIndex((t) => t.id === activeTaskId());
          if (idx > 0) setActiveTaskId(order[idx - 1].id);
          break;
        }
        case "open_settings":
          setSettingsOpen(true);
          break;
        case "check_update":
          setSettingsInitialTab("update");
          setSettingsOpen(true);
          break;
        default:
          console.warn("unknown global action", action);
      }
    });

    // Custom Actions(A4)— shortcut 表,响应 keydown 即时触发
    const [actionsList, setActionsList] = createSignal<import("@vibeterm/ipc-types").ActionEntry[]>([]);
    const reloadActions = async () => {
      try {
        const f = await ipc.getActions();
        setActionsList(f.actions);
      } catch (e) {
        console.error("[main] getActions failed", e);
      }
    };
    reloadActions();
    const offActions = await ipc.onActionsChanged(reloadActions);

    // 通知点击 → 主窗口 focused → 后端发 notification_focus_target. 切到对应 task.
    // task_id 在当前任务列表里才切;不在(已删)忽略,避免抖动.
    const offNotifyFocus = await ipc.onNotificationFocusTarget((taskId) => {
      if (tasks().some((t) => t.id === taskId)) {
        setActiveTaskId(taskId);
      }
    });

    // 自定义本地音频文件通知 → 后端 emit 字段, 前端用 <audio> 放.
    // 系统通知子系统只接受系统声音名, 文件路径走这个旁路.
    const offNotifySound = await ipc.onNotificationPlaySound((sound) => {
      playNotifySound(sound).catch((e) => console.warn("[main] play notify sound failed", e));
    });

    // 某 task 的某终端 agent 完成:若该 task 当前不是激活 task(用户在别处),记下来,
    // 等用户切回该 task 时把焦点定位到这个终端(见上方消费 effect)。已在看则不打扰。
    const offAgentDone = await ipc.onAgentTerminalCompleted(({ task_id, terminal_id }) => {
      if (activeTaskId() === task_id) return;
      setPendingFocusTermByTask((m) => {
        const n = new Map(m);
        n.set(task_id, terminal_id);
        return n;
      });
    });

    // 全局快捷键 — 从 keybindings.toml 路由 (用户改 toml / 设置 UI 立刻生效).
    // 旧版硬编码 Mod+K/N/, 已删, 全部走 dispatcher.
    await initKeybindings();
    const cycleTask = (delta: number) => {
      const order = tasks();
      const idx = order.findIndex((t) => t.id === activeTaskId());
      if (idx < 0) return;
      const next = idx + delta;
      if (next >= 0 && next < order.length) setActiveTaskId(order[next].id);
    };
    const globalDispatcher = createKeybindingDispatcher({
      command_palette: () => setPaletteOpen((v) => !v),
      new_task: () => handleCreateTask(),
      open_settings: () => setSettingsOpen(true),
      prompt_picker: () => setPromptPickerOpen(true),
      new_terminal: () => splitActive("h"),
      close_terminal: () => closeActiveSlot(),
      close_split: () => closeActiveSlot(),
      split_horizontal: () => splitActive("h"),
      split_vertical: () => splitActive("v"),
      next_task: () => cycleTask(1),
      prev_task: () => cycleTask(-1),
    });

    const onKey = (e: KeyboardEvent) => {
      // 🔴 红线4:IME 组合态(中文/日文候选)下任何按键交给输入法,不驱动全局快捷键——
      // 子组件(palette 等)各自守门后事件仍会冒泡到 window,Esc 取消候选若不在这拦,
      // 会把整个面板关掉、输入全丢(7daf710 修的症状漏了这条冒泡路径)。
      if (e.isComposing || e.keyCode === 229) return;
      // custom action (A4 自定义快捷键):焦点在可编辑控件(任务重命名/对话框输入)时跳过,
      // 防止 Shift+字母 这类宽松 shortcut 把用户正常打字劫持成命令注入终端。
      const target = e.target as HTMLElement | null;
      const inEditable =
        !!target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable);
      const norm = normalizeShortcut(e);
      if (norm && !inEditable) {
        const a = actionsList().find((x) => x.shortcut && normalizeShortcutStr(x.shortcut) === norm);
        if (a) {
          e.preventDefault();
          ipc.executeAction(a.id, activeTerminalId() ?? null).catch((err) =>
            console.error(`[main] executeAction(${a.id}) failed`, err),
          );
          return;
        }
      }
      // keybindings.toml 命令
      if (globalDispatcher(e)) return;
      // Cmd+1..9 跳第 N 个任务 — 不放进 keybindings 因为是数字范围
      const mod = e.metaKey || e.ctrlKey;
      if (mod && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const idx = parseInt(e.key, 10) - 1;
        const list = tasks();
        if (list[idx]) setActiveTaskId(list[idx].id);
      } else if (e.key === "Escape") {
        setPaletteOpen(false);
        setSettingsOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);

    unlisteners.push(
      offTasks,
      offTheme,
      offGlobal,
      offActions,
      offNotifyFocus,
      offNotifySound,
      offAgentDone,
      () => window.removeEventListener("keydown", onKey),
    );
  });

  // splitTree 读自 task.split_tree(后端 source of truth)。
  // 写入走 ipc.setTaskSplitTree → 后端持久化 → tasks_changed event 同步两窗。
  const getSplitTree = (taskId: number): SplitNode | undefined =>
    tasks().find((t) => t.id === taskId)?.split_tree;
  const writeSplitTree = (taskId: number, tree: SplitNode) => {
    // 乐观更新本地树:快速连续分屏/关屏不再基于陈旧快照互相覆盖
    // (第二次操作读到第一次的结果;后端 tasks_changed 到达后仍以后端为准)。
    setTasks((prev) =>
      prev.map((t) => (t.id === taskId ? { ...t, split_tree: tree } : t)),
    );
    ipc.setTaskSplitTree(taskId, tree).catch((e) =>
      console.error("[main] setTaskSplitTree failed", e),
    );
  };

  const [newTaskOpen, setNewTaskOpen] = createSignal(false);
  const [closeTarget, setCloseTarget] = createSignal<TaskDto | null>(null);

  // Cmd+N / 侧栏 + 按钮 / 命令面板 → 弹模态(macOS WKWebView 禁用 prompt())
  const handleCreateTask = () => setNewTaskOpen(true);

  const submitNewTask = async (
    name: string,
    cwd: string | null,
    worktree: import("@vibeterm/ipc-types").WorktreeRef | null,
  ) => {
    setNewTaskOpen(false);
    try {
      const task = await ipc.createTask({ name, cwd, worktree });
      setActiveTaskId(task.id);
      // 后端 create_task 已带 split_tree=Leaf(0) 默认;不需要再 ensure
    } catch (e) {
      console.error("[main] create_task failed", e);
    }
  };

  // G6:应用布局模板 —— 创建任务 + 按 panes 构建分屏树 + 登记各 pane 启动命令(onReady 消费)。
  // 链式模型:pane[0]=根 leaf(slot 0,复用 create_task 默认),后续每个把上一个按其方向劈开。
  const applyLayout = async (tpl: LayoutTemplate) => {
    const buildCmd = (p: { command?: string | null; cwd?: string | null }): string | null => {
      if (!p.command) return null;
      if (!p.cwd) return p.command;
      if (isWindowsPlatform()) {
        // 双引号 + `;`:pwsh 7 与 powershell 5 都认(`&&` 仅 pwsh 7、单引号 cmd 不认)。
        // 默认 shell 链 pwsh → powershell → cmd,cmd 用户极少,接受其 `;` 不可用的局限。
        return `cd "${p.cwd}"; ${p.command}`;
      }
      // 单引号包裹 cwd 防空格/特殊字符;内部单引号转义
      return `cd '${p.cwd.replace(/'/g, "'\\''")}' && ${p.command}`;
    };
    try {
      const task = await ipc.createTask({ name: tpl.name, cwd: tpl.cwd ?? null, worktree: null });
      setActiveTaskId(task.id);
      const panes = tpl.panes.length > 0 ? tpl.panes : [{ command: null }];
      let tree: SplitNode = singleLeaf(0);
      const cmdBySlot = new Map<string, string>();
      let prevSlot = 0;
      const c0 = buildCmd(panes[0]);
      if (c0) cmdBySlot.set(slotKey(task.id, 0), c0);
      for (let i = 1; i < panes.length; i++) {
        const orient = panes[i].split === "v" ? "v" : "h";
        const r = splitLeaf(tree, prevSlot, orient);
        tree = r.root;
        const c = buildCmd(panes[i]);
        if (c) cmdBySlot.set(slotKey(task.id, r.newSlot), c);
        prevSlot = r.newSlot;
      }
      setPendingCommands((m) => {
        const nx = new Map(m);
        for (const [k, v] of cmdBySlot) nx.set(k, v);
        return nx;
      });
      writeSplitTree(task.id, tree);
    } catch (e) {
      console.error("[main] applyLayout failed", e);
    }
  };

  // 在指定任务里劈一个新 pane 并登记其启动命令(onReady 时发送)。复用 G6 的 pendingCommands 机制。
  const spawnPaneWithCommand = (taskId: number, command: string) => {
    const tree = getSplitTree(taskId);
    if (!tree) return;
    const slot = resolveActiveSlot(taskId);
    if (slot === null) return;
    const { root, newSlot } = splitLeaf(tree, slot, "h");
    setPendingCommands((m) => {
      const nx = new Map(m);
      nx.set(slotKey(taskId, newSlot), command);
      return nx;
    });
    writeSplitTree(taskId, root);
    setActiveSlotFor(taskId, newSlot);
  };

  // Y1–Y3:恢复当前任务的 agent 会话 —— 只读嗅探 session_id 构造 resume 命令,
  // 开一个新 pane 跑它。用户手动触发,无 hook、无自动执行。
  const resumeAgent = async () => {
    const tk = activeTask();
    if (!tk || !tk.cwd) return;
    try {
      const info = await ipc.agentResumeCommand(tk.cwd, tk.agent_kind ?? null);
      if (info) spawnPaneWithCommand(tk.id, info.command);
      else console.warn("[main] no resumable agent session for", tk.cwd);
    } catch (e) {
      console.error("[main] resumeAgent failed", e);
    }
  };

  // 终端首次就绪时,若该 slot 有待发送的布局启动命令则发一次(150ms 让 shell rc 就位后再发)。
  const consumePendingCommand = (taskId: number, slotId: number, termId: number) => {
    const key = slotKey(taskId, slotId);
    const cmd = pendingCommands().get(key);
    if (cmd === undefined) return;
    setPendingCommands((m) => {
      const nx = new Map(m);
      nx.delete(key);
      return nx;
    });
    const timer = setTimeout(() => {
      pendingTimers.delete(timer);
      ipc.writePty(termId, PTY_ENCODER.encode(cmd + "\r")).catch((e) =>
        console.error("[main] layout command send failed", e),
      );
    }, 150);
    pendingTimers.add(timer);
  };

  // 关闭确认 — 可选同时 git worktree remove(+ force 若 dirty)
  const confirmCloseTask = async (removeWorktree: boolean, force: boolean) => {
    const tgt = closeTarget();
    setCloseTarget(null);
    if (!tgt) return;
    try {
      // 先 remove worktree(若勾选),失败时仍然继续 close task,避免悬空
      if (removeWorktree && tgt.worktree) {
        try {
          await ipc.gitRemoveWorktree(
            tgt.worktree.repo_path,
            tgt.worktree.worktree_path,
            force,
          );
        } catch (e) {
          console.error("[main] git_remove_worktree failed", e);
          // 继续 close,worktree 留盘上由用户自己处理
        }
      }
      await ipc.closeTask(tgt.id);
    } catch (e) {
      console.error("[main] close_task failed", e);
    }
  };

  // 分屏 / 关闭分屏
  // 任务持久挂载后,onReady 只在首次 mount 触发,切换任务时 activeSlot 不会自动重置。
  // 这里加 fallback:slot 为空 → 取活跃任务树的第一片叶子,让按钮在切换后立即可用。
  const resolveActiveSlot = (tid: number): number | null => {
    const tree = getSplitTree(tid);
    if (!tree) return null;
    const slots = collectSlots(tree);
    const explicit = activeSlot();
    if (explicit !== null && slots.includes(explicit)) return explicit;
    return slots[0] ?? null;
  };

  const splitActive = (orientation: "h" | "v") => {
    const tid = activeTaskId();
    if (tid === null) return;
    // 任务在浮窗中:主窗只是占位,分屏会凭空往浮窗里塞 pane
    // (工具栏按钮已隐藏,但快捷键/菜单仍路由到这里)
    if (isActiveTaskInFloating()) return;
    const tree = getSplitTree(tid);
    if (!tree) return;
    const slot = resolveActiveSlot(tid);
    if (slot === null) return;
    const { root, newSlot } = splitLeaf(tree, slot, orientation);
    writeSplitTree(tid, root);
    setActiveSlot(newSlot);
  };

  const closeActiveSlot = () => {
    const tid = activeTaskId();
    if (tid === null) return;
    // 任务在浮窗中:resolveActiveSlot 会 fallback 到 slots[0],在主窗按
    // close_terminal 会静默杀掉浮窗里正在用的第一个 pane 连同 PTY
    if (isActiveTaskInFloating()) return;
    const tree = getSplitTree(tid);
    if (!tree) return;
    const slot = resolveActiveSlot(tid);
    if (slot === null) return;
    const next = removeLeaf(tree, slot);
    const finalTree = next ?? singleLeaf(newSlotId());
    writeSplitTree(tid, finalTree);
    // 关闭对应 PTY
    const key = slotKey(tid, slot);
    const map = slotToTerm();
    const term = map.get(key);
    if (term !== undefined) {
      ipc.closePty(term).catch(console.error);
      setSlotToTerm((m) => {
        const nx = new Map(m);
        nx.delete(key);
        return nx;
      });
    }
    setActiveSlot(null);
  };

  const activeTask = () => tasks().find((t) => t.id === activeTaskId());
  const isActiveTaskInFloating = () =>
    activeTask()?.location.kind === "Floating";

  // 关键:用稳定的 id 数组 key <For>,避免后端 tasks_changed 给来的全新对象
  // 引起整列 row 重 mount → Terminal 重 spawn → 计数无限增长。
  // equals 自定义为逐元素比较,id 集合不变时返回 same → 不重渲染。
  const taskIds = createMemo<number[]>(
    () => tasks().map((t) => t.id),
    [],
    {
      equals: (a, b) =>
        a.length === b.length && a.every((v, i) => v === b[i]),
    },
  );

  // 默认即用户手动拖拽序(reorderTasks 已存),不再做"按 urgency 自动排"
  const displayedTasks = () => tasks();

  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        height: "100vh",
        background: "var(--color-bg)",
        color: "var(--color-text)",
        "font-family":
          "-apple-system, SF Pro, BlinkMacSystemFont, 'Segoe UI', system-ui, Helvetica, Arial, sans-serif",
      }}
    >
      <Titlebar
        left={<strong style={{ "font-size": "12px" }}>VibeTerm</strong>}
        right={
          <div style={{ display: "flex", "align-items": "center", gap: "4px" }}>
            <button
              data-testid="settings-btn"
              onClick={() => setSettingsOpen(true)}
              title={t("tooltip.settings")}
              style={{
                background: "transparent",
                color: "var(--color-text-2)",
                border: "none",
                "border-radius": "4px",
                padding: "3px 7px",
                cursor: "pointer",
                display: "flex",
                "align-items": "center",
                position: "relative",
              }}
            >
              <SettingsIcon size={13} />
              {/* 有新版可用 → 右上角 accent 小圆点(静默提示,点进设置·更新页查看) */}
              <Show when={updateAvailable()}>
                <span
                  data-testid="update-badge"
                  aria-label="update available"
                  style={{
                    position: "absolute",
                    top: "1px",
                    right: "2px",
                    width: "6px",
                    height: "6px",
                    "border-radius": "50%",
                    background: "var(--color-accent)",
                    "box-shadow": "0 0 4px var(--color-accent)",
                  }}
                />
              </Show>
            </button>
          </div>
        }
      />
      {/* AI CLI 缺失 banner */}
      <Show when={missingClis().length > 0 && !cliBannerDismissed()}>
        <div
          data-testid="cli-banner"
          style={{
            background: "var(--color-status-waiting)",
            color: "var(--color-bg)",
            padding: "6px 12px",
            "font-size": "12px",
            display: "flex",
            "align-items": "center",
            "justify-content": "space-between",
          }}
        >
          <span>{t("banner.cli_missing", { list: missingClis().join(" / ") })}</span>
          <button
            data-testid="cli-banner-dismiss"
            onClick={() => setCliBannerDismissed(true)}
            style={{
              background: "transparent",
              border: "1px solid var(--color-bg)",
              color: "var(--color-bg)",
              padding: "2px 6px",
              "border-radius": "3px",
              cursor: "pointer",
              display: "flex",
              "align-items": "center",
            }}
          >
            <X size={12} />
          </button>
        </div>
      </Show>
      <div
        style={{
          display: "grid",
          "grid-template-columns": `${sidebarWidth()}px 1px 1fr`,
          flex: 1,
          "min-height": 0,
        }}
      >
      {/* 左侧任务列表 + header */}
      <aside
        style={{
          background: "var(--color-surface)",
          // border-right 删了 — resizer 自己画 1px 中线, 不再画两条
          display: "flex",
          "flex-direction": "column",
        }}
      >
        <div
          style={{
            height: "32px",
            padding: "0 12px",
            "box-sizing": "border-box",
            "border-bottom": "1px solid var(--color-border)",
            "flex-shrink": 0,
            display: "flex",
            "align-items": "center",
            "justify-content": "space-between",
          }}
        >
          <span style={{ "font-size": "11px", color: "var(--color-text-2)", "text-transform": "uppercase", "letter-spacing": "0.6px" }}>{t("sidebar.tasks")}</span>
          <div style={{ display: "flex", gap: "4px", "align-items": "center" }}>
            {/* 设置按钮已搬到 Titlebar 右侧;此处只留新建任务 */}
            <button
              data-testid="task-create-btn"
              onClick={handleCreateTask}
              title={t("tooltip.new_task")}
              style={{
                background: "var(--color-accent)",
                color: "var(--color-bg)",
                border: "none",
                padding: "2px 6px",
                "border-radius": "4px",
                cursor: "pointer",
                display: "flex",
                "align-items": "center",
              }}
            >
              <Plus size={12} />
            </button>
          </div>
        </div>
        <div style={{ flex: "1", "min-height": 0, "min-width": 0, overflow: "auto" }}>
          <TaskList
            tasks={displayedTasks()}
            activeTaskId={activeTaskId()}
            onActivate={(id) => {
              setActiveTaskId(id);
              setActiveSlot(null);
            }}
            onRequestClose={(task) => {
              // 关闭确认开关(设置·终端):关掉则直接关任务、保留 worktree
              if (shouldConfirmCloseTask()) {
                setCloseTarget(task);
                return;
              }
              ipc.closeTask(task.id).catch((e) => console.error("[main] close_task failed", e));
            }}
            onReorder={async (newOrder) => {
              try {
                await ipc.reorderTasks(newOrder);
              } catch (e) {
                console.error("[main] reorderTasks failed", e);
              }
            }}
          />
        </div>
      </aside>

      {/* Resizer — grid 占 1px(线本体), 拖动 4px 命中区由 absolute overlay 提供.
          这样 sidebar 右缘紧贴竖线, workspace 左缘紧贴竖线, 任何 splitter 横线都能顶到. */}
      <div
        data-testid="sidebar-resizer"
        style={{
          background: "var(--color-border)",
          position: "relative",
          display: "block",
          "user-select": "none",
        }}
      >
        <div
          onMouseDown={startSidebarDrag}
          title={t("tooltip.resize_sidebar")}
          style={{
            position: "absolute",
            left: "-2px",
            right: "-2px",
            top: 0,
            bottom: 0,
            cursor: "col-resize",
            "z-index": 5,
          }}
        />
      </div>

      {/* 右侧工作区 —— 每个非浮窗任务的 SplitView 常驻挂载,切换任务只切 display
         (修 task switch 时 Terminal unmount → closePty 杀掉正在跑的 PTY 的 bug) */}
      <main style={{ display: "flex", "flex-direction": "column", "min-width": 0, position: "relative" }}>
        <Show when={activeTask() && !isActiveTaskInFloating()}>
          <div
            style={{
              display: "flex",
              background: "var(--color-surface)",
              "border-bottom": "1px solid var(--color-border)",
              height: "32px",
              "box-sizing": "border-box",
              "flex-shrink": 0,
              "align-items": "center",
              padding: "0 8px",
              gap: "4px",
            }}
          >
            <StatusBar activeTerminalId={activeTerminalId()} activeTask={activeTask() ?? null} />
            <div style={{ flex: 1, "min-width": 0 }} />
            <button data-testid="split-h-btn" onClick={() => splitActive("h")} title={t("tooltip.split_h")} style={tabBtnStyle(false)}>
              <SplitSquareHorizontal size={12} />
            </button>
            <button data-testid="split-v-btn" onClick={() => splitActive("v")} title={t("tooltip.split_v")} style={tabBtnStyle(false)}>
              <SplitSquareVertical size={12} />
            </button>
            <button data-testid="split-close-btn" onClick={closeActiveSlot} title={t("tooltip.close_slot")} style={tabBtnStyle(false)}>
              <X size={12} />
            </button>
          </div>
        </Show>

        {/* 终端工作区:SplitView 常驻 DOM,仅当前非浮窗任务可见。 */}
        <div
          data-testid="workspace"
          style={{
            flex: 1,
            "min-height": 0,
            position: "relative",
            overflow: "hidden",
          }}
        >
          <For each={taskIds()}>
            {(taskId) => {
              const taskOf = () => tasks().find((t) => t.id === taskId);
              const isFloating = () => taskOf()?.location.kind === "Floating";
              // 浮窗化时 **不** unmount task pane 也不 cleanup Terminal,
              // 否则 closePty 会杀掉 PTY,浮窗 attach 拿到死 id。
              // 改 display:none 隐藏即可;浮窗 attach 共享同一 PTY(multi-sink)。
              const visible = () =>
                activeTaskId() === taskId && !isFloating();
              return (
                <Show when={getSplitTree(taskId)}>
                  {(tree) => (
                    <div
                      data-testid={`task-pane-${taskId}`}
                      data-task-active={visible() ? "true" : "false"}
                      data-task-status={taskOf()?.status ?? "unknown"}
                      style={{
                        position: "absolute",
                        inset: 0,
                        display: visible() ? "block" : "none",
                      }}
                    >
                      <div style={{ "min-height": 0, position: "relative", height: "100%" }}>
                      <SplitView
                        node={tree()}
                        onRatiosChange={(path, ratios) => {
                          const cur = getSplitTree(taskId);
                          if (!cur) return;
                          writeSplitTree(taskId, setRatiosAt(cur, path, ratios));
                        }}
                        renderLeaf={(slotId) => {
                          // activeSlot 按 task 区分 — slot_id 不同 task 间可能重复(默认都是 0)
                          const isActiveSlot = () => activeSlotOf(taskId) === slotId;
                          // 只有触底叶子才跟外框圆角对齐;其他位置 (中间 / 顶部分屏) 直角
                          const isBottomRight = () => rightmostBottomSlot(tree()) === slotId;
                          // 激活边框只在主窗口右下角保留圆角。
                          const activeRadius = () =>
                            isActiveSlot() && isBottomRight() ? "0 0 17px 0" : "0";
                          return (
                          <div
                            data-testid={`split-slot-${taskId}-${slotId}`}
                            data-active={isActiveSlot() ? "true" : "false"}
                            onClick={() => {
                              setActiveTaskId(taskId);
                              setActiveSlotFor(taskId, slotId);
                              const tid = slotToTerm().get(slotKey(taskId, slotId));
                              if (tid !== undefined) setActiveTerminalId(tid);
                            }}
                            style={{
                              width: "100%",
                              height: "100%",
                              "box-sizing": "border-box",
                              background: "var(--color-bg)",
                              position: "relative",
                              "z-index": isActiveSlot() ? 2 : "auto",
                            }}
                          >
                            <Terminal
                              taskId={taskId}
                              slotId={slotId}
                              theme={currentTheme() ?? undefined}
                              onReady={(termId) => {
                                setSlotToTerm((m) => {
                                  const nx = new Map(m);
                                  nx.set(slotKey(taskId, slotId), termId);
                                  return nx;
                                });
                                if (
                                  activeTaskId() === taskId &&
                                  activeSlotOf(taskId) === null
                                ) {
                                  setActiveSlotFor(taskId, slotId);
                                  setActiveTerminalId(termId);
                                }
                                // G6:该 slot 有布局启动命令则发一次
                                consumePendingCommand(taskId, slotId, termId);
                              }}
                            />
                            <div
                              aria-hidden="true"
                              style={{
                                position: "absolute",
                                // 激活分屏显示 1px 内边框和微光。
                                inset: isActiveSlot() ? "1px" : "0",
                                "pointer-events": "none",
                                "box-sizing": "border-box",
                                border: isActiveSlot()
                                  ? "1px solid var(--color-accent)"
                                  : "none",
                                "border-radius": activeRadius(),
                                "box-shadow": isActiveSlot()
                                  ? "inset 0 0 8px -3px var(--color-accent)"
                                  : "none",
                                transition: "box-shadow 120ms ease, border-color 120ms ease",
                              }}
                            />
                          </div>
                          );
                        }}
                      />
                      </div>
                    </div>
                  )}
                </Show>
              );
            }}
          </For>

          {/* 空状态 / 浮窗占位 — overlay 形式,不影响下层 Terminal 常驻 */}
          <Show when={!activeTask() || isActiveTaskInFloating()}>
            <div
              data-testid="empty-hint"
              style={{
                position: "absolute",
                inset: 0,
                display: "flex",
                "flex-direction": "column",
                "align-items": "center",
                "justify-content": "center",
                color: "var(--color-text-2)",
                "font-size": "13px",
                gap: "16px",
                padding: "20px",
                "text-align": "center",
                background: "var(--color-bg)",
              }}
            >
              <Show
                when={isActiveTaskInFloating()}
                fallback={
                  <>
                    <div style={{ "font-size": "32px", opacity: 0.4 }}>$_</div>
                    <div style={{ "font-size": "15px", color: "var(--color-text)" }}>
                      {t("task.empty_hint")}
                    </div>
                    <div style={{ display: "flex", gap: "24px", "font-size": "12px" }}>
                      <span><kbd style={kbdStyle()}>{modKeyLabel()}+N</kbd> {t("task.new")}</span>
                      <span><kbd style={kbdStyle()}>{modKeyLabel()}+K</kbd> {t("kb.command.command_palette")}</span>
                      <span><kbd style={kbdStyle()}>{modKeyLabel()}+,</kbd> {t("settings.title")}</span>
                    </div>
                  </>
                }
              >
                <span>{t("task.in_floating")}</span>
              </Show>
            </div>
          </Show>
        </div>

      </main>
      </div>

      <Show when={paletteOpen()}>
        <CommandPalette
          tasks={tasks()}
          currentTerminalId={activeTerminalId()}
          onClose={() => setPaletteOpen(false)}
          onActivateTask={(id) => {
            setActiveTaskId(id);
            setActiveSlot(null);
            setPaletteOpen(false);
          }}
          onCreateTask={handleCreateTask}
          onOpenSettings={() => {
            setPaletteOpen(false);
            setSettingsOpen(true);
          }}
          onOpenDiff={() => {
            const cwd = activeTask()?.cwd ?? null;
            setPaletteOpen(false);
            if (cwd) setDiffCwd(cwd);
          }}
          onApplyLayout={applyLayout}
          onResumeAgent={resumeAgent}
        />
      </Show>

      <Show when={diffCwd()}>
        <DiffViewer cwd={diffCwd()!} onClose={() => setDiffCwd(null)} />
      </Show>

      <Show when={settingsOpen()}>
        <Settings
          activeThemeId={currentTheme()?.id ?? "vibe"}
          activeTerminalId={activeTerminalId()}
          initialTab={settingsInitialTab()}
          onClose={() => {
            setSettingsOpen(false);
            setSettingsInitialTab(undefined);
          }}
        />
      </Show>

      <Show when={promptPickerOpen()}>
        <PromptPicker
          terminalId={activeTerminalId()}
          onClose={() => setPromptPickerOpen(false)}
          kind={
            tasks().find((t) => t.id === activeTaskId())?.agent_kind
              ? "agent"
              : "terminal"
          }
        />
      </Show>

      <Show when={newTaskOpen()}>
        <NewTaskDialog
          onClose={() => setNewTaskOpen(false)}
          onSubmit={submitNewTask}
        />
      </Show>

      <Show when={closeTarget()}>
        {(getTask) => {
          const task = getTask();
          // 摘要 — 前端只有任务级聚合 status(无 per-terminal),用它替代旧的
          // 全造假 "running"。映射到弹窗接受的 idle/running/waiting_input:
          //   stalled→running(进程卡住仍在跑) · done/idle→idle · 其余原样
          const cs =
            task.status === "waiting_input"
              ? "waiting_input"
              : task.status === "running" || task.status === "stalled"
                ? "running"
                : "idle";
          const stats = task.terminal_ids.map(() => cs as "idle" | "running" | "waiting_input");
          return (
            <ConfirmCloseDialog
              task={task}
              terminalStatuses={stats}
              onCancel={() => setCloseTarget(null)}
              onConfirm={confirmCloseTask}
            />
          );
        }}
      </Show>

      {/* CSS:呼吸动画 */}
      <style>{`
        @keyframes vibeterm-breath {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.5; }
        }
        @media (prefers-reduced-motion: reduce) {
          @keyframes vibeterm-breath { 0%,100%,50% { opacity: 1; } }
        }
      `}</style>
    </div>
  );
}

// 注:用 SplitView 取代旧 TerminalsWorkspace(展示 split tree 内的 Terminal 们)

function kbdStyle() {
  return {
    background: "var(--color-surface)",
    border: "1px solid var(--color-border)",
    "border-radius": "3px",
    padding: "2px 6px",
    "font-family": "monospace",
    "font-size": "11px",
    color: "var(--color-text)",
    "margin-right": "4px",
  };
}

function tabBtnStyle(active: boolean) {
  return {
    background: active ? "var(--color-accent-subtle)" : "var(--color-bg)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    "border-radius": "4px",
    padding: "4px 8px",
    cursor: "pointer",
    "font-size": "12px",
    display: "flex",
    "align-items": "center",
    "justify-content": "center",
    "min-width": "28px",
    "min-height": "24px",
  };
}

// A4 Custom Actions shortcut 解析:
//   - 把 KeyboardEvent 标准化成 "MOD+SHIFT+R" 等规范字符串
//   - 把 actions.toml 里写的 "Mod+Shift+R" 标准化成相同形式
//   - 比较时大小写不敏感 / Mod ≡ Cmd/Ctrl
function normalizeShortcut(e: KeyboardEvent): string | null {
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("MOD");
  if (e.altKey) parts.push("ALT");
  if (e.shiftKey) parts.push("SHIFT");
  const k = e.key;
  if (!k || k === "Meta" || k === "Control" || k === "Alt" || k === "Shift") return null;
  parts.push(k.length === 1 ? k.toUpperCase() : k);
  // 至少要有修饰键,否则任意字母都可能命中,误触
  if (parts.length === 1) return null;
  return parts.join("+");
}

function normalizeShortcutStr(s: string): string {
  return s
    .split("+")
    .map((p) => p.trim())
    .map((p) => {
      const u = p.toUpperCase();
      if (u === "CMD" || u === "CTRL" || u === "CONTROL" || u === "MOD" || u === "META") return "MOD";
      if (u === "OPTION" || u === "OPT" || u === "ALT") return "ALT";
      if (u === "SHIFT") return "SHIFT";
      return p.length === 1 ? p.toUpperCase() : p;
    })
    .join("+");
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
render(() => <App />, root);
