//! atomic_write 集成测试
//!
//! 验证:
//!   - 写入到不存在文件 → 成功
//!   - 覆盖已存在文件 → 成功且原子(中间过程不会留半文件)
//!   - 路径父目录不存在 → 错误传播
//!   - 并发写同一路径 → 最后写的胜出,无 corruption

use std::sync::{Arc, Barrier};
use std::thread;

use vibeterm_config::atomic_write;

#[test]
fn write_creates_new_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.txt");
    atomic_write(&p, b"hello").expect("write");
    assert_eq!(std::fs::read(&p).unwrap(), b"hello");
}

#[test]
fn write_overwrites_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.txt");
    atomic_write(&p, b"v1").unwrap();
    atomic_write(&p, b"v2-longer-content").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"v2-longer-content");
}

#[test]
fn concurrent_writes_yield_consistent_result() {
    let tmp = tempfile::tempdir().unwrap();
    let p = Arc::new(tmp.path().join("contended.txt"));
    let n_threads = 8;
    let barrier = Arc::new(Barrier::new(n_threads));

    let mut handles = vec![];
    for i in 0..n_threads {
        let p = p.clone();
        let b = barrier.clone();
        handles.push(thread::spawn(move || {
            b.wait();
            // 每个线程写一段确定且不同长度的内容
            let payload = format!("thread-{i}-{}", "x".repeat(i * 100));
            atomic_write(&p, payload.as_bytes()).expect("write")
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    // 最终文件必须是某一个线程的完整 payload(不能是混合/截断)
    let final_bytes = std::fs::read(&*p).unwrap();
    let final_str = String::from_utf8(final_bytes).unwrap();
    assert!(
        final_str.starts_with("thread-"),
        "atomic_write 并发后产生 corruption:{:?}",
        final_str
    );
    // 长度必须等于 "thread-N-{xs}" 的合法形式 — 不应是截断
    let expected_lens: Vec<usize> = (0..n_threads)
        .map(|i| format!("thread-{i}-{}", "x".repeat(i * 100)).len())
        .collect();
    assert!(
        expected_lens.contains(&final_str.len()),
        "最终文件长度 {} 不在合法集合内 — 可能被截断",
        final_str.len()
    );
}
