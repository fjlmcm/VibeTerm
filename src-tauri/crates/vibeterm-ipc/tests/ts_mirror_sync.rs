//! Rust IPC schema 与 TS 手工镜像(web/packages/ipc-types)的同步防漂移测试。
//!
//! 镜像靠人肉对齐,后端加枚举变体忘改 TS 时编译期零报错、线上才发现。
//! 此测试把"漂移"提前到 CI:断言关键枚举的序列化字面量都出现在 TS 源文件里。
//! (根治是 specta 自动生成;在那之前先用快照比对兜底。)

use std::path::PathBuf;

fn ts_mirror_source() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../web/packages/ipc-types/src/generated.ts");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读 TS 镜像失败 {}: {e}", p.display()))
}

/// TaskStatus 全部变体(snake_case 序列化字面量)必须出现在 TS 镜像里。
#[test]
fn task_status_variants_present_in_ts_mirror() {
    let ts = ts_mirror_source();
    for v in [
        vibeterm_ipc::TaskStatus::Idle,
        vibeterm_ipc::TaskStatus::Running,
        vibeterm_ipc::TaskStatus::WaitingInput,
        vibeterm_ipc::TaskStatus::Done,
        vibeterm_ipc::TaskStatus::Stalled,
    ] {
        let lit = serde_json::to_string(&v).unwrap(); // 含引号,如 "waiting_input"
        assert!(
            ts.contains(&lit),
            "TaskStatus 变体 {lit} 不在 ipc-types/index.ts —— Rust/TS 镜像漂移"
        );
    }
}

/// IpcError 的 kind 标签必须出现在 TS 镜像里(serde tag = "kind")。
#[test]
fn ipc_error_kinds_present_in_ts_mirror() {
    let ts = ts_mirror_source();
    for kind in [
        "NotFound",
        "PermissionDenied",
        "PtySpawnFailed",
        "ConfigInvalid",
        "Unknown",
    ] {
        assert!(
            ts.contains(&format!("\"{kind}\"")),
            "IpcError kind {kind} 不在 ipc-types/index.ts —— Rust/TS 镜像漂移"
        );
    }
}
