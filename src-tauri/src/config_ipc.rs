//! Theme / Config / Keybindings / Prompts / Custom Actions / statusline IPC。
//! 从 main.rs 拆出(行为不变)。

use tauri::{AppHandle, Emitter};
use vibeterm_config::actions::{ActionMode, ActionsFile};
use vibeterm_config::{Config, EnvFile, KeybindingsFile, PromptsFile, Theme};
use vibeterm_ipc::{IpcError, IpcResult, TerminalId};

use crate::{emit_tasks_changed, map_task_err, AppState};

#[tauri::command]
pub(crate) async fn get_config() -> IpcResult<Config> {
    Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })
}

#[tauri::command]
pub(crate) async fn set_shell_integration(enabled: bool) -> IpcResult<()> {
    let mut cfg = Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })?;
    cfg.shell_integration = enabled;
    cfg.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("save:{e}"),
    })?;
    // 下次 spawn 的终端生效(已开终端不动);无需 emit。
    Ok(())
}

/// 启动时自动检查更新开关。关闭后开箱完全不主动联网。
#[tauri::command]
pub(crate) async fn set_auto_check_updates(enabled: bool) -> IpcResult<()> {
    let mut cfg = Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })?;
    cfg.auto_check_updates = enabled;
    cfg.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("save:{e}"),
    })?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn set_active_theme(id: String, app: AppHandle) -> IpcResult<Theme> {
    let mut cfg = Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })?;
    cfg.active_theme = id.clone();
    cfg.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("save:{e}"),
    })?;
    let theme = vibeterm_config::get_theme(&id);
    let _ = app.emit("theme_changed", &theme);
    Ok(theme)
}

#[tauri::command]
pub(crate) async fn list_themes() -> IpcResult<Vec<Theme>> {
    Ok(vibeterm_config::load_all_themes())
}

#[tauri::command]
pub(crate) async fn get_theme(id: String) -> IpcResult<Theme> {
    Ok(vibeterm_config::get_theme(&id))
}

// env.toml 管理
#[tauri::command]
pub(crate) async fn get_env_file() -> IpcResult<EnvFile> {
    EnvFile::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("env_load:{e}"),
    })
}

#[tauri::command]
pub(crate) async fn save_env_file(file: EnvFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("env_save:{e}"),
    })?;
    let _ = app.emit("env_changed", ());
    Ok(())
}

// keybindings.toml
#[tauri::command]
pub(crate) async fn get_keybindings() -> IpcResult<KeybindingsFile> {
    Ok(KeybindingsFile::load())
}

#[tauri::command]
pub(crate) async fn save_keybindings(file: KeybindingsFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("kb_save:{e}"),
    })?;
    let _ = app.emit("keybindings_changed", ());
    Ok(())
}

/// 重置所有快捷键为内置默认值. 删 keybindings.toml, 下次 load 返回 default.
/// 立即对指定 terminal 的 shell pid 做一次 agent 嗅探, 不等 3s 后台轮询.
/// PromptPicker 弹出时调一次, 确保 kind 与"用户当前焦点所在终端"一致.
/// 返回完整诊断信息: 命中的 agent + pid + pgid + 整个 process group cmdlines,
/// 前端 console 直接展示, 不需要后端日志.
#[derive(serde::Serialize, specta::Type)]
pub(crate) struct DetectAgentResult {
    agent_kind: Option<String>,
    shell_pid: Option<u32>,
    pgid: Option<u32>,
    cmdlines: Vec<String>,
    note: String,
}

#[tauri::command]
pub(crate) async fn detect_agent_for_terminal(
    terminal_id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<DetectAgentResult> {
    let result = match state.terminals.pid_of(terminal_id) {
        Some(pid) => {
            let (kind, diag) = vibeterm_status::detect_agent_with_diagnostics(pid);
            tracing::info!(
                terminal_id, pid, pgid = ?diag.pgid, agent_kind = ?kind,
                cmdlines = ?diag.cmdlines,
                "detect_agent_for_terminal"
            );
            DetectAgentResult {
                agent_kind: kind.map(|k| k.as_str().to_string()),
                shell_pid: Some(diag.shell_pid),
                pgid: diag.pgid,
                cmdlines: diag.cmdlines,
                note: diag.note,
            }
        }
        None => DetectAgentResult {
            agent_kind: None,
            shell_pid: None,
            pgid: None,
            cmdlines: vec![],
            note: format!("terminal {terminal_id}: pid_of returned None"),
        },
    };
    // 诊断已通过上面的 tracing::info! 输出. 额外写固定 /tmp 文件方便排查,
    // 但 cmdlines 可能含敏感命令行参数 + /tmp 世界可读, 故仅限 debug 构建.
    #[cfg(debug_assertions)]
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/vibeterm-detect.log")
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(
            f,
            "ts={} terminal_id={} agent={:?} pid={:?} pgid={:?} cmdlines={:?} note={}",
            ts,
            terminal_id,
            result.agent_kind,
            result.shell_pid,
            result.pgid,
            result.cmdlines,
            result.note,
        );
    }
    Ok(result)
}

/// 重置所有 prompts 为内置默认值. 删 prompts.toml, 下次 load 返回 default.
#[tauri::command]
pub(crate) async fn reset_prompts(app: AppHandle) -> IpcResult<PromptsFile> {
    let p = vibeterm_config::prompts_toml_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("prompts_path:{e}"),
    })?;
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| IpcError::Unknown {
            trace_id: format!("prompts_rm:{e}"),
        })?;
    }
    let _ = app.emit("prompts_changed", ());
    Ok(PromptsFile::load())
}

#[tauri::command]
pub(crate) async fn reset_keybindings(app: AppHandle) -> IpcResult<KeybindingsFile> {
    let p = vibeterm_config::keybindings_toml_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("kb_path:{e}"),
    })?;
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| IpcError::Unknown {
            trace_id: format!("kb_rm:{e}"),
        })?;
    }
    let _ = app.emit("keybindings_changed", ());
    Ok(KeybindingsFile::load())
}

// prompts.toml
#[tauri::command]
pub(crate) async fn get_prompts() -> IpcResult<PromptsFile> {
    Ok(PromptsFile::load())
}

#[tauri::command]
pub(crate) async fn save_prompts(file: PromptsFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("prompts_save:{e}"),
    })?;
    let _ = app.emit("prompts_changed", ());
    Ok(())
}

// ---- Custom Actions ----

/// 启动时拿上次激活的 task id
#[tauri::command]
pub(crate) async fn get_active_task(
    state: tauri::State<'_, AppState>,
) -> IpcResult<Option<vibeterm_ipc::TaskId>> {
    Ok(state.tasks.active_main())
}

#[tauri::command]
pub(crate) async fn get_actions() -> IpcResult<ActionsFile> {
    Ok(ActionsFile::load())
}

/// 布局模板列表(命令面板任务预设)。每次读盘,编辑 layouts.toml 即时生效。
#[tauri::command]
pub(crate) async fn list_layouts() -> IpcResult<Vec<vibeterm_config::LayoutTemplate>> {
    Ok(vibeterm_config::LayoutsFile::load().layouts)
}

#[tauri::command]
pub(crate) async fn save_actions(file: ActionsFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("actions_save:{e}"),
    })?;
    let _ = app.emit("actions_changed", ());
    Ok(())
}

/// 执行一个 action。
///
/// 模式:
///   - current_terminal: 写到指定 terminal_id(必传),自动追加 \n
///   - new_task: 创建新 task,命名 "<title>",cwd=$HOME,后台 spawn 由前端触发
///     (本命令只创建 task 并写回 command;前端拿 task_id 后 spawn + write)
///   - insert: 写到指定 terminal_id,不加 \n
///
/// 返回:
///   - current_terminal / insert → ExecuteActionResult::WrittenTo { terminal_id }
///   - new_task → ExecuteActionResult::NewTask { task_id, command }
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExecuteActionResult {
    WrittenTo {
        terminal_id: vibeterm_ipc::TerminalId,
    },
    NewTask {
        task_id: vibeterm_ipc::TaskId,
        command: String,
    },
}

#[tauri::command]
pub(crate) async fn execute_action(
    action_id: String,
    terminal_id: Option<vibeterm_ipc::TerminalId>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<ExecuteActionResult> {
    let actions = ActionsFile::load();
    let action = actions
        .actions
        .into_iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| IpcError::NotFound {
            resource: "action".into(),
            id: action_id.clone(),
        })?;

    match action.mode {
        ActionMode::CurrentTerminal | ActionMode::Insert => {
            let tid = terminal_id.ok_or(IpcError::PermissionDenied {
                reason: "current_terminal/insert mode requires terminal_id".into(),
            })?;
            let mut payload = action.command.into_bytes();
            if matches!(action.mode, ActionMode::CurrentTerminal) {
                payload.push(b'\n');
            }
            state
                .terminals
                .write(tid, &payload)
                .map_err(|e| IpcError::Unknown {
                    trace_id: format!("write:{e}"),
                })?;
            Ok(ExecuteActionResult::WrittenTo { terminal_id: tid })
        }
        ActionMode::NewTask => {
            let id = state
                .tasks
                .create(action.title.clone(), None, None)
                .map_err(map_task_err)?;
            emit_tasks_changed(&app, &state.tasks);
            Ok(ExecuteActionResult::NewTask {
                task_id: id,
                command: action.command,
            })
        }
    }
}

// ---- statusline.toml IO ----

#[tauri::command]
pub(crate) async fn get_statusline_config() -> IpcResult<vibeterm_config::StatusLineFile> {
    Ok(vibeterm_config::StatusLineFile::load())
}

#[tauri::command]
pub(crate) async fn save_statusline_config(
    config: vibeterm_config::StatusLineFile,
    app: AppHandle,
) -> IpcResult<()> {
    config.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("statusline save: {e}"),
    })?;
    let _ = app.emit("statusline_config_changed", ());
    Ok(())
}
