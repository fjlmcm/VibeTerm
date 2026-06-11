//! keybindings.toml — 用户自定义快捷键
//!
//! 结构:
//!   bindings = [
//!     { command = "new_task",        keys = "Mod+N" },
//!     { command = "command_palette", keys = "Mod+K" },
//!     ...
//!   ]
//!
//! 用户文件不存在时返回默认键位(default_bindings)
//! Web 端拉到后绑定全局快捷键监听

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct KeybindingEntry {
    pub command: String,
    pub keys: String,
    #[serde(default)]
    pub when: Option<String>, // 上下文限制
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct KeybindingsFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub bindings: Vec<KeybindingEntry>,
}

fn default_schema_version() -> u32 {
    1
}

impl KeybindingsFile {
    pub fn load() -> Self {
        let p = match super::keybindings_toml_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!(err = %e, "keybindings parse failed, fallback default");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), super::ConfigError> {
        let p = super::keybindings_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        super::atomic_write(&p, s.as_bytes())
    }
}

impl Default for KeybindingsFile {
    /// 默认键位
    fn default() -> Self {
        let bind = |cmd: &str, k: &str| KeybindingEntry {
            command: cmd.into(),
            keys: k.into(),
            when: None,
        };
        Self {
            schema_version: 1,
            bindings: vec![
                bind("command_palette", "Mod+K"),
                bind("new_task", "Mod+N"),
                bind("new_terminal", "Mod+T"),
                bind("close_terminal", "Mod+W"),
                bind("next_task", "Mod+]"),
                bind("prev_task", "Mod+["),
                bind("split_horizontal", "Mod+D"),
                bind("split_vertical", "Mod+Shift+D"),
                bind("close_split", "Mod+Shift+W"),
                bind("font_size_up", "Mod+="),
                bind("font_size_down", "Mod+-"),
                bind("font_size_reset", "Mod+0"),
                bind("find_in_terminal", "Mod+F"),
                bind("scroll_to_bottom", "Mod+End"),
                bind("open_settings", "Mod+,"),
                // prompt picker — 默认双击 Mod (macOS=Cmd / Win+Linux=Ctrl).
                // 不送字符流, agent TUI (codex/claude code/aider) 内都生效.
                // 即使 300ms 内连按两次误触发, 也有两层防御保证不会误干扰用户输入:
                //   1. highlighted 初始 -1, 没主动选 Enter 不 insert
                //   2. listener 注册时挂在 owner scope, 不会 leak 累积
                // 用户偏好改为 chord (Mod+Shift+P) 或换双击键 (DoubleTap+Shift) 可在设置改.
                bind("prompt_picker", "DoubleTap+Mod"),
            ],
        }
    }
}
