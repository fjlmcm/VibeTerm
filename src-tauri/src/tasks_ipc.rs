//! Tasks IPC:任务 CRUD / 通知偏好 / 排序与分屏树写回。从 main.rs 拆出(行为不变)。

use tauri::{AppHandle, Emitter};
use vibeterm_config::NotifyFile;
use vibeterm_ipc::{CreateTaskOpts, IpcError, IpcResult, TaskDto};

use crate::{emit_tasks_changed, map_task_err, refresh_dock_badge, AppState};

// ============================
// IPC commands — Tasks
// ============================

#[tauri::command]
pub(crate) async fn list_tasks(state: tauri::State<'_, AppState>) -> IpcResult<Vec<TaskDto>> {
    state.tasks.list().map_err(map_task_err)
}

#[tauri::command]
pub(crate) async fn create_task(
    opts: CreateTaskOpts,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<TaskDto> {
    let id = state
        .tasks
        .create(opts.name, opts.cwd, opts.worktree)
        .map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    state
        .tasks
        .task_dto(id)
        .map_err(map_task_err)?
        .ok_or(IpcError::NotFound {
            resource: "task".into(),
            id: id.to_string(),
        })
}

#[tauri::command]
pub(crate) async fn close_task(
    id: vibeterm_ipc::TaskId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    let term_ids = state.tasks.close(id).map_err(map_task_err)?;
    for tid in &term_ids {
        if let Err(e) = state.terminals.close(*tid) {
            tracing::warn!(err = %e, terminal_id = %tid, "close_task: terminal close failed");
        }
    }
    // 清掉相应 status detector 注册 — 全局 tick 任务随即不再 tick 它们
    if let Ok(mut map) = state.status_detectors.lock() {
        for tid in &term_ids {
            map.remove(tid);
        }
    }
    emit_tasks_changed(&app, &state.tasks);
    // 关掉的任务可能是 Done(未看)→ 刷新 Dock 角标
    refresh_dock_badge(&app, &state.tasks);
    Ok(())
}

#[tauri::command]
pub(crate) async fn rename_task(
    id: vibeterm_ipc::TaskId,
    name: String,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.rename(id, name).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

#[tauri::command]
pub(crate) async fn pin_task(
    id: vibeterm_ipc::TaskId,
    pinned: bool,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.pin(id, pinned).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

/// 切换 task 通知静音(持久化到 tasks.json).
#[tauri::command]
pub(crate) async fn set_task_notify_muted(
    id: vibeterm_ipc::TaskId,
    muted: bool,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state
        .tasks
        .set_notify_muted(id, muted)
        .map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

/// 读 notify.toml.
#[tauri::command]
pub(crate) async fn get_notify_prefs() -> IpcResult<NotifyFile> {
    Ok(NotifyFile::load())
}

/// 整体覆盖写 notify.toml. atomic_write 保证 reload 安全.
#[tauri::command]
pub(crate) async fn save_notify_prefs(prefs: NotifyFile) -> IpcResult<()> {
    prefs.save().map_err(|e| {
        tracing::warn!(err = %e, "save_notify_prefs failed");
        IpcError::Unknown {
            trace_id: format!("save_notify_prefs:{e}"),
        }
    })
}

/// 把插件的 `PermissionState` 显式映射为 TS 契约的固定字符串字面量,
/// 不依赖 `Debug`/`Display` 格式(否则 `Prompt` 会序列化成契约外的 "prompt").
/// wire 值严格落在 TS `NotifyPermissionState = "granted" | "denied" | "default"`.
pub(crate) fn permission_state_str(s: tauri_plugin_notification::PermissionState) -> &'static str {
    use tauri_plugin_notification::PermissionState;
    match s {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        // Prompt / PromptWithRationale = "尚未授权", 对应契约的 "default"
        PermissionState::Prompt | PermissionState::PromptWithRationale => "default",
    }
}

/// 查询系统通知权限. macOS 首次需要授权.
/// 返回 "granted" | "denied" | "default" (未问过).
#[tauri::command]
pub(crate) async fn notify_permission(app: AppHandle) -> IpcResult<String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .permission_state()
        .map(|s| permission_state_str(s).to_string())
        .map_err(|e| {
            tracing::warn!(err = %e, "notify_permission failed");
            IpcError::Unknown {
                trace_id: format!("notify_permission:{e}"),
            }
        })
}

/// 主动请求通知权限. macOS 第一次会弹系统授权对话框.
#[tauri::command]
pub(crate) async fn request_notify_permission(app: AppHandle) -> IpcResult<String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .request_permission()
        .map(|s| permission_state_str(s).to_string())
        .map_err(|e| {
            tracing::warn!(err = %e, "request_notify_permission failed");
            IpcError::Unknown {
                trace_id: format!("request_notify_permission:{e}"),
            }
        })
}

/// 声音预览/播放数据. bytes 为 base64 (避免 Vec<u8> 走 JSON 数字数组的 6× 膨胀).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct NotifySoundData {
    /// 音频 MIME (audio/aiff, audio/wav, audio/mpeg, audio/ogg, audio/mp4, ...).
    pub(crate) mime: String,
    /// 原始音频字节的 base64.
    pub(crate) base64: String,
}

#[tauri::command]
pub(crate) async fn reorder_tasks(
    order: Vec<vibeterm_ipc::TaskId>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.reorder(order).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

#[tauri::command]
pub(crate) async fn set_active_task(
    id: vibeterm_ipc::TaskId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.set_active_main(id).map_err(map_task_err)?;
    let _ = app.emit("active_task_changed", id);
    // 切换当前任务 → 重算各 task 聚合 status(切出的完成任务变 Done、切入的变 Idle)+ 刷新角标。
    emit_tasks_changed(&app, &state.tasks);
    refresh_dock_badge(&app, &state.tasks);
    Ok(())
}

// 写回任务的分屏布局,任意窗口可调,emit tasks_changed 同步另一窗
#[tauri::command]
pub(crate) async fn set_task_split_tree(
    id: vibeterm_ipc::TaskId,
    tree: vibeterm_ipc::SplitNode,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.set_split_tree(id, tree).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}
