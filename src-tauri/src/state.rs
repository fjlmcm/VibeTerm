//! AppState、跨模块共享常量与基础工具(emit_tasks_changed / 路径展开 / 原子写 / 错误映射)。
//! 从 main.rs 拆出(行为不变)。

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use vibeterm_core::{TaskRegistry, TerminalRegistry};
use vibeterm_ipc::{IpcError, TerminalId};
use vibeterm_status::StatusDetector;

use crate::menu::MenuLang;

// ---- App state ----
pub(crate) struct AppState {
    pub(crate) terminals: Arc<TerminalRegistry>,
    pub(crate) tasks: Arc<TaskRegistry>,
    // 顶栏菜单语言 — 前端 setLang() 时通过 set_menu_lang IPC 同步
    pub(crate) menu_lang: std::sync::Mutex<MenuLang>,
    /// 每个 terminal 的 StatusDetector handle. agent 嗅探层用它在识别到
    /// agent_kind 后开启 stall 检测; close_terminal 时清理.
    pub(crate) status_detectors: std::sync::Mutex<
        std::collections::HashMap<TerminalId, Arc<std::sync::Mutex<StatusDetector>>>,
    >,
    /// 通知点击聚焦:tauri-plugin-notification 2.x 桌面无 click callback,
    /// 用"最近通知 + 时间戳"近似 — notify 后写入此字段,window focused 事件触发时
    /// 若在 NOTIFY_FOCUS_GRACE 窗口内 → 视为点击,emit 给前端切 task; 否则忽略.
    pub(crate) last_notify: std::sync::Mutex<Option<(vibeterm_ipc::TaskId, std::time::Instant)>>,
    /// agent 完成通知 throttle (per-task, 30s). 防来回对话每个 turn 都响.
    /// 现由嗅探(标题 spinner→静态 / OSC D)触发完成通知, key 用 "task-<id>".
    pub(crate) last_agent_completed:
        std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// 间歇持续提醒(persistent_unseen_sound)节流时刻。None = 当前无"未看完成"或主窗口在
    /// 前台(已 reset);Some(t) = 上次响铃时刻,下次需隔 PERSISTENT_REMIND_INTERVAL。全局单路。
    pub(crate) last_persistent_remind: std::sync::Mutex<Option<std::time::Instant>>,
}

/// 窗口聚焦事件 → 视为"点击通知"的最大允许 gap. 超过则当作用户从 dock/Cmd-Tab
/// 主动激活,不强制切 task. 取 10s 是经验:用户看到通知到点击通常 < 5s.
pub(crate) const NOTIFY_FOCUS_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// agent_completed 通知冷却时间(per-task). transcript 完成检测已是轮级精确(claude end_turn /
/// codex task_complete,一轮一次 + set_agent_turn_done 去重),只需挡同一 task 几秒内的极快重复;
/// 5s 让正常多轮对话每轮都提示,又不至于同轮边界的瞬时重复连响两声.
pub(crate) const AGENT_COMPLETED_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);

/// transcript 完成检测的"归属窗口":只有本终端 PTY 在此窗口内**确有产出**,才认可
/// `read_for_cwd` 报出的"答完一轮"是本终端的完成。
///
/// 根因:`read_for_cwd(cwd)` 取的是该 cwd 编码目录下 mtime 最新的 claude 会话,而同一个
/// `~/.claude/projects/<编码cwd>/` 目录下可能并存多个 claude(同项目多终端、不同终端 app、
/// 甚至 `cwd_to_project_dir` 有损编码把不同项目映射进同一目录)。别的 claude 答完会把它的
/// jsonl 顶成最新 → 本终端的完成轮询误读成"自己答完了" → 给本不相干的别处任务发完成通知
/// (用户实测:在 ghostty 等别的终端里跑 claude 完成,VibeTerm 也弹通知)。
/// 本终端没产出 = 这一轮不是它答的,跳过(回退 PTY 嗅探 + 等下次轮询)。
///
/// 8s > 3s 轮询间隔,给本终端真完成留足余量(首次 in-window 轮询必采到),绝不漏报自己的完成;
/// 同时把"早已静默等输入 / 在别处跑"的终端干净排除。
pub(crate) const AGENT_COMPLETION_OUTPUT_WINDOW_MS: u128 = 8_000;

// ---- helpers ----
pub(crate) fn emit_tasks_changed(app: &AppHandle, tasks: &TaskRegistry) {
    let Ok(mut list) = tasks.list() else { return };
    // 注入 last_output(终端末行,Prowl 风格状态行)。
    // 分屏时遍历所有 terminal,挑 last_update_ms 最大的那个 — 即"最近有输出"的那块屏。
    if let Some(state) = app.try_state::<AppState>() {
        for dto in &mut list {
            if !dto.terminal_ids.is_empty() {
                dto.last_output = state.terminals.most_recent_tail(&dto.terminal_ids);
            }
        }
    }
    let _ = app.emit("tasks_changed", &list);
}

/// 主窗口当前是否聚焦(用户正盯着 VibeTerm)。
pub(crate) fn main_window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// 通知投递方式 —— preflight 放行后告诉调用方走哪条路。
/// 展开 cwd 字符串里的 `~` 和 `$VAR` / `${VAR}`,验证目录存在;
/// 路径不存在或无法 stat 时 fallback home(dirs::home_dir,Windows 无 $HOME)。
pub(crate) fn expand_and_validate_cwd(input: &str) -> String {
    let expanded = expand_path_str(input);
    if std::path::Path::new(&expanded).is_dir() {
        expanded
    } else {
        tracing::warn!(input, expanded, "cwd not a directory, falling back to home");
        dirs::home_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into())
    }
}

pub(crate) fn expand_path_str(input: &str) -> String {
    let trimmed = input.trim();
    let home_str = || {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    // ~ / ~/...(Windows 习惯的 ~\... 同样展开)
    let after_tilde: String = if trimmed == "~" {
        let home = home_str();
        if home.is_empty() {
            trimmed.into()
        } else {
            home
        }
    } else if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        let home = home_str();
        if home.is_empty() {
            trimmed.into()
        } else {
            format!(
                "{}{}{}",
                home.trim_end_matches(['/', '\\']),
                std::path::MAIN_SEPARATOR,
                rest
            )
        }
    } else {
        trimmed.into()
    };
    // $VAR / ${VAR}
    expand_env_vars(&after_tilde)
}

pub(crate) fn expand_env_vars(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            // ${NAME}
            if bytes[i + 1] == b'{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let name = &s[i + 2..i + 2 + end];
                    if let Ok(v) = std::env::var(name) {
                        out.push_str(&v);
                    }
                    i += 2 + end + 1;
                    continue;
                }
            }
            // $NAME — 取 [A-Za-z_][A-Za-z0-9_]*
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                let c = bytes[j];
                let valid = c.is_ascii_alphanumeric() || c == b'_';
                if j == start && !(c.is_ascii_alphabetic() || c == b'_') {
                    break;
                }
                if !valid {
                    break;
                }
                j += 1;
            }
            if j > start {
                let name = &s[start..j];
                if let Ok(v) = std::env::var(name) {
                    out.push_str(&v);
                }
                i = j;
                continue;
            }
        }
        // 非变量字节:按 UTF-8 char 边界整体推入,避免逐字节 `as char`
        // 把多字节序列(CJK 等)拆成 Latin-1 mojibake。
        match s[i..].chars().next() {
            Some(ch) => {
                out.push(ch);
                i += ch.len_utf8();
            }
            None => break,
        }
    }
    out
}

pub(crate) fn map_task_err(e: vibeterm_core::TaskError) -> IpcError {
    use vibeterm_core::TaskError::*;
    match e {
        NotFound(id) => IpcError::NotFound {
            resource: "task".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("task:{other}"),
        },
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 原子写: 同目录临时文件 + rename(tempfile 已在依赖).
#[cfg(not(target_os = "windows"))]
pub(crate) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, bytes)?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Windows 版: rename 可能被 AV / OneDrive 短暂锁住, 重试 3 次后降级覆盖写
/// (与 vibeterm-config::atomic_write 同策略).
#[cfg(target_os = "windows")]
pub(crate) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    for attempt in 0..3 {
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(bytes)?;
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                tracing::warn!(
                    attempt,
                    err = %e.error,
                    "atomic rename failed (likely OneDrive / AV / locked), retrying"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    tracing::warn!(
        ?path,
        "atomic rename retries exhausted, falling back to truncate+write"
    );
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod cwd_expand_tests {
    use super::*;
    // locale 检测函数随 spawn 链拆进了 pty_ipc,测试数据与路径展开同源,留在本模块
    use crate::pty_ipc::locale_env_has_utf8;

    #[test]
    fn locale_utf8_detection() {
        use std::collections::HashMap;
        let none = |_: &str| None;
        // 全空(GUI 启动常态)→ 不是 UTF-8 → 上层会注入 en_US.UTF-8
        assert!(!locale_env_has_utf8(&HashMap::new(), none));
        // 继承的 LANG=C / 空 LC_* → 仍非 UTF-8(中文路径乱码的真实场景)
        let inherit_c = |k: &str| (k == "LANG").then(|| "C".to_string());
        assert!(!locale_env_has_utf8(&HashMap::new(), inherit_c));
        // merged 显式 UTF-8 → 尊重,判定为已是 UTF-8
        let mut m = HashMap::new();
        m.insert("LANG".to_string(), "zh_CN.UTF-8".to_string());
        assert!(locale_env_has_utf8(&m, none));
        // 仅继承 LC_CTYPE=UTF-8(macOS 形态)→ UTF-8
        let inherit_ctype = |k: &str| (k == "LC_CTYPE").then(|| "UTF-8".to_string());
        assert!(locale_env_has_utf8(&HashMap::new(), inherit_ctype));
        // LC_ALL 大小写无关
        let inherit_upper = |k: &str| (k == "LC_ALL").then(|| "ja_JP.utf8".to_string());
        assert!(locale_env_has_utf8(&HashMap::new(), inherit_upper));
    }

    #[test]
    fn expand_tilde_only() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("~"), "/Users/test");
    }

    #[test]
    fn expand_tilde_slash() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(
            expand_path_str("~/projects/foo"),
            "/Users/test/projects/foo"
        );
    }

    #[test]
    fn expand_env_var_braced() {
        std::env::set_var("FOO_DIR", "/opt/foo");
        assert_eq!(expand_path_str("${FOO_DIR}/bar"), "/opt/foo/bar");
    }

    #[test]
    fn expand_env_var_plain() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("$HOME/x"), "/Users/test/x");
    }

    #[test]
    fn absolute_path_unchanged() {
        assert_eq!(expand_path_str("/usr/local/bin"), "/usr/local/bin");
    }

    #[test]
    fn cjk_path_preserved() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("/Users/test/项目"), "/Users/test/项目");
        assert_eq!(expand_path_str("~/日本語"), "/Users/test/日本語");
    }

    #[test]
    fn cjk_path_with_env_var() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("$HOME/中文目录"), "/Users/test/中文目录");
    }
}
