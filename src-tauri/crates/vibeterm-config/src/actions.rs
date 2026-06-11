//! actions.toml — Custom Actions(A4)
//!
//! 一键运行命令的可绑快捷键动作。与 prompts(AI 模板插入)语义分离:
//!   - prompts:`//` 触发 → 选模板 → 把内容插入当前终端,用户编辑后回车
//!   - actions:绑快捷键 / palette 一键执行 → 直接把命令送到终端(或新开 split / task)
//!
//! Schema:
//! ```toml
//! schema_version = 1
//!
//! [[actions]]
//! id = "review-diff"
//! title = "Review diff with Claude"
//! icon = "sparkles"                   # 可选,lucide 图标名
//! command = "claude -p 'review this diff'"
//! mode = "current_terminal"           # current_terminal | new_task | insert
//! shortcut = "Mod+Shift+R"            # 可选;Web 端按下时调 execute_action
//! close_on_success = false            # 仅 new_task 模式可用(暂未实现)
//! ```

use serde::{Deserialize, Serialize};

/// 执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ActionMode {
    /// 把 command 当作输入送到当前终端(自动追加 \n)
    CurrentTerminal,
    /// 新建一个 task 跑 command(继承当前 task 的 worktree?暂不;走 $HOME)
    NewTask,
    /// 仅插入 command 文本(不附加 \n,等同 prompt 插入)
    Insert,
}

fn default_mode() -> ActionMode {
    ActionMode::CurrentTerminal
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ActionEntry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub command: String,
    #[serde(default = "default_mode")]
    pub mode: ActionMode,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub close_on_success: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct ActionsFile {
    #[serde(default = "default_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub actions: Vec<ActionEntry>,
}

fn default_version() -> u32 {
    1
}

impl ActionsFile {
    pub fn load() -> Self {
        let p = match super::actions_toml_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!(err = %e, "actions parse failed, fallback empty");
                Self::default()
            }),
            Err(e) => {
                tracing::warn!(err = %e, path = ?p, "actions read failed, using default");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), super::ConfigError> {
        let p = super::actions_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        super::atomic_write(&p, s.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_roundtrip() {
        let f = ActionsFile {
            schema_version: 1,
            actions: vec![
                ActionEntry {
                    id: "a1".into(),
                    title: "Run tests".into(),
                    icon: Some("flask".into()),
                    command: "npm test".into(),
                    mode: ActionMode::CurrentTerminal,
                    shortcut: Some("Mod+T".into()),
                    close_on_success: false,
                },
                ActionEntry {
                    id: "a2".into(),
                    title: "Claude review".into(),
                    icon: None,
                    command: "claude -p 'review'".into(),
                    mode: ActionMode::NewTask,
                    shortcut: None,
                    close_on_success: true,
                },
            ],
        };
        let s = toml::to_string_pretty(&f).unwrap();
        let back: ActionsFile = toml::from_str(&s).unwrap();
        assert_eq!(back.actions.len(), 2);
        assert_eq!(back.actions[0].mode, ActionMode::CurrentTerminal);
        assert_eq!(back.actions[1].mode, ActionMode::NewTask);
    }

    #[test]
    fn mode_default_is_current_terminal() {
        let s = r#"
            schema_version = 1
            [[actions]]
            id = "x"
            title = "X"
            command = "ls"
        "#;
        let f: ActionsFile = toml::from_str(s).unwrap();
        assert_eq!(f.actions[0].mode, ActionMode::CurrentTerminal);
        assert!(!f.actions[0].close_on_success);
    }
}
