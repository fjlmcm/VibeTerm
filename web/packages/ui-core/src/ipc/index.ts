// 类型化 Tauri invoke 包装

import { invoke, Channel } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  SpawnPtyOpts,
  SpawnPtyResult,
  TerminalId,
  TaskId,
  TaskDto,
  CreateTaskOpts,
  Theme,
  Config,
  CliStatus,
  EnvFile,
  KeybindingsFile,
  PromptsFile,
  ActionsFile,
  ExecuteActionResult,
  SplitTreeNode,
  WorktreeRef,
  BranchSpec,
  ClaudeUsageCache,
  ClaudeSession,
  ClaudeActiveBlock,
  CodexSnapshot,
  GitStatusBrief,
  GitDiffResult,
  LayoutTemplate,
  ResumeInfo,
  StatusLineFile,
  NotifyFile,
  NotifyPermissionState,
  NotifySoundData,
  BuiltinSound,
  UsageStats,
  AppUpdateInfo,
  PricingStatus,
  AgentTerminalCompleted,
} from "@vibeterm/ipc-types";

// ===== Terminal =====
export async function startPty(
  opts: SpawnPtyOpts,
  channel: Channel<number[] | Uint8Array>,
): Promise<SpawnPtyResult> {
  return invoke<SpawnPtyResult>("start_pty", { opts, channel });
}

export async function spawnTerminalInTask(
  taskId: TaskId,
  slotId: number | null,
  opts: SpawnPtyOpts,
  channel: Channel<number[] | Uint8Array>,
): Promise<SpawnPtyResult> {
  return invoke<SpawnPtyResult>("spawn_terminal_in_task", { taskId, slotId, opts, channel });
}

export async function writePty(id: TerminalId, data: Uint8Array): Promise<void> {
  return invoke("write_pty", { id, data: Array.from(data) });
}

export async function resizePty(id: TerminalId, rows: number, cols: number): Promise<void> {
  return invoke("resize_pty", { id, rows, cols });
}

export async function closePty(id: TerminalId): Promise<void> {
  return invoke("close_pty", { id });
}

// 取消 slot attach 的订阅(组件卸载时;不关 PTY)。sinkId 来自 SpawnPtyResult.sink_id:
// Rust 端 u64 经 JSON 落地为 JS number,next_sink_id 单调递增,实际远不会触及 2^53,安全。
export async function detachTerminal(id: TerminalId, sinkId: number): Promise<void> {
  return invoke("detach_terminal", { id, sinkId });
}

// 读 scrollback 快照(独立 query,不订阅 stream)
// 后端 ring buffer 上限 256KB,超出按 FIFO 丢弃头部
export async function getScrollback(id: TerminalId): Promise<Uint8Array> {
  const raw = await invoke<number[] | Uint8Array>("get_scrollback", { id });
  return raw instanceof Uint8Array ? raw : Uint8Array.from(raw);
}

// 读 PTY 当前生效尺寸 [rows, cols](最近一次 resize 下发值;(0,0)=spawn 后未 resize)。
// 视图变可见时用它判断 PTY 是否被别的视图(浮窗)改过尺寸 → 不一致说明隐藏期 buffer 已污染。
export async function terminalSize(id: TerminalId): Promise<[number, number]> {
  return invoke<[number, number]>("terminal_size", { id });
}

// 一次 IPC 同时 try files + image + text。
// 优先顺序:files(剪贴板含文件 URL,如 Finder Cmd+C)→ image(截图 bitmap)→ text。
// files 在 image 之前是为了避免 Finder 把缩略 icon 当 bitmap 拿到。
export type PasteResult =
  | { kind: "files"; paths: string[] }
  | { kind: "image"; path: string }
  | { kind: "text"; text: string }
  | { kind: "empty" };

export async function pasteClipboard(): Promise<PasteResult> {
  return invoke<PasteResult>("paste_clipboard");
}

// 设置页查询 / 操作:当前生效目录 / 打开 / 清空
export async function getClipboardImagesDir(): Promise<string> {
  return invoke<string>("get_clipboard_images_dir");
}

export async function openClipboardImagesDir(): Promise<void> {
  return invoke("open_clipboard_images_dir");
}

export async function clearClipboardImages(): Promise<number> {
  return invoke<number>("clear_clipboard_images");
}

// ===== Tasks =====
export async function listTasks(): Promise<TaskDto[]> {
  return invoke<TaskDto[]>("list_tasks");
}

export async function createTask(opts: CreateTaskOpts): Promise<TaskDto> {
  return invoke<TaskDto>("create_task", { opts });
}

export async function closeTask(id: TaskId): Promise<void> {
  return invoke("close_task", { id });
}

export async function renameTask(id: TaskId, name: string): Promise<void> {
  return invoke("rename_task", { id, name });
}

export async function pinTask(id: TaskId, pinned: boolean): Promise<void> {
  return invoke("pin_task", { id, pinned });
}

/** 切换 task 通知静音 (持久化). 静音的 task 不弹系统通知. */
export async function setTaskNotifyMuted(id: TaskId, muted: boolean): Promise<void> {
  return invoke("set_task_notify_muted", { id, muted });
}

// ===== 全局通知偏好 =====
export async function getNotifyPrefs(): Promise<NotifyFile> {
  return invoke<NotifyFile>("get_notify_prefs");
}

export async function saveNotifyPrefs(prefs: NotifyFile): Promise<void> {
  return invoke("save_notify_prefs", { prefs });
}

export async function notifyPermission(): Promise<NotifyPermissionState> {
  return invoke<NotifyPermissionState>("notify_permission");
}

export async function requestNotifyPermission(): Promise<NotifyPermissionState> {
  return invoke<NotifyPermissionState>("request_notify_permission");
}

/** 预览/解析通知声音 → base64 字节 + MIME, 给 <audio> 播放 */
export async function previewNotifySound(sound: string): Promise<NotifySoundData> {
  return invoke<NotifySoundData>("preview_notify_sound", { sound });
}

/** 列 VibeTerm 自带声音库 (打包资源里的 sounds.json) */
export async function listBuiltinSounds(): Promise<BuiltinSound[]> {
  return invoke<BuiltinSound[]>("list_builtin_sounds");
}

export async function reorderTasks(order: TaskId[]): Promise<void> {
  return invoke("reorder_tasks", { order });
}

export async function setActiveTask(id: TaskId): Promise<void> {
  return invoke("set_active_task", { id });
}

// 写回任务分屏布局,后端 source of truth,主 + 浮窗都从此读写
export async function setTaskSplitTree(
  id: TaskId,
  tree: SplitTreeNode,
): Promise<void> {
  return invoke("set_task_split_tree", { id, tree });
}

// ===== Theme / Config =====
export async function getConfig(): Promise<Config> {
  return invoke<Config>("get_config");
}

export async function setAutoCheckUpdates(enabled: boolean): Promise<void> {
  return invoke<void>("set_auto_check_updates", { enabled });
}

export async function setShellIntegration(enabled: boolean): Promise<void> {
  return invoke<void>("set_shell_integration", { enabled });
}

export async function setActiveTheme(id: string): Promise<Theme> {
  return invoke<Theme>("set_active_theme", { id });
}

export async function listThemes(): Promise<Theme[]> {
  return invoke<Theme[]>("list_themes");
}

export async function getTheme(id: string): Promise<Theme> {
  return invoke<Theme>("get_theme", { id });
}

// ===== Window =====
export async function openFloating(taskId: TaskId): Promise<string> {
  return invoke<string>("open_floating", { taskId });
}

export async function closeFloating(label: string): Promise<void> {
  return invoke("close_floating", { label });
}

export async function focusWindow(label: string): Promise<void> {
  return invoke("focus_window", { label });
}

// 浮窗里按全局快捷键 → 拉主窗口前台 + 触发该 action
export async function invokeGlobalAction(action: string): Promise<void> {
  return invoke("invoke_global_action", { action });
}

export function onGlobalAction(handler: (action: string) => void): Promise<UnlistenFn> {
  return tauriListen<string>("global_action", (e) => handler(e.payload));
}

// ===== Events =====
export function onTasksChanged(handler: (tasks: TaskDto[]) => void): Promise<UnlistenFn> {
  return tauriListen<TaskDto[]>("tasks_changed", (e) => handler(e.payload));
}

export function onThemeChanged(handler: (theme: Theme) => void): Promise<UnlistenFn> {
  return tauriListen<Theme>("theme_changed", (e) => handler(e.payload));
}

export function onConfigChanged(handler: () => void): Promise<UnlistenFn> {
  return tauriListen("config_changed", () => handler());
}

/** 通知点击聚焦目标 — 主窗口聚焦时若窗口期内有 last_notify,后端发此事件 */
export function onNotificationFocusTarget(
  handler: (taskId: TaskId) => void,
): Promise<UnlistenFn> {
  return tauriListen<{ task_id: TaskId }>("notification_focus_target", (e) =>
    handler(e.payload.task_id),
  );
}

/** 自定义文件路径触发的通知 → 前端 <audio> 播放原始字节 */
export function onNotificationPlaySound(
  handler: (sound: string) => void,
): Promise<UnlistenFn> {
  return tauriListen<{ sound: string }>("notification_play_sound", (e) =>
    handler(e.payload.sound),
  );
}

/** 前台完成"非当前任务"→ 任务列表对应行闪一下高亮(配合轻提示音)。payload = task_id */
export function onTaskFlash(handler: (taskId: TaskId) => void): Promise<UnlistenFn> {
  return tauriListen<TaskId>("task_flash", (e) => handler(e.payload));
}

// ===== Agent watch (v1) =====
export async function getClaudeUsageCache(): Promise<ClaudeUsageCache | null> {
  return invoke<ClaudeUsageCache | null>("get_claude_usage_cache");
}

/** 使用统计面板 — 全量聚合最近 days 天 (默认 30). 全量扫描可能慢. */
export async function getUsageStats(days?: number): Promise<UsageStats> {
  return invoke<UsageStats>("get_usage_stats", { days: days ?? null });
}

/** 把面板导出的 PNG (base64, 无 data: 前缀) 写到用户选定的路径 (.png). */
export async function savePngFile(path: string, base64Png: string): Promise<void> {
  return invoke<void>("save_png_file", { path, base64Png });
}

// ===== 设置·更新页:软件版本检查 + 模型价格(手动, 仅点按钮时联网)=====

/** 检查软件更新 — GET GitHub latest release 比较版本. 仅展示 + 给 release 链接, 不下载安装. */
export async function checkAppUpdate(): Promise<AppUpdateInfo> {
  return invoke<AppUpdateInfo>("check_app_update");
}

/** 当前模型价格来源状态(内置快照 / 已手动更新的覆盖). */
export async function getPricingStatus(): Promise<PricingStatus> {
  return invoke<PricingStatus>("get_pricing_status");
}

/** 手动更新模型价格 — 拉取维护的最新价格表并应用(落本地 config). */
export async function updateModelPricing(): Promise<PricingStatus> {
  return invoke<PricingStatus>("update_model_pricing");
}

/** 还原内置默认价格(删除本地覆盖). */
export async function resetModelPricing(): Promise<PricingStatus> {
  return invoke<PricingStatus>("reset_model_pricing");
}

export function onClaudeUsageChanged(
  handler: (cache: ClaudeUsageCache | null) => void,
): Promise<UnlistenFn> {
  return tauriListen<ClaudeUsageCache | null>("claude_usage_changed", (e) => handler(e.payload));
}

export async function getClaudeSession(): Promise<ClaudeSession | null> {
  return invoke<ClaudeSession | null>("get_claude_session");
}

export function onClaudeSessionChanged(
  handler: (session: ClaudeSession | null) => void,
): Promise<UnlistenFn> {
  return tauriListen<ClaudeSession | null>("claude_session_changed", (e) => handler(e.payload));
}

export async function getCodexSession(): Promise<CodexSnapshot | null> {
  return invoke<CodexSnapshot | null>("get_codex_session");
}

export async function getClaudeSessionByCwd(cwd: string): Promise<ClaudeSession | null> {
  return invoke<ClaudeSession | null>("get_claude_session_by_cwd", { cwd });
}

export async function getCodexSessionByCwd(cwd: string): Promise<CodexSnapshot | null> {
  return invoke<CodexSnapshot | null>("get_codex_session_by_cwd", { cwd });
}

export async function getClaudeBlockByCwd(cwd: string): Promise<ClaudeActiveBlock | null> {
  return invoke<ClaudeActiveBlock | null>("get_claude_block_by_cwd", { cwd });
}

/// Codex 5h 块 — 本地从 rollout token_count 事件算 (跟 Claude 同算法).
/// 后端字段跟 ClaudeActiveBlock 同形, 类型直接复用; cost_usd 永远 null (Codex 没价格表).
export async function getCodexBlockByCwd(cwd: string): Promise<ClaudeActiveBlock | null> {
  return invoke<ClaudeActiveBlock | null>("get_codex_block_by_cwd", { cwd });
}

export async function getClaudeTokensToday(): Promise<number> {
  return invoke<number>("get_claude_tokens_today");
}

export async function getClaudePlan(): Promise<string | null> {
  return invoke<string | null>("get_claude_plan");
}

export function onCodexSessionChanged(
  handler: (snap: CodexSnapshot | null) => void,
): Promise<UnlistenFn> {
  return tauriListen<CodexSnapshot | null>("codex_session_changed", (e) => handler(e.payload));
}

export async function getTerminalCwd(terminalId: TerminalId): Promise<string | null> {
  return invoke<string | null>("get_terminal_cwd", { terminalId });
}

export async function gitStatusBrief(cwd: string): Promise<GitStatusBrief | null> {
  return invoke<GitStatusBrief | null>("git_status_brief", { cwd });
}

export async function gitStashCount(cwd: string): Promise<number> {
  return invoke<number>("git_stash_count", { cwd });
}

export async function ghPrStatus(cwd: string): Promise<string | null> {
  return invoke<string | null>("gh_pr_status", { cwd });
}

/** 保存全部终端 scrollback 快照(会话恢复用,覆盖式)。 */
export async function saveScrollback(entries: { key: string; data: string }[]): Promise<void> {
  return invoke<void>("save_scrollback", { entries });
}

/** 启动时读 scrollback 快照(键 "taskId:slotId" → 序列化缓冲)。 */
export async function loadScrollback(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("load_scrollback");
}

/** 布局模板列表(命令面板任务预设,读 layouts.toml)。 */
export async function listLayouts(): Promise<LayoutTemplate[]> {
  return invoke<LayoutTemplate[]>("list_layouts");
}

/** agent 会话恢复命令(只读嗅探 session_id;无可恢复会话 → null)。不自动执行。 */
export async function agentResumeCommand(
  cwd: string,
  agentKind?: string | null,
): Promise<ResumeInfo | null> {
  return invoke<ResumeInfo | null>("agent_resume_command", { cwd, agentKind: agentKind ?? null });
}

/** 三源 diff(纯只读 git diff,零侵入)。source: "unstaged" | "staged" | "base"。 */
export async function gitDiff(
  cwd: string,
  source: "unstaged" | "staged" | "base",
  base?: string | null,
): Promise<GitDiffResult | null> {
  return invoke<GitDiffResult | null>("git_diff", { cwd, source, base: base ?? null });
}

export async function getStatusLineConfig(): Promise<StatusLineFile> {
  return invoke<StatusLineFile>("get_statusline_config");
}

export async function saveStatusLineConfig(config: StatusLineFile): Promise<void> {
  return invoke<void>("save_statusline_config", { config });
}

export function onStatusLineConfigChanged(handler: () => void): Promise<UnlistenFn> {
  return tauriListen("statusline_config_changed", () => handler());
}

/** 某 task 的某终端 agent 刚完成一轮 — 用于切回任务时自动定位焦点到该终端。 */
export function onAgentTerminalCompleted(
  handler: (p: AgentTerminalCompleted) => void,
): Promise<UnlistenFn> {
  return tauriListen<AgentTerminalCompleted>("agent_terminal_completed", (e) =>
    handler(e.payload),
  );
}

// ===== env.toml =====
export async function getEnvFile(): Promise<EnvFile> {
  return invoke<EnvFile>("get_env_file");
}

export async function saveEnvFile(file: EnvFile): Promise<void> {
  return invoke("save_env_file", { file });
}

// ===== keybindings.toml =====
export async function getKeybindings(): Promise<KeybindingsFile> {
  return invoke<KeybindingsFile>("get_keybindings");
}

export async function saveKeybindings(file: KeybindingsFile): Promise<void> {
  return invoke("save_keybindings", { file });
}

/** 重置所有快捷键到默认值. 删 keybindings.toml + emit keybindings_changed. */
export async function resetKeybindings(): Promise<KeybindingsFile> {
  return invoke<KeybindingsFile>("reset_keybindings");
}

/** 重置所有 prompts 到默认值. 删 prompts.toml + emit prompts_changed. */
export async function resetPrompts(): Promise<PromptsFile> {
  return invoke<PromptsFile>("reset_prompts");
}

export interface DetectAgentResult {
  agent_kind: string | null;
  shell_pid: number | null;
  pgid: number | null;
  cmdlines: string[];
  note: string;
}

/**
 * 立即对指定 terminal 做 agent 嗅探, 不等 3s 后台轮询.
 * PromptPicker 弹出时调一次, 用焦点所在终端的实时 agent_kind 决定 picker 类.
 * 返回完整诊断 — agent_kind=null 时, cmdlines / note 能告诉你为什么没识别.
 */
export async function detectAgentForTerminal(terminalId: number): Promise<DetectAgentResult> {
  return invoke<DetectAgentResult>("detect_agent_for_terminal", { terminalId });
}

export function onKeybindingsChanged(handler: () => void): Promise<UnlistenFn> {
  return tauriListen("keybindings_changed", () => handler());
}

export function onEnvChanged(handler: () => void): Promise<UnlistenFn> {
  return tauriListen("env_changed", () => handler());
}

// ===== prompts.toml =====
export async function getPrompts(): Promise<PromptsFile> {
  return invoke<PromptsFile>("get_prompts");
}

export async function savePrompts(file: PromptsFile): Promise<void> {
  return invoke("save_prompts", { file });
}

export function onPromptsChanged(handler: () => void): Promise<UnlistenFn> {
  return tauriListen("prompts_changed", () => handler());
}

// ===== Custom Actions =====
export async function getActions(): Promise<ActionsFile> {
  return invoke<ActionsFile>("get_actions");
}

export async function saveActions(file: ActionsFile): Promise<void> {
  return invoke("save_actions", { file });
}

export async function executeAction(
  actionId: string,
  terminalId: TerminalId | null,
): Promise<ExecuteActionResult> {
  return invoke<ExecuteActionResult>("execute_action", { actionId, terminalId });
}

// 启动时恢复上次活跃 task
export async function getActiveTask(): Promise<TaskId | null> {
  return invoke<TaskId | null>("get_active_task");
}

export function onActionsChanged(handler: () => void): Promise<UnlistenFn> {
  return tauriListen("actions_changed", () => handler());
}

// ===== AI CLI 检测 =====
export async function detectAiClis(): Promise<CliStatus[]> {
  return invoke<CliStatus[]>("detect_ai_clis");
}

// ===== 打开外部 URL / 文件 =====
// 后端白名单:https / localhost / 已存在的本地路径(file:// 不放行——终端里的链接可伪造)
export async function openExternal(target: string): Promise<void> {
  return invoke("open_external", { target });
}

// ===== Git Worktree (L1) =====
export async function gitIsRepo(path: string): Promise<boolean> {
  return invoke<boolean>("git_is_repo", { path });
}

export async function gitRepoRoot(path: string): Promise<string> {
  return invoke<string>("git_repo_root", { path });
}

export async function gitListBranches(repoPath: string): Promise<string[]> {
  return invoke<string[]>("git_list_branches", { repoPath });
}

export async function gitAddWorktree(
  repoPath: string,
  newPath: string,
  spec: BranchSpec,
): Promise<WorktreeRef> {
  return invoke<WorktreeRef>("git_add_worktree", { repoPath, newPath, spec });
}

export async function gitRemoveWorktree(
  repoPath: string,
  worktreePath: string,
  force: boolean,
): Promise<void> {
  return invoke("git_remove_worktree", { repoPath, worktreePath, force });
}

export async function setMenuLang(lang: string): Promise<void> {
  return invoke("set_menu_lang", { lang });
}

/** 调试 — 写到 /tmp/vibeterm-tasklist-debug.log 方便从主机读 */
export function debugLog(msg: string): void {
  invoke("debug_log", { msg }).catch(() => {});
  // 同时打 console,方便 DevTools 看;仅 dev 构建保留,生产构建静态剔除
  if ((import.meta as { env?: { DEV?: boolean } }).env?.DEV) {
    // eslint-disable-next-line no-console
    console.debug(msg);
  }
}

export { Channel } from "@tauri-apps/api/core";
