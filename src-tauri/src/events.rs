//! G7 事件流:task 状态变更 append-only JSONL(带单调 seq)。
//! 🟢 零侵入:只 append 到 VibeTerm 自己的 config 目录。从 main.rs 拆出(行为不变)。

use vibeterm_ipc::{TaskId, TerminalId};

use crate::atomic_write;

// ============================================================
// G7: 事件流 —— task 状态变更 append-only JSONL + 内存游标
// ============================================================
// 🟢 零侵入:只 append 到 VibeTerm 自己的 config 目录(events.jsonl),外部脚本可 `tail -f` 订阅。
// 启动截尾时保留的行数。
pub(crate) const EVENT_TAIL_LINES: usize = 512;
pub(crate) const EVENT_FILE_MAX_BYTES: u64 = 2_000_000;

#[derive(serde::Serialize)]
pub(crate) struct VtEvent {
    seq: u64,
    ts_ms: u64,
    /// "status_changed" | "agent_completed"
    kind: String,
    task_id: TaskId,
    terminal_id: Option<TerminalId>,
    status: Option<serde_json::Value>,
}

pub(crate) struct EventLog {
    seq: std::sync::atomic::AtomicU64,
    file: std::sync::Mutex<Option<std::fs::File>>,
}

pub(crate) static EVENT_LOG: std::sync::OnceLock<EventLog> = std::sync::OnceLock::new();

impl EventLog {
    pub(crate) fn global() -> &'static EventLog {
        EVENT_LOG.get_or_init(EventLog::new)
    }

    fn new() -> Self {
        let file = vibeterm_config::events_jsonl_path().ok().and_then(|path| {
            // 启动时若文件过大 → 截尾保留最后 EVENT_TAIL_LINES 行,防无限增长.
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > EVENT_FILE_MAX_BYTES {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut tail: Vec<&str> =
                            content.lines().rev().take(EVENT_TAIL_LINES).collect();
                        tail.reverse();
                        // 原子写(同目录临时文件 + rename),防崩溃中断留下 0 字节文件
                        let _ = atomic_write(&path, format!("{}\n", tail.join("\n")).as_bytes());
                    }
                }
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
        });
        EventLog {
            seq: std::sync::atomic::AtomicU64::new(0),
            file: std::sync::Mutex::new(file),
        }
    }

    fn record(
        &self,
        kind: &str,
        task_id: TaskId,
        terminal_id: Option<TerminalId>,
        status: Option<serde_json::Value>,
    ) {
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        // seq 分配与写入必须在同一把锁内,否则两个并发写入方(PTY 读线程 / 200ms tick)
        // 可能让小 seq 后落盘,文件里 seq 不再单调。
        let Ok(mut g) = self.file.lock() else { return };
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let ev = VtEvent {
            seq,
            ts_ms,
            kind: kind.to_string(),
            task_id,
            terminal_id,
            status,
        };
        if let Some(f) = g.as_mut() {
            use std::io::Write as _;
            if let Ok(line) = serde_json::to_string(&ev) {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// 记录一条事件(供状态变更 / agent 完成 emit 点调用)。best-effort,失败不影响主流程。
pub(crate) fn record_event(
    kind: &str,
    task_id: TaskId,
    terminal_id: Option<TerminalId>,
    status: Option<serde_json::Value>,
) {
    EventLog::global().record(kind, task_id, terminal_id, status);
}
