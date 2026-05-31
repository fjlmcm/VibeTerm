//! Status 端到端集成测试
//!
//! 覆盖完整生命周期:
//!   - OSC 133 全周期(prompt → command → finished)
//!   - claude agent rule 与 OSC 联合
//!   - 多 chunk 跨越 OSC 序列边界(ring buffer 正确拼接)
//!   - idle timeout 自动转 idle

use std::thread;
use std::time::Duration;

use vibeterm_ipc::TaskStatus;
use vibeterm_status::StatusDetector;

#[test]
fn osc_133_full_cycle() {
    let mut d = StatusDetector::new("zsh");

    // 先制造 Running(detector 初始即 Idle, 否则观察不到 A 的转换)
    let _ = d.feed(b"output");
    assert_eq!(d.current(), TaskStatus::Running);

    // Prompt start (A) → idle-ready: 在 prompt 等输入 = 就绪空闲, 不是"agent 在问你"。
    // (普通 shell 坐在 prompt 上必须是 Idle/灰, 不能恒 WaitingInput/黄)
    let r = d.feed(b"\x1b]133;A\x1b\\");
    assert_eq!(
        r,
        Some(TaskStatus::Idle),
        "OSC 133;A (prompt 就绪) 应判 Idle, 不是 WaitingInput"
    );
    assert_eq!(d.current(), TaskStatus::Idle);

    // Command executed (C) → running
    let r = d.feed(b"\x1b]133;C\x1b\\");
    assert_eq!(r, Some(TaskStatus::Running));

    // Command finished (D) → idle
    let r = d.feed(b"\x1b]133;D;0\x1b\\");
    assert_eq!(r, Some(TaskStatus::Idle));
}

#[test]
fn osc_split_across_chunks_still_parses() {
    let mut d = StatusDetector::new("zsh");
    // OSC 序列被拆成两块发(模拟 PTY 真实碎片化)
    d.feed(b"hello\x1b]133;");
    let r = d.feed(b"D\x1b\\");
    assert_eq!(
        d.current(),
        TaskStatus::Idle,
        "跨 chunk 的 OSC 序列应被 ring buffer 正确重组"
    );
    let _ = r;
}

#[test]
fn claude_permission_menu_triggers_waiting_input() {
    let mut d = StatusDetector::new("claude");
    let _ = d.feed(b"thinking ...\n");
    assert_eq!(d.current(), TaskStatus::Running);
    // 校准后:认菜单独有措辞(选项 3), 不认泛化 (y/n)
    let _ = d.feed(b"3. No, and tell Claude what to do differently");
    assert_eq!(d.current(), TaskStatus::WaitingInput);
}

#[test]
fn osc_overrides_stdout_pattern() {
    // 即使 claude 的菜单 pattern 命中 → WaitingInput,后续 OSC 133;C(run)
    // 应该 override 回 Running
    let mut d = StatusDetector::new("claude");
    let _ = d.feed(b"No, and tell Claude what to do differently");
    assert_eq!(d.current(), TaskStatus::WaitingInput);
    let _ = d.feed(b"\x1b]133;C\x1b\\");
    assert_eq!(
        d.current(),
        TaskStatus::Running,
        "OSC 信号应该 override stdout pattern"
    );
}

#[test]
fn idle_after_no_output() {
    // 800ms 内无输出 → tick 应转 idle
    let mut d = StatusDetector::new("zsh");
    let _ = d.feed(b"some output");
    assert_eq!(d.current(), TaskStatus::Running);
    thread::sleep(Duration::from_millis(850));
    let r = d.tick();
    assert_eq!(r, Some(TaskStatus::Idle), "无输出 800ms 后应转 idle");
}

#[test]
fn ansi_stripped_before_pattern_match() {
    // 即使带 ANSI 颜色码,claude 菜单 pattern 仍应命中
    let mut d = StatusDetector::new("claude");
    let _ = d.feed(b"\x1b[1;31mNo, and tell Claude\x1b[0m what to do differently");
    assert_eq!(
        d.current(),
        TaskStatus::WaitingInput,
        "ANSI 应被 strip 后再做 pattern 匹配"
    );
}
