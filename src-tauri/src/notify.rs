//! 通知/提示音子系统 + agent 完成轮询:路由判定、系统横幅、前端兜底声、
//! 音频线程、持续提醒、声音解析/预览。从 main.rs 拆出(行为不变)。

use tauri::{AppHandle, Emitter, Manager};
use vibeterm_config::NotifyFile;
use vibeterm_core::TaskRegistry;
use vibeterm_ipc::{IpcError, IpcResult, TaskStatus, TerminalId};

use crate::events::record_event;
use crate::{
    emit_tasks_changed, main_window_focused, AppState, NotifySoundData, AGENT_COMPLETED_COOLDOWN,
    AGENT_COMPLETION_OUTPUT_WINDOW_MS,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NotifyRoute {
    /// 主窗口在后台:发系统通知 + 声音(完整通知)。
    Background,
    /// 主窗口在前台、但完成的不是当前选中任务:只前端轻提示音 + 任务列表行高亮,
    /// 不发系统横幅(macOS 前台横幅常被吞,且用户已在 app 里,无需强打扰)。
    ForegroundLight,
}

/// 通知预检 — 所有 task-level 守门集中一处, WaitingInput / agent_completed 共用.
/// 返回 Some((NotifyFile, route)) 表示放行, None 表示静默.
///
/// 守门顺序(命中任意一条即返回 None):
///   1. 非 agent task / per-task muted / 全局总开关 off / 免打扰时段 → 静默
///   2. 主窗口聚焦时:
///        - allow_foreground=false(如 waiting_input)→ 静默(维持前台不打扰)
///        - 完成的就是当前选中任务 → 静默(用户正看着,删除线/黄灯就在眼前)
///        - notify_focused_other_task 关 → 静默
///        - 否则 → ForegroundLight(前台轻提示音 + 列表高亮)
///   3. 主窗口失焦 → Background(完整系统通知 + 声音)
pub(crate) fn notify_preflight(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    allow_foreground: bool,
) -> Option<(NotifyFile, NotifyRoute)> {
    let is_agent = tasks.agent_kind_of(task_id).ok().flatten().is_some();
    let muted = tasks.notify_muted_of(task_id).unwrap_or(false);
    let prefs = NotifyFile::load();
    let now_hhmm = chrono::Local::now().format("%H:%M").to_string();
    let focused = main_window_focused(app);
    let is_active = tasks.active_main() == Some(task_id);
    decide_notify_route(
        is_agent,
        muted,
        &prefs,
        &now_hhmm,
        focused,
        is_active,
        allow_foreground,
    )
    .map(|route| (prefs, route))
}

/// 通知路由的纯判定核心(从 notify_preflight 抽出,无 AppHandle 依赖,可单测)。
/// 守门顺序见 notify_preflight 文档注释。
#[allow(clippy::too_many_arguments)]
fn decide_notify_route(
    is_agent: bool,
    muted: bool,
    prefs: &NotifyFile,
    now_hhmm: &str,
    focused: bool,
    is_active_task: bool,
    allow_foreground: bool,
) -> Option<NotifyRoute> {
    if !is_agent || muted || !prefs.enabled {
        return None;
    }
    if prefs.quiet_hours.contains(now_hhmm) {
        return None;
    }
    if focused {
        if !allow_foreground || is_active_task || !prefs.notify_focused_other_task {
            return None;
        }
        return Some(NotifyRoute::ForegroundLight);
    }
    Some(NotifyRoute::Background)
}

/// 未看完成数 → Dock 角标(macOS dock 图标红色数字)。开关关或数为 0 时清除角标。
/// 复用 TaskRegistry::unseen_done_count(聚合状态 = Done 的任务数)。状态跃迁 /
/// 切换 active / 关闭任务后调用,保持角标与"未看完成"实时一致。
pub(crate) fn refresh_dock_badge(app: &AppHandle, tasks: &TaskRegistry) {
    let n = if NotifyFile::load().dock_badge_unseen {
        tasks.unseen_done_count()
    } else {
        0
    };
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_badge_count(if n > 0 { Some(n as i64) } else { None });
    }
}

/// 3s 轮询兜底:对**一个 agent 终端**,用它自己的 cwd 读 transcript 完成态,据此 set_agent_turn_done
/// 并按需发完成通知。**per-terminal**:一个 task 可分屏多个 agent,逐终端各自检测(task_id+term_id
/// 已知,无需按 cwd 反查 task)—— 旧逻辑只认第一个 agent,后开的 agent 完成检测不到、不通知、答完误判黄圈。
/// claude 用 stop_reason==end_turn 判完成,codex 用 task_completed;turn_id 给完成去重(快轮鲁棒)。
/// 读不到 transcript 就跳过本轮(回退 PTY 嗅探 + 等下次轮询),绝不把完成态默认成 false 钉死 Running。
pub(crate) fn poll_agent_turn_for_terminal(
    app: &AppHandle,
    state: &AppState,
    task_id: vibeterm_ipc::TaskId,
    term_id: TerminalId,
    kind: &str,
    cwd: &str,
) {
    if cwd.is_empty() {
        return;
    }
    // 完成归属守门:本终端 PTY 近 AGENT_COMPLETION_OUTPUT_WINDOW_MS 内无任何产出,则
    // transcript 里"最新会话答完一轮"不是本终端干的 —— 是同 cwd 编码目录下别的 claude
    // (另一终端 / 另一终端 app / lossy 编码碰撞的另一项目)写的会话被 read_for_cwd 顶成最新。
    // 据此跳过,绝不给本不相干的任务发完成通知(回退 PTY 嗅探 + 等下次轮询)。
    let detector = state
        .status_detectors
        .lock()
        .ok()
        .and_then(|m| m.get(&term_id).cloned());
    let (recent_output, spinner_recent) = detector
        .as_ref()
        .and_then(|d| {
            d.lock().ok().map(|det| {
                (
                    det.since_last_chunk_ms() <= AGENT_COMPLETION_OUTPUT_WINDOW_MS,
                    // 强归属判据:窗口内见过标题 braille spinner = 本终端 agent 真在生成。
                    // recent_output 会把键入回显 / codex 空闲状态栏重绘也算"有产出",
                    // 不足以支撑「目录最新会话就是本终端写的」这一换绑/首绑前提。
                    det.since_last_spinner_ms()
                        .map(|ms| ms <= AGENT_COMPLETION_OUTPUT_WINDOW_MS)
                        .unwrap_or(false),
                )
            })
        })
        .unwrap_or((false, false));
    if !recent_output {
        return;
    }
    let (done, turn_id) = match kind {
        "claude" => {
            use vibeterm_agent_watch::claude::project as cproj;
            // 精确归属:本终端只读**自己绑定的会话**,而非"目录里最新会话"。
            //   - 已绑定 + 绑定会话近窗口内仍在更新(== 本终端正在写它)→ 信它,无视同目录别的 claude;
            //   - 已绑定但绑定会话久不更新(claude 重启 / /clear 换文件 / 早先误绑)→ 换绑到目录最新会话
            //     (本终端此刻在产出,最新会话就是它新写的那个);
            //   - 未绑定 → 此刻本终端在产出(recent_output 已守门),目录最新会话即本终端的 → 绑定它。
            let pin = state
                .tasks
                .agent_session_pin(task_id, term_id)
                .ok()
                .flatten();
            let sess = match pin {
                Some(sid)
                    if cproj::session_age_ms(cwd, &sid)
                        .map(|age| age <= AGENT_COMPLETION_OUTPUT_WINDOW_MS)
                        .unwrap_or(false) =>
                {
                    cproj::read_for_session(cwd, &sid)
                }
                _ => {
                    // 换绑/首绑要求强判据(窗口内见过 spinner):「本终端此刻在产出 ⇒ 目录最新
                    // 会话是它写的」在产出只是回显时不成立——同 cwd 外部 claude 答完会被错绑进来,
                    // 此后每次键入回显都持续误归属。无 spinner 则跳过本轮,保持现有 pin 不动。
                    // 代价:标题不透传的环境(tmux 默认)换绑保守延迟到下一次真生成。
                    if !spinner_recent {
                        return;
                    }
                    let newest = cproj::read_for_cwd(cwd);
                    if let Some(nsid) = newest.as_ref().map(|s| s.session_id.clone()) {
                        if !nsid.is_empty() {
                            let _ = state.tasks.set_agent_session_pin(task_id, term_id, &nsid);
                        }
                    }
                    newest
                }
            };
            let Some(sess) = sess else {
                return;
            };
            let done = sess.stop_reason.as_deref() == Some("end_turn");
            (done, if done { sess.last_turn_id } else { None })
        }
        "codex" => {
            // codex rollout 按日期平铺、`read_for_cwd` 按文件内 session_meta.cwd **精确**匹配,
            // 不存在 claude 那种"cwd 有损编码把别项目并进同目录"的跨项目漏判。无 session 绑定,
            // 归属全靠守门——但 codex 空闲时底部状态栏持续重绘,recent_output 恒 true 形同虚设,
            // 同 cwd 外部 codex 的完成会被误归属;故要求强判据(窗口内见过 spinner)。
            if !spinner_recent {
                return;
            }
            let Some(snap) = vibeterm_agent_watch::codex::session::read_for_cwd(cwd) else {
                return;
            };
            let done = snap.task_completed;
            (done, if done { snap.last_turn_id } else { None })
        }
        _ => return,
    };
    let tasks: &TaskRegistry = &state.tasks;
    // 兜底实时窗口焦点(macOS 切 app 时 Focused 事件不可靠;每轮校正 → 失焦时完成转 Done)。
    // 注意:焦点变更与完成态变更合并为轮次末尾一次 emit,避免同轮重复触发前端全量渲染。
    let focus_changed = tasks
        .set_window_focused(main_window_focused(app))
        .unwrap_or(false);
    // per-terminal:直接按 (task_id, term_id) 记完成态,无需按 cwd 反查 task。
    let (changed, just_completed) = tasks
        .set_agent_turn_done(task_id, term_id, done, turn_id.as_deref())
        .unwrap_or((false, false));
    if focus_changed || changed || just_completed {
        emit_tasks_changed(app, tasks);
        refresh_dock_badge(app, tasks);
    }
    // 补发被吞的授权通知:上一轮的 agent_turn_done=Some(true) 残留期间,终端在聚合里被
    // 跳过 → 新一轮第一个授权框的 WaitingInput 跃迁不产生聚合变化、横幅被吞。此刻轮询把
    // done 翻回 false(changed && !done),若嗅探态已是 WaitingInput,把欠的通知补上。
    if changed && !done {
        let still_waiting = detector
            .as_ref()
            .and_then(|d| {
                d.lock()
                    .ok()
                    .map(|det| det.current() == TaskStatus::WaitingInput)
            })
            .unwrap_or(false);
        if still_waiting {
            fire_waiting_input_notification(app, tasks, task_id);
        }
    }
    if just_completed {
        // 前端用:切回该 task 时把焦点自动定位到"最后完成的那个终端"(一个 task 多 agent 场景)。
        let _ = app.emit(
            "agent_terminal_completed",
            serde_json::json!({ "task_id": task_id, "terminal_id": term_id }),
        );
        record_event("agent_completed", task_id, Some(term_id), None);
        let last = tasks
            .task_dto(task_id)
            .ok()
            .flatten()
            .and_then(|d| state.terminals.most_recent_tail(&d.terminal_ids))
            .unwrap_or_default();
        // session_id 含 term_id:同一 task 的不同 agent 终端各自独立通知 + 独立 30s throttle。
        fire_agent_completed_notification(
            app,
            tasks,
            task_id,
            kind,
            &format!("task-{task_id}-term-{term_id}"),
            last.trim(),
        );
    }
}

/// 前台轻提示 / 持续提醒用的"前端可播声音名"。系统声音名(Glass 等)前端 `<audio>` 放不了,
/// 退到 bundled fallback,保证前台/持续场景一定有声(系统横幅那条路才用得了系统声音)。
pub(crate) fn frontend_sound_for(app: &AppHandle, configured: &str, fallback: &str) -> String {
    let (use_fe, _native, raw) = resolve_notify_sound(app, configured, fallback);
    if use_fe {
        raw
    } else {
        fallback.to_string()
    }
}

/// 间歇持续提醒(单路全局)。在 200ms tick 里调:有"未看完成"且主窗口失焦时,每隔
/// PERSISTENT_REMIND_INTERVAL 响 1 路声音催用户回来;未看数归零 / 主窗口聚焦 / 开关关
/// → reset(下次重新计时)。首次发现未看只记基准不响 —— "完成"通知本身已响过那一声,避免双响。
pub(crate) fn maybe_persistent_remind(app: &AppHandle, state: &AppState) {
    let reset = || {
        if let Ok(mut g) = state.last_persistent_remind.lock() {
            *g = None;
        }
    };
    let prefs = NotifyFile::load();
    if !prefs.enabled || !prefs.persistent_unseen_sound {
        reset();
        return;
    }
    if state.tasks.unseen_done_count() == 0 || main_window_focused(app) {
        // 看完了 / 人回到 app → 停止催促并重置计时
        reset();
        return;
    }
    let now_hhmm = chrono::Local::now().format("%H:%M").to_string();
    if prefs.quiet_hours.contains(&now_hhmm) {
        return; // 免打扰时段:不响,也不重置基准(出时段后接着按节奏来)
    }
    let now = std::time::Instant::now();
    {
        let mut g = match state.last_persistent_remind.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match *g {
            // 首次发现"未看 + 失焦":只记基准不响(完成通知已响过那一声)
            None => {
                *g = Some(now);
                return;
            }
            Some(t)
                if now.duration_since(t)
                    >= std::time::Duration::from_secs(
                        prefs.persistent_remind_secs.clamp(5, 3600),
                    ) =>
            {
                *g = Some(now);
            }
            _ => return,
        }
    }
    let configured = prefs.events.done.sound.as_deref().unwrap_or("");
    let fe = frontend_sound_for(app, configured, "ringtone2");
    let _ = app.emit(
        "notification_play_sound",
        serde_json::json!({ "sound": fe }),
    );
}

/// 解析 sound 字段, 返回 (use_frontend_audio, native_sound, raw_sound).
/// raw_sound 用于 emit 给前端 <audio> 播自定义文件 / 自带库.
///
/// 三类 sound 字段:
///   - 绝对路径 / ~/ → 前端 <audio> 放 (native silent)
///   - 自带库名 (resource_dir/sounds/<name>.mp3) → 前端 <audio> 放 (OS 没这个名)
///   - macOS 系统声音名 (Glass/Tink/...) → 走 NSUserNotification.sound
pub(crate) fn resolve_notify_sound(
    app: &AppHandle,
    configured: &str,
    fallback: &str,
) -> (bool, String, String) {
    let cfg = configured.trim();
    if sound_is_file_path(cfg) {
        return (true, String::new(), cfg.to_string());
    }
    // 自带库:打包资源里的 sound id → 走前端音频, 跨平台一致
    if !cfg.is_empty() && is_bundled_sound(app, cfg) {
        return (true, String::new(), cfg.to_string());
    }
    let native = if !cfg.is_empty() { cfg } else { fallback };
    (false, native.to_string(), native.to_string())
}

pub(crate) fn is_bundled_sound(app: &AppHandle, name: &str) -> bool {
    let Ok(res_dir) = app.path().resource_dir() else {
        return false;
    };
    res_dir
        .join("resources/sounds")
        .join(format!("{name}.mp3"))
        .is_file()
}

/// 在聚合状态跃迁时弹系统通知。
///
/// 触发: ① 任意 → WaitingInput(等用户)。② agent 终端 Running→Idle 且 by_osc
/// (真完成 —— 标题 spinner→静态 / OSC D)→ "完成"通知。纯嗅探, 不依赖 hook。
///
/// Stalled 不弹通知 — 区分"agent 真挂了"vs"agent 完成等输入"在通用 TUI 协议层
/// 做不到, 视觉徽标 (任务列表呼吸动画) 已足够提示.
pub(crate) fn notify_status_transition(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    terminal_id: TerminalId,
    prev: vibeterm_ipc::TaskStatus,
    new: vibeterm_ipc::TaskStatus,
    by_osc: bool,
) {
    use vibeterm_ipc::TaskStatus;

    if prev == new {
        return;
    }

    // agent 精确完成 → "完成"通知(纯嗅探, 替代原 hook 的 TurnComplete).
    // 触发条件: **该终端自己是 agent**(per-terminal kind——task 级 any-agent 会把分屏里
    // 普通 shell pane 的命令结束通报成 "claude 完成")+ Running→Idle/Done + by_osc
    // (真完成 —— OSC D 或标题 spinner→静态, 非 800ms 超时误判).
    // Done 也接受:窗口失焦时真完成 seen=false → 聚合直接落 Done,旧条件只认 Idle 会漏.
    // 授权等待会走 WaitingInput 分支而非 Idle, 不会误判完成.
    if matches!(new, TaskStatus::Idle | TaskStatus::Done)
        && matches!(prev, TaskStatus::Running)
        && by_osc
    {
        if let Some(agent) = tasks
            .agent_kind_of_terminal(task_id, terminal_id)
            .ok()
            .flatten()
        {
            // 切回该 task 时自动把焦点定位到"刚完成的那个终端"。这是实时 OSC 完成路径
            // (200ms tick / chunk-sink),与 poll_agent_turn_for_terminal 的兜底 emit 同一事件;
            // 前端按 (task_id, terminal_id) 幂等记录,双发无害。终端级,故 task 多 agent 时精确。
            let _ = app.emit(
                "agent_terminal_completed",
                serde_json::json!({ "task_id": task_id, "terminal_id": terminal_id }),
            );
            record_event("agent_completed", task_id, Some(terminal_id), None);
            let last = app
                .try_state::<AppState>()
                .and_then(|s| {
                    s.tasks
                        .task_dto(task_id)
                        .ok()
                        .flatten()
                        .and_then(|d| s.terminals.most_recent_tail(&d.terminal_ids))
                })
                .unwrap_or_default();
            // throttle key 与 poll_agent_turn_for_terminal 的兜底路径**同 key**(含 term_id):
            // 共享同一条 cooldown 记录 → 同一轮完成被两路先后看到时只响一次,不再双发。
            fire_agent_completed_notification(
                app,
                tasks,
                task_id,
                &agent,
                &format!("task-{task_id}-term-{terminal_id}"),
                last.trim(),
            );
        }
        return;
    }

    if !matches!(new, TaskStatus::WaitingInput) {
        return;
    }
    fire_waiting_input_notification(app, tasks, task_id);
}

/// "等待你的输入"通知(WaitingInput 黄灯)。从 notify_status_transition 抽出供两处调用:
///   1. 聚合状态跃迁到 WaitingInput(实时嗅探路径);
///   2. transcript 轮询把陈旧的 agent_turn_done 翻回 false 时,发现该终端嗅探态早已是
///      WaitingInput —— 跃迁发生在终端还被「上一轮已答完」聚合跳过的窗口里,通知被吞,
///      这里补发(否则每轮第一个授权框的横幅必丢)。
pub(crate) fn fire_waiting_input_notification(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
) {
    // waiting_input 不放前台轻提示(allow_foreground=false → route 恒 Background,
    // 维持"前台一律静默";前台轻提示只用于"完成"通知,符合用户预期)。
    let Some((prefs, _route)) = notify_preflight(app, tasks, task_id, false) else {
        return;
    };
    let event_prefs = &prefs.events.waiting_input;
    if !event_prefs.enabled {
        return;
    }

    let last_output = app.try_state::<AppState>().and_then(|s| {
        s.tasks
            .task_dto(task_id)
            .ok()
            .flatten()
            .and_then(|d| s.terminals.most_recent_tail(&d.terminal_ids))
    });
    let agent_kind = tasks.agent_kind_of(task_id).ok().flatten();
    let label = task_label(tasks, task_id);
    let body = format_notify_body(&label, last_output.as_deref(), agent_kind.as_deref());
    let title = "VibeTerm — 等待你的输入".to_string();
    let configured = event_prefs.sound.as_deref().unwrap_or("");
    // 通知点击聚焦:把 task_id 编进通知 id(i32),前端 click listener 用它切到对应 task。
    // 声音由 send_notification 内部 afplay(绕开 webview),tone20 作 fallback。
    send_notification(app, task_id, title, body, configured, "tone20");
}

/// 实际发系统通知 + 自定义文件音效旁路 + 记录 last_notify (用于点击聚焦).
/// 把 builder 那块从 notify_status_transition 抽出来给 hook 路径复用.
pub(crate) fn send_notification(
    app: &AppHandle,
    task_id: vibeterm_ipc::TaskId,
    title: String,
    body: String,
    configured: &str,
    fallback: &str,
) {
    use tauri_plugin_notification::NotificationExt;
    let id: i32 = i32::try_from(task_id).unwrap_or(0);
    // 横幅:tauri-plugin-notification 走 Rust 端、不经 webview。不带 .sound —— 声音单独 afplay。
    match app
        .notification()
        .builder()
        .id(id)
        .title(title)
        .body(body)
        .show()
    {
        Ok(()) => {
            // 声音:Rust afplay 直接播文件,绕开 webview 的 autoplay/后台限制
            // (webview <audio> 在无 user gesture / 窗口后台时被拦 —— 这正是"有时没声音"的根因)。
            play_sound_native(app, configured, fallback);
            if let Some(s) = app.try_state::<AppState>() {
                if let Ok(mut g) = s.last_notify.lock() {
                    *g = Some((task_id, std::time::Instant::now()));
                }
            }
        }
        Err(e) => {
            tracing::debug!(err = %e, "notification show failed");
            // 横幅失败(未授权 / 系统拒绝)也要出声 —— 否则后台完成既无横幅又无声,彻底静默。
            play_sound_native(app, configured, fallback);
        }
    }
}

/// 进程内音频播放线程的句柄。afplay 每次 fork 新进程、开关默认音频输出设备,GUI app 子进程
/// 下连续播放第二次常哑(afplay 自身 exit 0,声音却没出来)。改用 rodio:常驻线程持有一个
/// OutputStream(设备一直开、不反复开关),每次播放只塞一个 Sink。
pub(crate) static AUDIO_TX: std::sync::OnceLock<std::sync::mpsc::Sender<std::path::PathBuf>> =
    std::sync::OnceLock::new();

/// 启动常驻音频线程(持有 rodio OutputStream)。app 启动时调用一次。
pub(crate) fn init_audio_thread() {
    let (tx, rx) = std::sync::mpsc::channel::<std::path::PathBuf>();
    let spawned = std::thread::Builder::new()
        .name("vibeterm-audio".into())
        .spawn(move || {
            let (_stream, handle) = match rodio::OutputStream::try_default() {
                Ok(s) => {
                    tracing::info!("audio: OutputStream 就绪(常驻)");
                    s
                }
                Err(e) => {
                    tracing::warn!(err = %e, "audio: 无默认输出设备,通知声音禁用");
                    return;
                }
            };
            // _stream 在本线程常驻 alive。串行播放:每条通知开一个 Sink,append 后
            // sleep_until_end 同步播到完再释放 —— 不用 detach。detach 是把 source 挂到 mixer
            // 异步播,连续播放时第二个 source 常不出声(用户实测:第二次弹了通知却没声音)。
            // 串行 = 每次独立 Sink、播完即释放,连续多次稳定。每步打 debug 便于诊断。
            while let Ok(path) = rx.recv() {
                tracing::info!(path = %path.display(), "audio: 收到播放请求");
                let src = match std::fs::File::open(&path) {
                    Ok(f) => match rodio::Decoder::new(std::io::BufReader::new(f)) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(err = %e, "audio: 解码失败");
                            continue;
                        }
                    },
                    Err(e) => {
                        tracing::warn!(err = %e, "audio: 打开文件失败");
                        continue;
                    }
                };
                match rodio::Sink::try_new(&handle) {
                    Ok(sink) => {
                        sink.append(src);
                        tracing::info!("audio: 开始播放(Sink)");
                        sink.sleep_until_end(); // 同步播到完,本线程串行
                        tracing::info!("audio: 播放结束");
                    }
                    Err(e) => tracing::warn!(err = %e, "audio: Sink 创建失败"),
                }
            }
            tracing::warn!("audio: 播放线程退出(channel 关闭)");
        });
    if spawned.is_ok() {
        let _ = AUDIO_TX.set(tx);
    } else {
        tracing::warn!("audio: 音频线程启动失败");
    }
}

/// 播放通知声音 —— 解析文件路径后发给常驻音频线程(rodio 进程内播放,绕开 afplay 反复 fork)。
/// `configured` 解析不到(空 / "default" / 无此文件)就退到 `fallback`;再不行则静默。
pub(crate) fn play_sound_native(app: &AppHandle, configured: &str, fallback: &str) {
    let path =
        resolve_sound_to_path(app, configured).or_else(|| resolve_sound_to_path(app, fallback));
    let Some(path) = path else {
        tracing::debug!(configured, fallback, "notify 声音:无可播文件,静默");
        return;
    };
    tracing::info!(path = %path.display(), "notify 声音 → rodio(send)");
    match AUDIO_TX.get() {
        Some(tx) => {
            let _ = tx.send(path);
        }
        None => tracing::warn!("notify 声音:音频线程未初始化"),
    }
}

/// agent hook 触发的"完成"通知 — 走完整守门 (preflight 通用),
/// 但事件 prefs 走 events.done (老 toml 字段, 语义重定义为 "agent_completed via hook").
/// throttle: 同 session_id 30s 内最多 1 发, 防 agent 来回对话连发.
pub fn fire_agent_completed_notification(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    agent: &str,
    session_id: &str,
    last_message: &str,
) {
    let Some((prefs, route)) = notify_preflight(app, tasks, task_id, true) else {
        return;
    };
    let event_prefs = &prefs.events.done;
    if !event_prefs.enabled {
        return;
    }

    // session 级 throttle (来回对话 5~10s 一发 turn, 用 30s 间隔合并).
    if let Some(s) = app.try_state::<AppState>() {
        let mut g = match s.last_agent_completed.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let now = std::time::Instant::now();
        if let Some(prev) = g.get(session_id) {
            if now.duration_since(*prev) < AGENT_COMPLETED_COOLDOWN {
                tracing::debug!(session_id, "agent_completed throttled");
                return;
            }
        }
        // 清理超出冷却窗口(取 2× 留余量)的旧 session 条目, 避免 map 无界增长.
        g.retain(|_, t| now.duration_since(*t) < AGENT_COMPLETED_COOLDOWN * 2);
        g.insert(session_id.to_string(), now);
    }

    let label = task_label(tasks, task_id);
    let title = format!("VibeTerm — {agent} 完成");
    let body = if last_message.is_empty() {
        label
    } else {
        format!("{label} · {last_message}")
    };
    let configured = event_prefs.sound.as_deref().unwrap_or("");
    match route {
        NotifyRoute::Background => {
            // 后台:系统横幅 + afplay 声音,都走 Rust 端、不经 webview。
            tracing::info!(configured, "fire 完成通知 → Background(横幅 + afplay)");
            send_notification(app, task_id, title, body, configured, "ringtone2");
        }
        NotifyRoute::ForegroundLight => {
            // 前台轻提示:不发横幅(macOS 前台横幅常被吞),只 afplay 一声 + 列表行高亮。
            // afplay 是独立进程,前台也不受 webview autoplay 限制,稳。
            tracing::info!(
                configured,
                "fire 完成通知 → ForegroundLight(afplay + 行高亮)"
            );
            play_sound_native(app, configured, "ringtone2");
            let _ = app.emit("task_flash", task_id);
        }
    }
}

pub(crate) fn task_label(tasks: &TaskRegistry, id: vibeterm_ipc::TaskId) -> String {
    tasks
        .name_of(id)
        .ok()
        .flatten()
        .unwrap_or_else(|| format!("task #{id}"))
}

/// 拼通知 body. 三段式 — task_label · [agent_kind] · last_output(截 60 字).
/// last_output / agent_kind 缺失时优雅降级.
pub(crate) fn format_notify_body(
    label: &str,
    last_output: Option<&str>,
    agent_kind: Option<&str>,
) -> String {
    let mut parts = vec![label.to_string()];
    if let Some(k) = agent_kind {
        parts.push(format!("[{k}]"));
    }
    if let Some(t) = last_output {
        // 60 字符截断 (按 char count, 中日韩 char 也算 1)
        const MAX_TAIL: usize = 60;
        let chars: Vec<char> = t.chars().collect();
        let tail = if chars.len() > MAX_TAIL {
            let mut s: String = chars.into_iter().take(MAX_TAIL).collect();
            s.push('…');
            s
        } else {
            t.to_string()
        };
        parts.push(tail);
    }
    parts.join(" · ")
}

/// 判断声音字段是否为本地文件路径(而非内建声音名).
/// 绝对路径(Unix `/`、Windows 盘符 `C:\`/`C:/`、UNC `\\`)/ `~/`、`~\` 前缀视为路径;
/// 其它(空 / "default" / "Glass" 等)视为系统名.
pub(crate) fn sound_is_file_path(s: &str) -> bool {
    let s = s.trim();
    let drive_abs = {
        let b = s.as_bytes();
        b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
    };
    s.starts_with('/')
        || s.starts_with("~/")
        || s.starts_with("~\\")
        || s.starts_with("\\\\")
        || drive_abs
}

/// 把 `~/...`(或 Windows 习惯的 `~\...`)展开到 home, 其它路径原样.
/// 不读 $HOME 环境变量 —— Windows 默认没有, dirs::home_dir() 两边都对.
pub(crate) fn expand_tilde(s: &str) -> std::path::PathBuf {
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(s)
}

/// 把 NotifyPrefs.sound 字段解析为可读的本地音频文件.
/// 返回 None 表示走系统默认(空字符串 / "default" / 没匹配到任何声音文件).
///
/// 查找顺序:
///   1. 绝对路径 / `~/` → 用户自选文件
///   2. VibeTerm 自带 (resource_dir/sounds/<name>.mp3) — 跨平台一致
///   3. macOS 系统声音 (/System/Library/Sounds/<name>.aiff)
///   4. macOS 用户声音 (~/Library/Sounds/<name>.aiff)
pub(crate) fn resolve_sound_to_path(app: &AppHandle, sound: &str) -> Option<std::path::PathBuf> {
    let s = sound.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("default") {
        return None;
    }
    if sound_is_file_path(s) {
        let p = expand_tilde(s);
        if !p.is_file() {
            return None;
        }
        // 收敛任意文件读取:仅允许音频扩展名,且 canonicalize 后必须落在 $HOME 内.
        // (前端可控字符串 → 此前可读 /etc/passwd、~/.ssh/id_rsa 等任意 <10MB 文件)
        if mime_for_ext(&p) == "application/octet-stream" {
            tracing::warn!(path = %p.display(), "notify sound rejected: non-audio extension");
            return None;
        }
        let canon = match std::fs::canonicalize(&p) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %p.display(), err = %e, "notify sound canonicalize failed");
                return None;
            }
        };
        // HOME 也 canonicalize,避免 /var → /private/var 等符号链接导致合法路径被误拒.
        // dirs::home_dir() 而非 $HOME —— Windows 默认没有 $HOME, 否则所有自定义路径都被拒.
        let home_ok = dirs::home_dir().and_then(|h| {
            let home_canon = std::fs::canonicalize(&h).unwrap_or(h);
            canon.starts_with(&home_canon).then_some(())
        });
        if home_ok.is_some() {
            return Some(canon);
        }
        tracing::warn!(path = %canon.display(), "notify sound rejected: outside home dir");
        return None;
    }
    // 自带:打包资源里 resources/sounds/<id>.mp3
    if let Ok(res_dir) = app.path().resource_dir() {
        let bundled = res_dir.join("resources/sounds").join(format!("{s}.mp3"));
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    // macOS 系统名 fallback
    #[cfg(target_os = "macos")]
    {
        let sys = std::path::PathBuf::from("/System/Library/Sounds").join(format!("{s}.aiff"));
        if sys.is_file() {
            return Some(sys);
        }
        if let Some(home) = dirs::home_dir() {
            let user = home.join("Library/Sounds").join(format!("{s}.aiff"));
            if user.is_file() {
                return Some(user);
            }
        }
    }
    // Windows 系统名 fallback: %SystemRoot%\Media\<name>.wav (Windows Notify.wav 等)
    #[cfg(target_os = "windows")]
    {
        if let Ok(root) = std::env::var("SystemRoot") {
            let sys = std::path::PathBuf::from(root)
                .join("Media")
                .join(format!("{s}.wav"));
            if sys.is_file() {
                return Some(sys);
            }
        }
    }
    None
}

pub(crate) fn mime_for_ext(p: &std::path::Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
    {
        Some(ref e) if e == "aiff" || e == "aif" || e == "aifc" => "audio/aiff",
        Some(ref e) if e == "wav" => "audio/wav",
        Some(ref e) if e == "mp3" => "audio/mpeg",
        Some(ref e) if e == "ogg" || e == "oga" => "audio/ogg",
        Some(ref e) if e == "m4a" || e == "mp4" || e == "aac" => "audio/mp4",
        Some(ref e) if e == "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

/// 大文件保护. 音频通常 < 1MB; 上限 10MB 防意外塞超长 WAV.
pub(crate) const NOTIFY_SOUND_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 把声音字段解析后读字节回传, 前端用 <audio> 播放.
/// 既给设置面板"试听"按钮用, 也给"自定义文件路径"通知触发时实时播放用.
#[tauri::command]
pub(crate) async fn preview_notify_sound(
    app: AppHandle,
    sound: String,
) -> IpcResult<NotifySoundData> {
    use base64::Engine;
    let path = resolve_sound_to_path(&app, &sound).ok_or_else(|| IpcError::NotFound {
        resource: "notify_sound".into(),
        id: sound.clone(),
    })?;
    let meta = std::fs::metadata(&path).map_err(|e| IpcError::Unknown {
        trace_id: format!("notify_sound_meta:{e}"),
    })?;
    if meta.len() > NOTIFY_SOUND_MAX_BYTES {
        return Err(IpcError::PermissionDenied {
            reason: format!(
                "audio file too large ({} bytes, max {})",
                meta.len(),
                NOTIFY_SOUND_MAX_BYTES
            ),
        });
    }
    let bytes = std::fs::read(&path).map_err(|e| IpcError::Unknown {
        trace_id: format!("notify_sound_read:{e}"),
    })?;
    Ok(NotifySoundData {
        mime: mime_for_ext(&path).to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// 自带声音库:从 bundle resources/sounds/sounds.json 读清单.
/// 前端下拉用 (按 category 分组). 找不到 manifest 返回空数组 (前端降级).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub(crate) struct BuiltinSound {
    id: String,
    name: String,
    category: String,
    #[allow(dead_code)]
    file: String,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct SoundsManifest {
    sounds: Vec<BuiltinSound>,
}

#[tauri::command]
pub(crate) async fn list_builtin_sounds(app: AppHandle) -> IpcResult<Vec<BuiltinSound>> {
    let Ok(res_dir) = app.path().resource_dir() else {
        return Ok(vec![]);
    };
    let path = res_dir.join("resources/sounds/sounds.json");
    let Ok(s) = std::fs::read_to_string(&path) else {
        tracing::debug!(?path, "sounds.json not found");
        return Ok(vec![]);
    };
    let manifest: SoundsManifest = match serde_json::from_str(&s) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(err = %e, "sounds.json parse failed");
            return Ok(vec![]);
        }
    };
    Ok(manifest.sounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(enabled: bool, focused_other: bool) -> NotifyFile {
        NotifyFile {
            enabled,
            notify_focused_other_task: focused_other,
            ..NotifyFile::default()
        }
    }

    /// 后台 + agent 任务 → 完整系统通知
    #[test]
    fn background_agent_routes_background() {
        let r = decide_notify_route(
            true,
            false,
            &prefs(true, true),
            "12:00",
            false,
            false,
            false,
        );
        assert_eq!(r, Some(NotifyRoute::Background));
    }

    /// 非 agent 任务恒静默(普通 shell 命令结束不打扰)
    #[test]
    fn non_agent_is_silent() {
        let r = decide_notify_route(
            false,
            false,
            &prefs(true, true),
            "12:00",
            false,
            false,
            true,
        );
        assert_eq!(r, None);
    }

    /// per-task 静音 / 全局开关关 → 静默
    #[test]
    fn muted_or_disabled_is_silent() {
        let p = prefs(true, true);
        assert_eq!(
            decide_notify_route(true, true, &p, "12:00", false, false, true),
            None
        );
        let p = prefs(false, true);
        assert_eq!(
            decide_notify_route(true, false, &p, "12:00", false, false, true),
            None
        );
    }

    /// 前台 + allow_foreground=false(waiting_input)→ 静默,维持"前台不打扰"
    #[test]
    fn foreground_without_allow_is_silent() {
        let r = decide_notify_route(true, false, &prefs(true, true), "12:00", true, false, false);
        assert_eq!(r, None);
    }

    /// 前台 + 完成的是当前选中任务 → 静默(用户正看着)
    #[test]
    fn foreground_active_task_is_silent() {
        let r = decide_notify_route(true, false, &prefs(true, true), "12:00", true, true, true);
        assert_eq!(r, None);
    }

    /// 前台 + 其它任务完成 + 开关开 → 轻提示
    #[test]
    fn foreground_other_task_routes_light() {
        let r = decide_notify_route(true, false, &prefs(true, true), "12:00", true, false, true);
        assert_eq!(r, Some(NotifyRoute::ForegroundLight));
    }

    /// 前台 + 其它任务完成但 notify_focused_other_task 关 → 静默
    #[test]
    fn foreground_other_task_disabled_is_silent() {
        let r = decide_notify_route(true, false, &prefs(true, false), "12:00", true, false, true);
        assert_eq!(r, None);
    }

    /// 免打扰时段 → 静默(跨午夜语义由 QuietHours 自己的测试覆盖)
    #[test]
    fn quiet_hours_is_silent() {
        let mut p = prefs(true, true);
        p.quiet_hours.enabled = true;
        p.quiet_hours.start = "22:00".into();
        p.quiet_hours.end = "08:00".into();
        let r = decide_notify_route(true, false, &p, "23:30", false, false, true);
        assert_eq!(r, None);
    }

    /// 路径判定:Unix / Windows 盘符 / UNC / 波浪号都算路径;声音名不算
    #[test]
    fn sound_path_detection_cross_platform() {
        assert!(sound_is_file_path("/tmp/bell.mp3"));
        assert!(sound_is_file_path("~/sounds/bell.mp3"));
        assert!(sound_is_file_path("~\\sounds\\bell.wav"));
        assert!(sound_is_file_path("C:\\Users\\demo\\bell.wav"));
        assert!(sound_is_file_path("c:/Users/demo/bell.wav"));
        assert!(sound_is_file_path("\\\\server\\share\\bell.wav"));
        assert!(!sound_is_file_path("Glass"));
        assert!(!sound_is_file_path("default"));
        assert!(!sound_is_file_path(""));
        assert!(!sound_is_file_path("C:")); // 裸盘符不算
    }

    /// 波浪号展开:`~/` 与 `~\` 都展开到 home;非波浪号原样
    #[test]
    fn expand_tilde_uses_home_dir() {
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(expand_tilde("~/x/y.mp3"), home.join("x/y.mp3"));
        assert_eq!(expand_tilde("~\\x\\y.wav"), home.join("x\\y.wav"));
        assert_eq!(
            expand_tilde("/abs/p.mp3"),
            std::path::PathBuf::from("/abs/p.mp3")
        );
    }
}
