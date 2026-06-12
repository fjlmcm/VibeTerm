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
//!   - Windows 走 sysinfo 枚举进程表(pid/ppid/cmdline),后裔 DFS 与 unix 共用;
//!     npm 全局装的 claude 在 Windows 是 `node …\claude-code\cli.js`,靠
//!     mentions_binary 的反斜杠分隔兜底命中

use serde::{Deserialize, Serialize};

/// 11 种 agent — 与 Prowl 对齐
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Pi,
    Claude,
    Codex,
    Gemini,
    Cursor,
    Cline,
    // snake_case 会导出 "open_code",但运行时值(as_str()/tasks.json/前端比较)
    // 一律是 "opencode" —— rename 对齐,否则 TS 镜像类型与实际 wire 值漂移。
    #[serde(rename = "opencode")]
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

/// 单个 agent 的完整定义 —— **全项目唯一的 agent 数据源**。
/// 此前 classify / cmdline 关键字表 / pick_rules / AgentKind::ALL 四处平行维护,
/// 加一个 agent 要改四处且无机制保证同步;现在全部从本表派生。
pub struct AgentDef {
    pub kind: AgentKind,
    /// 进程/命令 basename 别名(classify 与 pick_rules 精确匹配,小写)
    pub aliases: &'static [&'static str],
    /// cmdline 关键字扫描用(wrapper 进程如 `node /path/claude` 的兜底识别)。
    /// **只列假阳性风险低的独特关键字**:pi/amp/cursor 这类泛词留空,
    /// 否则普通命令行(`pip`/`ampere`/编辑器路径)会让终端被误判成 agent。
    pub cmdline_keywords: &'static [&'static str],
    /// 授权框正则(strip ANSI 后在 16KB ring 上匹配 → WaitingInput)。
    /// 校准原则(2026-05-30,带血):必须用**菜单独有措辞**,agent 正文里
    /// 自然出现的泛词("do you want to proceed"/"(y/n)" 在 claude 叙述中常见)会闪假黄灯。
    pub waiting_patterns: &'static [&'static str],
    /// 编译后的 regex 缓存(OnceLock 只 build 一次)
    pub compiled: std::sync::OnceLock<Vec<regex::Regex>>,
}

impl AgentDef {
    pub fn compiled_patterns(&self) -> &[regex::Regex] {
        self.compiled.get_or_init(|| {
            self.waiting_patterns
                .iter()
                .filter_map(|p| regex::Regex::new(p).ok())
                .collect()
        })
    }
}

macro_rules! agent_def {
    ($kind:expr, $aliases:expr, $kws:expr, $patterns:expr) => {
        AgentDef {
            kind: $kind,
            aliases: $aliases,
            cmdline_keywords: $kws,
            waiting_patterns: $patterns,
            compiled: std::sync::OnceLock::new(),
        }
    };
}

/// 12 个 agent 的唯一数据源。新增 agent 只改这里(TS 镜像测试会提醒同步 ipc-types)。
pub static AGENT_DEFS: [AgentDef; 12] = [
    // claude 真实授权 UI(2.x, 实测): 带框编号菜单
    //   "Do you want to proceed?  ❯ 1. Yes / 2. Yes, and don't ask again / 3. No, and tell Claude…"
    // 信任框(实测): "❯ 1. Yes, I trust this folder / 2. No, exit"。
    //   - claude 2.x 菜单根本不用 (y/n);
    //   - "No, and tell Claude" 是授权菜单选项 3, 必与每个授权框同现 → 留它即可全覆盖。
    agent_def!(
        AgentKind::Claude,
        &["claude", "claude-code"],
        &["claude"],
        &[
            r"(?i)no, and tell claude",           // 授权菜单选项 3, 菜单独有, 最稳
            r"(?i)don't ask again",               // 授权菜单选项 2
            r"(?i)trust (the files|this folder)", // 首次进目录"信任此文件夹"(实测)
            r"(?i)continue with this plan\?",     // plan 模式确认
        ]
    ),
    // codex 命令审批 UI 未能实测(本机 codex 配置为自动放行命令, 抓不到审批框)。
    // 保守:只认低误报的字面 y/n 形态; codex 菜单式审批的独有措辞待拿到真实样本再校准。
    agent_def!(
        AgentKind::Codex,
        &["codex"],
        &["codex"],
        &[r"\(y/n\)", r"\[y/n\]"]
    ),
    agent_def!(
        AgentKind::Aider,
        &["aider"],
        &["aider"],
        &[
            r"(?i)yes/no",
            r"\(y\)",
            r"(?i)add .* to the chat\?",
            r"(?i)edit the files",
        ]
    ),
    // Gemini CLI — Google 官方, 多用 ? 结尾的英文确认 + y/n
    agent_def!(
        AgentKind::Gemini,
        &["gemini"],
        &["gemini"],
        &[
            r"\(y/n\)",
            r"\[y/n\]",
            r"(?i)proceed\?",
            r"(?i)apply changes\?",
        ]
    ),
    // Cursor CLI / cursor-agent — 关键字只认 cursor-agent(裸 "cursor" 误报高:编辑器路径)
    agent_def!(
        AgentKind::Cursor,
        &["cursor", "cursor-agent"],
        &["cursor-agent"],
        &[
            r"\(y/n\)",
            r"\[y/n\]",
            r"(?i)accept\?",
            r"(?i)keep changes\?",
        ]
    ),
    // Cline — VSCode 插件 + 独立 CLI 形态
    agent_def!(
        AgentKind::Cline,
        &["cline"],
        &["cline"],
        &[
            r"\(y/n\)",
            r"\[y/n\]",
            r"(?i)approve\?",
            r"(?i)proceed with",
        ]
    ),
    // OpenCode — 开源 Claude Code-like
    agent_def!(
        AgentKind::OpenCode,
        &["opencode", "open-code"],
        &["opencode"],
        &[r"\(y/n\)", r"\[y/n\]", r"(?i)yes/no", r"(?i)confirm"]
    ),
    // GitHub Copilot CLI / ghcs
    agent_def!(
        AgentKind::Copilot,
        &["copilot", "github-copilot", "ghcs"],
        &["copilot", "ghcs"],
        &[
            r"\(y/n\)",
            r"\[y/n\]",
            r"(?i)select an option",
            r"(?i)allow this command",
        ]
    ),
    // Kimi / Moonshot CLI
    agent_def!(
        AgentKind::Kimi,
        &["kimi"],
        &["kimi"],
        &[r"\(y/n\)", r"\[y/n\]", r"是否继续", r"是否同意"]
    ),
    // Droid (Factory.ai)
    agent_def!(
        AgentKind::Droid,
        &["droid"],
        &["droid"],
        &[
            r"\(y/n\)",
            r"\[y/n\]",
            r"(?i)approve plan",
            r"(?i)continue\?",
        ]
    ),
    // Amp (Sourcegraph) — "amp" 太泛, 不参与 cmdline 关键字扫描
    agent_def!(
        AgentKind::Amp,
        &["amp", "amp-local"],
        &[],
        &[r"\(y/n\)", r"\[y/n\]", r"(?i)approve", r"(?i)proceed"]
    ),
    // Pi — "pi" 太泛(pip/pipx…), 不参与 cmdline 关键字扫描
    agent_def!(AgentKind::Pi, &["pi"], &[], &[r"\(y/n\)", r"\[y/n\]"]),
];

/// 进程名 → AgentKind(小写比对;从 AGENT_DEFS 派生)
fn classify(name: &str) -> Option<AgentKind> {
    let lower = name.to_lowercase();
    AGENT_DEFS
        .iter()
        .find(|d| d.aliases.contains(&lower.as_str()))
        .map(|d| d.kind)
}

/// Wrapped runtime — 这些 binary 本身不是 agent,但其参数往往包含真正的 agent 命令。
/// 如 `node /usr/local/bin/claude` 应识别为 claude。
/// cmd / powershell / pwsh:Windows 上 npm shim(claude.cmd)经它们二跳启动。
fn is_wrapper(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "node"
            | "deno"
            | "bun"
            | "python"
            | "python3"
            | "ruby"
            | "npx"
            | "pnpx"
            | "yarn"
            | "cmd"
            | "powershell"
            | "pwsh"
    )
}

/// 从命令行 token 提取候选 agent 名(去引号、去 path、去扩展名、去 --flag)。
/// Windows 形态一并处理:反斜杠路径、`"C:\…\claude.exe"` 引号、.exe/.cmd/.bat/.ps1。
fn token_to_name(token: &str) -> Option<String> {
    let trimmed = token.trim().trim_matches('"');
    if trimmed.starts_with('-') || trimmed.is_empty() {
        return None;
    }
    // 去 path 前缀(`/` 与 `\` 都是分隔符)
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    // 去常见后缀(大小写不敏感 —— Windows 下可能是 .EXE)。
    // 长度从 last 自身减后缀长算(to_lowercase 对非 ASCII 可能变长,不能用 lower 的下标)。
    let lower = last.to_lowercase();
    let base_len = [".js", ".mjs", ".exe", ".cmd", ".bat", ".ps1"]
        .iter()
        .find(|ext| lower.ends_with(*ext))
        .map(|ext| last.len().saturating_sub(ext.len()))
        .unwrap_or(last.len());
    let base = &last[..base_len];
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
/// 其前一个字符须为 `/`、`\`(Windows 路径)、引号、空白或字符串开头。这样命中真正的
/// 二进制调用(`/opt/homebrew/bin/codex`、`node codex`、`node c:\…\claude-code\cli.js`),
/// 但排除 `~/.codex/...` 这类点目录路径(前缀是 `.`)—— 否则任何引用过 agent 配置目录
/// 的进程都会让终端被误判成该 agent。
fn mentions_binary(hay: &str, kw: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = hay[from..].find(kw) {
        let idx = from + rel;
        let before_ok = idx == 0
            || matches!(
                hay[..idx].chars().next_back(),
                Some('/' | '\\' | '"' | ' ' | '\t')
            );
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
/// 进程表来源平台分支(unix: ps;Windows: sysinfo),匹配逻辑共用.
pub fn detect_agent_with_diagnostics(shell_pid: u32) -> (Option<AgentKind>, Diagnostics) {
    let cmdlines = list_descendant_commands(shell_pid).unwrap_or_default();
    #[cfg(unix)]
    let pgid = get_pgid(shell_pid);
    #[cfg(not(unix))]
    let pgid: Option<u32> = None;
    let detected = detect_agent_in_cmdlines(&cmdlines);
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

/// 在后裔进程 cmdline 列表上跑识别(纯字符串逻辑,跨平台,单测友好)
fn detect_agent_in_cmdlines(cmdlines: &[String]) -> Option<AgentKind> {
    for cmd in cmdlines {
        let mut tokens = cmd.split_whitespace();
        let argv0 = tokens.next().unwrap_or("");
        if let Some(name) = token_to_name(argv0) {
            if let Some(agent) = classify(&name) {
                return Some(agent);
            }
            if is_wrapper(&name) {
                for t in tokens {
                    if let Some(n) = token_to_name(t) {
                        if let Some(agent) = classify(&n) {
                            return Some(agent);
                        }
                    }
                }
            }
        }
    }
    // 兜底: cmdline 把 agent 名作为**路径 basename / 独立 token**出现 — 修 codex 这种被
    // wrapper 隐藏在长 path 里、token 切分丢失的情况。
    // 关键: 用 mentions_binary 而非裸 contains —— 否则 `~/.codex/...` 配置目录路径
    // (开发 / 读配置 / 任何进程引用过)会让无 agent 的终端被误判成 codex("被 codex 抢")。
    for cmd in cmdlines {
        let lower = cmd.to_lowercase();
        // 关键字从 AGENT_DEFS 派生(cmdline_keywords 只含低假阳性的独特词)
        for def in &AGENT_DEFS {
            for kw in def.cmdline_keywords {
                if mentions_binary(&lower, kw) {
                    return Some(def.kind);
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
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

/// 全进程表 (pid, ppid, cmdline) —— unix 走 ps 文本解析
#[cfg(unix)]
fn list_all_processes() -> Option<Vec<(u32, u32, String)>> {
    let out = std::process::Command::new("ps")
        .args(["-ax", "-o", "pid=,ppid=,command="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
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
    Some(all)
}

/// 全进程表 —— Windows 走 sysinfo(内部 NtQueryInformationProcess 拿 cmdline,
/// 无 wmic/PowerShell 子进程开销;ConPTY 下 shell 子进程的 ppid 链同样成立)
#[cfg(windows)]
fn list_all_processes() -> Option<Vec<(u32, u32, String)>> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut all: Vec<(u32, u32, String)> = Vec::new();
    for (pid, proc_) in sys.processes() {
        let ppid = proc_.parent().map(|p| p.as_u32()).unwrap_or(0);
        // cmd() 拿不到(权限/系统进程)时退进程名,至少 argv0 可识别
        let cmd = if proc_.cmd().is_empty() {
            proc_.name().to_string_lossy().into_owned()
        } else {
            proc_
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        };
        all.push((pid.as_u32(), ppid, cmd));
    }
    Some(all)
}

#[cfg(not(any(unix, windows)))]
fn list_all_processes() -> Option<Vec<(u32, u32, String)>> {
    None
}

/// 列 shell_pid 的所有后裔进程 cmdline (DFS via PPID).
/// 穿透 process group 边界 — codex / node 等用 setsid 起新 pgid 时仍能找到.
fn list_descendant_commands(shell_pid: u32) -> Option<Vec<String>> {
    let all = list_all_processes()?;
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
    fn token_to_name_handles_windows_paths() {
        // native 安装:%USERPROFILE%\.local\bin\claude.exe
        assert_eq!(
            token_to_name(r"C:\Users\demo\.local\bin\claude.exe"),
            Some("claude".into())
        );
        // 引号包裹的带空格路径
        assert_eq!(
            token_to_name(r#""C:\Program Files\nodejs\node.exe""#),
            Some("node".into())
        );
        // npm shim
        assert_eq!(
            token_to_name(r"C:\Users\demo\AppData\Roaming\npm\claude.cmd"),
            Some("claude".into())
        );
        // 大写扩展名
        assert_eq!(token_to_name(r"D:\TOOLS\CODEX.EXE"), Some("CODEX".into()));
    }

    #[test]
    fn wrapper_detection() {
        assert!(is_wrapper("node"));
        assert!(is_wrapper("python3"));
        assert!(is_wrapper("pwsh"));
        assert!(is_wrapper("cmd"));
        assert!(!is_wrapper("claude"));
    }

    #[test]
    fn mentions_binary_windows_paths() {
        // npm 全局装的 claude 在 Windows 的真实形态:node + 反斜杠路径
        assert!(mentions_binary(
            r"node c:\users\demo\appdata\roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js",
            "claude"
        ));
        // 引号包裹
        assert!(mentions_binary(r#""c:\tools\codex.exe" --json"#, "codex"));
        // Windows 点目录路径不命中(与 unix ~/.codex 同语义)
        assert!(!mentions_binary(
            r"type c:\users\demo\.codex\config.toml",
            "codex"
        ));
    }

    #[test]
    fn detect_agent_in_windows_cmdlines() {
        // native claude.exe
        assert_eq!(
            detect_agent_in_cmdlines(&[r"C:\Users\demo\.local\bin\claude.exe".into()]),
            Some(AgentKind::Claude)
        );
        // node wrapper + cli.js 长路径(token 识别失败 → mentions_binary 兜底)
        assert_eq!(
            detect_agent_in_cmdlines(&[
                r"C:\Program Files\nodejs\node.exe C:\Users\demo\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js".into()
            ]),
            Some(AgentKind::Claude)
        );
        // cmd shim 二跳
        assert_eq!(
            detect_agent_in_cmdlines(&[
                r"cmd /c C:\Users\demo\AppData\Roaming\npm\codex.cmd".into()
            ]),
            Some(AgentKind::Codex)
        );
        // 普通 shell 不误判
        assert_eq!(
            detect_agent_in_cmdlines(&[
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe".into()
            ]),
            None
        );
    }
}

#[cfg(test)]
mod ts_mirror_sync_tests {
    use super::*;

    /// AgentKind 全集必须出现在 TS 镜像(ipc-types)里——后端加第 N 个 agent 忘改 TS 时
    /// `TaskDto.agent_kind` 会是 TS 类型里不存在的字符串且编译期零报错,此测试让它在 CI 红掉。
    #[test]
    fn agent_kinds_present_in_ts_mirror() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../web/packages/ipc-types/src/generated.ts");
        let ts = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("读 TS 镜像失败 {}: {e}", p.display()));
        for def in &AGENT_DEFS {
            let lit = format!("\"{}\"", def.kind.as_str());
            assert!(
                ts.contains(&lit),
                "AgentKind {lit} 不在 ipc-types/index.ts 的 AgentKind union —— Rust/TS 镜像漂移"
            );
        }
    }
}
