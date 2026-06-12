//! VibeTerm — Tauri app 入口
//!
//! 本文件:AppState、Terminal/Tasks/Theme/Window IPC、setup 与各后台 tick。
//! 其余 IPC 与子系统按关注点拆在同级模块:
//!   menu(菜单栏 i18n)/ notify(通知+声音+完成轮询)/ events(G7 事件流)/
//!   git_ipc(git worktree/状态)/ updates(手动更新检查)/ agent_ipc(agent 嗅探查询)。

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "macos")]
use tauri::RunEvent;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::EnvFilter;

use vibeterm_core::{TaskRegistry, TerminalRegistry};
use vibeterm_ipc::TaskLocation;

mod agent_ipc;
mod background;
mod clipboard_files;
mod config_ipc;
mod events;
mod git_ipc;
mod menu;
mod notify;
mod pty_ipc;
mod state;
mod tasks_ipc;
#[cfg(test)]
mod ts_export;
mod updates;
mod window_ipc;

// pub(crate) glob 重导出各模块项:main.rs 原有调用点 / generate_handler! 列表零改动,
// 跨模块的 `crate::xxx` 路径(如 events.rs 用 crate::atomic_write)也经重导出继续成立。
pub(crate) use agent_ipc::*;
pub(crate) use config_ipc::*;
pub(crate) use events::*;
pub(crate) use git_ipc::*;
pub(crate) use menu::*;
pub(crate) use notify::*;
pub(crate) use pty_ipc::*;
pub(crate) use state::*;
pub(crate) use tasks_ipc::*;
pub(crate) use updates::*;
pub(crate) use window_ipc::*;

// ============================
// 日志
// ============================

/// 编译期默认日志 level。
///
/// - debug build: `info` — 开发期需看到 spawn/close 等生命周期事件
/// - release build: `warn` — 生产只保留错误与异常信号,降低噪声
///
/// 用户可用 `RUST_LOG` 覆盖,语义同 `tracing_subscriber::EnvFilter`:
///   - `RUST_LOG=debug` 全 crate 提到 debug
///   - `RUST_LOG=vibeterm_status=trace,info` 单 crate 详查
///   - `RUST_LOG=off` 完全静默
fn default_log_filter() -> &'static str {
    if cfg!(debug_assertions) {
        "info"
    } else {
        "warn"
    }
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_filter()));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

/// macOS `.app` 从 Finder 启动时只继承系统 PATH (/usr/bin:/bin:...),
/// 看不到 /opt/homebrew/bin、~/.local/bin、~/.cargo/bin、nvm node 等用户安装路径,
/// 导致 which::which("claude") / PTY spawn 找不到 AI CLI.
/// 修法: 调起用户 login shell (interactive) 抓 PATH 写回当前进程.
/// (npm `fix-path` 包同款思路, VS Code/Cursor/Atom 也都这么做)
#[cfg(target_os = "macos")]
fn fix_path_for_gui_launch() {
    // 已经有完整 PATH (dev 模式 / 用户从 terminal 启动) → 跳过避免无谓 shell 启动
    let current = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let homebrew_present =
        current.contains("/opt/homebrew/bin") || current.contains("/usr/local/bin");
    let user_local_present = !home.is_empty() && current.contains(&format!("{home}/.local/bin"));
    if homebrew_present || user_local_present {
        return;
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    // 用 sentinel 提取干净的 PATH; -ilc = interactive login command, 强制 source .zshrc/.bash_profile
    let cmd = "printf '__VT_PATH_START__%s__VT_PATH_END__' \"$PATH\"";
    let out = match std::process::Command::new(&shell)
        .args(["-ilc", cmd])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("fix_path: spawn {shell} failed: {e}");
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let path = match (
        stdout.find("__VT_PATH_START__"),
        stdout.find("__VT_PATH_END__"),
    ) {
        (Some(a), Some(b)) if a + "__VT_PATH_START__".len() <= b => {
            &stdout[a + "__VT_PATH_START__".len()..b]
        }
        _ => {
            tracing::warn!("fix_path: sentinel not found in shell output");
            return;
        }
    };
    if path.is_empty() {
        return;
    }
    tracing::info!("fix_path: PATH inherited from {shell} (len={})", path.len());
    std::env::set_var("PATH", path);
}

#[cfg(not(target_os = "macos"))]
fn fix_path_for_gui_launch() {}

/// 进程级 CJK locale 兜底 —— **从底层一次性解决**所有"中文乱码"类问题的根因修复。
///
/// macOS 从 Dock/Launchpad/Finder 启动的 GUI app 不继承用户 shell 的 `LANG`/`LC_*`,整个进程
/// 落到 C/POSIX locale。后果遍布全栈:zsh 把中文路径在 `%~` prompt 里转义成 `\M-^F\M-^M…`
/// 乱码;`lsof`/`ps` 输出的中文路径/命令行被转义 → cwd 解析 / agent 识别失败。过去是逐个 spawn
/// 点打补丁(lsof 设了、ps 没设、PTY 没设),散且易漏。这里在 `main()` **最早期、建任何线程前**
/// (`set_var` 此刻单线程安全)一次性把 `LC_CTYPE` 兜成 UTF-8 —— 之后 PTY shell、lsof、ps、git、
/// which 等**所有子进程一律继承**,无需再各自 `.env()`。
///
/// 只设 `LC_CTYPE`(字符分类),**不碰 `LANG`/`LC_MESSAGES`**:乱码只与字符处理有关,不应顺带把
/// 程序消息、原生菜单语言(`MenuLang` 读 `LANG`)强行变英文。值用裸 `UTF-8`(macOS 专有、必然可用),
/// 不臆造 `zh_Hans_CN.UTF-8` 这类可能不存在、`setlocale` 失败反落 C 的 locale。
/// 已有任一 UTF-8 locale(用户从 terminal 启动 / 显式设过)→ 尊重,不动。
#[cfg(target_os = "macos")]
fn fix_locale_for_gui_launch() {
    if !locale_env_has_utf8(&std::collections::HashMap::new(), |k| std::env::var(k).ok()) {
        std::env::set_var("LC_CTYPE", "UTF-8");
        tracing::info!("fix_locale: 进程无 UTF-8 locale,兜底 LC_CTYPE=UTF-8(GUI 启动)");
    }
}

#[cfg(not(target_os = "macos"))]
fn fix_locale_for_gui_launch() {}

// ============================
// main
// ============================
fn main() {
    init_tracing();
    // CJK 根因兜底:必须最早(建任何线程前 set_var 才安全)。让全进程及所有子进程继承 UTF-8 ctype。
    fix_locale_for_gui_launch();
    // 常驻音频线程(通知声音用 rodio 进程内播放,替代反复 fork 的 afplay)。早启动,后续直接 send。
    init_audio_thread();
    // 必须早于任何 which::which / PTY spawn — 否则 GUI 启动的 .app 看不到用户 PATH
    fix_path_for_gui_launch();

    // 创建配置目录(首启动)
    if let Err(e) = vibeterm_config::config_dir() {
        eprintln!("config dir error: {e}");
    }

    let state = AppState {
        terminals: Arc::new(TerminalRegistry::new()),
        tasks: Arc::new(TaskRegistry::new()),
        menu_lang: std::sync::Mutex::new(MenuLang::from_env()),
        status_detectors: std::sync::Mutex::new(std::collections::HashMap::new()),
        last_notify: std::sync::Mutex::new(None),
        last_agent_completed: std::sync::Mutex::new(std::collections::HashMap::new()),
        last_persistent_remind: std::sync::Mutex::new(None),
    };

    // 首启动:若无任务则创建一个 Default
    if let Ok(list) = state.tasks.list() {
        if list.is_empty() {
            let name = match std::env::var("LANG")
                .unwrap_or_default()
                .to_lowercase()
                .as_str()
            {
                s if s.starts_with("zh") => "默认".to_string(),
                s if s.starts_with("ja") => "デフォルト".to_string(),
                _ => "Default".to_string(),
            };
            let _ = state.tasks.create(name, None, None);
        }
    }

    tauri::Builder::default()
        // 必须第一个注册: 第二次启动时聚焦已有主窗口而非起平行实例 —— 平行实例会与
        // 当前实例 last-writer-wins 抢 tasks.json, 把用户已删的任务覆盖回来.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        // 应用内自动更新 + 安装后重启. 🔴 零侵入: 插件只注册能力,实际 check()/install/restart
        // 全部仅在用户于设置·更新页手动点击时由前端调用,无任何启动期或后台自动触发.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // Terminal
            start_pty,
            write_pty,
            resize_pty,
            close_pty,
            spawn_terminal_in_task,
            detach_terminal,
            get_scrollback,
            terminal_size,
            paste_clipboard,
            write_clipboard_text,
            get_clipboard_images_dir,
            open_clipboard_images_dir,
            clear_clipboard_images,
            // Tasks
            list_tasks,
            create_task,
            close_task,
            rename_task,
            pin_task,
            reorder_tasks,
            set_active_task,
            set_task_split_tree,
            set_task_notify_muted,
            // 通知偏好
            get_notify_prefs,
            save_notify_prefs,
            notify_permission,
            request_notify_permission,
            preview_notify_sound,
            list_builtin_sounds,
            // Git worktree
            git_is_repo,
            git_repo_root,
            git_list_branches,
            git_add_worktree,
            git_remove_worktree,
            // Theme / Config
            get_config,
            set_shell_integration,
            set_auto_check_updates,
            set_active_theme,
            list_themes,
            get_theme,
            get_env_file,
            save_env_file,
            get_keybindings,
            save_keybindings,
            reset_keybindings,
            detect_agent_for_terminal,
            reset_prompts,
            get_prompts,
            save_prompts,
            // Custom Actions
            get_actions,
            save_actions,
            execute_action,
            // layout snapshot
            get_active_task,
            // Window
            open_floating,
            close_floating,
            focus_window,
            invoke_global_action,
            // i18n
            set_menu_lang,
            // AI CLI 检测
            detect_ai_clis,
            debug_log,
            // Agent watch (v1+v2+v3) — Claude usage_cache + Claude session + Codex session
            get_claude_usage_cache,
            get_claude_session,
            get_codex_session,
            // 按 cwd 精确查 session (per-active-terminal 语义)
            get_claude_session_by_cwd,
            get_codex_session_by_cwd,
            agent_usage_by_cwd,
            get_claude_block_by_cwd,
            get_codex_block_by_cwd,
            get_claude_tokens_today,
            get_usage_stats,
            save_png_file,
            get_claude_plan,
            // v4: cwd + git status (按需调, 非常驻 watcher)
            get_terminal_cwd,
            git_status_brief,
            git_stash_count,
            git_diff,
            gh_pr_status,
            read_events,
            list_layouts,
            agent_resume_command,
            save_scrollback,
            load_scrollback,
            // 状态栏自定义配置
            get_statusline_config,
            save_statusline_config,
            // 打开外部 URL / 文件
            open_external,
            // 设置·更新页:软件版本检查 + 模型价格更新(手动, 仅点按钮时联网)
            check_app_update,
            get_pricing_status,
            update_model_pricing,
            reset_model_pricing,
        ])
        .setup(|app| {
            // agent 状态走纯嗅探(OSC 标题 spinner + 输出时序)+ 只读文件监听, 不再装/起任何
            // hook server, 零侵入: 默认不碰 ~/.claude / ~/.codex, 也不会被外部会话污染.

            // G7 事件流:启动期预热 EventLog(在此同步线程做一次性文件截尾/打开),
            // 避免首个 read_events IPC 在 tokio worker 线程上触发同步 I/O.
            let _ = EventLog::global();

            // 启动加载已保存的模型价格覆盖(用户曾手动"更新模型价格"过). 纯读本地 config 文件, 不联网.
            if let Ok(path) = vibeterm_config::pricing_json_path() {
                if let Ok(bytes) = std::fs::read(&path) {
                    match serde_json::from_slice::<
                        vibeterm_agent_watch::claude::pricing::PricingTable,
                    >(&bytes)
                    {
                        Ok(table) => {
                            vibeterm_agent_watch::claude::pricing::set_pricing_override(table)
                        }
                        Err(e) => tracing::warn!("ignore corrupt pricing.json: {e}"),
                    }
                }
            }

            background::start_background_tasks(&app.handle().clone());

            // 主窗口:平台特异创建 — macOS 用 Overlay titleBar(原生 traffic lights),
            // 其它平台保持 decorations:false 走自渲 titleBar(Win 风格右上三按钮)
            // background_color #111 让原生窗口在 webview 加载前就是暗色,不闪白
            let main_builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("VibeTerm")
                    .inner_size(1100.0, 720.0)
                    .min_inner_size(800.0, 500.0)
                    .background_color(tauri::window::Color(0x11, 0x11, 0x11, 0xff));
            // 历史注释提到关 drag-drop handler 是为状态栏 widget 排序; 但 solid-dnd 用
            // pointer events (pointerdown/move/up), 与 HTML5 native DnD 无关. Tauri
            // native drag-drop 必须开 — 否则 WebView 自己处理 drop, 拖图片进终端
            // 会被当成浏览器导航直接打开文件 (terminal/index.tsx 的 onDragDropEvent 监听失效).
            #[cfg(target_os = "macos")]
            let main_builder = main_builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                // wry PR #1662:transparent(true) 触发 WKWebView drawsBackground=false,
                // 修 resize 时 WebView 渲染滞后露白底的问题(已用 NSWindow.bg #111 兜底显示)
                .transparent(true);
            #[cfg(not(target_os = "macos"))]
            let main_builder = main_builder.decorations(false);
            let main_win = main_builder
                .build()
                .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
            #[cfg(target_os = "macos")]
            apply_macos_vibrancy(&main_win);
            #[cfg(not(target_os = "macos"))]
            let _ = main_win;

            // macOS 完整菜单栏 + 派发到 web
            #[cfg(target_os = "macos")]
            {
                let lang = app
                    .try_state::<AppState>()
                    .map(|s| current_menu_lang(&s))
                    .unwrap_or(MenuLang::En);
                let menu = build_menu(app.handle(), lang)?;
                app.set_menu(menu)?;
                app.on_menu_event(|app_handle, ev| {
                    let id = ev.id().0.clone();
                    tracing::debug!(menu_id = %id, "menu event");
                    match id.as_str() {
                        "open_config_dir" => {
                            if let Ok(dir) = vibeterm_config::config_dir() {
                                #[cfg(target_os = "macos")]
                                let _ = std::process::Command::new("open").arg(dir).spawn();
                            }
                        }
                        "open_github" => open_url_safe(app_handle, "https://github.com"),
                        "open_issues" => open_url_safe(app_handle, "https://github.com"),
                        "open_privacy" => open_url_safe(app_handle, "https://github.com"),
                        "focus_main" => {
                            if let Some(w) = app_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        // 浮窗 focus(id 格式:focus_floating:<label>)
                        id if id.starts_with("focus_floating:") => {
                            let label = &id["focus_floating:".len()..];
                            if let Some(w) = app_handle.get_webview_window(label) {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        // 其余:转给 main 窗口的 global_action listener
                        _ => {
                            if let Some(main) = app_handle.get_webview_window("main") {
                                let _ = main.show();
                                let _ = main.set_focus();
                                let _ = app_handle.emit_to(
                                    tauri::EventTarget::WebviewWindow {
                                        label: "main".into(),
                                    },
                                    "global_action",
                                    id,
                                );
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // 主窗口焦点变化:① 同步 window_focused —— 失焦时当前选中 task 完成会显 Done(未看)
            // 并计入 Dock 角标(用户在别的 app 也能从 Dock 看到),聚焦时标当前 task 已读;
            // ② 聚焦 + NOTIFY_FOCUS_GRACE 内有 last_notify → 通知前端切 task(桌面通知无 click callback 的近似)。
            if let tauri::WindowEvent::Focused(focused) = event {
                if window.label() == "main" {
                    tracing::info!(
                        focused = *focused,
                        "WindowEvent::Focused(主窗口焦点事件触发)"
                    );
                    let app = window.app_handle().clone();
                    if let Some(state) = app.try_state::<AppState>() {
                        if state.tasks.set_window_focused(*focused).unwrap_or(false) {
                            emit_tasks_changed(&app, &state.tasks);
                            refresh_dock_badge(&app, &state.tasks);
                        }
                        if *focused {
                            let task = state.last_notify.lock().ok().and_then(|mut g| {
                                g.take().filter(|(_, t)| t.elapsed() < NOTIFY_FOCUS_GRACE)
                            });
                            if let Some((task_id, _)) = task {
                                let _ = app.emit(
                                    "notification_focus_target",
                                    serde_json::json!({"task_id": task_id}),
                                );
                            }
                        }
                    }
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label().to_string();
                if cfg!(target_os = "macos") && label == "main" {
                    // macOS:关主窗 = 隐藏而非真退,符合 Cocoa 惯例
                    tracing::info!("main close intercepted -> hide");
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                // 浮窗系统关 = 与右键"回到主窗口"同款流程 —
                // 把 task.location 改回 Nowhere + emit tasks_changed,
                // 让主窗 onTasksChanged 自动 setActive 召回到右侧。
                if label.starts_with("floating-") {
                    let app = window.app_handle().clone();
                    let label_for_async = label.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(tasks) = state.tasks.list() {
                                for t in tasks {
                                    if let TaskLocation::Floating(ref l) = t.location {
                                        if l == &label_for_async {
                                            let _ = state
                                                .tasks
                                                .set_location(t.id, TaskLocation::Nowhere);
                                        }
                                    }
                                }
                            }
                            emit_tasks_changed(&app, &state.tasks);
                            let _ = app.emit("floating_closed", &label_for_async);
                            #[cfg(target_os = "macos")]
                            if let Ok(menu) = build_menu(&app, current_menu_lang(&state)) {
                                let _ = app.set_menu(menu);
                            }
                        }
                    });
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error building tauri app")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            RunEvent::ExitRequested { code, api, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            _ => {
                let _ = (app, Duration::from_secs(0));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认 filter 必须能被 EnvFilter 解析(否则 release 启动会 panic)
    #[test]
    fn default_log_filter_parses_cleanly() {
        let f = default_log_filter();
        EnvFilter::new(f);
    }

    /// debug build → info,release build → warn。
    /// `cargo test` 默认以 debug profile 编译,故此处必为 "info"。
    #[test]
    fn default_log_filter_is_info_in_debug_build() {
        assert_eq!(default_log_filter(), "info");
    }
}
