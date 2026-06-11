//! G7 事件流:task 状态变更 append-only JSONL + 内存 ring 游标。
//! 🟢 零侵入:只 append 到 VibeTerm 自己的 config 目录。从 main.rs 拆出(行为不变)。

use vibeterm_ipc::{IpcResult, TaskId, TerminalId};

use crate::atomic_write;

// ============================================================
// G7: 事件流 —— task 状态变更 append-only JSONL + 内存游标
// ============================================================
// 🟢 零侵入:只 append 到 VibeTerm 自己的 config 目录(events.jsonl),外部脚本可 `tail -f` 订阅;
// 内存 ring 保最近 EVENT_RING_CAP 条带单调 seq,IPC `read_events(after_seq)` 支持断线游标续传。
pub(crate) const EVENT_RING_CAP: usize = 512;
pub(crate) const EVENT_FILE_MAX_BYTES: u64 = 2_000_000;

#[derive(Clone, serde::Serialize, specta::Type)]
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
    ring: std::sync::Mutex<std::collections::VecDeque<VtEvent>>,
    file: std::sync::Mutex<Option<std::fs::File>>,
}

pub(crate) static EVENT_LOG: std::sync::OnceLock<EventLog> = std::sync::OnceLock::new();

impl EventLog {
    pub(crate) fn global() -> &'static EventLog {
        EVENT_LOG.get_or_init(EventLog::new)
    }

    fn new() -> Self {
        let file = vibeterm_config::events_jsonl_path().ok().and_then(|path| {
            // 启动时若文件过大 → 截尾保留最后 EVENT_RING_CAP 行,防无限增长.
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > EVENT_FILE_MAX_BYTES {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut tail: Vec<&str> =
                            content.lines().rev().take(EVENT_RING_CAP).collect();
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
            ring: std::sync::Mutex::new(std::collections::VecDeque::with_capacity(EVENT_RING_CAP)),
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
        // seq 分配与 ring push 必须在同一把锁内:若先取 seq 再独立入 ring,两个并发写入方
        // (PTY 读线程 / 200ms tick)可能让小 seq 后入 ring —— 游标消费者(read_after)在
        // 读到大 seq 后推进游标,迟到的小 seq 事件从此永不可达。
        let ev = {
            let Ok(mut r) = self.ring.lock() else { return };
            let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let ev = VtEvent {
                seq,
                ts_ms,
                kind: kind.to_string(),
                task_id,
                terminal_id,
                status,
            };
            if r.len() >= EVENT_RING_CAP {
                r.pop_front();
            }
            r.push_back(ev.clone());
            ev
        };
        if let Ok(mut g) = self.file.lock() {
            if let Some(f) = g.as_mut() {
                use std::io::Write as _;
                if let Ok(line) = serde_json::to_string(&ev) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
    }

    fn read_after(&self, after: u64) -> Vec<VtEvent> {
        self.ring
            .lock()
            .map(|r| r.iter().filter(|e| e.seq > after).cloned().collect())
            .unwrap_or_default()
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

/// 读取 seq > after_seq 的事件(断线游标续传)。after_seq 省略 = 从头(内存 ring 内)。
#[tauri::command]
pub(crate) async fn read_events(after_seq: Option<u64>) -> IpcResult<Vec<VtEvent>> {
    Ok(EventLog::global().read_after(after_seq.unwrap_or(0)))
}
