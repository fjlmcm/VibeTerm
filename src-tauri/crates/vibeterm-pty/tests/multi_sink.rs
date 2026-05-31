//! 多 sink 广播 + scrollback 回放
//!
//! 验证:
//!   1. add_sink 后,新 chunk 同时到达所有 sink
//!   2. add_sink 时,scrollback 历史先回放给新 sink
//!   3. remove_sink 后,该 sink 不再收到新 chunk(但已收到的不丢)
//!
//! 这是浮窗 reparent 时序的真后端单测,替代 tauri-driver E2E。
//!
//! 注意:这些 test 用了 `echo X; echo Y; sleep N` 等 Unix shell 脚本,
//! 暂只在 Unix 跑(`#[cfg(unix)]`)。Windows 验证靠 tauri-cdp E2E。

#![cfg(unix)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use vibeterm_pty::sinks::MpscSink;
use vibeterm_pty::{SpawnOpts, Terminal};

fn opts(script: &str) -> SpawnOpts {
    SpawnOpts {
        rows: 24,
        cols: 80,
        cwd: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        command: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        env: vec![("LANG".into(), "C".into()), ("TERM".into(), "dumb".into())],
    }
}

fn drain_until(rx: &mpsc::Receiver<Vec<u8>>, needle: &str, timeout: Duration) -> String {
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

#[test]
fn add_sink_replays_scrollback_to_new_subscriber() {
    let (tx_initial, rx_initial) = mpsc::channel();
    let terminal = Terminal::spawn(
        opts("echo LINE_A; echo LINE_B; echo LINE_C; sleep 2"),
        MpscSink::new(tx_initial),
    )
    .expect("spawn");

    let initial_seen = drain_until(&rx_initial, "LINE_C", Duration::from_secs(3));
    assert!(
        initial_seen.contains("LINE_A"),
        "initial sink 未收到 LINE_A"
    );

    let (tx_new, rx_new) = mpsc::channel();
    let _new_sink_id = terminal.add_sink(MpscSink::new(tx_new));

    let replayed = drain_until(&rx_new, "LINE_C", Duration::from_millis(500));
    assert!(
        replayed.contains("LINE_A") && replayed.contains("LINE_B") && replayed.contains("LINE_C"),
        "新 sink 未收到完整 scrollback 回放;实际收到 = {:?}",
        replayed
    );
}

#[test]
fn new_chunks_fan_out_to_all_sinks() {
    let (tx1, rx1) = mpsc::channel();
    let terminal = Terminal::spawn(
        opts("read line; echo got=$line; sleep 1"),
        MpscSink::new(tx1),
    )
    .expect("spawn");

    let (tx2, rx2) = mpsc::channel();
    terminal.add_sink(MpscSink::new(tx2));

    terminal.write(b"BROADCAST\n").expect("write");

    let s1 = drain_until(&rx1, "got=BROADCAST", Duration::from_secs(3));
    let s2 = drain_until(&rx2, "got=BROADCAST", Duration::from_secs(3));
    assert!(s1.contains("got=BROADCAST"), "sink1 没收到 BROADCAST 回显");
    assert!(s2.contains("got=BROADCAST"), "sink2 没收到 BROADCAST 回显");
}

#[test]
fn scrollback_snapshot_returns_recent_output_without_subscribing() {
    let (tx, rx) = mpsc::channel();
    let terminal = Terminal::spawn(opts("echo SNAP_A; echo SNAP_B; sleep 2"), MpscSink::new(tx))
        .expect("spawn");

    // 等首个 sink 收到完整输出,确认 scrollback 已被读线程追加
    let seen = drain_until(&rx, "SNAP_B", Duration::from_secs(3));
    assert!(seen.contains("SNAP_A") && seen.contains("SNAP_B"));

    // snapshot 不订阅,直接返回 ring buffer 内容
    let snap = terminal.scrollback_snapshot();
    let snap_str = String::from_utf8_lossy(&snap);
    assert!(
        snap_str.contains("SNAP_A") && snap_str.contains("SNAP_B"),
        "scrollback snapshot 缺内容 = {:?}",
        snap_str
    );
    assert_eq!(terminal.scrollback_len(), snap.len());
}

#[test]
fn remove_sink_stops_receiving_new_chunks() {
    let (tx1, rx1) = mpsc::channel();
    let terminal = Terminal::spawn(
        opts("sleep 0.1; echo BEFORE_DETACH; sleep 0.5; echo AFTER_DETACH; sleep 1"),
        MpscSink::new(tx1),
    )
    .expect("spawn");

    let (tx2, rx2) = mpsc::channel();
    let sink2_id = terminal.add_sink(MpscSink::new(tx2));

    drain_until(&rx1, "BEFORE_DETACH", Duration::from_secs(2));
    drain_until(&rx2, "BEFORE_DETACH", Duration::from_secs(2));

    terminal.remove_sink(sink2_id);

    let s1 = drain_until(&rx1, "AFTER_DETACH", Duration::from_secs(3));
    assert!(s1.contains("AFTER_DETACH"), "sink1 应继续收到");

    let s2 = drain_until(&rx2, "AFTER_DETACH", Duration::from_millis(800));
    assert!(
        !s2.contains("AFTER_DETACH"),
        "sink2 detach 后仍收到新 chunk — 多 sink 隔离失败"
    );
}
