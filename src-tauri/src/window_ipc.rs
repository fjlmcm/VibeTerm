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

/// Darwin 内核 major 版本(macOS 26.x = Darwin 25);非 macOS / 读取失败返回 0。
///
/// 前端据此选 xterm 渲染后端:macOS 26.x WebKit 存在 WebGL 纹理对象级损坏
/// (xterm.js#5816,字形图集随机花屏),Darwin >= 25 时降级 canvas 渲染器。
/// WKWebView 的 UA 版本号已冻结("10_15_7"),前端拿不到真实系统版本,只能后端给。
/// 用 cfg! 运行时门而非 cfg 属性:全平台都编译,Windows 不会再踩 cfg 符号盲区。
#[tauri::command]
pub(crate) fn darwin_major_version() -> u32 {
    if !cfg!(target_os = "macos") {
        return 0;
    }
    std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().split('.').next()?.parse().ok())
        .unwrap_or(0)
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
        if let Err(e) = tauri_plugin_opener::open_url(url, None::<&str>) {
            tracing::warn!(url, err = %e, "open_url_safe failed");
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
//   - 文件路径:只收绝对路径或 `~/` 前缀(后端展开 HOME),canonicalize 后必须实际存在。
//     目录 → 系统文件管理器打开;普通文件 → 只在文件管理器里**定位**(reveal),不直接执行。
//     终端输出可被 agent / 远程程序伪造,Cmd+Click 一个 .app/.dmg/.command 若直接 open 就是
//     任意代码执行。
// 三平台统一走 tauri_plugin_opener 的 Rust API(macOS NSWorkspace / Windows ShellExecuteW /
// Linux xdg-open),不经过 cmd.exe 等 shell,URL 里的 & | ^ 不会被当命令分隔符。
//
// 不放行 file:// URL:确需 file:// 的调用方先剥前缀再传路径,走上面的 fs path 分支。
#[tauri::command]
pub(crate) async fn open_external(target: String) -> IpcResult<()> {
    // localhost/127.0.0.1 用精确 host 匹配,防 `http://localhost.evil.com` 前缀绕过.
    if target.starts_with("https://") || is_trusted_local_http(&target) {
        return tauri_plugin_opener::open_url(&target, None::<&str>).map_err(|e| {
            IpcError::Unknown {
                trace_id: format!("open_external: {e}"),
            }
        });
    }
    // fs path:`~/` 展开 HOME;相对路径拒绝(app 进程 cwd 与终端 cwd 无关,解析必错);
    // canonicalize 消除 `../` 穿越歧义并要求目标真实存在。
    let expanded = match target.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)),
        None => Some(std::path::PathBuf::from(&target)),
    };
    let resolved = expanded
        .filter(|p| p.is_absolute())
        .and_then(|p| std::fs::canonicalize(p).ok());
    let Some(path) = resolved else {
        tracing::warn!(target, "rejected open_external — not in whitelist");
        return Err(IpcError::PermissionDenied {
            reason: "target not in whitelist (need https / localhost / existing absolute fs path)"
                .into(),
        });
    };
    let result = if path.is_dir() {
        tauri_plugin_opener::open_path(&path, None::<&str>)
    } else {
        tauri_plugin_opener::reveal_item_in_dir(&path)
    };
    result.map_err(|e| IpcError::Unknown {
        trace_id: format!("open_external: {e}"),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn darwin_major_version_sane() {
        let v = super::darwin_major_version();
        if cfg!(target_os = "macos") {
            // 支持下限 macOS 11 = Darwin 20;解析失败会错给 0,这里兜住
            assert!(v >= 20, "expected Darwin >= 20 on macOS, got {v}");
        } else {
            assert_eq!(v, 0);
        }
    }
}
