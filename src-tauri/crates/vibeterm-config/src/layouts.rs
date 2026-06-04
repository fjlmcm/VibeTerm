//! 布局模板(layouts.toml)—— 借鉴 cmux.json 的 `commands` 布局定义。
//!
//! 一个模板 = 一个任务预设:name + 可选 cwd + 一串 pane(每 pane 可带启动命令 + 相对上一个
//! pane 的分屏方向)。从命令面板一键创建带预设分屏 + 自动跑命令的任务。
//!
//! 🟢 零侵入:纯本地配置,落 VibeTerm 自己的 config 目录。不碰 agent 配置、不起 server。
//!
//! 简化模型(KISS):pane 列表是「链式」—— 第一个 pane 是根 leaf,后续每个 pane 把上一个
//! pane 按其 `split` 方向(h=右 / v=下)劈开。覆盖"N 个终端横排/竖排"的 80% 场景;
//! 复杂嵌套树暂不支持(YAGNI)。

use serde::{Deserialize, Serialize};

/// 一个 pane:可选启动命令 + 相对上一个 pane 的分屏方向 + 可选 cwd。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutPane {
    /// 终端就绪后自动发送的命令(末尾自动补换行)。空 = 纯终端不发命令。
    #[serde(default)]
    pub command: Option<String>,
    /// 相对上一个 pane 的分屏方向:"h"(右)| "v"(下)。第一个 pane 忽略。默认 "h"。
    #[serde(default)]
    pub split: Option<String>,
    /// 该 pane 的工作目录(相对 / 绝对)。给定时命令前缀 `cd <cwd> &&`。
    #[serde(default)]
    pub cwd: Option<String>,
}

/// 一个布局模板。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutTemplate {
    pub name: String,
    /// 命令面板模糊搜索关键词。
    #[serde(default)]
    pub keywords: Vec<String>,
    /// 任务工作目录(留空 = 默认 ~)。
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub panes: Vec<LayoutPane>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutsFile {
    #[serde(default)]
    pub layouts: Vec<LayoutTemplate>,
}

impl LayoutsFile {
    /// 读 layouts.toml;不存在 / 解析失败 → 空(降级,不报错)。每次调用读盘,编辑即时生效。
    pub fn load() -> Self {
        let p = match super::layouts_toml_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!(err = %e, "layouts parse failed, fallback empty");
                Self::default()
            }),
            Err(e) => {
                tracing::warn!(err = %e, path = ?p, "layouts read failed, using default");
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_layout_with_panes() {
        let s = r#"
[[layouts]]
name = "Dev"
keywords = ["dev", "server"]
cwd = "~/proj"

[[layouts.panes]]
command = "npm run dev"

[[layouts.panes]]
command = "npm test --watch"
split = "v"
"#;
        let f: LayoutsFile = toml::from_str(s).unwrap();
        assert_eq!(f.layouts.len(), 1);
        let l = &f.layouts[0];
        assert_eq!(l.name, "Dev");
        assert_eq!(l.panes.len(), 2);
        assert_eq!(l.panes[0].command.as_deref(), Some("npm run dev"));
        assert_eq!(l.panes[1].split.as_deref(), Some("v"));
    }

    #[test]
    fn empty_file_is_default() {
        let f: LayoutsFile = toml::from_str("").unwrap();
        assert!(f.layouts.is_empty());
    }
}
