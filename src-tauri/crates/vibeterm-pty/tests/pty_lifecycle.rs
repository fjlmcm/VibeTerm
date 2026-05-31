//! PTY 端到端集成测试
//!
//! 真启 shell,echo 一段字符串,验证 chunk 通过 ChunkSink 回流。
//! 验证 Drop 时进程被强 kill,无 zombie。
//!
//! 仅在 Unix(/bin/sh)跑;Windows 上 portable-pty + ConPTY 的 Drop
//! 行为不稳(测试卡死),Windows 验证靠 tauri-cdp 真 E2E。

#![cfg(unix)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use vibeterm_pty::sinks::MpscSink;
use vibeterm_pty::{SpawnOpts, Terminal};

fn shell(script: &str) -> (String, Vec<String>) {
    ("/bin/sh".into(), vec!["-c".into(), script.into()])
}

fn default_opts(cmd: &str, args: Vec<String>) -> SpawnOpts {
    SpawnOpts {
        rows: 24,
        cols: 80,
        cwd: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        command: cmd.into(),
        args,
        env: vec![("LANG".into(), "C".into()), ("TERM".into(), "dumb".into())],
    }
}

/// 等指定字符串出现在累积 buffer 中,或超时
fn wait_for(rx: &mpsc::Receiver<Vec<u8>>, needle: &str, timeout: Duration) -> Option<String> {
    let mut acc = Vec::new();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(chunk) => {
                acc.extend_from_slice(&chunk);
                if let Ok(s) = std::str::from_utf8(&acc) {
                    if s.contains(needle) {
                        return Some(s.to_string());
                    }
                }
            }
            Err(_) => break,
        }
    }
    None
}

#[test]
fn pty_spawn_echo_returns_chunks() {
    let (tx, rx) = mpsc::channel();
    let sink = MpscSink::new(tx);
    let (cmd, args) = shell("echo VIBETERM_HELLO");
    let _terminal = Terminal::spawn(default_opts(&cmd, args), sink).expect("spawn shell");

    let output = wait_for(&rx, "VIBETERM_HELLO", Duration::from_secs(5));
    assert!(output.is_some(), "未收到预期 echo 输出");
}

#[test]
fn pty_write_drives_stdin() {
    let (tx, rx) = mpsc::channel();
    // 跨平台:Unix `read line; echo got=$line`,Windows `set /p line=&& echo got=%line%`
    let (cmd, args) = shell("read line; echo got=$line");
    let terminal =
        Terminal::spawn(default_opts(&cmd, args), MpscSink::new(tx)).expect("spawn shell");

    terminal.write(b"REPLY\n").expect("write to pty");
    let output = wait_for(&rx, "got=REPLY", Duration::from_secs(5));
    assert!(output.is_some(), "未读到 read+echo 回显");
}

#[test]
fn pty_drop_kills_child() {
    let (tx, rx) = mpsc::channel();
    let (cmd, args) = shell("sleep 60");
    let terminal =
        Terminal::spawn(default_opts(&cmd, args), MpscSink::new(tx)).expect("spawn long-running");

    drop(terminal);

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        }
    }
    panic!("PTY Drop 后 5 秒内 sink channel 未关闭");
}

#[test]
fn pty_resize_does_not_panic() {
    let (tx, _rx) = mpsc::channel();
    let (cmd, args) = shell("sleep 5");
    let terminal = Terminal::spawn(default_opts(&cmd, args), MpscSink::new(tx)).expect("spawn");
    terminal.resize(40, 120).expect("resize ok");
    terminal.resize(24, 80).expect("resize back ok");
}
