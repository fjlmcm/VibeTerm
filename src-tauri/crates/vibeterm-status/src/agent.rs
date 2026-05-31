//! 进程层 agent 识别(借鉴 Prowl 的 AgentClassifier)
//!
//! 输入:shell 子进程 pid。
//! 流程:
//!   1. 枚举 pid 子孙进程(unix:`ps -o pid,command --ppid <pgid>`;先 `ps -o pgid= -p <pid>` 取 pgid)
//!   2. 取每行 command 第一个 token,小写归一
//!   3. 对照 AGENT_NAMES 表识别 → 返回 AgentKind
//!
//! 跨 chunk 性能:这函数被 status crate 周期(~5s)调一次,不必 hot path。
//!
//! 当前简化版本:
//!   - 不缓存 — 每次 fresh 跑 ps(macOS 上 < 5ms,Linux 类似)
//!   - 不打分(Prowl 用打分应对 wrapped runtimes 如 npm-exec 启动 claude;
//!     这里先做基础识别,后续如有需要再加候选打分)
//!   - Windows 暂返 None(没装 ps;tasklist 解析复杂,后续再加)

use serde::{Deserialize, Serialize};

/// 11 种 agent — 与 Prowl 对齐
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Pi,
    Claude,
    Codex,
    Gemini,
    Cursor,
    Cline,
    OpenCode,
    Copilot,
    Kimi,
    Droid,
    Amp,
    Aider, // 本项目已有的(Prowl 没列入但 status crate 已有规则)
}

impl AgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::Pi => "pi",
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Gemini => "gemini",
            AgentKind::Cursor => "cursor",
            AgentKind::Cline => "cline",
            AgentKind::OpenCode => "opencode",
            AgentKind::Copilot => "copilot",
            AgentKind::Kimi => "kimi",
            AgentKind::Droid => "droid",
            AgentKind::Amp => "amp",
            AgentKind::Aider => "aider",
        }
    }
}

/// 进程名 → AgentKind(小写比对)
fn classify(name: &str) -> Option<AgentKind> {
    match name.to_lowercase().as_str() {
        "pi" => Some(AgentKind::Pi),
        "claude" | "claude-code" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        "gemini" => Some(AgentKind::Gemini),
        "cursor" | "cursor-agent" => Some(AgentKind::Cursor),
        "cline" => Some(AgentKind::Cline),
        "opencode" | "open-code" => Some(AgentKind::OpenCode),
        "copilot" | "github-copilot" | "ghcs" => Some(AgentKind::Copilot),
        "kimi" => Some(AgentKind::Kimi),
        "droid" => Some(AgentKind::Droid),
        "amp" | "amp-local" => Some(AgentKind::Amp),
        "aider" => Some(AgentKind::Aider),
        _ => None,
    }
}

/// Wrapped runtime — 这些 binary 本身不是 agent,但其参数往往包含真正的 agent 命令。
/// 如 `node /usr/local/bin/claude` 应识别为 claude。
fn is_wrapper(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "node" | "deno" | "bun" | "python" | "python3" | "ruby" | "npx" | "pnpx" | "yarn"
    )
}

/// 从命令行 token 提取候选 agent 名(去 path、去 .js 扩展、去 --flag)
fn token_to_name(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.starts_with('-') || trimmed.is_empty() {
        return None;
    }
    // 去 path 前缀
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    // 去常见后缀
    let base = last
        .strip_suffix(".js")
        .or_else(|| last.strip_suffix(".mjs"))
        .unwrap_or(last);
    if base.is_empty() {
        return None;
    }
    Some(base.to_string())
}

/// 给 shell pid,返回识别到的 agent;无前台命令或不在 agent 表中返回 None
pub fn detect_agent_for_shell(shell_pid: u32) -> Option<AgentKind> {
    detect_agent_with_diagnostics(shell_pid).0
}

/// `kw` 是否在 cmdline(已 lowercase)里作为**路径 basename 或独立 token**出现:
/// 其前一个字符须为 `/`、空白或字符串开头。这样命中真正的二进制调用
/// (`/opt/homebrew/bin/codex`、`node codex`), 但排除 `~/.codex/...` 这类点目录路径
/// (前缀是 `.`)—— 否则任何引用过 agent 配置目录的进程都会让终端被误判成该 agent。
fn mentions_binary(hay: &str, kw: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = hay[from..].find(kw) {
        let idx = from + rel;
        let before_ok =
            idx == 0 || matches!(hay[..idx].chars().next_back(), Some('/' | ' ' | '\t'));
        if before_ok {
            return true;
        }
        from = idx + kw.len();
    }
    false
}

/// 调试版: 返回 (识别结果, 诊断信息)
///   diagnostics 包含 pgid + 该 shell 的所有后裔进程 cmdlines (任意深度).
///
/// 关键设计:用 PPID 链追溯 descendants 而非 PGID. 原因:
///   node / codex 等 agent CLI 启动时常用 setsid 创建自己的 process group,
///   shell 的 pgid 里只有 zsh 自己, ps -g <pgid> 找不到 agent. 而 PPID 链
///   能穿透 process group 边界, 找到任意嵌套深度的子进程.
pub fn detect_agent_with_diagnostics(shell_pid: u32) -> (Option<AgentKind>, Diagnostics) {
    #[cfg(unix)]
    {
        let cmdlines = list_descendant_commands(shell_pid).unwrap_or_default();
        let pgid = get_pgid(shell_pid);
        let mut detected: Option<AgentKind> = None;
        for cmd in &cmdlines {
            let mut tokens = cmd.split_whitespace();
            let argv0 = tokens.next().unwrap_or("");
            if let Some(name) = token_to_name(argv0) {
                if let Some(agent) = classify(&name) {
                    detected = Some(agent);
                    break;
                }
                if is_wrapper(&name) {
                    for t in tokens {
                        if let Some(n) = token_to_name(t) {
                            if let Some(agent) = classify(&n) {
                                detected = Some(agent);
                                break;
                            }
                        }
                    }
                    if detected.is_some() {
                        break;
                    }
                }
            }
        }
        // 兜底: cmdline 把 agent 名作为**路径 basename / 独立 token**出现 — 修 codex 这种被
        // wrapper 隐藏在长 path 里、token 切分丢失的情况。
        // 关键: 用 mentions_binary 而非裸 contains —— 否则 `~/.codex/...` 配置目录路径
        // (开发 / 读配置 / 任何进程引用过)会让无 agent 的终端被误判成 codex("被 codex 抢")。
        if detected.is_none() {
            for cmd in &cmdlines {
                let lower = cmd.to_lowercase();
                // 只列假阳性风险低的独特关键字, 与 classify() 对齐.
                for &(kw, kind) in &[
                    ("claude", AgentKind::Claude),
                    ("codex", AgentKind::Codex),
                    ("aider", AgentKind::Aider),
                    ("gemini", AgentKind::Gemini),
                    ("cursor-agent", AgentKind::Cursor),
                    ("cline", AgentKind::Cline),
                    ("opencode", AgentKind::OpenCode),
                    ("copilot", AgentKind::Copilot),
                    ("ghcs", AgentKind::Copilot),
                    ("kimi", AgentKind::Kimi),
                    ("droid", AgentKind::Droid),
                ] {
                    if mentions_binary(&lower, kw) {
                        detected = Some(kind);
                        break;
                    }
                }
                if detected.is_some() {
                    break;
                }
            }
        }
        (
            detected,
            Diagnostics {
                shell_pid,
                pgid,
                cmdlines,
                note: String::new(),
            },
        )
    }
    #[cfg(not(unix))]
    {
        let _ = shell_pid;
        (
            None,
            Diagnostics {
                shell_pid,
                pgid: None,
                cmdlines: vec![],
                note: "windows: detection not implemented".into(),
            },
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostics {
    pub shell_pid: u32,
    pub pgid: Option<u32>,
    pub cmdlines: Vec<String>,
    pub note: String,
}

#[cfg(unix)]
fn get_pgid(pid: u32) -> Option<u32> {
    let out = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

#[cfg(unix)]
#[allow(dead_code)] // 保留, fallback 用得到
fn list_commands_in_pgid(pgid: u32) -> Option<Vec<String>> {
    let out = std::process::Command::new("ps")
        .args(["-g", &pgid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// 列 shell_pid 的所有后裔进程 cmdline (DFS via PPID).
/// 穿透 process group 边界 — codex / node 等用 setsid 起新 pgid 时仍能找到.
#[cfg(unix)]
fn list_descendant_commands(shell_pid: u32) -> Option<Vec<String>> {
    let out = std::process::Command::new("ps")
        .args(["-ax", "-o", "pid=,ppid=,command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // (pid, ppid, command)
    let mut all: Vec<(u32, u32, String)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let mut it = trimmed.splitn(3, char::is_whitespace);
        // splitn 对任意字符串(含空串)首次 next() 必为 Some, 故这里用 else continue
        // 而非 `?`(原 `?` 永不触发, 且若触发会 return None 丢弃整张表 — 语义错).
        // 空行/空 token 真正的过滤由下方 parse::<u32>() 失败自然跳过.
        let Some(pid_s) = it.next() else { continue };
        let after_pid = trimmed[pid_s.len()..].trim_start();
        let mut it2 = after_pid.splitn(2, char::is_whitespace);
        let Some(ppid_s) = it2.next() else { continue };
        let cmd = it2.next().unwrap_or("").trim_start();
        if let (Ok(pid), Ok(ppid)) = (pid_s.parse::<u32>(), ppid_s.parse::<u32>()) {
            all.push((pid, ppid, cmd.to_string()));
        }
    }
    // DFS: 从 shell_pid 找所有后裔. visited 防御 ppid 环导致死循环.
    let mut frontier = vec![shell_pid];
    let mut visited = std::collections::HashSet::new();
    visited.insert(shell_pid);
    let mut found = Vec::<String>::new();
    while let Some(parent) = frontier.pop() {
        for (pid, ppid, cmd) in &all {
            if *ppid == parent && visited.insert(*pid) {
                found.push(cmd.clone());
                frontier.push(*pid);
            }
        }
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_binary_matches_real_invocation_not_dotdir() {
        // 真正的 codex 二进制调用 → 命中
        assert!(mentions_binary("/opt/homebrew/bin/codex --json", "codex"));
        assert!(mentions_binary("node codex serve", "codex"));
        assert!(mentions_binary("codex", "codex"));
        // ~/.codex 配置目录路径 → 不命中(否则任何读配置的进程都误判终端为 codex)
        assert!(!mentions_binary(
            "cat /users/mt/.codex/sessions/x.jsonl",
            "codex"
        ));
        assert!(!mentions_binary("grep foo ~/.codex/config.toml", "codex"));
        // 粘连词不命中
        assert!(!mentions_binary("xcodex", "codex"));
    }

    #[test]
    fn classify_exact_names() {
        assert_eq!(classify("claude"), Some(AgentKind::Claude));
        assert_eq!(classify("CLAUDE-CODE"), Some(AgentKind::Claude));
        assert_eq!(classify("codex"), Some(AgentKind::Codex));
        assert_eq!(classify("cursor-agent"), Some(AgentKind::Cursor));
        assert_eq!(classify("aider"), Some(AgentKind::Aider));
        assert_eq!(classify("vim"), None);
    }

    #[test]
    fn token_to_name_strips_path_and_ext() {
        assert_eq!(
            token_to_name("/usr/local/bin/claude"),
            Some("claude".into())
        );
        assert_eq!(token_to_name("/opt/foo/index.js"), Some("index".into()));
        assert_eq!(token_to_name("--flag"), None);
        assert_eq!(token_to_name(""), None);
        assert_eq!(token_to_name("npx"), Some("npx".into()));
    }

    #[test]
    fn wrapper_detection() {
        assert!(is_wrapper("node"));
        assert!(is_wrapper("python3"));
        assert!(!is_wrapper("claude"));
    }
}
