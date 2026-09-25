//! PTY 抽象层
//!
//! 设计要点:
//!   - 每 PTY 1 个专属 std 阻塞 read 线程,chunk 通过用户提供的 sink 推出
//!   - read 用 try_clone_reader,write 走 take_writer + Mutex<Box<dyn Write>>
//!   - 字节级原样写入,Bracketed Paste 由上层包裹
//!   - 子进程退出 → child.wait → 通过 sink 推 EOF 标记,上层决定关 panel
//!   - 关闭顺序(unix):置 closing(读线程转为 try_wait + 读空)→ killpg SIGHUP 整个进程组
//!     → 500ms → killpg SIGKILL
//!
//! 本 crate 不知道 Tauri,不依赖 IPC schema。chunk sink 是注入的 trait,
//! 上层(vibeterm-core / src-tauri main.rs)负责把 sink 绑到 Tauri Channel<Vec<u8>>。

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// PTY chunk 消费者抽象。
///
/// 实现需在多线程下安全(Send + Sync,因为读线程持有 Arc<Self>)。
pub trait ChunkSink: Send + Sync + 'static {
    fn push(&self, chunk: Vec<u8>);
    /// PTY EOF / read 错误时调用。可不实现(noop)。
    fn finish(&self, _info: ExitInfo) {}
}

/// 子进程退出信息。
///
/// `signal` 是 portable-pty 提供的信号名(unix 上为 `strsignal` 结果,如 "Killed"/
/// "Terminated";非数字编号),正常退出为 `None`。用于区分「正常退出」与「被信号杀死」。
#[derive(Debug, Clone)]
pub struct ExitInfo {
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
}

/// Spawn 参数(与 IPC schema 解耦 — 由 core 层做转换)。
#[derive(Debug, Clone)]
pub struct SpawnOpts {
    pub rows: u16,
    pub cols: u16,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(thiserror::Error, Debug)]
pub enum PtyError {
    #[error("openpty: {0}")]
    OpenPty(String),
    #[error("spawn: {0}")]
    Spawn(String),
    #[error("resize: {0}")]
    Resize(String),
    #[error("lock poisoned: {0}")]
    Lock(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Sink subscription id —— `add_sink` 返回,`remove_sink` 用
pub type SinkId = u64;

type SinkList = Arc<Mutex<Vec<(SinkId, Box<dyn ChunkSink>)>>>;
type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type SharedMaster = Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>;
type SharedKiller = Arc<Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>>;

/// scrollback ring buffer。
/// 上限按字节(256KB 默认 ≈ 1000 行 80 列)。read 线程 fanout 后 push,
/// `add_sink` 时回放给新订阅,修复浮窗 reparent scrollback 丢失。
const SCROLLBACK_CAP: usize = 256 * 1024;
type Scrollback = Arc<Mutex<std::collections::VecDeque<u8>>>;

/// 一个活的 PTY + 子进程句柄。Drop 时强 kill。
pub struct Terminal {
    writer: SharedWriter,
    master: SharedMaster,
    /// 持有 child killer 以便强 kill;实际 wait 在 read 线程内
    killer: SharedKiller,
    /// 多 sink 订阅(per-Web-window 一个)
    sinks: SinkList,
    next_sink_id: Arc<Mutex<SinkId>>,
    /// 回滚缓冲(用于 add_sink 时回放给新订阅者)
    scrollback: Scrollback,
    /// shell 进程 pid。用于扫前台进程识别 agent。
    /// portable-pty 的 process_id() 在 unix 返回 Some(pid)。
    child_pid: Option<u32>,
    /// 子进程是否已被读线程 wait() 收尸。Drop 的 kill 链以此守卫:已收尸的 pid 可能
    /// 已被系统复用,再补刀会误杀无关进程。
    reaped: Arc<AtomicBool>,
    /// Drop 置位 → 读线程进入收口模式(unix):非阻塞 try_wait 子进程 + 读空 master,
    /// 子进程一退出就结束(≤200ms 轮询)。没有它,逃出进程组的后代(交互 shell 有
    /// job control,`nohup x & disown` 的进程自成一组)仍持 slave fd,master read 永不
    /// EOF,读线程 / master fd / sinks 链每关一个终端泄漏一份。
    closing: Arc<AtomicBool>,
    /// 最近一次实际下发的 (rows, cols)。resize 幂等守卫:尺寸没变就跳过 TIOCSWINSZ,
    /// 避免普通切任务/返回可见时多余的 SIGWINCH 让 TUI agent 白白重绘。
    /// 初始 (0,0) 保证 spawn 后首次 resize 必定生效。
    last_size: Arc<Mutex<(u16, u16)>>,
}

impl Terminal {
    /// 启动 PTY + 子进程 + read 线程;initial_sink 自动注册为第一个订阅者
    pub fn spawn<S: ChunkSink>(opts: SpawnOpts, initial_sink: S) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: opts.rows,
                cols: opts.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::OpenPty(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&opts.command);
        for a in &opts.args {
            cmd.arg(a);
        }
        cmd.cwd(&opts.cwd);
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;

        // slave 在 spawn 后立即 drop(防 fd 泄漏)
        drop(pair.slave);

        // unix:自己 dup 一个 master fd 当读端,拿得到 raw fd 才能 poll + closing 检查。
        // Windows:沿用 portable-pty reader(ConPTY 无 poll,Drop 后靠 ConPTY 关闭收口)。
        #[cfg(unix)]
        let reader = {
            let raw = pair
                .master
                .as_raw_fd()
                .ok_or_else(|| PtyError::Spawn("master as_raw_fd".into()))?;
            // SAFETY: raw 属于仍存活的 pair.master;仅借用来 dup 出独立 OwnedFd(cloexec)。
            let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(raw) }.try_clone_to_owned()?;
            std::fs::File::from(fd)
        };
        #[cfg(not(unix))]
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Spawn(format!("try_clone_reader: {e}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Spawn(format!("take_writer: {e}")))?;
        let killer = child.clone_killer();
        let child_pid = child.process_id();
        let reaped = Arc::new(AtomicBool::new(false));
        let reaped_for_read = reaped.clone();
        let closing = Arc::new(AtomicBool::new(false));
        let closing_for_read = closing.clone();

        // 多 sink:初始 sink 进 vec
        let sinks: SinkList = Arc::new(Mutex::new(vec![(0, Box::new(initial_sink))]));
        let sinks_for_read = sinks.clone();
        // scrollback
        let scrollback: Scrollback = Arc::new(Mutex::new(
            std::collections::VecDeque::with_capacity(SCROLLBACK_CAP),
        ));
        let scrollback_for_read = scrollback.clone();

        // 阻塞 read 线程 — fan-out chunk 到所有 sinks + 追加 scrollback
        let read_thread = std::thread::Builder::new()
            .name("pty-read".to_string())
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 8192];
                // closing 后由 try_wait 抢先拿到的退出状态(此时不再阻塞 wait)
                #[cfg(unix)]
                let mut early_status = None;
                #[cfg(not(unix))]
                let early_status: Option<portable_pty::ExitStatus> = None;
                loop {
                    let closing = closing_for_read.load(Ordering::SeqCst);
                    // unix:closing 后转为「非阻塞等子进程退出 + 继续读空 master(丢弃)」。
                    // 不能停读:关最后一个 slave fd 的进程会在 ttywait 里等 master 把 outq
                    // 读空(XNU ptsclose→ttywflush),停读 + 阻塞 wait() 会互相等死。
                    // 逃出进程组、仍持 slave 的后代不会给 EOF,所以退出条件是 try_wait。
                    #[cfg(unix)]
                    {
                        if closing {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    early_status = Some(status);
                                    break;
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    tracing::warn!(error = %e, "child.try_wait failed");
                                    break;
                                }
                            }
                        }
                        if !poll_readable(&reader) {
                            continue; // 200ms 超时,回到顶部重查 closing / try_wait
                        }
                    }
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) if closing => {} // 收口中:只读空不分发
                        Ok(n) => {
                            let chunk = buf[..n].to_vec();
                            // 修复竞态:fan-out 与 scrollback 追加在同一把
                            // sinks 锁内串行化。add_sink 也持 sinks 锁读 scrollback 快照
                            // + 注册,确保新 sink 不会在「快照之后、注册之前」漏掉 chunk。
                            let Ok(lock) = sinks_for_read.lock() else {
                                tracing::warn!("sinks mutex poisoned, pty-read 退出");
                                break;
                            };
                            for (_, s) in lock.iter() {
                                s.push(chunk.clone());
                            }
                            // 追加 ring buffer(顺序保留),仍在 sinks 锁保护下
                            match scrollback_for_read.lock() {
                                Ok(mut sb) => {
                                    sb.extend(chunk.iter().copied());
                                    if sb.len() > SCROLLBACK_CAP {
                                        let excess = sb.len() - SCROLLBACK_CAP;
                                        sb.drain(..excess);
                                    }
                                }
                                Err(_) => {
                                    drop(lock);
                                    tracing::warn!("scrollback mutex poisoned, pty-read 退出");
                                    break;
                                }
                            }
                            drop(lock);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "pty read error");
                            break;
                        }
                    }
                }
                let waited = match early_status {
                    Some(status) => Ok(status),
                    None => child.wait(),
                };
                let info = match waited {
                    Ok(status) => {
                        // 已收尸:Drop 的 kill 链据此跳过(pid 此后可能被复用)
                        reaped_for_read.store(true, Ordering::SeqCst);
                        let raw_code = status.exit_code();
                        // u32→i32:unix 上恒为 0-255,绝不溢出;仅 Windows 异常退出码
                        // (如 0xC0000005)可能 >= 2^31,此时记录警告而非静默丢失。
                        let exit_code = match i32::try_from(raw_code) {
                            Ok(c) => Some(c),
                            Err(_) => {
                                tracing::warn!(raw_code, "exit_code 超出 i32 范围,丢弃");
                                None
                            }
                        };
                        ExitInfo {
                            exit_code,
                            // 保留 portable-pty 提供的信号名(被信号杀死时)
                            signal: status.signal().map(|s| s.to_string()),
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "child.wait failed");
                        ExitInfo {
                            exit_code: None,
                            signal: None,
                        }
                    }
                };
                match sinks_for_read.lock() {
                    Ok(lock) => {
                        for (_, s) in lock.iter() {
                            s.finish(info.clone());
                        }
                    }
                    Err(_) => {
                        tracing::warn!("sinks mutex poisoned, 无法通知 sink finish");
                    }
                }
            });
        if let Err(e) = read_thread {
            // 线程起不来但子进程已在跑:必须收口,否则 openpty + 子进程泄漏;
            // 不能 panic(release panic=abort 会拖死整个 app)。
            #[cfg(unix)]
            if let Some(pid) = child_pid {
                let pgid = pid as libc::pid_t;
                // SAFETY: 纯 FFI;child 尚未 wait,pid/pgid 仍属本进程子进程。
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                    libc::waitpid(pgid, std::ptr::null_mut(), 0);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = killer.clone_killer().kill();
            }
            return Err(PtyError::Spawn(format!("pty-read thread: {e}")));
        }

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(pair.master)),
            killer: Arc::new(Mutex::new(killer)),
            sinks,
            next_sink_id: Arc::new(Mutex::new(1)), // 0 = initial
            scrollback,
            child_pid,
            reaped,
            closing,
            last_size: Arc::new(Mutex::new((0, 0))),
        })
    }

    /// 返回 shell 子进程 pid(unix 上一定 Some,Windows 也支持)
    pub fn child_pid(&self) -> Option<u32> {
        self.child_pid
    }

    /// 增加一个 sink 订阅(浮窗 attach 现有 PTY 用)
    /// 订阅前先回放 scrollback,保证新 sink 看到完整历史
    ///
    /// 竞态修复:全程持 sinks 锁完成「读 scrollback 快照 → 注册 → 回放」。
    /// 读线程 fan-out + 追加 scrollback 也在 sinks 锁内串行化,因此:
    ///   - 快照与注册之间不会有 chunk 漏给新 sink(读线程被 sinks 锁阻塞);
    ///   - 回放在注册后、释放锁前完成,保证历史先于后续实时 chunk 到达,顺序不乱。
    pub fn add_sink<S: ChunkSink>(&self, sink: S) -> SinkId {
        // 锁中毒恢复而非 panic:计数器/Vec 在持锁者 panic 后结构仍完整,
        // attach 是生产路径(浮窗),不应因别处线程 panic 而连环崩溃。
        let id = {
            let mut n = self.next_sink_id.lock().unwrap_or_else(|p| p.into_inner());
            let id = *n;
            *n += 1;
            id
        };

        let mut sinks = self.sinks.lock().unwrap_or_else(|p| p.into_inner());
        // 读历史快照(此时读线程被 sinks 锁挡在外,无法插入新 chunk)
        let history: Vec<u8> = match self.scrollback.lock() {
            Ok(sb) => sb.iter().copied().collect(),
            Err(_) => {
                tracing::warn!("scrollback poisoned, 新 sink 跳过历史回放");
                Vec::new()
            }
        };
        if !history.is_empty() {
            sink.push(history);
        }
        sinks.push((id, Box::new(sink)));
        id
    }

    /// 取消订阅(Web 端 Terminal 组件 onCleanup)
    pub fn remove_sink(&self, sink_id: SinkId) {
        match self.sinks.lock() {
            Ok(mut sinks) => sinks.retain(|(id, _)| *id != sink_id),
            Err(_) => tracing::warn!("sinks poisoned, remove_sink 跳过"),
        }
    }

    /// 快照 scrollback ring buffer。
    /// 不加 sink、不影响 stream;给 grep / export / debug-dump 等只读用途。
    /// 返回的 `Vec<u8>` 已脱离锁,调用方可任意处理。
    pub fn scrollback_snapshot(&self) -> Vec<u8> {
        match self.scrollback.lock() {
            Ok(sb) => sb.iter().copied().collect(),
            Err(_) => {
                tracing::warn!("scrollback poisoned, snapshot 返回空");
                Vec::new()
            }
        }
    }

    /// ring buffer 当前字节数(不复制内容)。
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.lock().map(|sb| sb.len()).unwrap_or(0)
    }

    /// 写字节到 PTY stdin。并发安全(内部 Mutex)。
    pub fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| PtyError::Lock("writer".into()))?;
        w.write_all(data)?;
        w.flush()?;
        Ok(())
    }

    /// 调整 PTY 尺寸(下发 TIOCSWINSZ → 子进程收 SIGWINCH)。
    ///
    /// 幂等:同尺寸跳过(TIOCSWINSZ 即便同尺寸也会发 SIGWINCH → TUI agent 白白重绘)。
    /// 此幂等也让上层可以"无条件断言尺寸"——视图变可见时直接下发本视图尺寸,真没变则 no-op
    /// (修浮窗调尺寸后返回主窗:主窗隐藏期 fit 是 no-op、PTY 停在浮窗尺寸,回主窗须强制断言回来)。
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        if rows == 0 || cols == 0 {
            return Ok(()); // 无效尺寸(0×0 容器),忽略
        }
        // 持 last_size 锁跨越整个下发(锁序 last_size → master,无他处反序,不死锁):
        // - TIOCSWINSZ 成功后才记录新值——失败不留脏值,否则同尺寸重试被幂等挡掉,
        //   且 size() 会报告一个 PTY 从未生效的尺寸,误导前端的污染检测;
        // - 并发 resize 串行化,保证 last_size 与 PTY 实际尺寸一致。
        let mut last = self
            .last_size
            .lock()
            .map_err(|_| PtyError::Lock("last_size".into()))?;
        if *last == (rows, cols) {
            return Ok(()); // 尺寸未变 → 跳过,避免多余 SIGWINCH
        }
        let m = self
            .master
            .lock()
            .map_err(|_| PtyError::Lock("master".into()))?;
        m.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| PtyError::Resize(e.to_string()))?;
        *last = (rows, cols);
        Ok(())
    }

    /// 最近一次实际下发的 (rows, cols)。spawn 后首次 resize 前为 (0, 0)。
    /// 供上层判断「本视图变可见时 PTY 是否已被别的视图(浮窗)改成了别的尺寸」——
    /// 若不一致说明隐藏期消费过别的宽度的重绘,buffer 已被污染,需清屏后重绘。
    pub fn size(&self) -> (u16, u16) {
        self.last_size.lock().map(|s| *s).unwrap_or((0, 0))
    }
}

/// poll master 最多 200ms。true = 可读(含 POLLHUP/POLLERR/非 EINTR 错误,交给 read()
/// 报 EOF / 错误走原路径);false = 超时,调用方回去重查 closing。
#[cfg(unix)]
fn poll_readable(fd: &impl std::os::fd::AsRawFd) -> bool {
    let mut pfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: pfd 是有效的单元素数组,fd 由调用方持有、存活。
    let n = unsafe { libc::poll(&mut pfd, 1, 200) };
    n > 0 || (n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted)
}

impl Drop for Terminal {
    /// 关闭顺序(unix):closing 置位(读线程转收口模式)→ killpg SIGHUP → 500ms → killpg SIGKILL。
    ///
    /// portable-pty spawn 时 setsid(),子进程是新会话 / 进程组 leader,pgid == pid。
    /// killpg 杀整组(shell + 同组后代,如非交互 shell 的 `x &`);组内还有成员时系统
    /// 不会复用该 pgid,故没有单 pid kill 在 reaped 检查与 kill 之间 pid 被复用的误杀窗口。
    /// 子进程已被读线程 wait() 收尸则跳过 kill 链。
    fn drop(&mut self) {
        // 先让读线程进入收口模式:逃出进程组、仍持 slave 的后代不会给 EOF,靠这个退出。
        self.closing.store(true, Ordering::SeqCst);
        if self.reaped.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(unix)]
        if let Some(pid) = self.child_pid {
            let pgid = pid as libc::pid_t;
            // SAFETY: 纯 FFI;pgid 未被收尸(reaped=false),仍是本进程子进程的进程组。
            unsafe { libc::killpg(pgid, libc::SIGHUP) };
            // SIGHUP 免疫进程(nohup/自定义 handler)500ms 后仍未退出 → SIGKILL。
            // 分离线程执行,不阻塞 Drop 调用方(close_pty IPC 持 registry 锁)。
            let reaped = self.reaped.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if !reaped.load(Ordering::SeqCst) {
                    // SAFETY: 同上。
                    unsafe { libc::killpg(pgid, libc::SIGKILL) };
                }
            });
            return;
        }
        // Windows(TerminateProcess)/ unix 无 pid 兜底
        if let Ok(mut k) = self.killer.lock() {
            let _ = k.kill();
        }
    }
}

#[cfg(all(test, unix))]
mod drop_tests {
    use super::sinks::MpscSink;
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn spawn_sh(script: &str, tx: mpsc::Sender<Vec<u8>>) -> Terminal {
        Terminal::spawn(
            SpawnOpts {
                rows: 24,
                cols: 80,
                cwd: "/".into(),
                command: "/bin/sh".into(),
                args: vec!["-c".into(), script.into()],
                env: vec![("TERM".into(), "dumb".into())],
            },
            MpscSink::new(tx),
        )
        .expect("spawn")
    }

    /// 读线程持有最后一个 MpscSink → 线程退出 ⇔ channel Disconnected。
    fn wait_for_ready_then_drop(rx: &mpsc::Receiver<Vec<u8>>, t: Terminal) -> Duration {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut acc = Vec::new();
        while !acc.windows(5).any(|w| w == b"READY") {
            let chunk = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("shell 未在 5s 内输出 READY");
            acc.extend(chunk);
        }
        let start = Instant::now();
        drop(t);
        loop {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return start.elapsed(),
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("读线程 2s 内未退出"),
            }
        }
    }

    #[test]
    fn drop_kills_whole_process_group_and_read_thread_exits() {
        let (tx, rx) = mpsc::channel();
        // 后台 sleep 与 exec 后的 sleep 同进程组,都持 slave fd
        let t = spawn_sh("sleep 30 & echo READY; exec sleep 30", tx);
        let pgid = t.child_pid().expect("pid") as libc::pid_t;
        wait_for_ready_then_drop(&rx, t);
        // 整组已无存活成员(zombie 被 init 收尸需一点时间)
        let deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { libc::killpg(pgid, 0) } == 0 {
            assert!(Instant::now() < deadline, "进程组 {pgid} 仍有存活成员");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn drop_unblocks_read_thread_when_descendant_escapes_pgrp() {
        let (tx, rx) = mpsc::channel();
        // perl setsid 逃出进程组、仍持 slave 3s → killpg 够不着,read 不会 EOF,
        // 读线程必须靠 closing + try_wait 退出。
        let t = spawn_sh(
            "perl -e 'use POSIX; POSIX::setsid(); sleep 3' & echo READY; exec sleep 30",
            tx,
        );
        let elapsed = wait_for_ready_then_drop(&rx, t);
        assert!(
            elapsed < Duration::from_secs(2),
            "读线程退出耗时 {elapsed:?}"
        );
    }
}

// 公开标准 sink 实现:把 chunk 推进 std mpsc channel,供测试 / 简单消费者用。
// 真实生产路径在 src-tauri/main.rs 中 sink 包 Tauri Channel<Vec<u8>>。
pub mod sinks {
    use super::*;
    use std::sync::mpsc::Sender;

    pub struct MpscSink {
        tx: Sender<Vec<u8>>,
    }

    impl MpscSink {
        pub fn new(tx: Sender<Vec<u8>>) -> Self {
            Self { tx }
        }
    }

    impl ChunkSink for MpscSink {
        fn push(&self, chunk: Vec<u8>) {
            let _ = self.tx.send(chunk);
        }
    }

    /// 任务名下状态行:跟踪终端 stdout 的「最后一行非空可见文本」。
    ///
    /// 设计:
    ///   - push 时实时 strip ANSI / OSC,逐字符累积到 current_line
    ///   - 遇到 \n 或 \r → 把 current 提交到 last_line(覆盖),清 current
    ///   - last_line/current_line 都限长(80 字符),防止异常长行爆内存
    ///   - 外部通过 `snapshot()` 拿当前末行,优先用 last_line(完成的),否则 current_line(进行中)
    pub struct TailSink {
        inner: Arc<Mutex<TailInner>>,
    }

    struct TailInner {
        current_line: String,
        last_line: String,
        max_chars: usize,
        /// 最近一次有可见输出的 Unix 毫秒时间戳; 0 表示从未更新.
        /// 分屏场景下 emit_tasks_changed 用这个挑该 task 下"最近活跃"的终端.
        last_update_ms: u64,
    }

    impl TailSink {
        pub fn new(max_chars: usize) -> (Self, TailSinkHandle) {
            let inner = Arc::new(Mutex::new(TailInner {
                current_line: String::new(),
                last_line: String::new(),
                max_chars,
                last_update_ms: 0,
            }));
            (
                Self {
                    inner: inner.clone(),
                },
                TailSinkHandle { inner },
            )
        }
    }

    fn now_ms() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 与 TailSink 共享内部状态的查询 handle(Send + Sync,可放 Registry)。
    #[derive(Clone)]
    pub struct TailSinkHandle {
        inner: Arc<Mutex<TailInner>>,
    }

    impl TailSinkHandle {
        /// 取末行快照:优先返回进行中的当前行(最新),否则返回最近完成行;为空返回 None。
        pub fn snapshot(&self) -> Option<String> {
            let inner = self.inner.lock().ok()?;
            let current_trimmed = inner.current_line.trim();
            let pick = if !current_trimmed.is_empty() {
                current_trimmed.to_string()
            } else {
                inner.last_line.clone()
            };
            if pick.is_empty() {
                None
            } else {
                Some(truncate_chars(&pick, inner.max_chars))
            }
        }

        /// 最近一次有可见输出的时间戳(Unix ms);从未更新返回 0。
        pub fn last_update_ms(&self) -> u64 {
            self.inner
                .lock()
                .ok()
                .map(|i| i.last_update_ms)
                .unwrap_or(0)
        }
    }

    impl ChunkSink for TailSink {
        fn push(&self, chunk: Vec<u8>) {
            let text = String::from_utf8_lossy(&chunk).into_owned();
            let stripped = strip_ansi(&text);
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            let mut touched = false;
            for c in stripped.chars() {
                if c == '\n' || c == '\r' {
                    let trimmed = inner.current_line.trim();
                    if !trimmed.is_empty() {
                        inner.last_line = truncate_chars(trimmed, inner.max_chars);
                        touched = true;
                    }
                    inner.current_line.clear();
                } else if (c as u32) >= 0x20 && c != '\x7F' {
                    // 排除 DEL(0x7F):它满足 >= 0x20 但不是可见字形
                    inner.current_line.push(c);
                    touched = true;
                    // 防爆 — current_line 不要无限增长
                    if inner.current_line.chars().count() > inner.max_chars * 4 {
                        let new_start = inner.current_line.chars().count() - inner.max_chars * 2;
                        inner.current_line = inner.current_line.chars().skip(new_start).collect();
                    }
                }
            }
            if touched {
                inner.last_update_ms = now_ms();
            }
        }
    }

    /// 去除 ANSI CSI(ESC[)、OSC(ESC])及 DCS/PM/APC(ESC P / ESC ^ / ESC _)、
    /// SS2/SS3(ESC N / ESC O)等控制序列;保留可见字符 + \n \r。
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        // 跳到字符串终止符:BEL(\x07)或 ST(ESC \\)。OSC/DCS/PM/APC 共用。
        let skip_to_st = |chars: &mut std::iter::Peekable<std::str::Chars>| {
            while let Some(c2) = chars.next() {
                if c2 == '\x07' {
                    break;
                }
                if c2 == '\x1B' && chars.peek() == Some(&'\\') {
                    let _ = chars.next();
                    break;
                }
            }
        };
        while let Some(c) = chars.next() {
            if c == '\x1B' {
                match chars.next() {
                    Some('[') => {
                        // CSI:跳到第一个 ASCII 字母(end byte 0x40-0x7E)
                        for c2 in chars.by_ref() {
                            if matches!(c2, '\x40'..='\x7E') {
                                break;
                            }
                        }
                    }
                    // OSC / DCS / PM / APC:跳到 BEL 或 ST(ESC \\)
                    Some(']') | Some('P') | Some('^') | Some('_') => skip_to_st(&mut chars),
                    // SS3 / SS2:仅影响紧随的一个字符
                    Some('O') | Some('N') => {
                        let _ = chars.next();
                    }
                    // 其余两字节 Fe 序列(ESC + 单字符)已消耗完毕,无正文
                    Some(_) | None => {}
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn truncate_chars(s: &str, max: usize) -> String {
        if s.chars().count() <= max {
            s.to_string()
        } else {
            let mut t: String = s.chars().take(max).collect();
            t.push('…');
            t
        }
    }

    #[cfg(test)]
    mod tail_tests {
        use super::*;

        #[test]
        fn strips_csi() {
            assert_eq!(strip_ansi("\x1B[31mhello\x1B[0m world"), "hello world");
        }

        #[test]
        fn strips_osc_bel() {
            assert_eq!(strip_ansi("\x1B]0;title\x07rest"), "rest");
        }

        #[test]
        fn tail_picks_last_nonempty_line() {
            let (sink, h) = TailSink::new(80);
            sink.push(b"foo\n\nbar\n".to_vec());
            assert_eq!(h.snapshot().as_deref(), Some("bar"));
        }

        #[test]
        fn carriage_return_resets_current() {
            let (sink, h) = TailSink::new(80);
            sink.push(b"progress 30%\rprogress 50%\rprogress 80%".to_vec());
            assert_eq!(h.snapshot().as_deref(), Some("progress 80%"));
        }

        #[test]
        fn truncates_to_max() {
            let (sink, h) = TailSink::new(5);
            sink.push(b"hello world\n".to_vec());
            assert_eq!(h.snapshot().as_deref(), Some("hello…"));
        }

        #[test]
        fn empty_when_no_visible_text() {
            let (sink, h) = TailSink::new(80);
            sink.push(b"\x1B[2K\x1B[1G".to_vec());
            assert_eq!(h.snapshot(), None);
        }

        #[test]
        fn last_update_ms_advances_on_visible_output() {
            // 初始为 0;有可见字节后 > 0;后续输出 ts 单调不减.
            let (sink, h) = TailSink::new(80);
            assert_eq!(h.last_update_ms(), 0);
            sink.push(b"foo\n".to_vec());
            let t1 = h.last_update_ms();
            assert!(t1 > 0);
            // 让时间向前(分辨率毫秒,sleep 几毫秒就够)
            std::thread::sleep(std::time::Duration::from_millis(5));
            sink.push(b"bar\n".to_vec());
            let t2 = h.last_update_ms();
            assert!(t2 >= t1, "ts 应单调不减: t1={t1} t2={t2}");
        }

        #[test]
        fn last_update_ms_unchanged_for_pure_ansi() {
            // 纯 ANSI 控制序列没有可见字符 → ts 不动.
            let (sink, h) = TailSink::new(80);
            sink.push(b"hi\n".to_vec());
            let before = h.last_update_ms();
            std::thread::sleep(std::time::Duration::from_millis(3));
            sink.push(b"\x1B[2K\x1B[1G".to_vec());
            assert_eq!(h.last_update_ms(), before, "纯 ANSI 不应推进 ts");
        }
    }
}

const _: fn() = || {
    // 强制 Send 检查:Terminal 必须能跨 tokio worker 传(被存进 core 注册表 + IPC handler 间共享)
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<Terminal>();
};
