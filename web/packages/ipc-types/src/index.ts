// IPC schema mirror — 与 src-tauri/crates/vibeterm-ipc 对应

// ---- IDs ----
export type TerminalId = number;
export type TaskId = number;
export type WindowId = string;

// ---- Spawn ----
export interface SpawnPtyOpts {
  rows: number;
  cols: number;
  cwd?: string | null;
  command?: string | null;
  args?: string[] | null;
  env?: [string, string][] | null;
}

export interface SpawnPtyResult {
  terminal_id: TerminalId;
}

// ---- Tasks ----
export type TaskStatus = "idle" | "running" | "waiting_input" | "done" | "stalled";

export type TaskLocation =
  | { kind: "Nowhere" }
  | { kind: "MainWorkspace" }
  | { kind: "Floating"; label: string };

/** 任务的分屏布局,后端 source of truth(主 + 浮窗都从此读写) */
export type SplitTreeNode =
  | { kind: "leaf"; slot_id: number }
  | { kind: "split"; orientation: "h" | "v"; children: SplitTreeNode[]; ratios?: number[] };

export interface TaskDto {
  id: TaskId;
  name: string;
  cwd: string | null;
  pinned: boolean;
  status: TaskStatus;
  terminal_ids: TerminalId[];
  location: TaskLocation;
  split_tree: SplitTreeNode;
  /** L1:挂载的 git worktree(可选)。挂载后 cwd === worktree.worktree_path */
  worktree?: WorktreeRef | null;
  /**
   * 进程层识别到的 agent(后端 3s 轮询刷新)。
   * Rust wire 类型为 Option<String>,但其值仅由 vibeterm-status::AgentKind::as_str()
   * 产出,值域与下方 AgentKind 联合一一对应;后端新增 agent 时须同步补齐 AgentKind。
   */
  agent_kind?: AgentKind | null;
  /** 终端最新输出末行(任务名下显示 Prowl 风格状态行);后端 750ms 节流推送 */
  last_output?: string | null;
  /** 通知静音(per-task);true 时该 task 不弹系统通知 */
  notify_muted?: boolean;
  /** agent 当前 permission mode (徽标用) */
  permission_mode?: PermissionMode | null;
  /** agent 当前 reasoning effort 等级 (low/medium/high/xhigh/max). 嗅探工作动画得到 */
  effort?: string | null;
}

/** hook:claude/codex 的 permission_mode 字段值 */
export type PermissionMode =
  | "default"
  | "acceptEdits"
  | "plan"
  | "dontAsk"
  | "bypassPermissions";

/** 自带声音库:一条预设 */
export interface BuiltinSound {
  /** 内部 id, 也是 sound 字段存的值 (NotifyPrefs.events.*.sound = "ding1" 等) */
  id: string;
  /** 显示名 */
  name: string;
  /** 分类 — "notification" | "tone" | "voice" | "ui" | "ringtone" | "other" */
  category: string;
  /** 资源文件名, 前端不用 */
  file: string;
}

/** agent 种类。与 vibeterm-status::AgentKind 一一对应。 */
export type AgentKind =
  | "pi" | "claude" | "codex" | "gemini" | "cursor"
  | "cline" | "opencode" | "copilot" | "kimi"
  | "droid" | "amp" | "aider";

export interface CreateTaskOpts {
  name: string;
  cwd: string | null;
  worktree?: WorktreeRef | null;
}

// ---- Git Worktree (L1) ----
export interface WorktreeRef {
  repo_path: string;
  worktree_path: string;
  branch: string | null;
  head: string;
  is_dirty: boolean;
  ahead: number;
  behind: number;
  /** unix ms;0 = 从未刷新 */
  status_updated_at: number;
}

/** `git worktree list --porcelain` 解析 */
export interface WorktreeEntry {
  path: string;
  head: string;
  /** 形如 `refs/heads/feature-x`;detached 时 null */
  branch: string | null;
  is_bare: boolean;
  is_detached: boolean;
  is_locked: boolean;
}

/** `git worktree add` 分支策略 */
export type BranchSpec =
  | { mode: "existing"; branch: string }
  | { mode: "new_from_head"; branch: string }
  | { mode: "new_from_ref"; branch: string; start_point: string };

// ---- Theme ----
export interface ThemeShell {
  background: string;
  surface: string;
  border: string;
  text_primary: string;
  text_secondary: string;
  accent: string;
  accent_subtle: string;
  status_running: string;
  status_waiting: string;
  status_idle: string;
}

export interface ThemeTerminal {
  background: string;
  foreground: string;
  cursor: string;
  selection_bg: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  bright_black: string;
  bright_red: string;
  bright_green: string;
  bright_yellow: string;
  bright_blue: string;
  bright_magenta: string;
  bright_cyan: string;
  bright_white: string;
}

export interface Theme {
  schema_version: number;
  id: string;
  name: string;
  appearance: "dark" | "light";
  author: string;
  shell: ThemeShell;
  terminal: ThemeTerminal;
}

export interface Config {
  schema_version: number;
  active_theme: string;
  follow_system_theme: boolean;
  language: string | null;
  /** zsh shell 集成自动注入(默认 true);下次开终端生效 */
  shell_integration: boolean;
}

// ---- Status events ----
// Rust 端 emit "task_status_changed",payload 为 { task_id, status }(见 main.rs)。
export interface TaskStatusChanged {
  task_id: TaskId;
  status: TaskStatus;
}

export interface TerminalExited {
  terminal_id: TerminalId;
  exit_code: number | null;
}

// ---- env.toml ----
export interface ProxySection {
  enabled: boolean;
  http: string | null;
  https: string | null;
  no_proxy: string | null;
}

// 粘贴图片落盘配置(全字段 nullable;Rust 侧整 section 也 nullable)
export interface ClipboardImagesSection {
  dir: string | null;
  max_count: number | null;
  max_mb: number | null;
}

export interface EnvFile {
  schema_version: number;
  env: Record<string, string>;
  proxy: ProxySection | null;
  clipboard_images: ClipboardImagesSection | null;
}

// ---- keybindings.toml ----
export interface KeybindingEntry {
  command: string;
  keys: string;
  when: string | null;
}

export interface KeybindingsFile {
  schema_version: number;
  bindings: KeybindingEntry[];
}

// ---- prompts.toml ----
/** 区分 agent prompt (给 LLM) vs terminal snippet (给 shell). */
export type PromptKind = "agent" | "terminal";

export interface PromptEntry {
  id: string;
  name: string;
  content: string;
  /** 旧 prompts.toml 没此字段时按 "agent" 处理. */
  kind?: PromptKind;
  shortcut?: string | null;
}

export interface PromptsFile {
  schema_version: number;
  prompts: PromptEntry[];
}

// ---- actions.toml ----
export type ActionMode = "current_terminal" | "new_task" | "insert";

export interface ActionEntry {
  id: string;
  title: string;
  icon: string | null;
  command: string;
  mode: ActionMode;
  shortcut: string | null;
  close_on_success: boolean;
}

export interface ActionsFile {
  schema_version: number;
  actions: ActionEntry[];
}

/** execute_action 返回:模式不同结果不同 */
export type ExecuteActionResult =
  | { kind: "written_to"; terminal_id: TerminalId }
  | { kind: "new_task"; task_id: TaskId; command: string };

// ---- Notify prefs — 镜像 vibeterm-config/src/notify_prefs.rs ----
export interface EventNotifyPrefs {
  enabled: boolean;
  /** 系统声音名 (macOS: Glass/Tink/Sosumi/Hero/Pop 或 ~/Library/Sounds/<name>.aiff);
   *  空字符串 / null → 平台默认 */
  sound: string | null;
}

export interface EventsPrefs {
  waiting_input: EventNotifyPrefs;
  done: EventNotifyPrefs;
}

export interface QuietHours {
  enabled: boolean;
  /** "HH:MM" 24h. start > end 表示跨夜 */
  start: string;
  end: string;
}

export interface NotifyFile {
  schema_version: number;
  /** 全局总开关;off 时所有通知都不弹 */
  enabled: boolean;
  events: EventsPrefs;
  quiet_hours: QuietHours;
}

/** 系统通知权限状态 — Tauri NotificationPermissionState 序列化为小写 string */
export type NotifyPermissionState = "granted" | "denied" | "default";

/** preview_notify_sound 返回:base64 编码的音频字节 + MIME */
export interface NotifySoundData {
  mime: string;
  /** 原始音频字节的 base64 */
  base64: string;
}

// ---- AI CLI 检测 ----
export interface CliStatus {
  name: string;
  installed: boolean;
  path: string | null;
}

// ---- Errors ----
export type IpcError =
  | { kind: "NotFound"; detail: { resource: string; id: string } }
  | { kind: "PermissionDenied"; detail: { reason: string } }
  | { kind: "PtySpawnFailed"; detail: { reason: string } }
  | { kind: "ConfigInvalid"; detail: { path: string; line: number; message: string } }
  | { kind: "Unknown"; detail: { trace_id: string } };

// ---- Agent watch (v1) ----

export interface ClaudeQuotaWindow {
  utilization: number;          // 0..=100, 服务端权威已用百分比
  resets_at: string | null;     // ISO8601 (UTC) 或 null
}

export interface ClaudeExtraUsage {
  is_enabled: boolean;
  monthly_limit: number | null;
  used_credits: number | null;
  utilization: number | null;
  currency: string | null;
  disabled_reason: string | null;
}

export interface ClaudeUsageCache {
  five_hour: ClaudeQuotaWindow | null;
  seven_day: ClaudeQuotaWindow | null;
  seven_day_sonnet: ClaudeQuotaWindow | null;
  seven_day_opus: ClaudeQuotaWindow | null;
  seven_day_oauth_apps: ClaudeQuotaWindow | null;
  extra_usage: ClaudeExtraUsage | null;
}

// ===== 设置·更新页:软件版本检查 + 模型价格(手动, 仅点按钮时联网)=====

/** 软件更新检查结果(仅展示 + 给 release 下载链接, 不下载安装)。 */
export interface AppUpdateInfo {
  current: string;
  latest: string | null;
  has_update: boolean;
  release_url: string | null;
  notes: string | null;
  published_at: string | null;
}

/** 当前模型价格来源状态。source: "builtin"(内置快照) | "override"(已手动更新)。 */
export interface PricingStatus {
  source: string;
  updated_at: string | null;
  origin: string | null;
}

export interface ClaudeActiveBlock {
  start_at_ms: number;
  end_at_ms: number;
  last_entry_at_ms: number;
  tokens_used: number;
  elapsed_ms: number;
  remaining_ms: number;
  elapsed_pct: number;
  tokens_per_min_avg: number;
  tokens_per_min_recent: number;
  burn_rate_level: "normal" | "moderate" | "high";
  cost_usd: number | null;
}

export interface ClaudeSession {
  session_id: string;
  project_path: string;
  model: string | null;
  context_tokens: number | null;
  context_window: number | null;
  session_cost_usd: number | null;
  /** Prompt cache 5min TTL 到期时刻 (unix ms). null = 没用过 5m cache */
  cache_5m_until_ms: number | null;
  /** Prompt cache 1h TTL 到期时刻 (unix ms). null = 没用过 1h cache */
  cache_1h_until_ms: number | null;
  /** 最新 hook 回传携带的 reasoning effort 等级 (low/medium/high/xhigh/max). null = 未取到 */
  effort: string | null;
}

export interface CodexRateLimit {
  used_percent: number;
  /** 窗口长度(分钟). free 计划/新模型可能为 null */
  window_minutes: number | null;
  /** unix 秒. 可能为 null */
  resets_at: number | null;
}

// ---- Usage stats (统计面板, get_usage_stats) ----

export interface UsageTotals {
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  /** Claude 总 token */
  claude_tokens: number;
  /** Codex 总 token */
  codex_tokens: number;
  /** Claude 估算总成本 (USD). 无可定价条目则 null */
  cost_usd: number | null;
  /** 未匹配定价表的 Claude 条目数 — UI 提示"含 N 条未计价" */
  cost_unknown_entries: number;
  /** 去重后计入的 Claude 消息数 */
  message_count: number;
}

export interface UsageDailyStat {
  /** 本地时区 YYYY-MM-DD */
  date: string;
  claude_tokens: number;
  codex_tokens: number;
  cost_usd: number | null;
}

export interface UsageModelStat {
  model: string;
  total_tokens: number;
  /** 无定价 (Codex / 未知模型) 则 null */
  cost_usd: number | null;
  message_count: number;
}

export interface UsageProjectStat {
  project_path: string;
  total_tokens: number;
  cost_usd: number | null;
  message_count: number;
}

export interface UsageStats {
  range_days: number;
  generated_at_ms: number;
  totals: UsageTotals;
  /** 按本地日期升序 */
  daily: UsageDailyStat[];
  /** 按 token 降序 */
  by_model: UsageModelStat[];
  /** 按 token 降序 */
  by_project: UsageProjectStat[];
}

// ---- Status line config (statusline.toml) ----

export interface StatusLineItemDetail {
  type: string;
  color?: string;
  bold?: boolean;
  max_width?: number;
  hide?: boolean;
  metadata?: Record<string, string>;
}

/** Item 可以是字符串简写 ("current-dir") 或对象 (`{ type, color, ... }`). */
export type StatusLineItem = string | StatusLineItemDetail;

/** 一个 profile = 某个终端模式的状态栏配置. key 跟 agent_kind 对齐. */
export interface ProfileConfig {
  display_name?: string;
  items: StatusLineItem[];
}

export interface StatusLineFile {
  schema_version: number;
  use_theme_colors: boolean;
  /** profiles 映射. key 跟 agent_kind 对齐 (`default` 为 fallback). */
  profiles: Record<string, ProfileConfig>;
  /** v1 兼容字段, 新 schema 不出现. */
  items?: StatusLineItem[];
}

export function statusLineItemKind(item: StatusLineItem): string {
  return typeof item === "string" ? item : item.type;
}

export function statusLineItemDetail(item: StatusLineItem): StatusLineItemDetail {
  return typeof item === "string" ? { type: item } : item;
}

export interface GitStatusBrief {
  branch: string | null;
  head: string;
  is_dirty: boolean;
  ahead: number;
  behind: number;
  staged?: number;
  unstaged?: number;
  untracked?: number;
}

export interface CodexSnapshot {
  session_id: string;
  cwd: string;
  model: string | null;
  model_provider: string | null;
  cli_version: string | null;
  /** 当前 turn 的 total_tokens (跟 Codex CLI 一致) */
  context_tokens: number | null;
  context_window: number | null;
  /**
   * Context 占用百分比 — 按 Codex CLI 算法 (扣 BASELINE_TOKENS=12000).
   * 前端 codex-ctx widget 用这个,不要自己 tokens/window,否则跟 Codex CLI 显示对不上.
   */
  context_used_pct: number | null;
  primary_limit: CodexRateLimit | null;
  secondary_limit: CodexRateLimit | null;
  plan_type: string | null;
  updated_at_ms: number;
  tokens_per_min_recent: number;
  burn_rate_level: "normal" | "moderate" | "high";
  effort: string | null;
}

export function ipcErrorMessage(e: IpcError): string {
  switch (e.kind) {
    case "NotFound":
      return `not found: ${e.detail.resource} (${e.detail.id})`;
    case "PermissionDenied":
      return `permission denied: ${e.detail.reason}`;
    case "PtySpawnFailed":
      return `pty spawn failed: ${e.detail.reason}`;
    case "ConfigInvalid":
      return `config invalid: ${e.detail.path}:${e.detail.line} — ${e.detail.message}`;
    case "Unknown":
      return `internal error (trace_id=${e.detail.trace_id})`;
  }
}
