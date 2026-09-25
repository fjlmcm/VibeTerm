//! agent 嗅探 IPC:AI CLI 检测、claude/codex session/usage/blocks 查询与 resume 命令。
//! 全部只读,不写任何 agent 配置。

use vibeterm_ipc::IpcResult;

// ============================
// IPC commands — AI CLI 检测
// ============================
#[derive(serde::Serialize, Clone, specta::Type)]
pub(crate) struct CliStatus {
    name: String,
    installed: bool,
    path: Option<String>,
}

/// 从 login shell 读完整 PATH —— macOS GUI app(Dock/Launchpad 启动)的进程 PATH
/// 不含 ~/.zshrc/.zprofile 里加的目录(homebrew / npm global / nvm 等), 直接 which 会
/// 漏报 "未安装"。读 login shell 的 PATH 修正, 用唯一标记提取避免 rc 其它输出干扰。
/// Windows 无 $SHELL → 返回 None 即正确降级:GUI 进程的 PATH 来自注册表(系统+用户),
/// 不存在 macOS 的 PATH 丢失问题,npm 全局目录默认就在用户 PATH 里。
///
/// `-ilc` 会 source rc 文件,rc 里卡住(等输入 / 网络)会挂死调用方:stdin 接 null,
/// 并加 3s 超时 —— 超时 kill 子进程返回 None,调用方退回当前进程 PATH。
pub(crate) fn login_shell_path() -> Option<String> {
    use std::process::{Command, Stdio};
    const START: &str = "__VT_PATH_START__";
    const END: &str = "__VT_PATH_END__";
    let shell = std::env::var("SHELL").ok()?;
    let mut child = Command::new(&shell)
        .args([
            "-ilc",
            "printf '__VT_PATH_START__%s__VT_PATH_END__' \"$PATH\"",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| tracing::warn!("login_shell_path: spawn {shell} failed: {e}"))
        .ok()?;
    // 独立线程读完 stdout(避免管道写满死锁),主线程带超时等结果。
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        let _ = tx.send(buf);
    });
    let out = match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(buf) => buf,
        Err(_) => {
            tracing::warn!("login_shell_path: {shell} -ilc timed out after 3s, killing");
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let _ = child.wait();
    let s = String::from_utf8_lossy(&out);
    let a = s.find(START)? + START.len();
    let b = s[a..].find(END)? + a;
    let p = &s[a..b];
    (!p.is_empty()).then(|| p.to_string())
}

/// 等 `spawn_blocking` 任务完成;panic / 取消时记日志并降级为 `fallback`。
/// 数据读取类 IPC 失败降级为空值是预期行为,但 panic 是程序 bug,必须留日志而非静默吞掉。
pub(crate) async fn join_blocking_or<T>(
    handle: tokio::task::JoinHandle<T>,
    fallback: T,
    what: &str,
) -> T {
    handle.await.unwrap_or_else(|e| {
        tracing::warn!(task = what, error = %e, "spawn_blocking 任务异常, 降级为默认值");
        fallback
    })
}

#[tauri::command]
pub(crate) async fn detect_ai_clis() -> IpcResult<Vec<CliStatus>> {
    // 暂时只检测 claude / codex(其它 agent 的状态嗅探不依赖此检测, 需要时再加回)
    let targets = ["claude", "codex"];
    // login shell 的完整 PATH(GUI 启动时进程 PATH 不全);失败回退进程 PATH.
    let path = join_blocking_or(
        tokio::task::spawn_blocking(login_shell_path),
        None,
        "login_shell_path",
    )
    .await;
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
    Ok(targets
        .iter()
        .map(|name| {
            let found = match &path {
                Some(p) => which::which_in(name, Some(p), &cwd).ok(),
                None => which::which(name).ok(),
            };
            let path = found.map(|p| p.to_string_lossy().into_owned());
            CliStatus {
                name: name.to_string(),
                installed: path.is_some(),
                path,
            }
        })
        .collect())
}

/// Claude usage_cache.json 当前快照 — 前端启动时拉一次, 之后靠
/// `claude_usage_changed` 事件增量更新.
#[tauri::command]
pub(crate) async fn get_claude_usage_cache() -> IpcResult<Option<vibeterm_agent_watch::UsageCache>>
{
    Ok(vibeterm_agent_watch::claude::usage_cache::read_once())
}

/// Claude 当前活跃 session (mtime 最新的 jsonl). 前端启动拉一次, 之后靠
/// `claude_session_changed` 事件增量更新.
/// 注意 v2 实现是全局取最新 — v4 会改成按 cwd 过滤.
#[tauri::command]
pub(crate) async fn get_claude_session() -> IpcResult<Option<vibeterm_agent_watch::ClaudeSession>> {
    // 文件 I/O 重 (扫多个 project dir + 最大 64MB jsonl), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(vibeterm_agent_watch::claude::project::read_once),
        None,
        "get_claude_session",
    )
    .await)
}

#[tauri::command]
pub(crate) async fn get_codex_session() -> IpcResult<Option<vibeterm_agent_watch::CodexSnapshot>> {
    // 文件 I/O 重 (扫近 3 天 rollout), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(vibeterm_agent_watch::codex::session::read_once),
        None,
        "get_codex_session",
    )
    .await)
}

/// 按 cwd 查 Claude session — 精确到当前活跃终端的 cwd 而非全局最新.
#[tauri::command]
pub(crate) async fn get_claude_session_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::ClaudeSession>> {
    // 文件 I/O 重, 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(move || {
            vibeterm_agent_watch::claude::project::read_for_cwd(&cwd)
        }),
        None,
        "get_claude_session_by_cwd",
    )
    .await)
}

/// 按 cwd 查 Codex session — 同上.
#[tauri::command]
pub(crate) async fn get_codex_session_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::CodexSnapshot>> {
    // 文件 I/O 重 (扫近 3 天 rollout), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(move || {
            vibeterm_agent_watch::codex::session::read_for_cwd(&cwd)
        }),
        None,
        "get_codex_session_by_cwd",
    )
    .await)
}

/// agent 会话恢复命令(Y1–Y3 的零侵入版)。
/// 🟢 只读嗅探 session_id(复用 read_for_cwd,不写任何 agent 配置、无 hook),据此构造 agent 原生
/// resume 命令字符串返回给前端。**不自动执行** —— 由用户在命令面板手动点"恢复 agent",
/// 前端再开一个新 pane 跑这条命令。无 cmux 那套签名审批信任模型(命令由 VibeTerm 内部构造 +
/// 用户手动触发,非外部进程提议,无需防伪)。
#[derive(serde::Serialize, specta::Type)]
pub(crate) struct ResumeInfo {
    agent: String,
    session_id: String,
    command: String,
}

/// 单引号包裹 + 转义,安全拼进发往 shell 的命令(session_id 为 UUID,防御性处理)。
pub(crate) fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// session_id 格式断言:仅允许 ASCII 字母数字 + `-` `_`(claude/codex 均为 UUID 形态)。
/// 拒绝换行 / 控制字符 / shell 元字符 —— 命令注入纵深防御:不合格直接不构造 resume 命令。
pub(crate) fn valid_session_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[tauri::command]
pub(crate) async fn agent_resume_command(
    cwd: String,
    agent_kind: Option<String>,
) -> IpcResult<Option<ResumeInfo>> {
    let kind = agent_kind.unwrap_or_default();
    Ok(tokio::task::spawn_blocking(move || -> Option<ResumeInfo> {
        match kind.as_str() {
            "claude" => {
                let sid = vibeterm_agent_watch::claude::project::read_for_cwd(&cwd)?.session_id;
                if !valid_session_id(&sid) {
                    return None;
                }
                Some(ResumeInfo {
                    command: format!("claude --resume {}", shell_single_quote(&sid)),
                    agent: "claude".into(),
                    session_id: sid,
                })
            }
            "codex" => {
                let sid = vibeterm_agent_watch::codex::session::read_for_cwd(&cwd)?.session_id;
                if !valid_session_id(&sid) {
                    return None;
                }
                Some(ResumeInfo {
                    command: format!("codex resume {}", shell_single_quote(&sid)),
                    agent: "codex".into(),
                    session_id: sid,
                })
            }
            _ => None,
        }
    })
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(error = %e, "agent_resume_command blocking task panicked");
        None
    }))
}

/// 当前 cwd 对应的 Claude 5h 滚动块 (移植 ccusage `blocks.rs`).
#[tauri::command]
pub(crate) async fn get_claude_block_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::claude::blocks::ActiveBlock>> {
    // 文件 I/O 重 (read_to_string jsonl), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(move || {
            vibeterm_agent_watch::claude::blocks::active_block_for_cwd(&cwd)
        }),
        None,
        "get_claude_block_by_cwd",
    )
    .await)
}

/// Codex 5h 滚动块 (本地按 token_count 事件算, 跟 Claude 同算法).
/// `cwd` 参数是 IPC 对称占位 — 实际 Codex 配额按账号算, 不按 cwd 过滤.
/// 文件 I/O 重 (扫多个 rollout), 走 `spawn_blocking` 不阻塞 tokio runtime.
#[tauri::command]
pub(crate) async fn get_codex_block_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::claude::blocks::ActiveBlock>> {
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(move || {
            vibeterm_agent_watch::codex::blocks::active_block_for_cwd(&cwd)
        }),
        None,
        "get_codex_block_by_cwd",
    )
    .await)
}

/// 跨所有 Claude project 累加过去 24h 的 token 用量.
/// 文件 I/O + 行扫描重, 走 `spawn_blocking` 不阻塞 tokio runtime.
#[tauri::command]
pub(crate) async fn get_claude_tokens_today() -> IpcResult<u64> {
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(vibeterm_agent_watch::claude::project::total_tokens_last_24h),
        0,
        "get_claude_tokens_today",
    )
    .await)
}

/// Claude 当前订阅 plan (`Max 20x` / `Pro` / `Free` ...). 未登录返回 None.
/// 读 ~/.claude.json + 解析, 走 `spawn_blocking`.
#[tauri::command]
pub(crate) async fn get_claude_plan() -> IpcResult<Option<String>> {
    Ok(join_blocking_or(
        tokio::task::spawn_blocking(vibeterm_agent_watch::claude::claude_config::plan_label),
        None,
        "get_claude_plan",
    )
    .await)
}
