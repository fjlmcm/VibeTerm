//! prompts.toml — 用户的 prompt 模板
//!
//! 结构:
//!   prompts = [
//!     { id = "review", name = "Code review", content = "Review this code:\n{{cursor}}\n" },
//!     ...
//!   ]
//!
//! 模板变量:`{{cursor}}` 标记光标停留位置(Web 端插入后定位光标)

use serde::{Deserialize, Serialize};

/// 区分两类 prompt
///   - Agent: 给 LLM agent (claude / codex / aider 等) 的提问片段, 自然语言
///   - Terminal: 给 shell (zsh / bash) 的命令片段, 通常含 cursor 占位让用户调参
///
/// Picker 根据当前 task 的 agent_kind 自动过滤显示哪一类.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PromptKind {
    #[default]
    Agent,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PromptEntry {
    pub id: String,
    /// 显示名. 内置预设的 i18n key 是 `prompts.preset.<id>.name`, 前端先查 i18n,
    /// 找不到再 fallback 到此 name. 用户自定义的 prompt 不会撞 i18n key, 直接用 name.
    pub name: String,
    pub content: String,
    /// 旧 prompts.toml 没此字段时按 "agent" 处理 (历史 prompt 都是给 agent 的).
    #[serde(default)]
    pub kind: PromptKind,
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PromptsFile {
    #[serde(default = "default_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub prompts: Vec<PromptEntry>,
}

impl Default for PromptsFile {
    /// 首次启动时默认装两类示例 prompt — 来自真实高频证据 (调研报告基于
    /// awesome-claude-code 36.8k★ / Copilot Chat docs / Aider docs / oh-my-zsh /
    /// JetBrains DevEcosystem 2025 等 40+ 源).
    ///
    /// 用户改 prompts.toml 后这个 default 不再生效 (用户的文件完全替换).
    ///
    /// Agent: 内容英文 (LLM 兼容性最好), name 三语 " · " 分隔 (中/英/日 一行显示).
    /// Terminal: AI CLI 一键启动 + git 高频 + 端口/进程 + 搜索 + tail.
    fn default() -> Self {
        // 内置预设 — name 字段填 id (i18n fallback);前端按 lang 查 i18n key
        // `prompts.preset.<id>.name`, 找不到就显示 id. content 是给 LLM / shell
        // 的实际文本, 不走 i18n.
        let p = |id: &str, content: &str, kind: PromptKind| PromptEntry {
            id: id.into(),
            name: id.into(),
            content: content.into(),
            kind,
            shortcut: None,
        };
        let agent = |id, content| p(id, content, PromptKind::Agent);
        let term = |id, content| p(id, content, PromptKind::Terminal);
        Self {
            schema_version: 1,
            prompts: vec![
                // ===== Agent (前端按 lang 翻译 id → 显示名) =====
                agent("explain", "Explain this code, focusing on the non-obvious parts:\n{{cursor}}"),
                agent("fix-error", "This throws an error. Diagnose root cause and fix.\n\nERROR:\n{{cursor}}\n\nCODE:\n"),
                agent("tests", "Write focused unit tests covering edge cases. Use the project's existing framework:\n{{cursor}}"),
                agent("review", "Review this code for bugs, performance issues, and idiom violations. Be specific:\n{{cursor}}"),
                agent("refactor", "Refactor for clarity and maintainability. Keep behavior identical, no scope creep:\n{{cursor}}"),
                agent("commit-msg", "Write a concise conventional-commits message for the staged diff. Format: <type>: <subject> with optional body. Type one of feat/fix/refactor/docs/test/chore/perf/ci. Diff:\n{{cursor}}"),
                agent("create-pr", "Draft a pull request title (under 70 chars) and a body with ## Summary (3 bullets) and ## Test plan (checklist). Context:\n{{cursor}}"),
                agent("plan", "Plan the implementation step by step BEFORE coding. List: files to touch, risks, validation strategy. Don't write code yet.\n{{cursor}}"),
                agent("security", "Review for security issues: injection, secrets, unsafe input handling, auth/authz, OWASP Top 10. Be specific about each finding.\n{{cursor}}"),
                agent("docs", "Add concise doc comments explaining the WHY (not the WHAT). Skip obvious lines:\n{{cursor}}"),
                // ===== Terminal: AI CLI 一键 =====
                term("claude-yolo", "claude --dangerously-skip-permissions{{cursor}}"),
                term("codex-auto", "codex --sandbox workspace-write --ask-for-approval never{{cursor}}"),
                term("codex-yolo", "codex --dangerously-bypass-approvals-and-sandbox{{cursor}}"),
                term("aider-arch", "aider --architect{{cursor}}"),
                term("gemini", "gemini{{cursor}}"),
                // ===== Terminal: git =====
                term("git-status", "git status -sb{{cursor}}"),
                term("git-diff", "git diff{{cursor}}"),
                term("git-log", "git log --oneline -20{{cursor}}"),
                term("git-commit-all", "git add -A && git commit -m \"{{cursor}}\""),
                term("git-worktree", "git worktree add ../{{cursor}} -b {{cursor}}"),
                // ===== Terminal: 进程 / 端口 =====
                term("port-find", "lsof -i :{{cursor}}"),
                term("port-kill", "kill -9 $(lsof -ti:{{cursor}})"),
                term("ps-grep", "ps aux | grep {{cursor}}"),
                // ===== Terminal: 日志 / 搜索 =====
                term("tail-log", "tail -f {{cursor}}"),
                term("grep-rn", "grep -rn \"{{cursor}}\" ."),
                term("find-name", "find . -name \"*{{cursor}}*\""),
            ],
        }
    }
}

fn default_version() -> u32 {
    1
}

impl PromptsFile {
    pub fn load() -> Self {
        let p = match super::prompts_toml_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!(err = %e, "prompts parse failed, fallback empty");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), super::ConfigError> {
        let p = super::prompts_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        super::atomic_write(&p, s.as_bytes())
    }
}
