//! 应用核心 — 状态机 / 终端注册表 / 任务注册表 / 协调器
//!
//! 本 crate 不依赖 Tauri(纯领域),可独立单元测试。

pub mod tasks;
pub mod terminals;

pub use tasks::{TaskError, TaskRegistry};
pub use terminals::{TerminalRegistry, TerminalRegistryError};
