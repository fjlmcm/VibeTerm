//! 窗口 IPC(浮窗/菜单语言/聚焦)、外部资源打开白名单、macOS vibrancy。
//! 从 main.rs 拆出(行为不变)。

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use vibeterm_ipc::{IpcError, IpcResult, TaskLocation};

// build_menu / current_menu_lang 仅在 macOS 下定义(调用点都有 cfg 门),
// 导入不分门会在 Windows 上 E0432(v1.1.1 首次发版即栽于此)。
use crate::menu::MenuLang;
#[cfg(target_os = "macos")]
use crate::menu::{build_menu, current_menu_lang};
use crate::{emit_tasks_changed, AppState};

/// macOS:把 NSVisualEffectView underWindowBackground material 装到窗口下层。
/// WebView 设了 transparent → resize 时新扩展区域露出毛玻璃模糊层,
/// 看着像故意的视觉设计,而不是 lag 的死黑/死白。
/// 借鉴 Tabby `references/tabby/app/lib/window.ts:118` setVibrancy(macOSVibrancyType)。
#[cfg(target_os = "macos")]
pub(crate) fn apply_macos_vibrancy(window: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
    if let Err(e) = apply_vibrancy(
        window,
        NSVisualEffectMaterial::UnderWindowBackground,
        Some(NSVisualEffectState::Active),
        None,
    ) {
        tracing::warn!(err = %e, "apply_vibrancy failed");
    }
}

// ============================
// IPC commands — Window
// ============================

#[tauri::command]
pub(crate) async fn open_floating(
    task_id: vibeterm_ipc::TaskId,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> IpcResult<String> {
    let label = format!("floating-{}", chrono_label());
    let builder = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App(format!("floating.html?taskId={task_id}").into()),
    )
    .title(format!("VibeTerm — Task {task_id}"))
    .inner_size(800.0, 600.0)
    .background_color(tauri::window::Color(0x11, 0x11, 0x11, 0xff));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .transparent(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let float_win = builder.build().map_err(|e| IpcError::Unknown {
        trace_id: format!("window:{e}"),
    })?;
    #[cfg(target_os = "macos")]
    apply_macos_vibrancy(&float_win);
    #[cfg(not(target_os = "macos"))]
    let _ = float_win;
    let _ = state
        .tasks
        .set_location(task_id, TaskLocation::Floating(label.clone()));
    emit_tasks_changed(&app, &state.tasks);
    let _ = app.emit(
        "floating_opened",
        serde_json::json!({"label": label, "task_id": task_id}),
    );
    // rebuild menu(windows submenu 含动态浮窗列表)
    #[cfg(target_os = "macos")]
    if let Ok(menu) = build_menu(&app, current_menu_lang(&state)) {
        let _ = app.set_menu(menu);
    }
    Ok(label)
}

#[tauri::command]
pub(crate) async fn close_floating(
    label: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }
    // 找到对应 task 并改回 nowhere(主工作区不主动激活)
    if let Ok(tasks) = state.tasks.list() {
        for t in tasks {
            if let TaskLocation::Floating(ref l) = t.location {
                if l == &label {
                    let _ = state.tasks.set_location(t.id, TaskLocation::Nowhere);
                }
            }
        }
    }
    emit_tasks_changed(&app, &state.tasks);
    let _ = app.emit("floating_closed", &label);
    // rebuild menu
    #[cfg(target_os = "macos")]
    if let Ok(menu) = build_menu(&app, current_menu_lang(&state)) {
        let _ = app.set_menu(menu);
    }
    Ok(())
}

// 前端 setLang() 触发 — 切换顶栏菜单语言并重建。非 macOS 上是 noop。
#[tauri::command]
pub(crate) async fn set_menu_lang(
    lang: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    let l = MenuLang::from_tag(&lang);
    if let Ok(mut g) = state.menu_lang.lock() {
        *g = l;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        if let Ok(menu) = build_menu(&app, l) {
            let _ = app.set_menu(menu);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, l);
    Ok(())
}

#[tauri::command]
pub(crate) async fn focus_window(label: String, app: AppHandle) -> IpcResult<()> {
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}

// 浮窗里按 Cmd+K 等全局快捷键时 → 通知主窗口 + 拉前台 + 触发该 action
// (浮窗内全局快捷键自动拉主窗口前台执行)
#[tauri::command]
pub(crate) async fn invoke_global_action(action: String, app: AppHandle) -> IpcResult<()> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
        let _ = app.emit_to(
            tauri::EventTarget::WebviewWindow {
                label: "main".into(),
            },
            "global_action",
            action,
        );
    }
    Ok(())
}

/// 判断是否为受信任的本地 http URL(精确 host, 防 `http://localhost.evil.com` 前缀绕过).
/// `http://localhost` / `http://127.0.0.1` 后必须紧跟 `/`、`:`(端口)或字符串结束.
pub(crate) fn is_trusted_local_http(url: &str) -> bool {
    for host in ["http://localhost", "http://127.0.0.1"] {
        if let Some(rest) = url.strip_prefix(host) {
            if rest.is_empty() || rest.starts_with('/') || rest.starts_with(':') {
                return true;
            }
        }
    }
    false
}

// Open URL via OS;white-list:仅 https:// + http://localhost*
#[cfg(target_os = "macos")]
pub(crate) fn open_url_safe(_app: &AppHandle, url: &str) {
    if url.starts_with("https://") || is_trusted_local_http(url) {
        if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
            tracing::warn!(url, err = %e, "open_url_safe spawn failed");
        }
    } else {
        tracing::warn!(url, "rejected URL not in whitelist");
    }
}

pub(crate) fn chrono_label() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

// ============================
// 打开外部资源(URL / 文件路径)
// ============================
//
// 单一 command:open_external,先判断是 URL 还是 fs path:
//   - URL 白名单:https:// / http://localhost / http://127.0.0.1
//   - 文件路径必须实际存在(防注入 + 防误触发)
// std::process::Command 是 execve 不走 shell,无需担心元字符注入。
//
// 不放行 file:// URL:终端输出里的链接是 agent/远程程序可伪造的内容,Cmd+Click 一个
// file:///... 会直接交给 `open` 打开任意本地文件(.app/.dmg 等)。本地文件统一走下面的
// fs path 分支(canonicalize + 存在性检查);确需 file:// 的调用方先剥前缀再传路径。
#[tauri::command]
pub(crate) async fn open_external(target: String) -> IpcResult<()> {
    // localhost/127.0.0.1 用精确 host 匹配,防 `http://localhost.evil.com` 前缀绕过.
    let is_url = target.starts_with("https://") || is_trusted_local_http(&target);
    // 非 URL 的 fs path:canonicalize 消除 `../` 穿越歧义,用真实绝对路径打开,
    // 拒绝无法规范化的目标(不存在或非法).
    let resolved_path = if is_url {
        None
    } else {
        std::fs::canonicalize(&target).ok()
    };
    if !is_url && resolved_path.is_none() {
        tracing::warn!(target, "rejected open_external — not in whitelist");
        return Err(IpcError::PermissionDenied {
            reason: "target not in whitelist (need https / localhost / existing fs path)".into(),
        });
    }
    // URL 用原始 target;fs path 用规范化后的绝对路径
    let open_target: &std::ffi::OsStr = match &resolved_path {
        Some(p) => p.as_os_str(),
        None => std::ffi::OsStr::new(&target),
    };
    let spawn_result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(open_target).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open")
            .arg(open_target)
            .spawn()
    } else {
        // windows:cmd /c start "" "<target>" — "" 是 start 的 title 占位
        std::process::Command::new("cmd")
            .args([
                std::ffi::OsStr::new("/c"),
                std::ffi::OsStr::new("start"),
                std::ffi::OsStr::new(""),
                open_target,
            ])
            .spawn()
    };
    spawn_result.map(|_| ()).map_err(|e| IpcError::Unknown {
        trace_id: format!("open_external: {e}"),
    })
}
