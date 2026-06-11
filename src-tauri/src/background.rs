//! 后台常驻任务:config/usage/session/codex 监听、agent 识别轮询、tail/worktree 轮询、
//! 200ms status-tick。setup 时一次性启动。从 main.rs 拆出(行为不变)。

use std::sync::Arc;

use tauri::{Emitter, Manager};
use vibeterm_ipc::TerminalId;
use vibeterm_status::StatusDetector;

use crate::events::record_event;
use crate::{
    emit_tasks_changed, maybe_persistent_remind, notify_status_transition, now_ms,
    poll_agent_turn_for_terminal, refresh_dock_badge, terminal_cwd_for, validated_worktree_path,
    AppState,
};

pub(crate) fn start_background_tasks(app: &tauri::AppHandle) {
    // 启动 config watcher(50ms debounce)
    // 同时 fan-out 到 statusline_config_changed — 用户改 statusline.toml 即时生效
    let app_h = app.clone();
    if let Ok(w) = vibeterm_config::ConfigWatcher::start(move || {
        tracing::info!("config dir changed → emit config_changed + statusline_config_changed");
        let _ = app_h.emit("config_changed", ());
        let _ = app_h.emit("statusline_config_changed", ());
    }) {
        // watcher 被 leak 让其活到 app 退出(简化)
        Box::leak(Box::new(w));
    }

    // Agent watch v1: Claude usage_cache.json 监听
    let app_for_usage = app.clone();
    let (usage_tx, mut usage_rx) = tokio::sync::mpsc::unbounded_channel::<
        vibeterm_agent_watch::claude::usage_cache::UsageCacheUpdate,
    >();
    vibeterm_agent_watch::claude::usage_cache::spawn_watcher(usage_tx);
    tauri::async_runtime::spawn(async move {
        while let Some(update) = usage_rx.recv().await {
            let _ = app_for_usage.emit("claude_usage_changed", &update.cache);
        }
    });

    // Agent watch v2: Claude project transcript 监听
    let app_for_session = app.clone();
    let (sess_tx, mut sess_rx) =
        tokio::sync::mpsc::unbounded_channel::<Option<vibeterm_agent_watch::ClaudeSession>>();
    vibeterm_agent_watch::claude::project::spawn_watcher(sess_tx);
    tauri::async_runtime::spawn(async move {
        while let Some(sess) = sess_rx.recv().await {
            // watcher 只刷新显示(model/ctx/cost),不驱动完成检测。
            // 为何:这里的 ClaudeSession 来自 find_active_session_file() —— 全局 mtime 最新
            // 的会话,未必是本任务 agent 的。典型反例:同一仓库里 Claude Code 自身几百 MB 的
            // transcript,每条消息都在写 → mtime 几乎永远最新,且超限只能读末尾 stop_reason。
            // 若用它驱动完成,会把"另一个 claude 会话答完了"误判成本任务 agent 答完 → claude
            // 完成漏报(只剩 claude 自己 hook 弹的无声通知)。完成检测一律走 3s 轮询的
            // poll_agent_turn_from_transcript → read_for_cwd(按 task.cwd 精确定位 + 排除超限
            // 巨型会话)。codex 因会话按日期分目录、snapshot 自带精确 cwd,无此碰撞,保留其
            // watcher 完成路径。
            let _ = app_for_session.emit("claude_session_changed", &sess);
        }
    });

    // Agent watch v3: Codex session 监听
    let app_for_codex = app.clone();
    let (codex_tx, mut codex_rx) =
        tokio::sync::mpsc::unbounded_channel::<Option<vibeterm_agent_watch::CodexSnapshot>>();
    vibeterm_agent_watch::codex::session::spawn_watcher(codex_tx);
    tauri::async_runtime::spawn(async move {
        while let Some(snap) = codex_rx.recv().await {
            // watcher 只刷新显示(model/ctx/cost)。完成检测**一律走 3s 轮询的
            // poll_agent_turn_for_terminal**(per-terminal:按每个 agent 终端各自 cwd 检测)——
            // watcher 推的是全局最新 rollout,不知对应哪个 task/terminal,无法 per-terminal 归属。
            let _ = app_for_codex.emit("codex_session_changed", &snap);
        }
    });

    // agent 进程识别轮询(每 3s)
    //   扫每个 task 的 terminals 的 shell pid → detect_agent_for_shell。
    //   有变化 emit tasks_changed。pgrep 在前台 idle 时也会运行,~1ms 量级,可接受。
    let app_for_agent = app.clone();
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_secs(3);
        loop {
            tokio::time::sleep(interval).await;
            let state = match app_for_agent.try_state::<AppState>() {
                Some(s) => s,
                None => continue,
            };
            let pairs = match state.tasks.task_terminal_ids() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mut any_changed = false;
            for (task_id, term_ids) in pairs {
                // **per-terminal**:一个 task 可分屏多个 agent,逐终端独立识别 + 独立完成检测,
                // 不再只认第一个 agent(否则后开的 agent 完成检测不到 → 不通知 + 答完误判黄圈)。
                // stall 也只对真跑 agent 的终端开(辅助 idle shell 不开,见下方)。
                let mut agent_per_term: Vec<(TerminalId, Option<String>)> = Vec::new();
                for term_id in &term_ids {
                    let kind = state
                        .terminals
                        .pid_of(*term_id)
                        .and_then(vibeterm_status::detect_agent_for_shell)
                        .map(|k| k.as_str().to_string());
                    agent_per_term.push((*term_id, kind.clone()));
                    // per-terminal 写入该终端的 agent kind。
                    if let Ok(true) = state.tasks.set_agent_kind(task_id, *term_id, kind) {
                        any_changed = true;
                    }
                }
                // 兜底完成检测:逐 agent 终端,用**该终端自己的 cwd**主动读 transcript 完成态。
                // (watcher 不可靠 + 不知 per-terminal 归属;这里 task_id + term_id + cwd 都精确。)
                for (term_id, kind) in &agent_per_term {
                    if let Some(kind) = kind {
                        if let Some(cwd) = terminal_cwd_for(&state, *term_id) {
                            poll_agent_turn_for_terminal(
                                &app_for_agent,
                                &state,
                                task_id,
                                *term_id,
                                kind,
                                &cwd,
                            );
                        }
                    }
                }
                // 按 per-terminal 嗅探到的 agent kind:
                //   - 装对应授权框正则(set_agent_rules) → body 正则识别 WaitingInput;
                //   - 真跑 agent 的 terminal 才开 stall 检测(辅助 idle shell 不开)。
                if let Ok(detectors) = state.status_detectors.lock() {
                    for (term_id, kind) in &agent_per_term {
                        if let Some(d) = detectors.get(term_id) {
                            let flipped = if let Ok(mut det) = d.lock() {
                                det.set_agent_rules(kind.as_deref());
                                if kind.is_some() {
                                    det.enable_stall_detection(0);
                                    None
                                } else {
                                    // agent 退出回 shell:Stalled→Idle 的返回值要回写
                                    // registry,否则红橙描边环挂到下一个 PTY chunk 才消。
                                    det.disable_stall_detection()
                                }
                            } else {
                                None
                            };
                            if let Some(s) = flipped {
                                if let Ok(Some(_)) =
                                    state.tasks.update_terminal_status(*term_id, s, false)
                                {
                                    any_changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if any_changed {
                emit_tasks_changed(&app_for_agent, &state.tasks);
            }
        }
    });

    // last_output 节流轮询(750ms/轮)
    //   每个 task 取 terminal_ids.last() 的末行,与上轮快照比较,
    //   有差异才 emit_tasks_changed。避免 stdout 频繁刷新时 emit 风暴。
    let app_for_tail = app.clone();
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_millis(750);
        let mut prev: std::collections::HashMap<vibeterm_ipc::TaskId, Option<String>> =
            std::collections::HashMap::new();
        loop {
            tokio::time::sleep(interval).await;
            let state = match app_for_tail.try_state::<AppState>() {
                Some(s) => s,
                None => continue,
            };
            let pairs = match state.tasks.task_terminal_ids() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mut any_changed = false;
            let mut alive: std::collections::HashSet<vibeterm_ipc::TaskId> =
                std::collections::HashSet::new();
            for (task_id, term_ids) in pairs {
                alive.insert(task_id);
                let tail = if term_ids.is_empty() {
                    None
                } else {
                    state.terminals.most_recent_tail(&term_ids)
                };
                let prev_tail = prev.get(&task_id).cloned().unwrap_or(None);
                if prev_tail != tail {
                    prev.insert(task_id, tail);
                    any_changed = true;
                }
            }
            // 清理已删任务的快照
            prev.retain(|k, _| alive.contains(k));
            if any_changed {
                emit_tasks_changed(&app_for_tail, &state.tasks);
            }
        }
    });

    // worktree 状态轮询(5s/轮)
    //   仅扫挂了 worktree 的 task,对每个跑 `git status --porcelain=v2 --branch`。
    //   有任意字段变化 → emit tasks_changed。
    //   只在内存更新(update_worktree_status 不写盘),IO 噪音低。
    let app_for_poll = app.clone();
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_secs(5);
        loop {
            tokio::time::sleep(interval).await;
            let state = match app_for_poll.try_state::<AppState>() {
                Some(s) => s,
                None => continue,
            };
            let pairs = match state.tasks.worktree_tasks() {
                Ok(v) => v,
                Err(_) => continue,
            };
            if pairs.is_empty() {
                continue;
            }
            let mut any_changed = false;
            for (task_id, wt) in pairs {
                let wt_path = match validated_worktree_path(&wt.worktree_path) {
                    Some(p) => p,
                    None => continue,
                };
                let st = match vibeterm_git::worktree_status(&wt_path).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!(?task_id, err = %e, "worktree status poll failed");
                        continue;
                    }
                };
                let changed = st.head != wt.head
                    || st.branch != wt.branch
                    || st.is_dirty != wt.is_dirty
                    || st.ahead != wt.ahead
                    || st.behind != wt.behind;
                if changed {
                    let _ = state.tasks.update_worktree_status(
                        task_id,
                        st.head,
                        st.branch,
                        st.is_dirty,
                        st.ahead,
                        st.behind,
                        now_ms(),
                    );
                    any_changed = true;
                }
            }
            if any_changed {
                emit_tasks_changed(&app_for_poll, &state.tasks);
            }
        }
    });

    // 全局 status-tick 任务(200ms/轮):遍历所有活跃 StatusDetector 做 stall/idle
    // 时间态判定. 取代旧的"每终端一个 OS 线程"——单任务即可; detector 一旦从
    // status_detectors 摘除(close / PTY 退出)就自动不再被 tick,无线程泄漏可能.
    let app_for_status_tick = app.clone();
    tauri::async_runtime::spawn(async move {
        let interval = std::time::Duration::from_millis(200);
        loop {
            tokio::time::sleep(interval).await;
            let state = match app_for_status_tick.try_state::<AppState>() {
                Some(s) => s,
                None => continue,
            };
            // 快照 (tid, detector) 后立即释放 map 锁,避免 tick/emit 期间持锁.
            let detectors: Vec<(TerminalId, Arc<std::sync::Mutex<StatusDetector>>)> =
                match state.status_detectors.lock() {
                    Ok(m) => m.iter().map(|(k, v)| (*k, v.clone())).collect(),
                    Err(_) => continue,
                };
            for (tid, det) in detectors {
                let (new_state, idle_by_osc) = {
                    let mut d = det.lock().unwrap_or_else(|p| p.into_inner());
                    (d.tick(), d.idle_by_osc())
                };
                let Some(s) = new_state else { continue };
                // TOCTOU 复核:释放 detector 锁到回写 registry 之间,PTY 读线程的 feed
                // 可能已把状态推进(如 Idle 判定后 chunk 到达 → Running)。乱序回写会把
                // 活跃终端钉在 Idle 直到下一个 ≥800ms 输出间歇。不一致则放弃本次回写。
                let still_current = det.lock().map(|d| d.current() == s).unwrap_or(false);
                if !still_current {
                    continue;
                }
                if let Ok(Some((task_id, prev_agg, new_agg))) =
                    state.tasks.update_terminal_status(tid, s, idle_by_osc)
                {
                    let _ = app_for_status_tick.emit(
                        "task_status_changed",
                        serde_json::json!({"task_id": task_id, "status": s}),
                    );
                    record_event(
                        "status_changed",
                        task_id,
                        Some(tid),
                        serde_json::to_value(s).ok(),
                    );
                    emit_tasks_changed(&app_for_status_tick, &state.tasks);
                    notify_status_transition(
                        &app_for_status_tick,
                        &state.tasks,
                        task_id,
                        tid,
                        prev_agg,
                        new_agg,
                        idle_by_osc,
                    );
                    refresh_dock_badge(&app_for_status_tick, &state.tasks);
                }
            }
            // 每轮 tick 末尾:间歇持续提醒(单路全局声音)检查
            maybe_persistent_remind(&app_for_status_tick, &state);
        }
    });
}
