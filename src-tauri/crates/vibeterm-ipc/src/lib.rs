//! IPC schema
//!
//! 本 crate 只定义跨 Rust/Web 的数据结构与统一错误类型,不依赖任何业务 crate。
//! Web 侧(packages/ipc-types)对应类型当前**首期手写**,**未来用 specta 自动生成**。

use serde::{Deserialize, Serialize};

// ---- IDs ----
pub type TerminalId = u32;
pub type TaskId = u32;
pub type WindowId = String;

// ---- Tasks ----
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDto {
    pub id: TaskId,
    pub name: String,
    pub cwd: Option<String>,
    pub pinned: bool,
    pub status: TaskStatus,
    pub terminal_ids: Vec<TerminalId>,
    /// 任务在哪显示("main" / "floating-<label>" / "nowhere")
    pub location: TaskLocation,
    /// 任务的分屏布局,后端为 source of truth,主 + 浮窗都读它。
    /// 老 tasks.json 没此字段时 default singleLeaf(0)。
    #[serde(default = "default_split_tree")]
    pub split_tree: SplitNode,
    /// Git worktree 挂载(可选,L1)。挂载后 PTY cwd = worktree_path。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeRef>,
    /// 识别到的 agent(从前台进程扫出来,周期刷新)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    /// 终端最新输出末行(任务名下显示一行 Prowl 风格状态)。
    /// 不持久化到 tasks.json,emit_tasks_changed 时实时填充。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output: Option<String>,
    /// 通知静音. true 时该 task 不弹系统通知 (持久化到 tasks.json).
    #[serde(default)]
    pub notify_muted: bool,
    /// hook: agent 当前 permission mode (claude/codex hook 携带).
    /// "default" | "acceptEdits" | "plan" | "dontAsk" | "bypassPermissions". None = 未知.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// agent 当前 reasoning effort 等级 (low/medium/high/xhigh/max). None = 未知.
    /// 来源:嗅探 claude 工作动画 "thinking with <effort> effort"(零侵入)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// 任务挂载的 git worktree 信息(L1)。
/// `head/is_dirty/ahead/behind/status_updated_at` 由后台轮询刷新。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeRef {
    /// 主仓库 toplevel(用于 `git worktree` 命令的 cwd)
    pub repo_path: String,
    /// worktree 实际路径(也是 PTY cwd)
    pub worktree_path: String,
    /// 当前分支(detached 时为 None)
    pub branch: Option<String>,
    /// HEAD commit sha(可能为空,首次未刷新时)
    #[serde(default)]
    pub head: String,
    #[serde(default)]
    pub is_dirty: bool,
    #[serde(default)]
    pub ahead: u32,
    #[serde(default)]
    pub behind: u32,
    /// 上次状态刷新的 unix ms(0 = 从未刷新)
    #[serde(default)]
    pub status_updated_at: u64,
}

fn default_split_tree() -> SplitNode {
    SplitNode::Leaf { slot_id: 0 }
}

/// 分屏树 mirror(前端 ui-core/src/split TS 类型 1:1 对应)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SplitNode {
    Leaf {
        slot_id: u32,
    },
    Split {
        orientation: Orientation,
        children: Vec<SplitNode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ratios: Option<Vec<f64>>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    H,
    V,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Idle,
    Running,
    WaitingInput,
    /// agent/命令"完成但用户未看"。语义层:
    /// 当所有终端归于 idle 且该 task 不是当前 active main 时,聚合状态 = Done。
    /// 用户切到该 task 后转回 Idle。
    Done,
    /// agent 在 Running 状态超过 stall_threshold_ms (默认 5 分钟) 无任何输出,
    /// 且进程层识别为 agent_kind.is_some()(普通 shell 命令不打这个标)。
    /// 通常是 agent 卡死 / 网络挂了 / 等待用户但 prompt 没识别出来.
    /// 触发系统通知,用户可点回看。
    Stalled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "label")]
pub enum TaskLocation {
    Nowhere,
    MainWorkspace,
    Floating(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskOpts {
    pub name: String,
    pub cwd: Option<String>,
    /// 同时挂一个 worktree(可选)。携带时 task.cwd 会被覆盖为 worktree_path。
    #[serde(default)]
    pub worktree: Option<WorktreeRef>,
}

/// `git worktree add` 的分支策略(IPC 层 mirror,与 vibeterm-git::BranchSpec 对齐)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BranchSpecDto {
    Existing { branch: String },
    NewFromHead { branch: String },
    NewFromRef { branch: String, start_point: String },
}

// ---- Spawn ----
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPtyOpts {
    pub rows: u16,
    pub cols: u16,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<Vec<(String, String)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPtyResult {
    pub terminal_id: TerminalId,
}

// ---- Statistics 等(暂不用,留 schema) ----

// ---- 统一错误模型 ----
#[derive(Debug, thiserror::Error, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", content = "detail")]
pub enum IpcError {
    #[error("not found: {resource} ({id})")]
    NotFound { resource: String, id: String },

    #[error("permission denied: {reason}")]
    PermissionDenied { reason: String },

    #[error("pty spawn failed: {reason}")]
    PtySpawnFailed { reason: String },

    #[error("config invalid: {path}:{line} — {message}")]
    ConfigInvalid {
        path: String,
        line: u32,
        message: String,
    },

    /// panic 在 IPC handler 内被捕获 / 未分类错误。trace_id 关联日志。
    #[error("internal error (trace_id={trace_id})")]
    Unknown { trace_id: String },
}

/// IPC Result 别名 — 所有 invoke 命令应返回 `IpcResult<T>`。
pub type IpcResult<T> = Result<T, IpcError>;
