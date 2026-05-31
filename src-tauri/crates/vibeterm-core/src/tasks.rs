//! 任务注册表 + 任务/终端关联
//!
//!   - 任务 CRUD(create / close / rename / pin / reorder)
//!   - 每个 task 关联多个 terminal(平级 tab,无分屏)
//!   - 任务激活态(主工作区当前显示哪个 task)
//!   - 任务"在哪显示"(main / floating-<label> / nowhere)
//!   - 状态聚合规则:max(waiting_input > running > idle)
//!   - 持久化(每次 mutate 立刻 save tasks.json)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vibeterm_ipc::{SplitNode, TaskDto, TaskLocation, TaskStatus, TerminalId, WorktreeRef};
use vibeterm_tasks::{TaskId, TaskSnapshot, TasksFile};

#[derive(thiserror::Error, Debug)]
pub enum TaskError {
    #[error("task not found: {0}")]
    NotFound(TaskId),
    #[error("tasks io: {0}")]
    TasksIo(#[from] vibeterm_tasks::TasksError),
    #[error("registry poisoned")]
    Poisoned,
}

#[derive(Debug, Clone)]
struct TaskRuntime {
    name: String,
    cwd: Option<String>,
    pinned: bool,
    terminal_ids: Vec<TerminalId>,
    terminal_statuses: HashMap<TerminalId, TaskStatus>,
    location: TaskLocation,
    split_tree: SplitNode,
    /// 挂载的 git worktree(可选)
    worktree: Option<WorktreeRef>,
    /// 用户是否看过最近一次"完成事件"。
    /// 终端从 Running → Idle 且该 task 不是 active_main 时 → seen=false。
    /// 用户切到该 task 时 → seen=true。
    /// 任何终端转为 Running/WaitingInput → seen=true(被新工作覆盖,不再算"未看")。
    seen: bool,
    /// 进程层 agent 识别结果(由后台轮询写入)
    agent_kind: Option<String>,
    /// 关键:slot_id → terminal_id 映射(后端做幂等,前端无需判断 spawn/attach)。
    /// 同一 (task, slot) 第二次 spawn 直接返回已有 terminal_id + add_sink。
    /// Canvas 和 Normal 视图同时挂载时,这是避免"开两个独立 PTY"的唯一防线。
    slot_terminals: HashMap<u32, TerminalId>,
    /// 通知静音 (持久化). 通知层每次弹之前查它.
    notify_muted: bool,
    /// agent 当前 permission mode (claude/codex hook payload 的 permission_mode 字段).
    /// 视觉徽标用 — yolo 模式高亮提示.
    permission_mode: Option<String>,
    /// agent 当前 reasoning effort 等级 (low/medium/high/xhigh/max), 来自 hook payload 的
    /// effort.level. 实时(每个携带 effort 的 event 刷新); 比 transcript 解析更直接.
    effort: Option<String>,
    /// hook auto-naming: 任务名是否还能被 UserPromptSubmit 自动重命名.
    /// 新建时 true. 自动改过名 / 用户手动改过名后 → false.
    auto_namable: bool,
}

fn default_split_tree() -> SplitNode {
    SplitNode::Leaf { slot_id: 0 }
}

impl TaskRuntime {
    /// 状态聚合:
    ///   优先级:WaitingInput > Stalled > Running > Done > Idle
    ///   - 任一终端 WaitingInput → WaitingInput
    ///   - 任一终端 Stalled (无任何 Running/WaitingInput) → Stalled
    ///   - 任一终端 Running → Running
    ///   - 全部 Idle 且 !seen → Done
    ///   - 否则 Idle
    fn aggregated_status(&self) -> TaskStatus {
        let mut has_running = false;
        let mut has_stalled = false;
        for v in self.terminal_statuses.values() {
            match v {
                TaskStatus::WaitingInput => return TaskStatus::WaitingInput,
                TaskStatus::Stalled => has_stalled = true,
                TaskStatus::Running => has_running = true,
                _ => {}
            }
        }
        if has_running {
            return TaskStatus::Running;
        }
        if has_stalled {
            return TaskStatus::Stalled;
        }
        if !self.seen {
            TaskStatus::Done
        } else {
            TaskStatus::Idle
        }
    }
}

/// (task, slot) → 串行化锁 — 类型本身就是文档, 拆 type alias 没什么收益.
type SlotLockMap = HashMap<(TaskId, u32), Arc<Mutex<()>>>;

pub struct TaskRegistry {
    inner: Mutex<Inner>,
    /// 每 (task, slot) 一个 mutex,用于序列化"查 + spawn + bind"避免并发 race。
    /// Normal 和 Canvas 同 task+slot 并发挂载时,后到的请求会等先到的完成 bind,
    /// 然后查到 existing 走 attach 路径。
    slot_locks: Mutex<SlotLockMap>,
}

struct Inner {
    next_id: TaskId,
    tasks: HashMap<TaskId, TaskRuntime>,
    order: Vec<TaskId>,
    active_main: Option<TaskId>,
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskRegistry {
    pub fn new() -> Self {
        let inner = match vibeterm_tasks::load() {
            Ok(f) => Inner::from_file(f),
            Err(e) => {
                tracing::warn!(err = %e, "tasks.json load failed, starting empty");
                Inner::empty()
            }
        };
        Self {
            inner: Mutex::new(inner),
            slot_locks: Mutex::new(HashMap::new()),
        }
    }

    /// 返回 (task, slot) 的 mutex。同一 (task, slot) 多次调用返回同一个 Arc。
    /// 用于 spawn_terminal_in_task 的临界区。
    pub fn slot_lock(&self, task_id: TaskId, slot_id: u32) -> Result<Arc<Mutex<()>>, TaskError> {
        let mut map = self.slot_locks.lock().map_err(|_| TaskError::Poisoned)?;
        Ok(map
            .entry((task_id, slot_id))
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, TaskError> {
        self.inner.lock().map_err(|_| TaskError::Poisoned)
    }

    pub fn list(&self) -> Result<Vec<TaskDto>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.list_dto())
    }

    pub fn create(
        &self,
        name: String,
        cwd: Option<String>,
        worktree: Option<WorktreeRef>,
    ) -> Result<TaskId, TaskError> {
        let mut inner = self.lock()?;
        let id = inner.next_id;
        inner.next_id = inner.next_id.wrapping_add(1);
        // 挂了 worktree:cwd 强制等于 worktree_path,避免两处不一致
        let effective_cwd = worktree.as_ref().map(|w| w.worktree_path.clone()).or(cwd);
        inner.tasks.insert(
            id,
            TaskRuntime {
                name,
                cwd: effective_cwd,
                pinned: false,
                terminal_ids: vec![],
                terminal_statuses: HashMap::new(),
                location: TaskLocation::MainWorkspace,
                split_tree: default_split_tree(),
                worktree,
                seen: true,
                agent_kind: None,
                slot_terminals: HashMap::new(),
                notify_muted: false,
                permission_mode: None,
                effort: None,
                auto_namable: true,
            },
        );
        inner.order.push(id);
        if inner.active_main.is_none() {
            inner.active_main = Some(id);
        }
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(id)
    }

    pub fn close(&self, id: TaskId) -> Result<Vec<TerminalId>, TaskError> {
        let mut inner = self.lock()?;
        let runtime = inner.tasks.remove(&id).ok_or(TaskError::NotFound(id))?;
        inner.order.retain(|x| *x != id);
        if inner.active_main == Some(id) {
            inner.active_main = inner.order.first().copied();
        }
        let term_ids = runtime.terminal_ids.clone();
        let snap = inner.snapshot();
        drop(inner);
        // 清理该 task 残留的 slot 锁,避免 slot_locks 随 task 关闭无界增长
        // (对齐 unbind_terminal 用 retain 清理 slot_terminals 的同类模式)。
        match self.slot_locks.lock() {
            Ok(mut map) => map.retain(|(tid, _), _| *tid != id),
            Err(_) => tracing::warn!("slot_locks poisoned, skipping cleanup on close"),
        }
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(term_ids)
    }

    pub fn rename(&self, id: TaskId, name: String) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.name = name;
        // 用户手动改名 → 关掉自动命名 (避免后续 prompt 覆盖用户的命名)
        t.auto_namable = false;
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    /// hook auto-naming. 仅当 auto_namable 仍 true 时执行重命名,
    /// 执行后翻成 false (一次性). 返回 Ok(true) 表示真改了名.
    pub fn rename_auto(&self, id: TaskId, name: String) -> Result<bool, TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        if !t.auto_namable {
            return Ok(false);
        }
        t.name = name;
        t.auto_namable = false;
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(true)
    }

    pub fn is_auto_namable(&self, id: TaskId) -> Result<bool, TaskError> {
        let inner = self.lock()?;
        Ok(inner
            .tasks
            .get(&id)
            .map(|t| t.auto_namable)
            .unwrap_or(false))
    }

    /// 设置 permission_mode (徽标 + 通知 body 用).
    /// 返回 Ok(true) 表示真变化, 调用方可决定是否 emit tasks_changed.
    pub fn set_permission_mode(&self, id: TaskId, mode: Option<String>) -> Result<bool, TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        if t.permission_mode == mode {
            return Ok(false);
        }
        t.permission_mode = mode;
        Ok(true)
    }

    /// 设置 reasoning effort 等级 (hook payload 的 effort.level). 返回 Ok(true) 表示真变化.
    pub fn set_effort(&self, id: TaskId, effort: Option<String>) -> Result<bool, TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        if t.effort == effort {
            return Ok(false);
        }
        t.effort = effort;
        Ok(true)
    }

    /// 按 terminal 找所属 task 并设 effort(嗅探层从 PTY 工作动画抠到的 effort 等级)。
    /// 返回 Some(task_id) 表示真变化(供 emit tasks_changed); 仅内存, 不写盘(effort 是 live 态)。
    pub fn set_effort_for_terminal(
        &self,
        term_id: TerminalId,
        effort: Option<String>,
    ) -> Result<Option<TaskId>, TaskError> {
        let mut inner = self.lock()?;
        for (tid, t) in inner.tasks.iter_mut() {
            if t.terminal_ids.contains(&term_id) {
                if t.effort == effort {
                    return Ok(None);
                }
                t.effort = effort;
                return Ok(Some(*tid));
            }
        }
        Ok(None)
    }

    pub fn pin(&self, id: TaskId, pinned: bool) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.pinned = pinned;
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    /// 切换该 task 的通知静音.
    pub fn set_notify_muted(&self, id: TaskId, muted: bool) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.notify_muted = muted;
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    pub fn notify_muted_of(&self, id: TaskId) -> Result<bool, TaskError> {
        let inner = self.lock()?;
        Ok(inner
            .tasks
            .get(&id)
            .map(|t| t.notify_muted)
            .unwrap_or(false))
    }

    pub fn reorder(&self, new_order: Vec<TaskId>) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let valid: Vec<TaskId> = new_order
            .into_iter()
            .filter(|id| inner.tasks.contains_key(id))
            .collect();
        // reorder 只重排,不得丢弃任何已有 task:把 new_order 没覆盖到的合法 id
        // 按其在现有 order 中的相对顺序追加到末尾,保住 "order 是 tasks key 的排列"
        // 这个不变量。从 inner.order(而非 HashMap keys)派生以保证顺序确定。
        let missing: Vec<TaskId> = inner
            .order
            .iter()
            .filter(|id| !valid.contains(id) && inner.tasks.contains_key(id))
            .copied()
            .collect();
        inner.order = [valid, missing].concat();
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    pub fn set_active_main(&self, id: TaskId) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        if !inner.tasks.contains_key(&id) {
            return Err(TaskError::NotFound(id));
        }
        let changed = inner.active_main != Some(id);
        inner.active_main = Some(id);
        // 用户聚焦此 task → mark seen,任何 Done 状态会消化为 Idle
        if let Some(t) = inner.tasks.get_mut(&id) {
            t.seen = true;
        }
        // active_main 变更才写盘(switch tab 频率高,小心写放大)
        if changed {
            let snap = inner.snapshot();
            drop(inner);
            if let Err(e) = vibeterm_tasks::save(&snap) {
                tracing::warn!(err = %e, "tasks.json save failed");
            }
        }
        Ok(())
    }

    pub fn active_main(&self) -> Option<TaskId> {
        self.lock().ok().and_then(|i| i.active_main)
    }

    // ---- terminal 关联(spawn / close 时由上层调)----
    pub fn attach_terminal(&self, task_id: TaskId, term_id: TerminalId) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner
            .tasks
            .get_mut(&task_id)
            .ok_or(TaskError::NotFound(task_id))?;
        if !t.terminal_ids.contains(&term_id) {
            t.terminal_ids.push(term_id);
        }
        t.terminal_statuses.insert(term_id, TaskStatus::Idle);
        Ok(())
    }

    /// 关键:在 (task, slot) 上幂等 spawn。
    /// 已有 terminal → 返回 (existing_id, false /* not_new */);
    /// 没有 → 返回 None,由上层 spawn 后调 bind_slot 回填。
    pub fn terminal_for_slot(
        &self,
        task_id: TaskId,
        slot_id: u32,
    ) -> Result<Option<TerminalId>, TaskError> {
        let inner = self.lock()?;
        Ok(inner
            .tasks
            .get(&task_id)
            .and_then(|t| t.slot_terminals.get(&slot_id).copied()))
    }

    pub fn bind_slot(
        &self,
        task_id: TaskId,
        slot_id: u32,
        term_id: TerminalId,
    ) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner
            .tasks
            .get_mut(&task_id)
            .ok_or(TaskError::NotFound(task_id))?;
        t.slot_terminals.insert(slot_id, term_id);
        Ok(())
    }

    /// terminal 真死(closePty)时调,清掉所有 task 里指向它的 slot 绑定
    pub fn unbind_terminal(&self, term_id: TerminalId) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        for t in inner.tasks.values_mut() {
            t.slot_terminals.retain(|_, v| *v != term_id);
        }
        Ok(())
    }

    pub fn detach_terminal(&self, term_id: TerminalId) -> Result<Option<TaskId>, TaskError> {
        let mut inner = self.lock()?;
        let mut owner = None;
        for (tid, t) in inner.tasks.iter_mut() {
            if t.terminal_ids.contains(&term_id) {
                t.terminal_ids.retain(|x| *x != term_id);
                t.terminal_statuses.remove(&term_id);
                owner = Some(*tid);
                break;
            }
        }
        Ok(owner)
    }

    /// 更新终端状态。
    /// `by_osc`:Idle 是否由 OSC 133/633 D 真触发(true)还是 idle_timeout 升上来(false)。
    /// 后者会被 ping/tail -f 这种长 streaming 命令的输出间歇期误触发, 不应升为 Done。
    /// 返回 `Some((task_id, prev_aggregated, new_aggregated))` — 聚合状态发生跃迁时;
    /// 返回 `None` — 终端不存在或聚合状态未变(不需要触发上层通知)。
    pub fn update_terminal_status(
        &self,
        term_id: TerminalId,
        status: TaskStatus,
        by_osc: bool,
    ) -> Result<Option<(TaskId, TaskStatus, TaskStatus)>, TaskError> {
        let mut inner = self.lock()?;
        let active_main = inner.active_main;
        for (tid, t) in inner.tasks.iter_mut() {
            if t.terminal_ids.contains(&term_id) {
                let prev_agg = t.aggregated_status();
                let prev = t.terminal_statuses.insert(term_id, status);
                // seen 维护:
                //   - 任何终端转为 Running/WaitingInput → seen=true
                //   - 任何终端从 Running 转 Idle 且 (1) 不是当前 active_main, (2) by_osc=true → seen=false
                //   普通 shell 命令的 idle_timeout 升上来的 Idle 不算"真完成", 不打 Done 红点.
                match status {
                    TaskStatus::Running | TaskStatus::WaitingInput => {
                        t.seen = true;
                    }
                    TaskStatus::Idle => {
                        if by_osc && prev == Some(TaskStatus::Running) && active_main != Some(*tid)
                        {
                            t.seen = false;
                        }
                    }
                    TaskStatus::Stalled => {
                        // Stalled 不影响 seen — 用户应该被通知,但不应该让 task 升 Done.
                    }
                    TaskStatus::Done => {
                        // 终端层不主动报 Done;ignore
                    }
                }
                let new_agg = t.aggregated_status();
                return Ok(if prev_agg != new_agg {
                    Some((*tid, prev_agg, new_agg))
                } else {
                    None
                });
            }
        }
        Ok(None)
    }

    /// 获取 task 的当前聚合状态(供通知 throttle 判断)
    pub fn aggregated_status_of(&self, id: TaskId) -> Result<Option<TaskStatus>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.tasks.get(&id).map(|t| t.aggregated_status()))
    }

    /// 未看完成数 —— 聚合状态为 Done(完成但用户未看)的任务个数。
    /// Dock 角标用它显示"待看"数;用户切到某 Done 任务 → set_active_main 翻 seen=true →
    /// 该任务转回 Idle → 计数自然减一。锁失败按 0 处理(失败开放,不误报角标)。
    pub fn unseen_done_count(&self) -> usize {
        match self.lock() {
            Ok(inner) => inner
                .tasks
                .values()
                .filter(|t| matches!(t.aggregated_status(), TaskStatus::Done))
                .count(),
            Err(_) => 0,
        }
    }

    /// hook 收到"agent 完成 turn"信号时调用. 把 seen 翻为 false —
    /// 当所有 terminal 自然回落到 Idle 时, aggregated_status 会变成 Done.
    /// 返回 Ok(true) = seen 状态真切换了 (调用方可 emit tasks_changed).
    /// 不主动改 terminal_statuses, 让 status detector 按字节流自然推进.
    pub fn mark_agent_completed(&self, id: TaskId) -> Result<bool, TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        if !t.seen {
            return Ok(false); // 已经是 unseen 状态, 不重复
        }
        t.seen = false;
        Ok(true)
    }

    pub fn name_of(&self, id: TaskId) -> Result<Option<String>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.tasks.get(&id).map(|t| t.name.clone()))
    }

    pub fn agent_kind_of(&self, id: TaskId) -> Result<Option<String>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.tasks.get(&id).and_then(|t| t.agent_kind.clone()))
    }

    /// 写入 agent 识别结果。返回 true 表示有变化(供 caller 决定是否 emit)。
    pub fn set_agent_kind(&self, id: TaskId, kind: Option<String>) -> Result<bool, TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        let changed = t.agent_kind != kind;
        if changed {
            t.agent_kind = kind;
        }
        Ok(changed)
    }

    /// 列出所有 task 的 (task_id, terminal_ids) 供后台轮询批量识别 agent
    pub fn task_terminal_ids(&self) -> Result<Vec<(TaskId, Vec<TerminalId>)>, TaskError> {
        let inner = self.lock()?;
        Ok(inner
            .tasks
            .iter()
            .map(|(id, t)| (*id, t.terminal_ids.clone()))
            .collect())
    }

    pub fn set_location(&self, id: TaskId, loc: TaskLocation) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.location = loc;
        Ok(())
    }

    /// 写回任务的分屏布局,持久化 tasks.json
    pub fn set_split_tree(&self, id: TaskId, tree: SplitNode) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.split_tree = tree;
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    /// 给已有 task 挂载 / 替换 worktree。同时把 task.cwd 同步为 worktree_path。
    pub fn attach_worktree(&self, id: TaskId, wt: WorktreeRef) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.cwd = Some(wt.worktree_path.clone());
        t.worktree = Some(wt);
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    /// 卸载 worktree(不删 git worktree,只解除关联)。task.cwd 清空。
    pub fn detach_worktree(&self, id: TaskId) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        t.worktree = None;
        t.cwd = None;
        let snap = inner.snapshot();
        drop(inner);
        if let Err(e) = vibeterm_tasks::save(&snap) {
            tracing::warn!(err = %e, "tasks.json save failed");
        }
        Ok(())
    }

    /// 刷新 worktree 状态字段(head / dirty / ahead / behind / status_updated_at)
    #[allow(clippy::too_many_arguments)]
    pub fn update_worktree_status(
        &self,
        id: TaskId,
        head: String,
        branch: Option<String>,
        is_dirty: bool,
        ahead: u32,
        behind: u32,
        updated_at_ms: u64,
    ) -> Result<(), TaskError> {
        let mut inner = self.lock()?;
        let t = inner.tasks.get_mut(&id).ok_or(TaskError::NotFound(id))?;
        if let Some(w) = t.worktree.as_mut() {
            w.head = head;
            w.branch = branch;
            w.is_dirty = is_dirty;
            w.ahead = ahead;
            w.behind = behind;
            w.status_updated_at = updated_at_ms;
        }
        // 状态字段变更不需要每次写盘,会被显著放大 IO;只在内存更新,UI 通过 emit 拿
        Ok(())
    }

    /// 返回所有挂了 worktree 的 task(供后台轮询)
    pub fn worktree_tasks(&self) -> Result<Vec<(TaskId, WorktreeRef)>, TaskError> {
        let inner = self.lock()?;
        Ok(inner
            .tasks
            .iter()
            .filter_map(|(id, t)| t.worktree.clone().map(|w| (*id, w)))
            .collect())
    }

    pub fn worktree_of(&self, id: TaskId) -> Result<Option<WorktreeRef>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.tasks.get(&id).and_then(|t| t.worktree.clone()))
    }

    pub fn task_dto(&self, id: TaskId) -> Result<Option<TaskDto>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.tasks.get(&id).map(|t| inner.runtime_to_dto(id, t)))
    }

    pub fn cwd(&self, id: TaskId) -> Result<Option<String>, TaskError> {
        let inner = self.lock()?;
        Ok(inner.tasks.get(&id).and_then(|t| t.cwd.clone()))
    }
}

impl Inner {
    fn empty() -> Self {
        Self {
            next_id: 0,
            tasks: HashMap::new(),
            order: vec![],
            active_main: None,
        }
    }
    fn from_file(f: TasksFile) -> Self {
        let mut tasks = HashMap::new();
        for snap in f.tasks.iter() {
            tasks.insert(
                snap.id,
                TaskRuntime {
                    name: snap.name.clone(),
                    cwd: snap.cwd.clone(),
                    pinned: snap.pinned,
                    terminal_ids: vec![], // 终端不自动 rerun
                    terminal_statuses: HashMap::new(),
                    location: TaskLocation::MainWorkspace,
                    split_tree: snap.split_tree.clone(),
                    worktree: snap.worktree.clone(),
                    seen: true,
                    agent_kind: None,
                    slot_terminals: HashMap::new(),
                    notify_muted: snap.notify_muted,
                    permission_mode: None,
                    effort: None,
                    // 从 disk 恢复:已 persist 过则不再自动重命名 (snap.auto_namable 字段)
                    auto_namable: snap.auto_namable,
                },
            );
        }
        let order = if f.order.is_empty() {
            f.tasks.iter().map(|t| t.id).collect()
        } else {
            f.order
        };
        // 优先恢复上次 active_main;不存在或已被删的 fallback 到 order 首
        let active_main = f
            .active_main
            .filter(|id| tasks.contains_key(id))
            .or_else(|| order.first().copied());
        Self {
            next_id: f
                .next_task_id
                .max(f.tasks.iter().map(|t| t.id + 1).max().unwrap_or(0)),
            tasks,
            order,
            active_main,
        }
    }
    fn snapshot(&self) -> TasksFile {
        let tasks: Vec<TaskSnapshot> = self
            .order
            .iter()
            .filter_map(|id| {
                self.tasks.get(id).map(|t| TaskSnapshot {
                    id: *id,
                    name: t.name.clone(),
                    cwd: t.cwd.clone(),
                    pinned: t.pinned,
                    last_terminal_ids: t.terminal_ids.clone(),
                    split_tree: t.split_tree.clone(),
                    worktree: t.worktree.clone(),
                    notify_muted: t.notify_muted,
                    auto_namable: t.auto_namable,
                })
            })
            .collect();
        TasksFile {
            schema_version: 1,
            next_task_id: self.next_id,
            tasks,
            order: self.order.clone(),
            active_main: self.active_main,
        }
    }
    fn list_dto(&self) -> Vec<TaskDto> {
        self.order
            .iter()
            .filter_map(|id| self.tasks.get(id).map(|t| self.runtime_to_dto(*id, t)))
            .collect()
    }
    fn runtime_to_dto(&self, id: TaskId, t: &TaskRuntime) -> TaskDto {
        TaskDto {
            id,
            name: t.name.clone(),
            cwd: t.cwd.clone(),
            pinned: t.pinned,
            status: t.aggregated_status(),
            terminal_ids: t.terminal_ids.clone(),
            location: t.location.clone(),
            split_tree: t.split_tree.clone(),
            worktree: t.worktree.clone(),
            agent_kind: t.agent_kind.clone(),
            // last_output 由 src-tauri 在 emit 前注入(需要 TerminalRegistry,核心层不持有)
            last_output: None,
            notify_muted: t.notify_muted,
            permission_mode: t.permission_mode.clone(),
            effort: t.effort.clone(),
        }
    }
}
