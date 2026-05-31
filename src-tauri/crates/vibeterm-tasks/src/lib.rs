//! 任务持久化
//!
//! 范围:
//!   - Task struct(id / name / cwd / pinned / terminal_ids)
//!   - 读写 tasks.json(atomic,通过 vibeterm-config::atomic_write)
//!   - 不做 5s 去抖;每次 mutate 立刻 save

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use vibeterm_ipc::{SplitNode, TerminalId, WorktreeRef};

// 单一来源:TaskId 由 vibeterm_ipc 定义,这里只 re-export 避免类型契约漂移。
pub use vibeterm_ipc::TaskId;

#[derive(thiserror::Error, Debug)]
pub enum TasksError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("config: {0}")]
    Config(#[from] vibeterm_config::ConfigError),
}

/// 任务的可持久化快照(不含终端进程,只含定义)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub name: String,
    pub cwd: Option<String>,
    pub pinned: bool,
    /// 持久化时记录"曾经存在过"的 terminal id;
    /// 重启时 PTY 不自动 rerun,此列表用作"上次有哪些终端"参考。
    pub last_terminal_ids: Vec<TerminalId>,
    /// 分屏布局后端为 source of truth;老 tasks.json 没此字段时默认 leaf(0)
    #[serde(default = "default_split_tree")]
    pub split_tree: SplitNode,
    /// Git worktree 挂载(L1)。可选,旧文件没此字段 = None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeRef>,
    /// 通知静音 — 设为 true 时 notify_status_transition 不弹该 task 的通知.
    /// 旧文件默认 false (不静音).
    #[serde(default)]
    pub notify_muted: bool,
    /// hook auto-naming: 任务名是否还能被 UserPromptSubmit hook 自动重命名.
    /// 新建 task 默认 true; 用户手动改名 / 自动改过 一次后 → false.
    /// 旧文件缺此字段默认 false (老 task 名已是用户手设).
    #[serde(default)]
    pub auto_namable: bool,
}

fn default_split_tree() -> SplitNode {
    SplitNode::Leaf { slot_id: 0 }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasksFile {
    pub schema_version: u32,
    pub next_task_id: TaskId,
    pub tasks: Vec<TaskSnapshot>,
    pub order: Vec<TaskId>,
    /// 上次激活的 task(启动时恢复)。None / 不存在的 id 时 fallback first。
    #[serde(default)]
    pub active_main: Option<TaskId>,
}

impl Default for TasksFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_task_id: 0,
            tasks: vec![],
            order: vec![],
            active_main: None,
        }
    }
}

fn path() -> Result<PathBuf, TasksError> {
    Ok(vibeterm_config::tasks_json_path()?)
}

pub fn load() -> Result<TasksFile, TasksError> {
    let p = path()?;
    if !p.exists() {
        return Ok(TasksFile::default());
    }
    let bytes = std::fs::read(&p)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save(file: &TasksFile) -> Result<(), TasksError> {
    let p = path()?;
    let bytes = serde_json::to_vec_pretty(file)?;
    vibeterm_config::atomic_write(&p, &bytes)?;
    Ok(())
}
