//! statusline.toml — 状态栏自定义 (schema v2: 多 profile).
//!
//! 设计:
//!   - profiles: HashMap<String, ProfileConfig> — 按 agent_kind key (`default` / `claude` / `codex` / `aider` ...)
//!   - 运行时根据当前终端的 agent_kind 选 profile, 没匹配的 fallback 到 `default`
//!   - 用户可 add custom profile 给其他 agent / 程序
//!
//! v1 (旧 `items` 字段) 自动迁移成 v2 default profile.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 一个状态栏 widget 项 (同 v1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum StatusLineItem {
    Bare(String),
    Detailed(StatusLineItemDetail),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusLineItemDetail {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide: Option<bool>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl StatusLineItem {
    pub fn kind(&self) -> &str {
        match self {
            Self::Bare(s) => s,
            Self::Detailed(d) => &d.kind,
        }
    }
}

/// 一个 profile = 一个终端模式的状态栏.
/// key 在 file 层是 HashMap 的 key (例如 `default` / `claude` / `codex`),
/// display_name 是 UI 显示用的中文标签 (可选).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileConfig {
    /// UI 显示名 (例如 "终端" / "Claude" / "Codex"). 缺省用 key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// widget 列表, 数组顺序 = 显示顺序
    #[serde(default)]
    pub items: Vec<StatusLineItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusLineFile {
    #[serde(default = "default_version")]
    pub schema_version: u32,
    /// 主题色开关
    #[serde(default = "default_true")]
    pub use_theme_colors: bool,
    /// profile 映射. key 跟 agent_kind 对齐 (`default` 用作未识别 agent 的 fallback).
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,
    /// v1 旧字段, 迁移用. 新写不会输出.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<StatusLineItem>>,
}

fn default_version() -> u32 {
    2
}
fn default_true() -> bool {
    true
}

impl Default for StatusLineFile {
    /// 默认布局 — 已根据真实使用打磨简化 (= 作者实际在用的配置):
    ///   - default profile: cwd + worktree + branch + stash + PR (5 项, 极简)
    ///   - claude profile: plan/model/effort/ctx/短窗/长窗 (6 项核心 quota)
    ///   - codex profile: plan/model/effort/ctx/短窗/长窗 (6 项, 多 effort 维度)
    ///   - 不放 burn-rate / flex-separator: 视觉噪声大, 用户用不上
    fn default() -> Self {
        let bare = |s: &str| StatusLineItem::Bare(s.into());
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".into(),
            ProfileConfig {
                display_name: Some("终端".into()),
                items: vec![
                    bare("current-dir"),
                    bare("worktree-name"),
                    bare("git-branch"),
                    bare("git-stash-count"),
                    bare("pr-status"),
                ],
            },
        );
        profiles.insert(
            "claude".into(),
            ProfileConfig {
                display_name: Some("Claude".into()),
                items: vec![
                    bare("current-dir"),
                    bare("git-branch"),
                    bare("separator"),
                    bare("claude-plan"),
                    bare("claude-model"),
                    bare("claude-effort"),
                    bare("claude-ctx"),
                    bare("claude-5h"),
                    bare("claude-7d"),
                ],
            },
        );
        profiles.insert(
            "codex".into(),
            ProfileConfig {
                display_name: Some("Codex".into()),
                items: vec![
                    bare("current-dir"),
                    bare("git-branch"),
                    bare("separator"),
                    bare("codex-plan"),
                    bare("codex-model"),
                    bare("codex-effort"),
                    bare("codex-ctx"),
                    bare("codex-5h"),
                    bare("codex-7d"),
                ],
            },
        );
        Self {
            schema_version: 2,
            use_theme_colors: true,
            profiles,
            items: None,
        }
    }
}

impl StatusLineFile {
    pub fn load() -> Self {
        let p = match super::statusline_toml_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                let mut parsed: Self = toml::from_str(&s).unwrap_or_else(|e| {
                    tracing::warn!(err = %e, "statusline parse failed, fallback default");
                    Self::default()
                });
                // v1 迁移: 旧 items 字段 → default profile
                if parsed.schema_version < 2 || parsed.profiles.is_empty() {
                    if let Some(legacy) = parsed.items.take() {
                        let mut profiles = HashMap::new();
                        profiles.insert(
                            "default".into(),
                            ProfileConfig {
                                display_name: Some("终端".into()),
                                items: legacy,
                            },
                        );
                        // 补 claude / codex 用默认
                        let defaults = Self::default();
                        for (k, v) in defaults.profiles {
                            profiles.entry(k).or_insert(v);
                        }
                        parsed.profiles = profiles;
                    } else if parsed.profiles.is_empty() {
                        parsed.profiles = Self::default().profiles;
                    }
                    parsed.schema_version = 2;
                    parsed.items = None;
                    // 保存迁移结果
                    if let Err(e) = parsed.save() {
                        tracing::warn!(err = %e, "statusline v1->v2 migration save failed");
                    }
                }
                parsed
            }
            Err(e) => {
                tracing::warn!(err = %e, path = ?p, "statusline read failed, using default");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), super::ConfigError> {
        let p = super::statusline_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        super::atomic_write(&p, s.as_bytes())
    }

    /// 根据 agent_kind 选 profile. 无匹配返回 default. profiles 全空时返回内置 default profile.
    pub fn profile_for<'a>(&'a self, agent_kind: Option<&str>) -> &'a ProfileConfig {
        let key = agent_kind.unwrap_or("default");
        self.profiles
            .get(key)
            .or_else(|| self.profiles.get("default"))
            .unwrap_or(EMPTY_PROFILE)
    }
}

const EMPTY_PROFILE: &ProfileConfig = &ProfileConfig {
    display_name: None,
    items: Vec::new(),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_three_profiles() {
        let d = StatusLineFile::default();
        assert!(d.profiles.contains_key("default"));
        assert!(d.profiles.contains_key("claude"));
        assert!(d.profiles.contains_key("codex"));
    }

    #[test]
    fn profile_for_claude_picks_claude_profile() {
        let d = StatusLineFile::default();
        let p = d.profile_for(Some("claude"));
        assert!(p.items.iter().any(|i| i.kind() == "claude-model"));
        // 默认布局已含 effort (= 作者实际在用的配置, 见 Default::default)
        assert!(p.items.iter().any(|i| i.kind() == "claude-effort"));
    }

    #[test]
    fn profile_for_unknown_falls_back_to_default() {
        let d = StatusLineFile::default();
        let p = d.profile_for(Some("some-future-agent"));
        // default profile 没 claude widget
        assert!(!p.items.iter().any(|i| i.kind() == "claude-model"));
        assert!(p.items.iter().any(|i| i.kind() == "current-dir"));
    }

    #[test]
    fn v1_migration() {
        let v1_toml = r#"
schema_version = 1
items = ["current-dir", "git-branch", "claude-ctx"]
"#;
        let parsed: StatusLineFile = toml::from_str(v1_toml).unwrap();
        // 解析后还没自动迁移 (load 才会迁), 这里只检查兼容
        assert_eq!(parsed.schema_version, 1);
        assert!(parsed.items.is_some());
        assert_eq!(parsed.items.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn detailed_item_roundtrip() {
        let raw = r##"
schema_version = 2

[profiles.default]
display_name = "终端"
items = [
  { type = "claude-ctx", color = "#f5a623", metadata = { showReset = "false" } },
]"##;
        let parsed: StatusLineFile = toml::from_str(raw).unwrap();
        let p = parsed.profile_for(None);
        assert_eq!(p.items[0].kind(), "claude-ctx");
        if let StatusLineItem::Detailed(d) = &p.items[0] {
            assert_eq!(d.color.as_deref(), Some("#f5a623"));
            assert_eq!(
                d.metadata.get("showReset").map(|s| s.as_str()),
                Some("false")
            );
        } else {
            panic!("expected detailed");
        }
    }
}
