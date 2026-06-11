//! 终端注册表 — 管理活的 Terminal 实例的生命周期(Rust 是 truth)
//!
//!   - 单调递增 TerminalId
//!   - Mutex<HashMap> 包裹(并发 IPC 命令安全)
//!   - close 时 Terminal 自动走 Drop 路径

use std::collections::HashMap;
use std::sync::Mutex;

use vibeterm_ipc::TerminalId;
use vibeterm_pty::sinks::{TailSink, TailSinkHandle};
use vibeterm_pty::{ChunkSink, SinkId, SpawnOpts, Terminal};

/// 每行最多 80 字符;够给 UI 截断显示,超过部分以「…」结尾。
const TAIL_MAX_CHARS: usize = 80;

#[derive(thiserror::Error, Debug)]
pub enum TerminalRegistryError {
    #[error("terminal not found: {0}")]
    NotFound(TerminalId),
    #[error("spawn failed: {0}")]
    SpawnFailed(#[from] vibeterm_pty::PtyError),
    #[error("registry poisoned")]
    Poisoned,
}

pub struct TerminalRegistry {
    next_id: Mutex<TerminalId>,
    terminals: Mutex<HashMap<TerminalId, Terminal>>,
    /// 任务名下状态行:每个 terminal 一个 TailSinkHandle,
    /// spawn 时 attach 一个 TailSink,后续读末行非空可见文本用。
    tail_handles: Mutex<HashMap<TerminalId, TailSinkHandle>>,
}

impl Default for TerminalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRegistry {
    pub fn new() -> Self {
        Self {
            next_id: Mutex::new(0),
            terminals: Mutex::new(HashMap::new()),
            tail_handles: Mutex::new(HashMap::new()),
        }
    }

    /// Spawn 新 PTY,返回新分配的 TerminalId。chunk sink 由调用方提供。
    pub fn spawn<S: ChunkSink>(
        &self,
        opts: SpawnOpts,
        sink: S,
    ) -> Result<TerminalId, TerminalRegistryError> {
        let id = {
            let mut n = self
                .next_id
                .lock()
                .map_err(|_| TerminalRegistryError::Poisoned)?;
            let id = *n;
            *n = n.wrapping_add(1);
            id
        };
        let term = Terminal::spawn(opts, sink)?;
        // 额外挂一个 TailSink:跟踪末行可见文本,供 UI "任务名下状态行" 渲染。
        let (tail_sink, tail_handle) = TailSink::new(TAIL_MAX_CHARS);
        let _tail_sink_id = term.add_sink(tail_sink);
        self.terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?
            .insert(id, term);
        if let Ok(mut map) = self.tail_handles.lock() {
            map.insert(id, tail_handle);
        }
        tracing::info!(terminal_id = id, "pty spawned");
        Ok(id)
    }

    pub fn write(&self, id: TerminalId, data: &[u8]) -> Result<(), TerminalRegistryError> {
        let map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        let t = map.get(&id).ok_or(TerminalRegistryError::NotFound(id))?;
        t.write(data)?;
        Ok(())
    }

    /// 调整终端尺寸(幂等:同尺寸 no-op,见 `Terminal::resize`)。
    pub fn resize(
        &self,
        id: TerminalId,
        rows: u16,
        cols: u16,
    ) -> Result<(), TerminalRegistryError> {
        let map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        let t = map.get(&id).ok_or(TerminalRegistryError::NotFound(id))?;
        t.resize(rows, cols)?;
        Ok(())
    }

    pub fn close(&self, id: TerminalId) -> Result<(), TerminalRegistryError> {
        let mut map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        match map.remove(&id) {
            Some(_) => {
                if let Ok(mut t) = self.tail_handles.lock() {
                    t.remove(&id);
                }
                tracing::info!(terminal_id = id, "pty closed (drop)");
                Ok(())
            }
            None => Err(TerminalRegistryError::NotFound(id)),
        }
    }

    /// 取 terminal 当前末行(优先进行中的当前行;否则最近完成的非空行)。
    /// 没数据 / terminal 不存在 → None。
    pub fn tail_of(&self, id: TerminalId) -> Option<String> {
        let map = self.tail_handles.lock().ok()?;
        map.get(&id)?.snapshot()
    }

    /// 从给定终端集合中挑"最近活跃"的那个,返回它的末行。
    /// 分屏场景下 emit_tasks_changed 用这个挑该 task 下当前正在输出的那块屏的 tail,
    /// 避免只看 terminal_ids.last() 漏掉前面那些屏的更新。
    pub fn most_recent_tail(&self, ids: &[TerminalId]) -> Option<String> {
        let map = self.tail_handles.lock().ok()?;
        let mut best: Option<(u64, String)> = None;
        for id in ids {
            let Some(h) = map.get(id) else { continue };
            let ts = h.last_update_ms();
            let Some(text) = h.snapshot() else { continue };
            match &best {
                Some((cur_ts, _)) if *cur_ts >= ts => {}
                _ => best = Some((ts, text)),
            }
        }
        best.map(|(_, t)| t)
    }

    /// 把额外 sink 加到现有 terminal(浮窗 attach 用)
    pub fn attach_sink<S: ChunkSink>(
        &self,
        id: TerminalId,
        sink: S,
    ) -> Result<SinkId, TerminalRegistryError> {
        let map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        let t = map.get(&id).ok_or(TerminalRegistryError::NotFound(id))?;
        Ok(t.add_sink(sink))
    }

    /// 取消订阅(注意:此处不删除 terminal,仅取消 chunk 推送给该 sink)
    pub fn detach_sink(
        &self,
        id: TerminalId,
        sink_id: SinkId,
    ) -> Result<(), TerminalRegistryError> {
        let map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        if let Some(t) = map.get(&id) {
            t.remove_sink(sink_id);
        }
        Ok(())
    }

    /// 读取 terminal 当前生效的 (rows, cols)(最近一次 resize 下发值;纯查询)。
    /// 返回 (0, 0) 表示 spawn 后尚未 resize。视图变可见时用它判断 PTY 是否已被
    /// 别的视图(浮窗)改成别的尺寸 → 不一致则本视图 buffer 已被污染,需清屏重绘。
    pub fn size(&self, id: TerminalId) -> Result<(u16, u16), TerminalRegistryError> {
        let map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        let t = map.get(&id).ok_or(TerminalRegistryError::NotFound(id))?;
        Ok(t.size())
    }

    /// 读取 terminal 的 scrollback 快照(不订阅,纯查询)
    pub fn scrollback(&self, id: TerminalId) -> Result<Vec<u8>, TerminalRegistryError> {
        let map = self
            .terminals
            .lock()
            .map_err(|_| TerminalRegistryError::Poisoned)?;
        let t = map.get(&id).ok_or(TerminalRegistryError::NotFound(id))?;
        Ok(t.scrollback_snapshot())
    }

    pub fn count(&self) -> usize {
        self.terminals.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// terminal 的 shell pid(供 agent 进程识别)
    pub fn pid_of(&self, id: TerminalId) -> Option<u32> {
        let map = self.terminals.lock().ok()?;
        map.get(&id)?.child_pid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use vibeterm_pty::sinks::MpscSink;

    #[test]
    fn spawn_and_close() {
        let reg = TerminalRegistry::new();
        let (tx, _rx) = mpsc::channel::<Vec<u8>>();
        #[cfg(unix)]
        let (command, args) = (std::env::var("SHELL").unwrap_or("/bin/sh".into()), vec![]);
        #[cfg(windows)]
        let (command, args) = ("cmd.exe".into(), vec!["/C".into(), "exit".into()]);
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".into());
        let id = reg
            .spawn(
                SpawnOpts {
                    rows: 24,
                    cols: 80,
                    cwd,
                    command,
                    args,
                    env: vec![("TERM".into(), "xterm-256color".into())],
                },
                MpscSink::new(tx),
            )
            .expect("spawn");
        assert_eq!(reg.count(), 1);
        reg.close(id).expect("close");
        assert_eq!(reg.count(), 0);
    }
}
