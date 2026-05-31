// 状态栏 — widget 注册表驱动, popover 详情用子目录 ./popover/*.
//
// 架构:
//   - 数据信号统一收集 (cwd / git / agentKind / claudeSession / codexSnap / usageCache / block)
//   - 用户配置: ~/.config/vibeterm/statusline.toml `items = ["current-dir", ...]`
//   - 主循环按 items 顺序调 WIDGETS[id] (item, ctx) → JSX | null
//   - 适用不到的 widget 自己返回 null, 配置层不做分支
//   - ⓘ 按钮打开 DetailPopover, 内容随当前 agentKind 自动展示

import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
  type Component,
} from "solid-js";
import {
  getClaudeUsageCache,
  onClaudeUsageChanged,
  onClaudeSessionChanged,
  onCodexSessionChanged,
  getClaudeSession,
  getCodexSession,
  getClaudeSessionByCwd,
  getCodexSessionByCwd,
  getClaudeBlockByCwd,
  getCodexBlockByCwd,
  getClaudeTokensToday,
  getClaudePlan,
  getTerminalCwd,
  ghPrStatus,
  gitStatusBrief,
  gitStashCount,
  detectAgentForTerminal,
  getStatusLineConfig,
  onStatusLineConfigChanged,
} from "../ipc";
import type {
  ClaudeActiveBlock,
  ClaudeUsageCache,
  ClaudeSession,
  CodexSnapshot,
  GitStatusBrief,
  TaskDto,
  TerminalId,
  StatusLineFile,
} from "@vibeterm/ipc-types";
import { statusLineItemDetail } from "@vibeterm/ipc-types";
import { WIDGETS, type RenderContext } from "./widgets";
import { t } from "../i18n";
import { DetailPopover } from "./popover/DetailPopover";

export interface StatusBarProps {
  activeTerminalId?: number | null;
  /** 活跃 task (task-status / task-name / worktree-name widget 用) */
  activeTask?: TaskDto | null;
}

/// fallback 配置 — 仅在 `getStatuslineConfig()` 尚未返回时短暂使用.
/// `display_name` 走 `t()` 保证 EN/JA 用户不会看到中文.
function defaultConfig(): StatusLineFile {
  return {
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
  };
}

// 刷新分档: 快档(便宜常变: cwd/git/session) vs 慢档(贵且少变:
// lsof agent 探测 / gh PR 网络 / 扫目录 tokens). 慢档没必要 3s 一刷.
const FAST_REFRESH_MS = 3000;
const SLOW_REFRESH_MS = 30000;

export const StatusBar: Component<StatusBarProps> = (props) => {
  // ---- 数据信号 ----
  const [cache, setCache] = createSignal<ClaudeUsageCache | null>(null);
  const [session, setSession] = createSignal<ClaudeSession | null>(null);
  const [block, setBlock] = createSignal<ClaudeActiveBlock | null>(null);
  const [codex, setCodex] = createSignal<CodexSnapshot | null>(null);
  const [codexBlock, setCodexBlock] = createSignal<ClaudeActiveBlock | null>(null);
  const [cwd, setCwd] = createSignal<string | null>(null);
  const [git, setGit] = createSignal<GitStatusBrief | null>(null);
  const [stash, setStash] = createSignal<number>(0);
  const [tokensToday, setTokensToday] = createSignal<number>(0);
  const [claudePlan, setClaudePlan] = createSignal<string | null>(null);
  const [prStatus, setPrStatus] = createSignal<string | null>(null);
  const [agentKind, setAgentKind] = createSignal<string | null>(null);
  const [config, setConfig] = createSignal<StatusLineFile>(defaultConfig());

  let unUsage: (() => void) | null = null;
  let unClaude: (() => void) | null = null;
  let unCodex: (() => void) | null = null;
  let unConfig: (() => void) | null = null;
  let pollTimer: number | null = null;
  let slowTimer: number | null = null;
  // plan 只跟账号有关, 整个 app 生命周期取一次即可.
  let planFetched = false;
  // 每次发起新一轮刷新自增; refreshAll 捕获当时的 gen, await 恢复后比对,
  // 不一致说明已被更新的刷新 (切终端 / 下一轮轮询 / 事件) 取代, 直接放弃, 防竞态覆盖.
  let currentGen = 0;

  // ---- 数据拉取 (分档) ----
  // gen 仅在切换终端时自增 (见 createEffect); 各档/事件共享当前 gen, 写的是
  // 互不相交的 signal, 不会互相取消; await 后比对 gen 防切换后回写陈旧数据.

  // 仅刷新当前 agent 的 session + block (cwd-scoped). 事件 + 快档共用 —
  // 只重解析 jsonl, 不 spawn 子进程, 比全量刷新轻得多.
  const refreshSession = async (gen: number, c: string | null) => {
    const kind = agentKind();
    if (kind === "claude") {
      const s = c
        ? ((await getClaudeSessionByCwd(c)) ?? (await getClaudeSession()))
        : await getClaudeSession();
      const blk = c ? await getClaudeBlockByCwd(c) : null;
      if (gen !== currentGen) return;
      setSession(s);
      setBlock(blk);
      setCodex(null);
      setCodexBlock(null);
    } else if (kind === "codex") {
      const s = c
        ? ((await getCodexSessionByCwd(c)) ?? (await getCodexSession()))
        : await getCodexSession();
      const blk = c ? await getCodexBlockByCwd(c) : null;
      if (gen !== currentGen) return;
      setCodex(s);
      setCodexBlock(blk);
      setSession(null);
      setBlock(null);
    } else {
      setSession(null);
      setBlock(null);
      setCodex(null);
      setCodexBlock(null);
    }
  };

  // 快档 (3s): cwd / git / stash / agent 类型 / 当前 agent session — 随用户操作常变.
  // agent 类型探测放快档(原在慢档 30s): 同一终端里"进入 agent"~3s 内认出, 且严格按
  // **该终端自己的进程树**判 —— 不用 task 级 agent_kind(分屏里空终端会被兄弟终端的
  // agent 盖掉, 表现为"被 claude/codex 抢"). detectAgentForTerminal 一次 ps -ax, 够便宜.
  const refreshFast = async (tid: number, gen: number) => {
    try {
      const c = await getTerminalCwd(tid as TerminalId);
      if (gen !== currentGen) return;
      setCwd(c);
      if (c) {
        const [gitBrief, stashCount] = await Promise.all([gitStatusBrief(c), gitStashCount(c)]);
        if (gen !== currentGen) return;
        setGit(gitBrief);
        setStash(stashCount);
      } else {
        setGit(null);
        setStash(0);
      }
      const det = await detectAgentForTerminal(tid);
      if (gen !== currentGen) return;
      setAgentKind(det.agent_kind ?? null);
      await refreshSession(gen, c);
    } catch (e) {
      console.warn("[status-bar] fast refresh failed", e);
    }
  };

  // 慢档 (30s): PR 状态 (gh 网络) / 今日 token (扫目录) / plan (一辈子一次).
  // 都贵且少变. agent 类型探测已移到快档(终端级, 见 refreshFast).
  const refreshSlow = async (_tid: number, gen: number) => {
    try {
      const tokensToday = await getClaudeTokensToday();
      if (gen !== currentGen) return;
      // tokens-today 跨 cwd 累计, 跟 cwd 无关
      setTokensToday(tokensToday);
      const c = cwd();
      if (c) {
        const pr = await ghPrStatus(c);
        if (gen !== currentGen) return;
        setPrStatus(pr);
      } else {
        setPrStatus(null);
      }
      // plan 只跟账号有关 — 一辈子取一次. null 是合法值 (未登录), 不能用 == null 触发重试.
      if (!planFetched) {
        planFetched = true;
        try {
          const plan = await getClaudePlan();
          if (gen !== currentGen) {
            planFetched = false; // 本轮被取代, 没真正 set 上, 允许下轮重取
            return;
          }
          setClaudePlan(plan);
        } catch (e) {
          planFetched = false; // 失败允许下次再试
          console.warn("[status-bar] getClaudePlan failed", e);
        }
      }
    } catch (e) {
      console.warn("[status-bar] slow refresh failed", e);
    }
  };

  // 切换终端时的完整建立: **先快档**(cwd/git/agentKind/session — 可见状态, 立即更新),
  // **再慢档**(PR/tokens/plan — 含 ghPrStatus 网络调用, 可能秒级). 顺序关键: 若先跑慢档,
  // 它的网络 PR 会把可见状态的更新一起拖住, 网络慢时表现为"切了不更新".
  // refreshFast 自己开头会取 cwd, 慢档随后读 cwd() 即可, 无需在此重复取.
  const refreshAll = async (tid: number, gen: number) => {
    await refreshFast(tid, gen);
    if (gen !== currentGen) return;
    await refreshSlow(tid, gen);
  };

  createEffect(() => {
    const tid = props.activeTerminalId;
    if (pollTimer !== null) {
      window.clearInterval(pollTimer);
      pollTimer = null;
    }
    if (slowTimer !== null) {
      window.clearInterval(slowTimer);
      slowTimer = null;
    }
    if (tid == null) {
      // 让任何飞行中的旧刷新失效, 防止它在清空后回写陈旧数据.
      currentGen++;
      setCwd(null);
      setGit(null);
      setStash(0);
      setPrStatus(null);
      setSession(null);
      setBlock(null);
      setCodex(null);
      setCodexBlock(null);
      setAgentKind(null);
      return;
    }
    // 切换终端 → gen 自增一次, 旧终端飞行中的刷新随即失效.
    const gen = ++currentGen;
    void refreshAll(tid, gen);
    // 定时器不自增 gen: 快/慢档写不相交的 signal, 共享 gen 即可, 切换时统一失效.
    pollTimer = window.setInterval(() => void refreshFast(tid, currentGen), FAST_REFRESH_MS);
    slowTimer = window.setInterval(() => void refreshSlow(tid, currentGen), SLOW_REFRESH_MS);
  });

  onMount(async () => {
    try {
      setCache(await getClaudeUsageCache());
      setConfig(await getStatusLineConfig());
    } catch (e) {
      console.warn("[status-bar] initial fetch failed", e);
    }
    try {
      unUsage = await onClaudeUsageChanged((c) => setCache(c));
      unClaude = await onClaudeSessionChanged(() => {
        const tid = props.activeTerminalId;
        // session 写入事件 → 只刷 session+block (cwd-scoped), 不重跑 git/gh/lsof.
        if (tid != null && agentKind() === "claude") void refreshSession(currentGen, cwd());
      });
      unCodex = await onCodexSessionChanged(() => {
        const tid = props.activeTerminalId;
        if (tid != null && agentKind() === "codex") void refreshSession(currentGen, cwd());
      });
      unConfig = await onStatusLineConfigChanged(async () => {
        try {
          setConfig(await getStatusLineConfig());
        } catch (e) {
          console.warn("[status-bar] reload config failed", e);
        }
      });
    } catch (e) {
      console.warn("[status-bar] subscribe failed", e);
    }
  });

  onCleanup(() => {
    unUsage?.();
    unClaude?.();
    unCodex?.();
    unConfig?.();
    if (pollTimer !== null) window.clearInterval(pollTimer);
    if (slowTimer !== null) window.clearInterval(slowTimer);
  });

  // ---- RenderContext (传给每个 widget + popover) ----
  const ctx: RenderContext = {
    cwd,
    git,
    gitStashCount: stash,
    claudeTokensToday: tokensToday,
    claudePlan,
    prStatus,
    agentKind,
    claudeSession: session,
    claudeUsage: cache,
    claudeBlock: block,
    codexSnap: codex,
    codexBlock,
    task: () => props.activeTask ?? null,
  };

  // 按当前 agentKind 选 profile (没匹配 → default)
  const activeProfile = createMemo(() => {
    const cfg = config();
    const kind = agentKind() ?? "default";
    return cfg.profiles?.[kind] ?? cfg.profiles?.default ?? { items: [] };
  });

  // 渲染当前 profile 的 items, 过滤掉返回 null 的 (条件隐藏靠 widget 自己).
  const renderedItems = createMemo(() => {
    return activeProfile()
      .items.map((raw) => {
        const item = statusLineItemDetail(raw);
        if (item.hide) return null;
        const renderer = WIDGETS[item.type];
        if (!renderer) return null;
        const node = renderer(item, ctx);
        if (node == null) return null;
        return { id: item.type, node };
      })
      .filter((x): x is { id: string; node: any } => x != null);
  });

  const hasAnyData = () => renderedItems().length > 0;

  // ---- Popover state ----
  const [popoverOpen, setPopoverOpen] = createSignal(false);
  let triggerRef: HTMLButtonElement | undefined;

  const togglePopover = (e: MouseEvent) => {
    e.stopPropagation();
    setPopoverOpen((v) => !v);
  };

  onMount(() => {
    const handler = (e: MouseEvent) => {
      if (!popoverOpen()) return;
      const target = e.target as HTMLElement;
      if (target.closest("[data-status-popover='true']")) return;
      if (target.closest("[data-status-toggle='true']")) return;
      setPopoverOpen(false);
    };
    document.addEventListener("mousedown", handler);
    onCleanup(() => document.removeEventListener("mousedown", handler));
  });

  return (
    <Show when={hasAnyData()}>
      <div
        style={{
          position: "relative",
          display: "flex",
          "align-items": "center",
          "min-width": 0,
        }}
      >
        <div
          data-testid="status-bar"
          style={{
            display: "flex",
            "align-items": "center",
            gap: "10px",
            "font-size": "11px",
            "padding-left": "4px",
            color: "var(--color-text-2)",
            "min-width": 0,
            overflow: "hidden",
          }}
        >
          <button
            ref={triggerRef}
            data-status-toggle="true"
            onClick={togglePopover}
            title={
              popoverOpen()
                ? t("statusbar.popover.toggle_close")
                : t("statusbar.popover.toggle_open")
            }
            style={{
              padding: "2px 6px",
              background: popoverOpen() ? "var(--color-border)" : "transparent",
              border: "1px solid var(--color-border)",
              "border-radius": "4px",
              color: "var(--color-text-2)",
              cursor: "pointer",
              "font-size": "11px",
              "line-height": 1,
              "flex-shrink": 0,
            }}
          >
            ⓘ
          </button>
          <For each={renderedItems()}>{(item) => item.node as any}</For>
        </div>

        <Show when={popoverOpen() && hasAnyData()}>
          <DetailPopover
            ctx={ctx}
            anchor={triggerRef}
            onClose={() => setPopoverOpen(false)}
          />
        </Show>
      </div>
    </Show>
  );
};
