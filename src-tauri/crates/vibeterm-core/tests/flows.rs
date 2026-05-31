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
