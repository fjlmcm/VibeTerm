//! 真业务流集成测试
//!
//! main.rs 的每个 #[tauri::command] handler 是薄壳,真业务逻辑在
//! TaskRegistry / TerminalRegistry。这里直接跑业务流:
//!
//!   1. create_task → attach_terminal → spawn → write → echo → close
//!   2. 多任务并发 spawn(verify TerminalRegistry id 分配 + 隔离)
//!   3. close_task 联动 detach terminals
//!   4. pin / rename / reorder 任务级操作
//!
//! 覆盖 main.rs handler 背后 80% 业务流(handler 只是参数转换 + emit event)。
//!
//! 注意:涉及 PTY spawn 的测试用 Unix shell 语义。Windows ConPTY 上
//! portable-pty Drop 行为不稳(测试卡死),所以 PTY 相关 case
//! gate 到 `#[cfg(unix)]`。Windows 验证靠 tauri-cdp 真 E2E。
//! 纯任务 state 操作(pin/rename/reorder/status/location)继续跑两个平台。

use vibeterm_core::TaskRegistry;
use vibeterm_ipc::{TaskLocation, TaskStatus};

/// 测试隔离守卫 —— 必须在任何 `TaskRegistry::new()` 之前持有。
///
/// `TaskRegistry::new()` → `vibeterm_tasks::load()/save()` 走全局 `config_dir()`,
/// 即 `~/Library/Application Support/VibeTerm/tasks.json`(debug build 下 `config_dir()`
/// 读 `VIBETERM_CONFIG_DIR`,没设就落用户真实目录)。不隔离时 `cargo test` 会把测试
/// 任务(a / b / c …)写进用户真实 tasks.json,污染真实数据(本守卫即为修此根因)。
///
/// 守卫职责:① 全局 `Mutex` 串行化所有碰 `TaskRegistry` 的测试(`VIBETERM_CONFIG_DIR`
/// 是进程级 env,并行 `set_var` 会互相覆盖);② 每次指向全新临时目录(clean slate +
/// 绝不碰真实 tasks.json)。返回值须绑定到具名变量(`let _cfg = isolated_config();`)
/// 以在整个测试期间存活(返回的 `MutexGuard` / `TempDir` 本身即 `#[must_use]`,
/// 漏绑会触发 `unused_must_use` 警告兜底)。
fn isolated_config() -> (std::sync::MutexGuard<'static, ()>, tempfile::TempDir) {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // 中毒(某测试 panic 持锁)也继续:隔离仍有效,不连累后续测试。
    let guard = GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().expect("create temp config dir for test isolation");
    std::env::set_var("VIBETERM_CONFIG_DIR", dir.path());
    (guard, dir)
}

#[cfg(unix)]
mod pty_helpers {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use vibeterm_pty::SpawnOpts;

    pub fn shell_args(script: &str) -> (String, Vec<String>) {
        ("/bin/sh".into(), vec!["-c".into(), script.into()])
    }

    pub fn opts(script: &str) -> SpawnOpts {
        let (command, args) = shell_args(script);
        SpawnOpts {
            rows: 24,
            cols: 80,
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            command,
            args,
            env: vec![("LANG".into(), "C".into()), ("TERM".into(), "dumb".into())],
        }
    }

    pub fn tmp_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    pub fn drain_until(rx: &mpsc::Receiver<Vec<u8>>, needle: &str, timeout: Duration) -> String {
        let mut acc = Vec::new();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(chunk) => {
                    acc.extend_from_slice(&chunk);
                    if let Ok(s) = std::str::from_utf8(&acc) {
                        if s.contains(needle) {
                            return s.to_string();
                        }
                    }
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&acc).into_owned()
    }
}

#[cfg(unix)]
#[test]
fn full_create_spawn_write_close_cycle() {
    use pty_helpers::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use vibeterm_core::TerminalRegistry;
    use vibeterm_pty::sinks::MpscSink;

    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let terminals = TerminalRegistry::new();

    // 1. create task
    let tmp = tmp_dir();
    let task_id = tasks
        .create("dev".into(), Some(tmp.clone()), None)
        .expect("create");
    let dto = tasks.task_dto(task_id).unwrap().unwrap();
    assert_eq!(dto.name, "dev");
    assert_eq!(dto.cwd.as_deref(), Some(tmp.as_str()));
    assert_eq!(dto.terminal_ids.len(), 0);
    assert_eq!(dto.location, TaskLocation::MainWorkspace);

    let (tx, rx) = mpsc::channel();
    let script = "echo VIBETERM_BIZ_FLOW; read line; echo got=$line";
    let term_id = terminals
        .spawn(opts(script), MpscSink::new(tx))
        .expect("spawn");
    tasks.attach_terminal(task_id, term_id).expect("attach");
    let dto = tasks.task_dto(task_id).unwrap().unwrap();
    assert_eq!(dto.terminal_ids, vec![term_id]);

    let out = drain_until(&rx, "VIBETERM_BIZ_FLOW", Duration::from_secs(3));
    assert!(out.contains("VIBETERM_BIZ_FLOW"));

    terminals
        .write(term_id, b"HELLO_FROM_TEST\n")
        .expect("write");
    let out = drain_until(&rx, "got=HELLO_FROM_TEST", Duration::from_secs(3));
    assert!(out.contains("got=HELLO_FROM_TEST"));

    terminals.close(term_id).expect("close terminal");
    assert_eq!(terminals.count(), 0);

    let detached_terms = tasks.close(task_id).expect("close task");
    assert_eq!(detached_terms, vec![term_id]);
    assert!(tasks.task_dto(task_id).unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn multiple_tasks_isolated_terminals() {
    use pty_helpers::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use vibeterm_core::TerminalRegistry;
    use vibeterm_pty::sinks::MpscSink;

    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let terminals = TerminalRegistry::new();

    let t1 = tasks.create("task-1".into(), None, None).unwrap();
    let t2 = tasks.create("task-2".into(), None, None).unwrap();

    let (tx1, rx1) = mpsc::channel();
    let (tx2, rx2) = mpsc::channel();
    let term1 = terminals
        .spawn(opts("echo FROM_T1"), MpscSink::new(tx1))
        .unwrap();
    let term2 = terminals
        .spawn(opts("echo FROM_T2"), MpscSink::new(tx2))
        .unwrap();

    tasks.attach_terminal(t1, term1).unwrap();
    tasks.attach_terminal(t2, term2).unwrap();

    // ID 必须不同
    assert_ne!(term1, term2);

    // sink 隔离:rx1 只看到 FROM_T1,rx2 只看到 FROM_T2
    let out1 = drain_until(&rx1, "FROM_T1", Duration::from_secs(2));
    let out2 = drain_until(&rx2, "FROM_T2", Duration::from_secs(2));
    assert!(out1.contains("FROM_T1") && !out1.contains("FROM_T2"));
    assert!(out2.contains("FROM_T2") && !out2.contains("FROM_T1"));

    // task DTO 各自正确
    assert_eq!(
        tasks.task_dto(t1).unwrap().unwrap().terminal_ids,
        vec![term1]
    );
    assert_eq!(
        tasks.task_dto(t2).unwrap().unwrap().terminal_ids,
        vec![term2]
    );
}

#[test]
fn pin_rename_reorder_persistence_in_dto() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let t1 = tasks.create("a".into(), None, None).unwrap();
    let t2 = tasks.create("b".into(), None, None).unwrap();
    let t3 = tasks.create("c".into(), None, None).unwrap();

    // Pin t2
    tasks.pin(t2, true).unwrap();
    assert!(tasks.task_dto(t2).unwrap().unwrap().pinned);
    assert!(!tasks.task_dto(t1).unwrap().unwrap().pinned);

    // Rename t1
    tasks.rename(t1, "a-renamed".into()).unwrap();
    assert_eq!(tasks.task_dto(t1).unwrap().unwrap().name, "a-renamed");

    // Reorder
    tasks.reorder(vec![t3, t1, t2]).unwrap();
    let list = tasks.list().unwrap();
    // reorder 不再丢弃未在 new_order 中的已有 task(TaskRegistry 共享持久化 tasks.json,
    // 同一测试二进制内其它 case 可能留下遗留 task)。只断言本测试自己的三个 task
    // 相对顺序为 [t3, t1, t2],对预先存在的 task 保持隔离健壮。
    let mine: Vec<_> = list
        .iter()
        .map(|t| t.id)
        .filter(|id| [t1, t2, t3].contains(id))
        .collect();
    assert_eq!(mine, vec![t3, t1, t2]);
}

// hook 触发"agent 完成" → seen=false → 当所有 terminal 自然到 Idle 时
// aggregate 应升 Done (描边环视觉).
#[test]
fn mark_agent_completed_flips_aggregate_to_done() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let task_id = tasks.create("agent-task".into(), None, None).unwrap();
    // Done 现在要求"非当前 active"(当前正看的任务完成显示 Idle、不打扰)。create 第一个 task
    // 默认即 active_main,故另建一个切过去,让 agent-task 成为后台任务 —— 完成后才聚合成 Done。
    let other = tasks.create("other".into(), None, None).unwrap();
    tasks.set_active_main(other).unwrap();
    let term = 7;
    tasks.attach_terminal(task_id, term).unwrap();
    // agent 跑起来 → Running
    tasks
        .update_terminal_status(term, TaskStatus::Running, false)
        .unwrap();
    assert_eq!(
        tasks.aggregated_status_of(task_id).unwrap(),
        Some(TaskStatus::Running)
    );

    // hook 收到完成信号 — 此时 terminal 输出可能还在 flush, status 仍是 Running
    let flipped = tasks.mark_agent_completed(task_id).unwrap();
    assert!(flipped, "首次 mark 应翻 seen");
    // 此时还是 Running (Running 优先级 > Done)
    assert_eq!(
        tasks.aggregated_status_of(task_id).unwrap(),
        Some(TaskStatus::Running)
    );

    // status detector 自然推进到 Idle (timeout)
    tasks
        .update_terminal_status(term, TaskStatus::Idle, false)
        .unwrap();
    // 现在 seen=false + 全部 Idle → Done
    assert_eq!(
        tasks.aggregated_status_of(task_id).unwrap(),
        Some(TaskStatus::Done),
        "hook 完成 + 自然 Idle 应升 Done"
    );

    // 重复 mark 是幂等 — 返回 false
    assert!(!tasks.mark_agent_completed(task_id).unwrap());
}

// codex 答完后那个常驻输入框会被嗅探成 WaitingInput。锁住:transcript "轮已结束"
// (set_agent_turn_done(true)) 必须压过 WaitingInput 黄灯;切回看过(seen=true)后再切出 = Idle,
// 不再被输入框黄灯顶回去(用户实测 bug:切回变灰、再切出又变黄圈)。
#[test]
fn agent_turn_done_overrides_waiting_input_and_respects_seen() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let agent_task = tasks.create("codex-task".into(), None, None).unwrap();
    let other = tasks.create("other".into(), None, None).unwrap();
    tasks.set_active_main(other).unwrap(); // agent_task 非当前
    let term = 7;
    tasks.attach_terminal(agent_task, term).unwrap();

    // 复现前提:没有 transcript 信号时,codex 答完后输入框误判 WaitingInput,聚合确会显示黄灯。
    tasks
        .update_terminal_status(term, TaskStatus::WaitingInput, false)
        .unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::WaitingInput),
    );

    // transcript 读到 task_complete → 轮已结束。压过 WaitingInput,非当前 + 未看 → Done。
    let (changed, just_completed) = tasks
        .set_agent_turn_done(agent_task, term, true, None)
        .unwrap();
    assert!(changed && just_completed, "首次答完应是跃迁");
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Done),
        "轮已结束应压过 WaitingInput,非当前未看 → Done"
    );

    // 切回看(set_active_main 置 seen=true)→ 当前任务不打扰 → Idle。
    tasks.set_active_main(agent_task).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Idle),
        "切回当前 → Idle"
    );

    // 再切出 —— 已看过(seen=true),即使输入框仍 WaitingInput、轮仍 Some(true),也应是 Idle,
    // 不再被黄灯顶回去(正是用户报的 bug)。
    tasks.set_active_main(other).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Idle),
        "看过后再切出应保持 Idle,不再变黄圈"
    );

    // 用户提下一个 prompt → codex 又开始干(Some(false))→ Running(WaitingInput 上面已优先,这里无黄灯)。
    let (changed, _) = tasks
        .set_agent_turn_done(agent_task, term, false, None)
        .unwrap();
    assert!(changed, "done→working 应是变化");
    tasks
        .update_terminal_status(term, TaskStatus::Idle, false)
        .unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Running),
        "轮进行中 → Running"
    );
}

// claude 答完后 caffeinate keep-alive / MCP server 的后台输出会把终端打成 Running,
// 旧逻辑无条件 seen=true 把未读 Done 冲成 Idle(用户报:答完弹通知,但切回去前圆点就变灰了)。
// 锁住:轮已结束(Some(true))时,终端后台 Running/WaitingInput 不清未读。
#[test]
fn agent_done_not_cleared_by_background_terminal_noise() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let agent_task = tasks.create("claude-task".into(), None, None).unwrap();
    let other = tasks.create("other".into(), None, None).unwrap();
    tasks.set_active_main(other).unwrap(); // agent_task 非当前
    let term = 9;
    tasks.attach_terminal(agent_task, term).unwrap();

    // claude 答完一轮 → 非当前未看 → Done。
    tasks
        .set_agent_turn_done(agent_task, term, true, None)
        .unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Done)
    );

    // 答完后 caffeinate / MCP / 收尾输出把终端打成 Running —— 不该清掉未读 Done。
    tasks
        .update_terminal_status(term, TaskStatus::Running, false)
        .unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Done),
        "轮已结束时后台 Running 不该把未读 Done 冲成 Idle"
    );

    // 用户切回看 → Idle;再切出 → 已看过,保持 Idle。
    tasks.set_active_main(agent_task).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Idle)
    );
    tasks.set_active_main(other).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Idle),
        "看过后再切出保持 Idle"
    );
}

// 用户把 VibeTerm 切到后台(窗口失焦)时,当前选中 task 完成应显 Done(未看)并计入 Dock 角标 ——
// 不能因"它是 active_main"就当作"正盯着"。回到窗口标已读。(用户报:失焦完成只通知,圆点不变。)
#[test]
fn active_task_done_when_window_unfocused() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let agent = tasks.create("agent".into(), None, None).unwrap();
    tasks.set_active_main(agent).unwrap(); // agent 就是当前选中 task

    // 窗口有焦点(默认)+ 当前 task 答完 → Idle(正盯着,不打扰)。
    tasks.set_agent_turn_done(agent, 1, true, None).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent).unwrap(),
        Some(TaskStatus::Idle),
        "聚焦时当前 task 完成 → Idle"
    );

    // 切到别的 app(窗口失焦)→ 当前 task 不再算"盯着" → Done(未看)+ 计角标。
    assert!(
        tasks.set_window_focused(false).unwrap(),
        "焦点变化应返回 true"
    );
    assert_eq!(
        tasks.aggregated_status_of(agent).unwrap(),
        Some(TaskStatus::Done),
        "失焦时当前 task 完成 → Done(未看)"
    );
    assert_eq!(tasks.unseen_done_count(), 1, "失焦完成计入 Dock 角标");

    // 回到窗口(聚焦)→ 标已读 → Idle + 角标清。
    assert!(tasks.set_window_focused(true).unwrap());
    assert_eq!(
        tasks.aggregated_status_of(agent).unwrap(),
        Some(TaskStatus::Idle),
        "回到窗口 → 已读 Idle"
    );
    assert_eq!(tasks.unseen_done_count(), 0);

    // 再失焦 → 已看过,不再翻 Done。
    tasks.set_window_focused(false).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent).unwrap(),
        Some(TaskStatus::Idle),
        "看过后再失焦保持 Idle"
    );
}

// 根因回归(用户报:claude 失焦后"第二次起"完成漏判,只剩 claude 自己 hook 的无声通知)。
// 快轮(纯文本秒回、无工具调用)时 3s 轮询常采不到中间 working 那一拍,agent_turn_done 一直停在
// Some(true)。完成判定若只靠布尔跃迁(was != Some(true))就会漏。改用 turn_id 去重:end_turn 的
// uuid 变了 = 新一轮答完,即使没采到 working 也判出;同 uuid 不重复。
#[test]
fn agent_completion_detected_by_new_turn_id_without_working_sample() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let agent_task = tasks.create("claude".into(), None, None).unwrap();
    let other = tasks.create("other".into(), None, None).unwrap();
    tasks.set_active_main(other).unwrap(); // agent_task 非当前

    // 第一轮答完(uuid=A)→ 新 turn → 判完成 → 非当前未看 → Done。
    let (_c, jc1) = tasks
        .set_agent_turn_done(agent_task, 1, true, Some("A"))
        .unwrap();
    assert!(jc1, "首轮答完应判完成");
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Done)
    );

    // 同一轮被轮询反复读到(uuid 仍 A)→ 去重,不重复判完成。
    let (_c, jc_dup) = tasks
        .set_agent_turn_done(agent_task, 1, true, Some("A"))
        .unwrap();
    assert!(!jc_dup, "同 turn_id 不应重复判完成");

    // 用户切回看过(seen=true)再切出 → Idle。
    tasks.set_active_main(agent_task).unwrap();
    tasks.set_active_main(other).unwrap();
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Idle)
    );

    // 关键:第二轮是"快轮",轮询没采到 working(始终读到 done),agent_turn_done 一直停在
    // Some(true) —— 布尔跃迁判定在此会漏(正是用户报的 bug)。但 end_turn 的 uuid 变成 B,
    // turn_id 去重必须判出新完成 → 再次 Done(未看)。
    let (changed, jc2) = tasks
        .set_agent_turn_done(agent_task, 1, true, Some("B"))
        .unwrap();
    assert!(
        jc2,
        "新 turn_id(uuid 变化)即使没采到 working、turn_done 布尔没变也应判完成"
    );
    assert!(
        !changed,
        "turn_done 布尔未变(Some(true)→Some(true))→ changed=false"
    );
    assert_eq!(
        tasks.aggregated_status_of(agent_task).unwrap(),
        Some(TaskStatus::Done),
        "第二轮(快轮)完成也应 → Done(未看)"
    );
}

// 用户实测根因(分屏多 agent):一个 task 开 2 个 agent,旧逻辑 agent_kind/agent_turn_done 是
// task 级单字段、完成检测只认第一个 agent → 后开的 agent 答完检测不到(不通知 + 答完误判 WaitingInput
// 黄圈)。per-terminal 后各终端独立:A 答完 + B 在跑 → 整 task Running;B 答完独立判 just_completed。
#[test]
fn multi_agent_per_terminal_completion_independent() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let task = tasks.create("multi".into(), None, None).unwrap();
    let other = tasks.create("other".into(), None, None).unwrap();
    tasks.set_active_main(other).unwrap(); // task 非当前
    let (term_a, term_b) = (1, 2);
    tasks.attach_terminal(task, term_a).unwrap();
    tasks.attach_terminal(task, term_b).unwrap();

    // A 答完(claude),B 仍在跑 —— 整 task 应是 Running(B 还没完,不能显完成)。
    tasks
        .set_agent_turn_done(task, term_a, true, Some("a1"))
        .unwrap();
    tasks
        .set_agent_turn_done(task, term_b, false, None)
        .unwrap();
    assert_eq!(
        tasks.aggregated_status_of(task).unwrap(),
        Some(TaskStatus::Running),
        "一个 agent 答完但另一个在跑 → 整 task Running"
    );

    // 关键回归:B 答完必须独立判 just_completed —— 旧 task 级单字段会被 A 的 Some(true) 占据,
    // B 答完 was==Some(true) → just_completed=false 漏掉(正是用户报的 bug)。per-terminal 不会。
    let (_c, jc_b) = tasks
        .set_agent_turn_done(task, term_b, true, Some("b1"))
        .unwrap();
    assert!(jc_b, "第二个 agent 答完应独立判完成,不被第一个吞掉");
    assert_eq!(
        tasks.aggregated_status_of(task).unwrap(),
        Some(TaskStatus::Done),
        "两个 agent 都答完(未看)非当前 → Done"
    );

    // B 再开一轮又答完(新 turn_id)→ 仍独立判完成(per-terminal turn_id 去重,不与 A 串)。
    tasks.set_active_main(task).unwrap(); // 看过
    tasks.set_active_main(other).unwrap();
    let (_c, jc_b2) = tasks
        .set_agent_turn_done(task, term_b, true, Some("b2"))
        .unwrap();
    assert!(jc_b2, "B 第二轮(新 turn_id)答完仍独立判完成");
}

#[test]
fn unseen_done_count_tracks_done_and_clears_on_view() {
    // Dock 角标的数据源:聚合 Done(完成但未看)的任务数;用户切到该任务(set_active_main)即减。
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let _t1 = tasks.create("a".into(), None, None).unwrap(); // 默认 active_main、无终端 → Idle
    let t2 = tasks.create("b".into(), None, None).unwrap();
    let t3 = tasks.create("c".into(), None, None).unwrap();
    let (term2, term3) = (12, 13);
    tasks.attach_terminal(t2, term2).unwrap();
    tasks.attach_terminal(t3, term3).unwrap();
    assert_eq!(tasks.unseen_done_count(), 0);

    // t2(非 active)Running → Idle(by_osc 真完成)→ Done
    tasks
        .update_terminal_status(term2, TaskStatus::Running, false)
        .unwrap();
    tasks
        .update_terminal_status(term2, TaskStatus::Idle, true)
        .unwrap();
    assert_eq!(
        tasks.unseen_done_count(),
        1,
        "t2 真完成且非 active → 计未看"
    );

    // t3 同样完成 → 2
    tasks
        .update_terminal_status(term3, TaskStatus::Running, false)
        .unwrap();
    tasks
        .update_terminal_status(term3, TaskStatus::Idle, true)
        .unwrap();
    assert_eq!(tasks.unseen_done_count(), 2);

    // 用户切到 t2 → set_active_main 翻 seen=true → t2 不再 Done
    tasks.set_active_main(t2).unwrap();
    assert_eq!(tasks.unseen_done_count(), 1, "看过 t2 → 只剩 t3 未看");
}

#[test]
fn task_status_updates_propagate_to_dto() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let task_id = tasks.create("statusy".into(), None, None).unwrap();

    // 模拟 status 嗅探层反馈 — 1 个虚构 terminal_id,先 attach,再更新它的 status
    let fake_term = 42;
    tasks.attach_terminal(task_id, fake_term).unwrap();
    tasks
        .update_terminal_status(fake_term, TaskStatus::WaitingInput, true)
        .unwrap();
    let dto = tasks.task_dto(task_id).unwrap().unwrap();
    assert_eq!(
        dto.status,
        TaskStatus::WaitingInput,
        "任务 status 应聚合 terminal 状态"
    );

    tasks
        .update_terminal_status(fake_term, TaskStatus::Idle, true)
        .unwrap();
    let dto = tasks.task_dto(task_id).unwrap().unwrap();
    assert_eq!(dto.status, TaskStatus::Idle);
}

#[test]
fn running_to_idle_without_osc_does_not_mark_done() {
    // ping/tail -f 这种长 streaming: feed -> Running, timeout -> Idle (by_osc=false).
    // 即使该 task 不是 active_main, 也不该升为 Done (黄环+删除线).
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let task_a = tasks.create("a".into(), None, None).unwrap();
    let task_b = tasks.create("b".into(), None, None).unwrap();
    // task_a 是默认 active_main; 把 active 切到 task_b, 使 task_a 进入"后台 task"语境
    tasks.set_active_main(task_b).unwrap();
    let fake_term = 7;
    tasks.attach_terminal(task_a, fake_term).unwrap();
    tasks
        .update_terminal_status(fake_term, TaskStatus::Running, false)
        .unwrap();
    // timeout 触发 Idle, by_osc=false
    tasks
        .update_terminal_status(fake_term, TaskStatus::Idle, false)
        .unwrap();
    let dto = tasks.task_dto(task_a).unwrap().unwrap();
    assert_eq!(
        dto.status,
        TaskStatus::Idle,
        "timeout-driven Idle 不应升为 Done"
    );
}

#[test]
fn running_to_idle_by_osc_marks_done_for_background_task() {
    // OSC 133/633 D 真完成 + 非 active_main → seen=false → 聚合 Done.
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let task_a = tasks.create("a".into(), None, None).unwrap();
    let task_b = tasks.create("b".into(), None, None).unwrap();
    tasks.set_active_main(task_b).unwrap();
    let fake_term = 9;
    tasks.attach_terminal(task_a, fake_term).unwrap();
    tasks
        .update_terminal_status(fake_term, TaskStatus::Running, false)
        .unwrap();
    tasks
        .update_terminal_status(fake_term, TaskStatus::Idle, true)
        .unwrap();
    let dto = tasks.task_dto(task_a).unwrap().unwrap();
    assert_eq!(
        dto.status,
        TaskStatus::Done,
        "OSC D 触发的 Idle 在后台 task 上应升为 Done"
    );
}

#[test]
fn set_location_main_then_floating() {
    let _cfg = isolated_config();
    let tasks = TaskRegistry::new();
    let task_id = tasks.create("t".into(), None, None).unwrap();
    // 新任务默认 MainWorkspace
    assert_eq!(
        tasks.task_dto(task_id).unwrap().unwrap().location,
        TaskLocation::MainWorkspace
    );

    // 推到浮窗
    tasks
        .set_location(task_id, TaskLocation::Floating("float-1".into()))
        .unwrap();
    assert_eq!(
        tasks.task_dto(task_id).unwrap().unwrap().location,
        TaskLocation::Floating("float-1".into())
    );

    // 拽回 nowhere(关浮窗时)
    tasks.set_location(task_id, TaskLocation::Nowhere).unwrap();
    assert_eq!(
        tasks.task_dto(task_id).unwrap().unwrap().location,
        TaskLocation::Nowhere
    );
}

#[cfg(unix)]
#[test]
fn multi_sink_attach_via_registry() {
    use pty_helpers::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use vibeterm_core::TerminalRegistry;
    use vibeterm_pty::sinks::MpscSink;

    let terminals = TerminalRegistry::new();
    let (tx1, rx1) = mpsc::channel();
    let script = "echo SHARED_HISTORY; sleep 1";
    let term_id = terminals.spawn(opts(script), MpscSink::new(tx1)).unwrap();
    drain_until(&rx1, "SHARED_HISTORY", Duration::from_secs(2));

    let (tx2, rx2) = mpsc::channel();
    let _sink_id = terminals.attach_sink(term_id, MpscSink::new(tx2)).unwrap();

    let replayed = drain_until(&rx2, "SHARED_HISTORY", Duration::from_millis(500));
    assert!(
        replayed.contains("SHARED_HISTORY"),
        "TerminalRegistry::attach_sink 应触发 scrollback 回放;实际 = {:?}",
        replayed
    );
}
