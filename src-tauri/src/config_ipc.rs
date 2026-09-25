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
pub(crate) async fn save_env_file(file: EnvFile) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("env_save:{e}"),
    })?;
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
/// 返回识别到的 agent kind(`claude` / `codex` / ...),未识别或无 pid → None.
#[tauri::command]
pub(crate) async fn detect_agent_for_terminal(
    terminal_id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<Option<String>> {
    let kind = state.terminals.pid_of(terminal_id).and_then(|pid| {
        vibeterm_status::ProcessTable::snapshot()
            .detect_agent_for_shell(pid)
            .map(|k| k.as_str().to_string())
    });
    tracing::info!(terminal_id, agent_kind = ?kind, "detect_agent_for_terminal");
    Ok(kind)
}

/// 重置所有 prompts 为内置默认值. 删 prompts.toml, 下次 load 返回 default.
#[tauri::command]
pub(crate) async fn reset_prompts() -> IpcResult<PromptsFile> {
    let p = vibeterm_config::prompts_toml_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("prompts_path:{e}"),
    })?;
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| IpcError::Unknown {
            trace_id: format!("prompts_rm:{e}"),
        })?;
    }
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
pub(crate) async fn save_prompts(file: PromptsFile) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("prompts_save:{e}"),
    })?;
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
