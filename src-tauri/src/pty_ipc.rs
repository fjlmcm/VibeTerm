//! Terminal/PTY IPC:spawn 链(含 LazyChannelSink 嗅探 sink 与统一收口 reap)、
//! G5 scrollback 快照、剪贴板粘贴、终端 cwd 诊断。从 main.rs 拆出(行为不变)。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use vibeterm_config::{Config, EnvFile};
use vibeterm_core::TaskRegistry;
use vibeterm_ipc::{IpcError, IpcResult, SpawnPtyOpts, SpawnPtyResult, TerminalId};
use vibeterm_pty::{ChunkSink, ExitInfo, SpawnOpts};
use vibeterm_status::StatusDetector;

use crate::clipboard_files;
use crate::events::record_event;
use crate::{
    atomic_write, emit_tasks_changed, expand_and_validate_cwd, notify_status_transition,
    refresh_dock_badge, AppState,
};

// ============================
// IPC commands — Terminal
// ============================

#[tauri::command]
pub(crate) async fn start_pty(
    opts: SpawnPtyOpts,
    channel: Channel<Vec<u8>>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<SpawnPtyResult> {
    spawn_inner(opts, channel, None, &state, &app)
}

// 在指定 task 下 spawn(可选 slot_id 做幂等)
#[tauri::command]
pub(crate) async fn spawn_terminal_in_task(
    task_id: vibeterm_ipc::TaskId,
    slot_id: Option<u32>,
    opts: SpawnPtyOpts,
    channel: Channel<Vec<u8>>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<SpawnPtyResult> {
    // (task, slot) 幂等 — 用 slot_lock 序列化"查 + spawn + bind",避免并发 race。
    // 没 slot_id 走旧 spawn 路径(不做幂等)。
    let Some(sid) = slot_id else {
        return spawn_inner(opts, channel, Some(task_id), &state, &app);
    };

    let lock = state
        .tasks
        .slot_lock(task_id, sid)
        .map_err(|_| IpcError::Unknown {
            trace_id: "slot_locks poisoned".into(),
        })?;
    let _guard = lock.lock().map_err(|_| IpcError::Unknown {
        trace_id: "slot_lock poisoned".into(),
    })?;

    // 临界区:lock 拿到后,先查;还没绑 → spawn 后写回;已绑 → attach 共享 PTY
    if let Ok(Some(existing)) = state.tasks.terminal_for_slot(task_id, sid) {
        // PassThroughSink:只透传字节,不做 status 嗅探(主 sink 已在做)
        struct PassThroughSink {
            channel: Channel<Vec<u8>>,
        }
        impl vibeterm_pty::ChunkSink for PassThroughSink {
            fn push(&self, chunk: Vec<u8>) {
                let _ = self.channel.send(chunk);
            }
            fn finish(&self, _info: vibeterm_pty::ExitInfo) {}
        }
        let sid = state
            .terminals
            .attach_sink(existing, PassThroughSink { channel })
            .map_err(|e| IpcError::PtySpawnFailed {
                reason: e.to_string(),
            })?;
        // sink_id 回传给前端:组件卸载(浮窗关闭等)时 detach,否则每次开关浮窗都给
        // 共享 PTY 永久多挂一个 sink(fan-out 给死 webview + 重复 clone,无界累积)。
        return Ok(SpawnPtyResult {
            terminal_id: existing,
            sink_id: Some(sid),
        });
    }

    let result = spawn_inner(opts, channel, Some(task_id), &state, &app)?;
    if let Err(e) = state.tasks.bind_slot(task_id, sid, result.terminal_id) {
        // 绑定失败会导致下次同 slot 幂等 spawn 无法复用,重复造 PTY — 至少留痕
        tracing::warn!(err = %e, task_id = %task_id, slot = %sid, terminal_id = %result.terminal_id, "bind_slot failed");
    }
    Ok(result)
}

/// command(shell 路径)的 basename 是否为 zsh。
pub(crate) fn is_zsh(command: &str) -> bool {
    std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "zsh")
}

/// 读 config.toml 的 shell_integration 开关(默认 true)。每次 spawn 读一次,
/// 设置改动下次开终端即生效,无需重启。
pub(crate) fn shell_integration_enabled() -> bool {
    Config::load().map(|c| c.shell_integration).unwrap_or(true)
}

/// 确保 zsh 集成的 ZDOTDIR 目录存在(config_dir/shell-integration/zsh),写入 4 个
/// wrapper 文件。app 自有目录,非用户 dotfiles。整个 app 生命周期写一次(OnceLock 缓存)。
pub(crate) fn ensure_zsh_zdotdir() -> Option<std::path::PathBuf> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = vibeterm_config::config_dir()
            .ok()?
            .join("shell-integration")
            .join("zsh");
        std::fs::create_dir_all(&dir).ok()?;
        let files = [
            (".zshenv", include_str!("shell-hooks/zdotdir/zshenv")),
            (".zprofile", include_str!("shell-hooks/zdotdir/zprofile")),
            (".zshrc", include_str!("shell-hooks/zdotdir/zshrc")),
            (".zlogin", include_str!("shell-hooks/zdotdir/zlogin")),
        ];
        for (name, content) in files {
            if let Err(e) = std::fs::write(dir.join(name), content) {
                tracing::warn!(err = %e, file = name, "write zsh zdotdir wrapper failed");
                return None;
            }
        }
        Some(dir)
    })
    .clone()
}

/// 子环境的 locale 类别(`LC_ALL`/`LC_CTYPE`/`LANG`)是否任一为 UTF-8。
/// `overrides`(env.toml / 内联,优先)缺则走 `inherit`(进程继承)。纯函数便于测,
/// 进程级 [`fix_locale_for_gui_launch`] 复用它判断"是否已有 UTF-8 locale 无需兜"。
pub(crate) fn locale_env_has_utf8(
    overrides: &std::collections::HashMap<String, String>,
    inherit: impl Fn(&str) -> Option<String>,
) -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|k| {
        overrides
            .get(*k)
            .cloned()
            .or_else(|| inherit(k))
            .map(|v| {
                let v = v.to_ascii_lowercase();
                v.contains("utf-8") || v.contains("utf8")
            })
            .unwrap_or(false)
    })
}

pub(crate) fn spawn_inner(
    opts: SpawnPtyOpts,
    channel: Channel<Vec<u8>>,
    task_id: Option<vibeterm_ipc::TaskId>,
    state: &AppState,
    app: &AppHandle,
) -> IpcResult<SpawnPtyResult> {
    // cwd 优先级:opts.cwd > task.cwd > $HOME。
    // 不变式:挂了 worktree 的 task,task.cwd 始终 = worktree_path
    //   (见 vibeterm-core::tasks::create / attach_worktree),所以本路径自动用对。
    let cwd_raw = opts
        .cwd
        .or_else(|| task_id.and_then(|t| state.tasks.cwd(t).ok().flatten()))
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    // 用户可能填 `~/projects/foo` 或 `$HOME/x` — shell 展开 ~ 但 posix_spawn 不会,
    // 这里手工展开;展开后路径不存在 fallback $HOME,避免 PTY 静默 chdir 失败 → 进程 cwd。
    let cwd = expand_and_validate_cwd(&cwd_raw);
    let command = opts
        .command
        .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| default_shell().into()));
    let args = opts.args.unwrap_or_default();

    // 4 层 env 合并:
    //   1. 进程继承(自动,由 portable-pty 处理)
    //   2. env.toml 全局
    //   3. 任务级 env(待加 task.env 字段;当前 None)
    //   4. 命令行内联(opts.env)
    let mut merged: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Ok(envfile) = EnvFile::load() {
        for (k, v) in envfile.to_env_pairs() {
            merged.insert(k, v);
        }
    }
    for (k, v) in opts.env.unwrap_or_default() {
        merged.insert(k, v);
    }
    // 默认 TERM / COLORTERM / TERM_PROGRAM(后者让 shell integration hook 识别)
    merged
        .entry("TERM".into())
        .or_insert_with(|| "xterm-256color".into());
    merged
        .entry("COLORTERM".into())
        .or_insert_with(|| "truecolor".into());
    merged
        .entry("TERM_PROGRAM".into())
        .or_insert_with(|| "vibeterm".into());
    merged
        .entry("TERM_PROGRAM_VERSION".into())
        .or_insert_with(|| env!("CARGO_PKG_VERSION").into());
    // CJK locale 不在这里逐 spawn 兜:进程级 `fix_locale_for_gui_launch()`(main 最早期)已把
    // LC_CTYPE 兜成 UTF-8,PTY shell 连同 lsof/ps/git 等所有子进程一律继承。env.toml 仍可覆盖。

    // shell 集成自动注入(默认开,config 可关):为 zsh 设临时 ZDOTDIR,让现成的
    // OSC 133 parser 拿到 shell 权威的 prompt/command/exit-code 标记。VS Code/Ghostty
    // 式 ephemeral 注入 —— 只设临时 env + 写 app 自有目录的 wrapper,绝不碰用户 dotfiles。
    if is_zsh(&command) && shell_integration_enabled() {
        if let Some(dir) = ensure_zsh_zdotdir() {
            // 用户原始 ZDOTDIR(通常未设 → $HOME)交给 wrapper 链接回去
            let user_zdotdir = merged
                .get("ZDOTDIR")
                .cloned()
                .or_else(|| std::env::var("ZDOTDIR").ok())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "~".into()));
            merged.insert("VIBETERM_USER_ZDOTDIR".into(), user_zdotdir);
            merged.insert("VIBETERM_INJECTION".into(), "1".into());
            merged.insert("ZDOTDIR".into(), dir.to_string_lossy().into_owned());
        }
    }

    let env: Vec<(String, String)> = merged.into_iter().collect();

    // sink 在 spawn 前构造但需要 terminal_id(spawn 内部才分配)——sink 持
    // Mutex<Option<TerminalId>>,spawn 返回后回填;读线程 push/finish 对 None 容忍。
    let term_id_holder: Arc<std::sync::Mutex<Option<TerminalId>>> =
        Arc::new(std::sync::Mutex::new(None));
    let status = Arc::new(std::sync::Mutex::new(StatusDetector::new(&command)));
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sink = LazyChannelSink {
        channel,
        terminal_id_holder: term_id_holder.clone(),
        status: status.clone(),
        tasks: state.tasks.clone(),
        app: app.clone(),
        finished: finished.clone(),
    };
    let id = state
        .terminals
        .spawn(
            SpawnOpts {
                rows: opts.rows,
                cols: opts.cols,
                cwd,
                command,
                args,
                env,
            },
            sink,
        )
        .map_err(|e| IpcError::PtySpawnFailed {
            reason: e.to_string(),
        })?;
    *term_id_holder.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);

    // 注册 status detector 到 AppState. 全局 status-tick 任务(见 setup)周期 tick
    // 这里所有 detector 做 stall/idle 时间态判定; close / PTY 退出时摘除条目.
    if let Ok(mut map) = state.status_detectors.lock() {
        map.insert(id, status.clone());
    }

    if let Some(task_id) = task_id {
        if let Err(e) = state.tasks.attach_terminal(task_id, id) {
            // 绑定失败(典型:task 在 spawn 期间被并发 close)→ 回滚刚起的 PTY,
            // 否则 shell 进程/detector/registry 条目成为无人回收的孤儿。
            tracing::warn!(err = %e, task_id = %task_id, terminal_id = id, "attach_terminal failed, rolling back spawn");
            reap_exited_terminal(app, state, id);
            return Err(IpcError::NotFound {
                resource: "task".into(),
                id: task_id.to_string(),
            });
        }
        emit_tasks_changed(app, &state.tasks);
    }
    // 极速退出竞态:命令 fork 成功但瞬间退出(如 exec 失败 127)时,读线程的 finish 可能
    // 跑在上面 id 回填之前 —— 那时 holder 还是 None,finish 什么都清不了。此处补收口。
    if finished.load(std::sync::atomic::Ordering::SeqCst) {
        reap_exited_terminal(app, state, id);
    }
    Ok(SpawnPtyResult {
        terminal_id: id,
        sink_id: None,
    })
}

// ============================================================
// G5: 会话 scrollback 快照(best-effort 恢复)
// ============================================================
// 🟢 零侵入:序列化的终端缓冲按 "taskId:slotId" 键落 VibeTerm 自己的 scrollback.json。
// 重启后前端为每个 task/slot 重建终端时回放旧缓冲(纯展示;旧 shell 进程已不在)。
// 布局/cwd 本就由 tasks.json 持久化,这里只补"看得见的历史"。

/// 单条 scrollback:键 "taskId:slotId" + 序列化缓冲(SerializeAddon 输出)。
#[derive(serde::Deserialize)]
pub(crate) struct ScrollbackEntry {
    key: String,
    data: String,
}

/// 每条 scrollback 上限 ~256KB(char 边界安全截尾),防 scrollback.json 膨胀。
pub(crate) const SCROLLBACK_MAX_BYTES: usize = 256 * 1024;

/// 保存全部终端的 scrollback(覆盖式原子写)。前端定期 + pagehide 时调用。
#[tauri::command]
pub(crate) async fn save_scrollback(entries: Vec<ScrollbackEntry>) -> IpcResult<()> {
    let path = match vibeterm_config::scrollback_json_path() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    let map: std::collections::HashMap<String, String> = entries
        .into_iter()
        .map(|e| {
            let data = if e.data.len() > SCROLLBACK_MAX_BYTES {
                // 保留尾部(最近内容)。向 0 方向找 char 边界:即使尾段非法 UTF-8,
                // 最坏回退到 start=0,绝不越过 len 而 panic(0 永远是边界)。
                let mut start = e.data.len() - SCROLLBACK_MAX_BYTES;
                while start > 0 && !e.data.is_char_boundary(start) {
                    start -= 1;
                }
                e.data[start..].to_string()
            } else {
                e.data
            };
            (e.key, data)
        })
        .collect();
    let json = serde_json::to_vec(&map).unwrap_or_default();
    let _ = atomic_write(&path, &json);
    Ok(())
}

/// 启动时读 scrollback 快照(键 "taskId:slotId" → 序列化缓冲)。缺失 → 空 map。
#[tauri::command]
pub(crate) async fn load_scrollback() -> IpcResult<std::collections::HashMap<String, String>> {
    let path = match vibeterm_config::scrollback_json_path() {
        Ok(p) => p,
        Err(_) => return Ok(Default::default()),
    };
    let map = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(map)
}

pub(crate) struct LazyChannelSink {
    channel: Channel<Vec<u8>>,
    terminal_id_holder: Arc<std::sync::Mutex<Option<TerminalId>>>,
    status: Arc<std::sync::Mutex<StatusDetector>>,
    tasks: Arc<TaskRegistry>,
    app: AppHandle,
    /// finish 已执行(PTY 已退出)。spawn_inner 在回填 terminal_id 后检查它,
    /// 补做收口——命令极速退出时 finish 跑在回填之前(holder 还是 None),什么都清不了。
    finished: Arc<std::sync::atomic::AtomicBool>,
}

/// 终端退出后的统一收口(与 close_pty 同语义):解除 task 关联 + slot 绑定、摘 detector、
/// 从 registry 移除(释放 PTY fd)。供三条此前各漏一截的路径共用:
///   1. PTY 自然退出(finish → 延迟线程,否则 ghost Running 蓝灯永挂 + slot 指向死 PTY);
///   2. spawn_inner 回滚(attach 到已关闭的 task → 孤儿 PTY);
///   3. spawn_inner 极速退出补收(finish 先于 id 回填)。
///
/// ⚠️ 不得在 PTY 读线程的 sinks 锁内直接调用(terminals.close 取 registry 锁,
/// 与 attach_sink 的 registry→sinks 锁序成环)——finish 路径必须经独立线程。
pub(crate) fn reap_exited_terminal(app: &AppHandle, state: &AppState, id: TerminalId) {
    let _ = state.tasks.detach_terminal(id);
    let _ = state.tasks.unbind_terminal(id);
    if let Ok(mut map) = state.status_detectors.lock() {
        map.remove(&id);
    }
    let _ = state.terminals.close(id);
    emit_tasks_changed(app, &state.tasks);
    refresh_dock_badge(app, &state.tasks);
}

impl ChunkSink for LazyChannelSink {
    fn push(&self, chunk: Vec<u8>) {
        let id_opt = *self
            .terminal_id_holder
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // 状态嗅探(C1 修订:status 用 broadcast 模式,这里同步 feed 简化为单调用)
        // 一次锁取 (new_status, idle_by_osc),后者标记 Idle 是 OSC D 真完成
        // 还是 timeout 误判,通知层用它过滤 ping 之类的假完成.
        let (new_status, idle_by_osc, sniffed_effort) = {
            let mut d = self.status.lock().unwrap_or_else(|p| p.into_inner());
            let s = d.feed(&chunk);
            (s, d.idle_by_osc(), d.last_effort().map(|x| x.to_string()))
        };
        // 状态 / effort 两类变更合并为本 chunk 末尾一次 tasks_changed,避免同 chunk 双发。
        let mut tasks_dirty = false;
        if let (Some(id), Some(s)) = (id_opt, new_status) {
            // TOCTOU 复核(与 status-tick 同型):feed 取到转换后、回写 registry 前,
            // tick 线程可能已推进状态;不一致则放弃本次回写,以 detector 当前值为准。
            let still_current = self
                .status
                .lock()
                .map(|d| d.current() == s)
                .unwrap_or(false);
            if still_current {
                if let Ok(Some((task_id, prev_agg, new_agg))) =
                    self.tasks.update_terminal_status(id, s, idle_by_osc)
                {
                    let _ = self.app.emit(
                        "task_status_changed",
                        serde_json::json!({"task_id": task_id, "status": s}),
                    );
                    record_event(
                        "status_changed",
                        task_id,
                        Some(id),
                        serde_json::to_value(s).ok(),
                    );
                    tasks_dirty = true;
                    notify_status_transition(
                        &self.app,
                        &self.tasks,
                        task_id,
                        id,
                        prev_agg,
                        new_agg,
                        idle_by_osc,
                    );
                    refresh_dock_badge(&self.app, &self.tasks);
                }
            }
        }
        // effort: 嗅探到 "thinking with X effort" → 写 task.effort(widget 回退读它).
        // set_effort_for_terminal 仅在真变化时返回 Some, 故 emit 不会随每帧 spinner 刷屏.
        if let (Some(id), Some(eff)) = (id_opt, sniffed_effort) {
            if let Ok(Some(_)) = self.tasks.set_effort_for_terminal(id, Some(eff)) {
                tasks_dirty = true;
            }
        }
        if tasks_dirty {
            emit_tasks_changed(&self.app, &self.tasks);
        }
        if let Err(e) = self.channel.send(chunk) {
            tracing::warn!(error = %e, "channel send failed");
        }
    }
    fn finish(&self, info: ExitInfo) {
        // 先置标志:spawn_inner 回填 id 后据此补收口(极速退出竞态)。
        self.finished
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(id) = *self
            .terminal_id_holder
            .lock()
            .unwrap_or_else(|p| p.into_inner())
        {
            // 立即摘 detector(廉价,让 200ms tick 马上停);其余收口(detach/unbind/registry
            // close)延迟到独立线程 —— 本方法在读线程的 sinks 锁内被调,直接取 registry 锁
            // 会与 attach_sink 的锁序成环(见 reap_exited_terminal 注释)。
            if let Some(state) = self.app.try_state::<AppState>() {
                if let Ok(mut map) = state.status_detectors.lock() {
                    map.remove(&id);
                }
            }
            tracing::info!(?info, terminal_id = id, "pty exited");
            let _ = self.app.emit(
                "terminal_exited",
                serde_json::json!({"terminal_id": id, "exit_code": info.exit_code}),
            );
            let app = self.app.clone();
            std::thread::spawn(move || {
                if let Some(state) = app.try_state::<AppState>() {
                    reap_exited_terminal(&app, &state, id);
                }
            });
        }
    }
}

#[tauri::command]
pub(crate) async fn write_pty(
    id: TerminalId,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    // 标记"用户在动" — Stalled 检测要求 last_user_input > last_chunk_at,
    // 排除 agent 跑完任务停在 prompt 长期空闲被误判为卡住的场景.
    if !data.is_empty() {
        // 快照 Arc 后立即释放 detectors 外锁(与 status-tick 同模式):
        // 内锁可能被 PTY 读线程长持,持外锁等内锁会让所有 write_pty 堆积。
        let det = state
            .status_detectors
            .lock()
            .ok()
            .and_then(|m| m.get(&id).cloned());
        if let Some(d) = det {
            let flipped = d.lock().ok().and_then(|mut det| det.mark_user_input());
            // WaitingInput→Running(用户已应答):立即回写聚合,圆点不等下一个 chunk。
            // 每个 WaitingInput episode 只翻一次,不在键入热路径上引入额外开销。
            if let Some(s) = flipped {
                if let Ok(Some(_)) = state.tasks.update_terminal_status(id, s, false) {
                    emit_tasks_changed(&app, &state.tasks);
                }
            }
        }
    }
    state.terminals.write(id, &data).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("write_pty:{other}"),
        },
    })
}

#[tauri::command]
pub(crate) async fn resize_pty(
    id: TerminalId,
    rows: u16,
    cols: u16,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    // PTY 尺寸幂等(见 Terminal::resize):同尺寸 no-op。前端在视图变可见时会无条件断言
    // 本视图尺寸,以修"浮窗调尺寸后 PTY 停在浮窗尺寸、返回主窗 fit 是 no-op 不下发"的错乱。
    state
        .terminals
        .resize(id, rows, cols)
        .map_err(|e| match e {
            vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
                resource: "terminal".into(),
                id: id.to_string(),
            },
            other => IpcError::Unknown {
                trace_id: format!("resize_pty:{other}"),
            },
        })?;
    Ok(())
}

// 设置页用:返回当前生效的 clipboard images dir 绝对路径(便于 UI 显示)
#[tauri::command]
pub(crate) async fn get_clipboard_images_dir() -> IpcResult<String> {
    vibeterm_config::clipboard_images_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("get_clipboard_images_dir:{e}"),
        })
}

// 设置页"打开目录"按钮:在 Finder / Explorer / xdg-open 里 reveal
#[tauri::command]
pub(crate) async fn open_clipboard_images_dir() -> IpcResult<()> {
    let dir = vibeterm_config::clipboard_images_dir().map_err(|e| IpcError::Unknown {
        trace_id: format!("open_clipboard_images_dir:dir:{e}"),
    })?;
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&dir).status();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(&dir).status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&dir).status();
    r.map_err(|e| IpcError::Unknown {
        trace_id: format!("open_clipboard_images_dir:spawn:{e}"),
    })?;
    Ok(())
}

// 设置页"清空所有"按钮:删 dir 下所有 *.png
#[tauri::command]
pub(crate) async fn clear_clipboard_images() -> IpcResult<usize> {
    let dir = vibeterm_config::clipboard_images_dir().map_err(|e| IpcError::Unknown {
        trace_id: format!("clear_clipboard_images:dir:{e}"),
    })?;
    let mut removed = 0usize;
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return Ok(0),
    };
    for entry in read.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("png") && std::fs::remove_file(&p).is_ok()
        {
            removed += 1;
        }
    }
    tracing::info!(removed, "clear_clipboard_images");
    Ok(removed)
}

// 一次性读剪贴板图片 → 编 PNG → 存盘,返回绝对路径(无图返回 None)。
// 把整条链放 Rust:
//   1) 走 tauri-plugin-clipboard-manager(包 arboard),直接 OS 级访问
//   2) image crate 编 PNG(剪贴板返回 RGBA raw)
//   3) vibeterm-config FIFO 落盘
// 前端 Cmd+V 主动调用,绕开 WebView paste 事件的 image content 兼容性问题。
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PasteResult {
    /// 优先:剪贴板里有文件 URL → 直接插路径(避开 Finder Cmd+C 图片
    /// 把缩略 icon 放进 bitmap 字段的陷阱)
    Files {
        paths: Vec<String>,
    },
    /// 剪贴板里是 image bitmap(截图工具) → Rust 后台编 PNG + 落盘
    Image {
        path: String,
    },
    /// 纯文本 → 走 xterm.paste(保留 bracketed paste 行为)
    Text {
        text: String,
    },
    Empty,
}

// 一次 IPC 同时尝试 image + text,省一次往返。
// image 优先(screenshot 场景同时有 text 时仍应注入图);无图 fallback text。
//
// 命名策略:**内容 hash**(blake3 截前 16 hex 字符)— 同一截图反复粘贴
// 命中同一文件,二次秒返回 + 不占额外磁盘。也避免了"异步落盘"竞态
// (codex/claude-code 等 TUI 立即去 image::image_dimensions 读文件,
// 必须返回前文件已就位)。
#[tauri::command]
pub(crate) async fn paste_clipboard(app: AppHandle) -> IpcResult<PasteResult> {
    use std::time::Instant;

    let t0 = Instant::now();
    let clip = app.clipboard();

    // 优先 1:剪贴板含文件 URL(Finder Cmd+C 图片/文件 → 路径直插)
    // 这一步必须在 read_image 之前 —— 否则 Finder 的图标缩略会被当作 bitmap 落盘
    let files = clipboard_files::read_clipboard_files();
    if !files.is_empty() {
        tracing::debug!(
            n = files.len(),
            total_ms = t0.elapsed().as_millis() as u64,
            "paste_clipboard files"
        );
        return Ok(PasteResult::Files { paths: files });
    }

    if let Ok(img) = clip.read_image() {
        let (w, h) = (img.width(), img.height());
        let rgba = img.rgba();
        let t_read = t0.elapsed();

        // 内容 hash —— 同图反复粘贴命中同一路径(blake3 ~10GB/s,16MB → ~1.5ms)
        let hash = blake3::hash(rgba);
        let hex = hash.to_hex();
        let short = &hex.as_str()[..16];
        let dir = vibeterm_config::clipboard_images_dir().map_err(|e| IpcError::Unknown {
            trace_id: format!("paste_clipboard:dir:{e}"),
        })?;
        let target = dir.join(format!("{short}.png"));
        let target_str = target.to_string_lossy().into_owned();

        // 文件已存在 → 直接返回,跳过编码 + 写盘
        if target.exists() {
            // 刷新 mtime,让 FIFO 清理把它当"最近用过"留到最后
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&target) {
                let _ = f.set_modified(std::time::SystemTime::now());
            }
            tracing::debug!(
                hash = short,
                total_ms = t0.elapsed().as_millis() as u64,
                "paste_clipboard image (hit cache)"
            );
            return Ok(PasteResult::Image { path: target_str });
        }

        // 首次见到此图 → 同步编 PNG + 落盘(codex 进程后续读盘必须命中)
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};
        let t_enc_start = Instant::now();
        let mut png_bytes: Vec<u8> = Vec::with_capacity((w as usize) * (h as usize));
        let encoder = PngEncoder::new_with_quality(
            &mut png_bytes,
            CompressionType::Fast,
            FilterType::NoFilter,
        );
        encoder
            .write_image(rgba, w, h, ExtendedColorType::Rgba8)
            .map_err(|e| IpcError::Unknown {
                trace_id: format!("paste_clipboard:encode:{e}"),
            })?;
        let t_encode = t_enc_start.elapsed();

        vibeterm_config::save_clipboard_image_at(&target, &png_bytes).map_err(|e| {
            IpcError::Unknown {
                trace_id: format!("paste_clipboard:save:{e}"),
            }
        })?;
        // FIFO 清理(用统一上限)
        let (max_count, max_bytes) = vibeterm_config::clipboard_images_caps();
        let _ = vibeterm_config::enforce_clipboard_images_caps(&dir, max_count, max_bytes);

        tracing::info!(
            w,
            h,
            hash = short,
            png_kb = png_bytes.len() / 1024,
            read_ms = t_read.as_millis() as u64,
            encode_ms = t_encode.as_millis() as u64,
            total_ms = t0.elapsed().as_millis() as u64,
            "paste_clipboard image (saved)"
        );
        return Ok(PasteResult::Image { path: target_str });
    }
    if let Ok(text) = clip.read_text() {
        if !text.is_empty() {
            tracing::debug!(
                len = text.len(),
                total_ms = t0.elapsed().as_millis() as u64,
                "paste_clipboard text"
            );
            return Ok(PasteResult::Text { text });
        }
    }
    Ok(PasteResult::Empty)
}

// 写文本进系统剪贴板 —— 右键菜单"复制"用。
// 不走 navigator.clipboard.writeText:WebView(尤其 Windows WebView2)要求页面有焦点 +
// clipboard-write 权限,右键菜单弹出后焦点已漂移 → 静默失败。Rust 侧 arboard 无此约束。
#[tauri::command]
pub(crate) async fn write_clipboard_text(app: AppHandle, text: String) -> IpcResult<()> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("write_clipboard_text:{e}"),
        })
}

// 读 scrollback 快照(独立查询,不订阅;给搜索/导出/调试用)
#[tauri::command]
pub(crate) async fn get_scrollback(
    id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<Vec<u8>> {
    state.terminals.scrollback(id).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("get_scrollback:{other}"),
        },
    })
}

// 读 PTY 当前生效尺寸 (rows, cols)。前端在视图变可见时用它判断 PTY 是否被别的视图
// (浮窗)改过尺寸 → 不一致说明隐藏期按别的宽度消费了 TUI 重绘、buffer 已污染,需清屏重绘。
#[tauri::command]
pub(crate) async fn terminal_size(
    id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<(u16, u16)> {
    state.terminals.size(id).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("terminal_size:{other}"),
        },
    })
}

// 取消订阅(浮窗 Terminal 组件 onCleanup;**不** 关闭 PTY)
#[tauri::command]
pub(crate) async fn detach_terminal(
    id: TerminalId,
    sink_id: u64,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    state
        .terminals
        .detach_sink(id, sink_id)
        .map_err(|e| match e {
            vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
                resource: "terminal".into(),
                id: id.to_string(),
            },
            other => IpcError::Unknown {
                trace_id: format!("detach:{other}"),
            },
        })
}

#[tauri::command]
pub(crate) async fn close_pty(
    id: TerminalId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    let _ = state.tasks.detach_terminal(id);
    // 幂等 slot 映射也要清,不然下次同 slot spawn 还会 attach 死 PTY
    let _ = state.tasks.unbind_terminal(id);
    // 清掉 status detector 注册 — 全局 tick 任务随即不再 tick 它
    if let Ok(mut map) = state.status_detectors.lock() {
        map.remove(&id);
    }
    let r = state.terminals.close(id).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("close_pty:{other}"),
        },
    });
    emit_tasks_changed(&app, &state.tasks);
    r
}

// ---- shell defaults ----
#[cfg(target_os = "windows")]
pub(crate) fn default_shell() -> &'static str {
    if which::which("pwsh.exe").is_ok() {
        "pwsh.exe"
    } else if which::which("powershell.exe").is_ok() {
        "powershell.exe"
    } else {
        "cmd.exe"
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn default_shell() -> &'static str {
    "/bin/zsh"
}

/// macOS: 用 lsof 直接拿进程 cwd. 这是不需要 shell integration 的"内核回退路径",
/// 普通用户不配 OSC 7/633 也能拿到 cwd. 失败 (lsof 不可用 / 进程已死) 返回 None.
#[cfg(target_os = "macos")]
pub(crate) fn kernel_cwd_of(pid: u32) -> Option<String> {
    // 关键:GUI 从 Dock/Launchpad 启动时进程不继承 shell 的 LANG,lsof 在非 UTF-8 locale 下
    // 会把路径里的非 ASCII 字节转义成 `\xNN` 字面串(实测中文 "剧" → `\xe5\x89\xa7`),导致顶栏
    // cwd 乱码、且按此 cwd 找 transcript 全落空。强制 LC_CTYPE=UTF-8 让 lsof 原样输出 UTF-8 路径。
    let out = std::process::Command::new("lsof")
        .env("LC_CTYPE", "UTF-8")
        .args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-F", "n"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // 输出格式:
    //   p<pid>
    //   fcwd
    //   n<path>
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix('n') {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

/// Windows: sysinfo 拿进程 cwd(内核侧真实工作目录,等价 lsof cwd)。
/// 没它 per-terminal 完成检测在 Windows 拿不到 cwd → transcript 归属全落空。
#[cfg(target_os = "windows")]
pub(crate) fn kernel_cwd_of(pid: u32) -> Option<String> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let target = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[target]), true);
    sys.process(target)?
        .cwd()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn kernel_cwd_of(_pid: u32) -> Option<String> {
    None
}

/// 取某终端当前 cwd(per-terminal 完成检测用):优先 OSC 633(shell 集成,最准),退到内核
/// cwd(macOS lsof / Windows sysinfo)。与 get_terminal_cwd 同源,供后台 3s 轮询直接调(非
/// IPC),让每个 agent 终端按各自 cwd 定位 transcript —— 这是多 agent 完成检测 per-terminal
/// 工作的前提。
pub(crate) fn terminal_cwd_for(state: &AppState, terminal_id: TerminalId) -> Option<String> {
    if let Ok(map) = state.status_detectors.lock() {
        if let Some(det) = map.get(&terminal_id) {
            if let Ok(d) = det.lock() {
                if let Some(cwd) = d.current_cwd() {
                    return Some(cwd.to_string());
                }
            }
        }
    }
    state.terminals.pid_of(terminal_id).and_then(kernel_cwd_of)
}

/// 拿某个 terminal 当前 cwd:
///   1. 优先用 StatusDetector 解析的 OSC 633 Cwd (要 shell integration, 最准)
///   2. 退到 lsof 拉 PTY 子进程 (或更深的后裔) 的内核 cwd — 无需 shell 配置
/// 双路径都失败返回 None.
#[tauri::command]
pub(crate) async fn get_terminal_cwd(
    terminal_id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<Option<String>> {
    // 路径 1: OSC 633
    if let Ok(map) = state.status_detectors.lock() {
        if let Some(det) = map.get(&terminal_id) {
            if let Ok(det) = det.lock() {
                if let Some(cwd) = det.current_cwd() {
                    return Ok(Some(cwd.to_string()));
                }
            }
        }
    }
    // 路径 2: 内核 lsof — 找 PTY 进程的最深后裔 (跑着的命令), 没后裔就用 shell 自己
    let Some(shell_pid) = state.terminals.pid_of(terminal_id) else {
        return Ok(None);
    };
    // 嗅探的 cmdlines 副产物里只有命令字符串, 不带 pid; 这里简单点直接用 shell_pid 的 cwd.
    // shell 的 cwd 在用户 `cd` 后会更新, 通常就是 prompt 上下文.
    Ok(kernel_cwd_of(shell_pid))
}

/// 调试用 — 把前端 console 信息追加到 <temp_dir>/vibeterm-tasklist-debug.log,
/// 方便从主机直接 tail 文件诊断 webview 行为。
/// 仅 debug 构建落盘:temp 世界可读 + 写任意前端内容, release 下为 no-op.
#[tauri::command]
pub(crate) async fn debug_log(msg: String) -> IpcResult<()> {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        let path = std::env::temp_dir().join("vibeterm-tasklist-debug.log");
        let mut f = match std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "{ts} {msg}");
    }
    #[cfg(not(debug_assertions))]
    let _ = msg;
    Ok(())
}
