//! Tasks 持久化端到端
//!
//! 通过 VIBETERM_CONFIG_DIR env override 重定向到 tmpdir,
//! 验证 load / save 真写入磁盘、原子可重读、schema_version 兼容。

use std::sync::Mutex;

use vibeterm_ipc::SplitNode;
use vibeterm_tasks::{load, save, TaskSnapshot, TasksError, TasksFile};

fn default_tree() -> SplitNode {
    SplitNode::Leaf { slot_id: 0 }
}

/// 所有 tests 串行(env 是进程级共享状态)
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_isolated_config<F>(test: F)
where
    F: FnOnce(),
{
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().expect("tmpdir");
    // SAFETY: tests serialized via ENV_LOCK
    unsafe {
        std::env::set_var("VIBETERM_CONFIG_DIR", tmp.path());
    }
    test();
    unsafe {
        std::env::remove_var("VIBETERM_CONFIG_DIR");
    }
}

#[test]
fn save_then_load_roundtrips() {
    with_isolated_config(|| {
        let file = TasksFile {
            next_task_id: 42,
            tasks: vec![TaskSnapshot {
                id: 1,
                name: "test-task".into(),
                cwd: Some("/tmp".into()),
                last_terminal_ids: vec![10, 11],
                split_tree: default_tree(),
                worktree: None,
                notify_muted: false,
            }],
            order: vec![1],
            ..Default::default()
        };
        save(&file).expect("save");

        let loaded = load().expect("load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.next_task_id, 42);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].name, "test-task");
        assert_eq!(loaded.tasks[0].last_terminal_ids, vec![10, 11]);
        assert_eq!(loaded.order, vec![1]);
    });
}

#[test]
fn load_missing_file_returns_default() {
    with_isolated_config(|| {
        let loaded = load().expect("load empty");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.next_task_id, 0);
        assert!(loaded.tasks.is_empty());
        assert!(loaded.order.is_empty());
    });
}

#[test]
fn save_overwrites_atomically() {
    with_isolated_config(|| {
        // 写第一版
        let v1 = TasksFile {
            next_task_id: 1,
            tasks: vec![TaskSnapshot {
                id: 1,
                name: "v1".into(),
                cwd: None,
                last_terminal_ids: vec![],
                split_tree: default_tree(),
                worktree: None,
                notify_muted: false,
            }],
            ..Default::default()
        };
        save(&v1).expect("save v1");

        // 覆盖第二版
        let v2 = TasksFile {
            next_task_id: 99,
            tasks: vec![TaskSnapshot {
                id: 99,
                name: "v2-replaced".into(),
                cwd: None,
                last_terminal_ids: vec![],
                split_tree: default_tree(),
                worktree: None,
                notify_muted: false,
            }],
            ..Default::default()
        };
        save(&v2).expect("save v2");

        let loaded = load().expect("load");
        // 必须是 v2(原子替换,不会留 v1 数据混合)
        assert_eq!(loaded.next_task_id, 99);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].name, "v2-replaced");
    });
}

/// 读失败(权限 000)→ load 必须返回 Err 而非空启动;原文件不能被改名/覆盖。
#[cfg(unix)]
#[test]
fn unreadable_file_returns_err_and_is_left_intact() {
    use std::os::unix::fs::PermissionsExt;
    with_isolated_config(|| {
        let file = TasksFile {
            next_task_id: 7,
            tasks: vec![],
            order: vec![],
            ..Default::default()
        };
        save(&file).expect("save");
        let p = vibeterm_config::tasks_json_path().unwrap();
        let original = std::fs::read(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&p).is_ok() {
            // root 下 chmod 000 挡不住读,本测试无意义
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }
        let res = load();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(res, Err(TasksError::Io(_))), "got {res:?}");
        assert!(p.exists(), "原文件不得被改名");
        assert_eq!(std::fs::read(&p).unwrap(), original, "原文件不得被覆盖");
        let dir = p.parent().unwrap();
        let corrupt = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(!corrupt, "读失败不该产生 .corrupt 备份");
    });
}
