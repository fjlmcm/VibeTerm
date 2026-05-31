//! 状态嗅探
//!
//! 实现:
//!   - OSC 133 / OSC 633 序列解析(优先,准确率最高)
//!   - 通用进程行为推断(N 秒无输出 → idle)
//!   - 内置 agent stdout 规则(claude / codex / aider — strip ANSI 后匹配)
//!   - 16KB ring buffer 跨 chunk 正则
//!   - OSC 限速(每 terminal 每秒 ≤10 个事件,防伪造)
//!
//! 本 crate 输入:PTY chunk(由上层 fan-out 调用 StatusDetector::feed);
//!         输出:StatusChange { new_status }(由上层 emit IPC event)。
//! 不依赖 tauri / channel,纯算法。

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use vibeterm_ipc::TaskStatus;

pub mod agent;
pub use agent::{detect_agent_for_shell, detect_agent_with_diagnostics, AgentKind, Diagnostics};

const RING_SIZE: usize = 16 * 1024; // 16KB
const IDLE_TIMEOUT_MS: u64 = 800; // N 秒无输出 → idle
const OSC_RATE_LIMIT: u32 = 10; // 每秒最多 N 个 OSC 事件
const OSC_RATE_WINDOW_MS: u64 = 1000;
/// Stalled 默认阈值: agent 在 Idle 状态超过这个时间无任何输出 → Stalled
const DEFAULT_STALL_THRESHOLD_MS: u64 = 5 * 60 * 1000;

pub struct StatusDetector {
    current: TaskStatus,
    last_chunk_at: Instant,
    ring: Vec<u8>,
    osc_recent_count: u32,
    osc_window_start: Instant,
    agent_rules: Option<&'static AgentRules>,
    /// 跨 chunk OSC 序列缓存(上次 chunk 末尾的不完整 `\x1b]<id>;<kind>;<payload>` 片段)
    /// 上限 OSC_CARRY_MAX 字节,足够承载 cwd / commandline 等长 payload
    osc_carry: Vec<u8>,
    /// OSC 133/633 D 携带的退出码(最近一次 command 结束)
    last_exit_code: Option<i32>,
    /// OSC 633 E 携带的命令行文本(VSCode shell integration)
    last_command_line: Option<String>,
    /// OSC 633 P;Cwd= 携带的当前工作目录
    current_cwd: Option<String>,
    /// 当 current == Idle 时, 标记这个 Idle 是否由 OSC 133/633 D 触发(真完成)
    /// 而非 IDLE_TIMEOUT_MS 超时触发(可能是 ping/tail -f 这种长 streaming
    /// 命令的间歇期). 通知层用它判断要不要弹"任务完成", 避免误报.
    idle_finalized_by_osc: bool,
    /// Stalled 检测开关. 默认关; agent 嗅探层识别到 agent_kind 后开启.
    /// 普通 shell (ping/tail -f) 不开 — 它们的"长时间无输出"是正常的.
    stall_enabled: bool,
    stall_threshold_ms: u64,
    /// 本 detector 生命周期内是否曾 Running.
    /// 纯 idle 的终端 (新开窗口 + 无输入) 不应升 Stalled — "从来没活过" 不算 "卡住".
    has_been_running: bool,
    /// 最近一次用户按键 / 写入 PTY 的时刻.
    /// Stalled 真实语义是"用户发指令但 agent 没回应",而非"用户离席,agent 空闲".
    /// claude/codex 跑完任务停在 `>` 提示符长时间不动,不应被判为卡住.
    /// 升 Stalled 要求 last_user_input_at > last_chunk_at — 用户是最新事件.
    last_user_input_at: Option<Instant>,
    /// agent 在 OSC 0/2 窗口标题里编码工作态: 含 braille spinner(⠋⠙⠹…) = 正在思考/工作,
    /// 转静态(claude `✳` / codex 纯 cwd 名) = 本 turn 完成. claude/codex 官方行为
    /// (VS Code 也靠此识别 agent 终端). 这是 agent 自己发的语义信号, 比"输出时序"可靠.
    /// 本字段记录"上一次标题是否在 spin", 用于检测 spinner→静态 的 turn-done 边界.
    title_spinner: bool,
    /// 从 claude 工作动画 "thinking with <effort> effort" 嗅探到的 reasoning effort
    /// 等级(high/xhigh/max…). 零侵入、不依赖 hook(原 effort 靠 hook 写 transcript, 已删).
    last_effort: Option<String>,
}

/// carry 容纳一整条 OSC(含 cwd 长 path 等)的上限。
/// 大多数 payload < 256B;> 4KB 视为异常并丢弃。
const OSC_CARRY_MAX: usize = 4 * 1024;

/// 一组 stdout 模式(strip ANSI 后匹配)
struct AgentRules {
    waiting_input_patterns: &'static [&'static str],
    /// 编译后的 regex(OnceLock 保证只 build 一次)
    compiled: OnceLock<Vec<regex::Regex>>,
}

impl AgentRules {
    fn compiled_patterns(&self) -> &[regex::Regex] {
        self.compiled.get_or_init(|| {
            self.waiting_input_patterns
                .iter()
                .filter_map(|p| regex::Regex::new(p).ok())
                .collect()
        })
    }
}

// claude 真实授权 UI(2.x, 实测): 带框编号菜单
//   "Do you want to proceed?  ❯ 1. Yes / 2. Yes, and don't ask again / 3. No, and tell Claude…"
// 信任框(实测): "❯ 1. Yes, I trust this folder / 2. No, exit"。
//
// 校准(2026-05-30): 这些正则跑在 agent 终端的输出 ring 上, agent 正文很长, 必须用
// **菜单独有措辞**避免误命中(否则 claude 叙述里写 "do you want to proceed"/"(y/n)"
// 就会闪假黄灯)。故去掉泛词 "(y/n)"/"[y/n]"/"do you want to proceed":
//   - claude 2.x 菜单根本不用 (y/n);
//   - "No, and tell Claude" 是授权菜单选项 3, 必与每个授权框同现 → 留它即可全覆盖,
//     不需要再留会误命中的 "do you want to proceed"。
static CLAUDE_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"(?i)no, and tell claude",           // 授权菜单选项 3, 菜单独有, 最稳
        r"(?i)don't ask again",               // 授权菜单选项 2
        r"(?i)trust (the files|this folder)", // 首次进目录"信任此文件夹"(实测)
        r"(?i)continue with this plan\?",     // plan 模式确认
    ],
    compiled: OnceLock::new(),
};

// codex 命令审批 UI 未能实测(本机 codex 配置为自动放行命令, 抓不到审批框)。
// 保守:只认低误报的字面 y/n 形态; 去掉 "approve this"/"apply.*patch"(会在 codex
// 正文里误命中)。codex 菜单式审批的独有措辞待拿到真实样本再校准。
static CODEX_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[r"\(y/n\)", r"\[y/n\]"],
    compiled: OnceLock::new(),
};

static AIDER_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"(?i)yes/no",
        r"\(y\)",
        r"(?i)add .* to the chat\?",
        r"(?i)edit the files",
    ],
    compiled: OnceLock::new(),
};

// Gemini CLI — Google 官方, 多用 ? 结尾的英文确认 + y/n
static GEMINI_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"\(y/n\)",
        r"\[y/n\]",
        r"(?i)proceed\?",
        r"(?i)apply changes\?",
    ],
    compiled: OnceLock::new(),
};

// Cursor CLI / cursor-agent — Cursor 桌面端的 agent 模式
static CURSOR_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"\(y/n\)",
        r"\[y/n\]",
        r"(?i)accept\?",
        r"(?i)keep changes\?",
    ],
    compiled: OnceLock::new(),
};

// Cline — VSCode 插件 + 独立 CLI 形态
static CLINE_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"\(y/n\)",
        r"\[y/n\]",
        r"(?i)approve\?",
        r"(?i)proceed with",
    ],
    compiled: OnceLock::new(),
};

// OpenCode — 开源 Claude Code-like
static OPENCODE_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[r"\(y/n\)", r"\[y/n\]", r"(?i)yes/no", r"(?i)confirm"],
    compiled: OnceLock::new(),
};

// GitHub Copilot CLI / ghcs
static COPILOT_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"\(y/n\)",
        r"\[y/n\]",
        r"(?i)select an option",
        r"(?i)allow this command",
    ],
    compiled: OnceLock::new(),
};

// Kimi / Moonshot CLI
static KIMI_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[r"\(y/n\)", r"\[y/n\]", r"是否继续", r"是否同意"],
    compiled: OnceLock::new(),
};

// Droid (Factory.ai)
static DROID_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[
        r"\(y/n\)",
        r"\[y/n\]",
        r"(?i)approve plan",
        r"(?i)continue\?",
    ],
    compiled: OnceLock::new(),
};

// Amp (Sourcegraph)
static AMP_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[r"\(y/n\)", r"\[y/n\]", r"(?i)approve", r"(?i)proceed"],
    compiled: OnceLock::new(),
};

// Pi
static PI_RULES: AgentRules = AgentRules {
    waiting_input_patterns: &[r"\(y/n\)", r"\[y/n\]"],
    compiled: OnceLock::new(),
};

fn pick_rules(command: &str) -> Option<&'static AgentRules> {
    let basename = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .to_lowercase();
    match basename.as_str() {
        "claude" | "claude-code" => Some(&CLAUDE_RULES),
        "codex" => Some(&CODEX_RULES),
        "aider" => Some(&AIDER_RULES),
        "gemini" => Some(&GEMINI_RULES),
        "cursor" | "cursor-agent" => Some(&CURSOR_RULES),
        "cline" => Some(&CLINE_RULES),
        "opencode" | "open-code" => Some(&OPENCODE_RULES),
        "copilot" | "github-copilot" | "ghcs" => Some(&COPILOT_RULES),
        "kimi" => Some(&KIMI_RULES),
        "droid" => Some(&DROID_RULES),
        "amp" | "amp-local" => Some(&AMP_RULES),
        "pi" => Some(&PI_RULES),
        _ => None,
    }
}

impl StatusDetector {
    pub fn new(command: &str) -> Self {
        Self {
            current: TaskStatus::Idle,
            last_chunk_at: Instant::now(),
            ring: Vec::with_capacity(RING_SIZE),
            osc_recent_count: 0,
            osc_window_start: Instant::now(),
            agent_rules: pick_rules(command),
            osc_carry: Vec::new(),
            last_exit_code: None,
            last_command_line: None,
            current_cwd: None,
            idle_finalized_by_osc: false,
            stall_enabled: false,
            stall_threshold_ms: DEFAULT_STALL_THRESHOLD_MS,
            has_been_running: false,
            last_user_input_at: None,
            title_spinner: false,
            last_effort: None,
        }
    }

    /// 上层在 write_pty (用户键入 → PTY) 时调用. 标记用户当前正在等响应.
    /// 触发 Stalled 的必要条件之一: 此时间 > last_chunk_at.
    pub fn mark_user_input(&mut self) {
        self.last_user_input_at = Some(Instant::now());
    }

    /// 打开 Stalled 检测. 通常由 main.rs 的 agent 嗅探线程
    /// 在识别到 agent_kind 后调用. threshold_ms = 0 表示用默认值.
    pub fn enable_stall_detection(&mut self, threshold_ms: u64) {
        self.stall_enabled = true;
        if threshold_ms > 0 {
            self.stall_threshold_ms = threshold_ms;
        }
    }

    /// 关闭 Stalled 检测 (task 的 agent_kind 从 Some 变 None 时, 例如 agent 退出回 shell).
    /// 如果当前已是 Stalled, 同步转回 Idle.
    pub fn disable_stall_detection(&mut self) -> Option<TaskStatus> {
        self.stall_enabled = false;
        if self.current == TaskStatus::Stalled {
            self.current = TaskStatus::Idle;
            self.idle_finalized_by_osc = false;
            return Some(TaskStatus::Idle);
        }
        None
    }

    /// agent 检测线程识别到该终端在跑某 agent 后调用, 装上对应授权框正则,
    /// 让 body 正则识别"等你决策"(WaitingInput)。kind = AgentKind::as_str()
    /// ("claude"/"codex"/…); None = 非 agent(回纯 shell), 清空规则。
    ///
    /// 关键修复:detector 用 shell 命令("zsh")构造, agent_rules 恒 None →
    /// match_waiting 从不执行。agent 在 shell 内跑, 必须在这里按嗅探到的 kind 补装。
    pub fn set_agent_rules(&mut self, kind: Option<&str>) {
        self.agent_rules = kind.and_then(pick_rules);
    }

    pub fn current(&self) -> TaskStatus {
        self.current
    }

    /// 最近从 claude 工作动画嗅探到的 reasoning effort 等级(high/xhigh/max…). 无则 None.
    pub fn last_effort(&self) -> Option<&str> {
        self.last_effort.as_deref()
    }

    /// 最近一次 OSC 133/633;D 携带的退出码(无则 None)
    pub fn last_exit_code(&self) -> Option<i32> {
        self.last_exit_code
    }

    /// 最近一次 OSC 633;E 携带的命令行(VSCode shell integration)
    pub fn last_command_line(&self) -> Option<&str> {
        self.last_command_line.as_deref()
    }

    /// 最近一次 OSC 633;P;Cwd= 上报的工作目录
    pub fn current_cwd(&self) -> Option<&str> {
        self.current_cwd.as_deref()
    }

    /// 当前 Idle 状态是否由 OSC 133/633 D 触发(真完成),
    /// 而非 IDLE_TIMEOUT_MS 超时触发(可能误判).
    /// 仅在 current() == Idle 时有意义,其他状态返回 false.
    pub fn idle_by_osc(&self) -> bool {
        matches!(self.current, TaskStatus::Idle) && self.idle_finalized_by_osc
    }

    /// 喂入一个 chunk,返回新状态(若变化)。
    pub fn feed(&mut self, chunk: &[u8]) -> Option<TaskStatus> {
        if chunk.is_empty() {
            return self.tick();
        }
        self.last_chunk_at = Instant::now();
        let prev = self.current;

        // 通用规则:任何输出 → running(若不在 waiting_input)
        // Stalled 状态收到任何输出立即解除回 Running.
        if self.current != TaskStatus::WaitingInput {
            self.current = TaskStatus::Running;
            self.idle_finalized_by_osc = false;
            self.has_been_running = true;
        }

        // OSC 133/633 优先
        if self.parse_osc(chunk) {
            // OSC 已设置 state
        } else if let Some(rules) = self.agent_rules {
            // stdout 规则(append 到 ring,跑正则)
            self.append_ring(chunk);
            if self.match_waiting(rules) {
                self.current = TaskStatus::WaitingInput;
                self.idle_finalized_by_osc = false;
            }
        }

        // 嗅探 effort: claude 工作动画带 "thinking with <effort> effort"(零侵入).
        // **不**按 agent_rules 把门 —— detector 是用 shell 命令(zsh)创建的, agent_rules
        // 恒为 None(agent 在 shell 里跑), 否则此处永不执行(effort 抓不到的真因).
        // "thinking" 预检省开销; extract_effort 要求完整 "thinking with <word> effort",
        // 是 claude 独有字串, shell 输出几乎不会误命中。
        if contains_subslice(chunk, b"thinking") {
            if let Some(eff) = extract_effort(&strip_ansi(chunk)) {
                self.last_effort = Some(eff);
            }
        }

        if self.current != prev {
            Some(self.current)
        } else {
            None
        }
    }

    /// 周期性 tick(由上层定时调用) — 检测 idle 超时 + stalled 超时。
    pub fn tick(&mut self) -> Option<TaskStatus> {
        // 1) Running 持续 IDLE_TIMEOUT_MS 无输出 → Idle (原有逻辑)
        if self.current == TaskStatus::Running
            && self.last_chunk_at.elapsed() > Duration::from_millis(IDLE_TIMEOUT_MS)
        {
            self.current = TaskStatus::Idle;
            self.idle_finalized_by_osc = false;
            return Some(TaskStatus::Idle);
        }
        // 2) Idle 状态 + stall 检测开启 + 曾 Running + 用户发过输入且
        //    比最后一次 agent 输出更新 + 超过 stall 阈值 → Stalled.
        //    WaitingInput 不升 Stalled (已知在等用户).
        //    Done/Stalled 也不重复触发.
        //    四道守门 (按代价从低到高):
        //      a. stall_enabled — 用户主动开启 Stalled 通知 + 该 terminal 跑 agent
        //      b. current == Idle — 还没卡且 agent 也不在产出 (Running 期间不算卡)
        //      c. has_been_running — 新开窗口从未活过不算"卡住"
        //      d. user_input 是最新事件 — 排除"agent 任务完成后停在 prompt"误报.
        //         claude/codex 跑完任务等输入和 agent 真挂了表象相同, 只有用户
        //         实际提了问 agent 没回应才算"卡". 用户离席 → 自然不响.
        let user_recent = match self.last_user_input_at {
            Some(t) => t > self.last_chunk_at,
            None => false,
        };
        if self.stall_enabled
            && self.current == TaskStatus::Idle
            && self.has_been_running
            && user_recent
            && self.last_chunk_at.elapsed() > Duration::from_millis(self.stall_threshold_ms)
        {
            self.current = TaskStatus::Stalled;
            return Some(TaskStatus::Stalled);
        }
        None
    }

    fn append_ring(&mut self, chunk: &[u8]) {
        let stripped = strip_ansi(chunk);
        self.ring.extend_from_slice(&stripped);
        if self.ring.len() > RING_SIZE {
            let drain = self.ring.len() - RING_SIZE;
            self.ring.drain(0..drain);
        }
    }

    fn match_waiting(&self, rules: &AgentRules) -> bool {
        let text = String::from_utf8_lossy(&self.ring);
        let regs = rules.compiled_patterns();
        for re in regs {
            if re.is_match(&text) {
                return true;
            }
        }
        false
    }

    /// OSC 133/633 完整 parser
    /// 形如 `\x1b]<133|633>;<kind>[;<payload>]<ST>` ,ST = `\x1b\\` 或 BEL `\x07`。
    ///
    /// 在识别 type byte 之外,**捕获 payload**:
    ///   - `133;D[;<exit>]` / `633;D[;<exit>]` → `last_exit_code`
    ///   - `633;E;<commandline>` → `last_command_line`
    ///   - `633;P;Cwd=<path>` → `current_cwd`
    ///
    /// 支持跨 chunk 重组:本次未找到终结符的完整 OSC 头会原样进 `osc_carry`,
    /// 下次 chunk 头部拼接后再解析。
    /// 返回 true 若识别到合法 OSC 并应用了状态。
    fn parse_osc(&mut self, chunk: &[u8]) -> bool {
        // 窗口重置
        if self.osc_window_start.elapsed() > Duration::from_millis(OSC_RATE_WINDOW_MS) {
            self.osc_window_start = Instant::now();
            self.osc_recent_count = 0;
        }

        // Fast path:无 ESC 且 carry 空 → 不可能有 OSC
        if self.osc_carry.is_empty() && !chunk.contains(&0x1b) {
            return false;
        }

        // Slow path:拼接 carry + chunk
        let buf: std::borrow::Cow<'_, [u8]> = if self.osc_carry.is_empty() {
            std::borrow::Cow::Borrowed(chunk)
        } else {
            let mut v: Vec<u8> = Vec::with_capacity(self.osc_carry.len() + chunk.len());
            v.extend_from_slice(&self.osc_carry);
            v.extend_from_slice(chunk);
            self.osc_carry.clear();
            std::borrow::Cow::Owned(v)
        };
        let buf: &[u8] = &buf;

        let mut applied = false;
        let mut i = 0;
        let mut last_consumed = 0;
        let mut incomplete_start: Option<usize> = None;
        // 被限速时不向 carry 保存任何尾部 OSC, 否则超额那条会逃逸到下一窗口被处理.
        let mut rate_limited = false;

        while i < buf.len() {
            // 寻找 `\x1b]`(OSC introducer)
            if buf[i] != 0x1b {
                i += 1;
                continue;
            }
            if i + 1 >= buf.len() {
                incomplete_start = Some(i);
                break;
            }
            if buf[i + 1] != b']' {
                // 非 OSC 的 ESC(可能是 CSI),跳过
                i += 1;
                continue;
            }
            // OSC 0/1/2 = icon/window title. claude/codex 把工作态写进标题
            // (braille spinner=working / 静态=idle). 单独处理: 不计入 133/633 限速
            // (spinner ~10/s, 设 Running/Idle 幂等廉价), 且不计入 `applied`
            // (好让 feed 仍跑 body 正则, 静态标题+body 授权框 → WaitingInput).
            // `0;`/`1;`/`2;` 才是标题; `10;`/`11;`(颜色查询)/`133;`/`633;` 第 4 字节非 `;`, 不命中.
            if i + 3 < buf.len() && matches!(buf[i + 2], b'0' | b'1' | b'2') && buf[i + 3] == b';' {
                let payload_start = i + 4;
                match find_osc_terminator(&buf[payload_start..]) {
                    None => {
                        incomplete_start = Some(i);
                        break;
                    }
                    Some((rel_end, rel_after)) => {
                        let payload = &buf[payload_start..payload_start + rel_end];
                        self.apply_title(payload);
                        last_consumed = payload_start + rel_after;
                        i = last_consumed;
                        continue;
                    }
                }
            }
            // 必须有 "<id>;<kind>" — 至少再要 5 字节("133;X" 或 "633;X")
            if i + 6 >= buf.len() {
                incomplete_start = Some(i);
                break;
            }
            let id_field = &buf[i + 2..i + 6];
            let is_633 = id_field == b"633;";
            let is_133 = id_field == b"133;";
            if !is_633 && !is_133 {
                i += 1;
                continue;
            }
            // 限速:超过窗口阈值 → 停止本次解析(已识别的不撤销)
            if self.osc_recent_count >= OSC_RATE_LIMIT {
                rate_limited = true;
                break;
            }

            let kind_idx = i + 6;
            let kind_byte = buf[kind_idx];
            // payload 起点 = kind 之后(可能跟 `;` 分隔,也可能直接 ST)
            let payload_start = if kind_idx + 1 < buf.len() && buf[kind_idx + 1] == b';' {
                kind_idx + 2
            } else {
                kind_idx + 1
            };
            // 找终结符
            match find_osc_terminator(&buf[payload_start..]) {
                None => {
                    incomplete_start = Some(i);
                    break;
                }
                Some((rel_payload_end, rel_after)) => {
                    let payload = &buf[payload_start..payload_start + rel_payload_end];
                    self.osc_recent_count += 1;
                    if self.apply_osc(is_633, kind_byte, payload) {
                        applied = true;
                    }
                    last_consumed = payload_start + rel_after;
                    i = last_consumed;
                }
            }
        }

        // 不完整的 OSC 头存进 carry
        if let Some(start) = incomplete_start {
            let tail = &buf[start..];
            if tail.len() <= OSC_CARRY_MAX {
                self.osc_carry.extend_from_slice(tail);
            }
            // 超长 → 视为伪造/损坏,丢弃
        } else if !rate_limited && last_consumed < buf.len() {
            // 没有 in-progress OSC,但尾部可能有孤立 ESC ] 开头的下一条.
            // 被限速时跳过: 超额的 OSC 应彻底丢弃, 不留到下一窗口逃逸限速.
            if let Some(esc_pos) = buf[last_consumed..]
                .iter()
                .rposition(|&b| b == 0x1b)
                .map(|p| last_consumed + p)
            {
                let tail = &buf[esc_pos..];
                if tail.len() <= OSC_CARRY_MAX && tail.len() >= 2 && tail.get(1) == Some(&b']') {
                    self.osc_carry.extend_from_slice(tail);
                }
            }
        }

        applied
    }

    /// 应用解析出的 OSC payload 到状态字段。返回 true 若 kind 已识别。
    fn apply_osc(&mut self, is_633: bool, kind: u8, payload: &[u8]) -> bool {
        match kind {
            b'A' | b'B' => {
                // FTCS 133;A=prompt 开始 / B=prompt 结束: shell 就绪、在 prompt 等输入。
                // 这是"空闲就绪"态(= Idle),不是"agent 在问你"(= WaitingInput, 黄/呼吸,
                // 最高注意力)。普通 shell 坐在 prompt 上 → 应灰(Idle), 不该恒黄。
                // 真正的"需要你决策"(授权框 y/n)由 body 正则 match_waiting 高置信识别,
                // 不依赖这里。详见状态模型说明。
                self.current = TaskStatus::Idle;
                self.idle_finalized_by_osc = true;
                true
            }
            b'C' => {
                self.current = TaskStatus::Running;
                self.idle_finalized_by_osc = false;
                // 状态转出 WaitingInput: 清空 ring, 否则旧的 `(y/n)` 等残留文本
                // 会在下一个普通输出 chunk 触发 match_waiting, 把状态误弹回 WaitingInput.
                self.ring.clear();
                true
            }
            b'D' => {
                self.current = TaskStatus::Idle;
                self.idle_finalized_by_osc = true;
                // 同上: 命令结束转 Idle 时清空 ring, 防残留确认提示误命中.
                self.ring.clear();
                // payload 形如 `0` 或 `0;...`,取首字段为退出码
                if !payload.is_empty() {
                    if let Ok(s) = std::str::from_utf8(payload) {
                        let first = s.split(';').next().unwrap_or("").trim();
                        if let Ok(code) = first.parse::<i32>() {
                            self.last_exit_code = Some(code);
                        }
                    }
                }
                true
            }
            b'E' if is_633 => {
                // 633;E;<commandline>
                if let Ok(s) = std::str::from_utf8(payload) {
                    self.last_command_line = Some(s.to_owned());
                }
                true
            }
            b'P' if is_633 => {
                // 633;P;Cwd=<path>(VSCode 还有 IsWindows= 等其他键,只关心 Cwd)
                if let Ok(s) = std::str::from_utf8(payload) {
                    if let Some(path) = s.strip_prefix("Cwd=") {
                        self.current_cwd = Some(path.to_owned());
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// 应用 OSC 0/1/2 窗口标题。claude/codex 把工作态编码进标题字形:
    ///   - 含 braille spinner(⠋⠙⠹… U+2800–U+28FF) → 正在工作 → Running(并清 ring 防残留
    ///     上一轮确认提示误命中)。
    ///   - 不含 braille 且**上次在 spin** → spinner 刚停 → 本 turn 结束 → Idle(标 finalized,
    ///     等同 OSC D 的"真完成", 供通知层用)。同 chunk 若 body 有授权框, feed 的 body 正则
    ///     会把 Idle 覆盖成 WaitingInput(故本函数不计入 `applied`)。
    ///   - 不含 braille 且本就没在 spin → 普通标题变更, 不动状态。
    fn apply_title(&mut self, payload: &[u8]) {
        if contains_braille(payload) {
            self.current = TaskStatus::Running;
            self.idle_finalized_by_osc = false;
            self.has_been_running = true;
            self.title_spinner = true;
            self.ring.clear();
        } else if self.title_spinner {
            self.current = TaskStatus::Idle;
            self.idle_finalized_by_osc = true;
            self.title_spinner = false;
        }
    }
}

/// 标题里是否含 braille 字符(U+2800–U+28FF)。agent 的 spinner 动画帧都在此区间,
/// 出现即表示"正在工作"。UTF-8 编码为 `E2 A0..A3 80..BF`,判前两字节即可。
fn contains_braille(payload: &[u8]) -> bool {
    payload
        .windows(2)
        .any(|w| w[0] == 0xE2 && (0xA0..=0xA3).contains(&w[1]))
}

/// 子串包含(廉价 pre-check, 避免对每个 chunk 都 strip_ansi)。
fn contains_subslice(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}

/// 从 claude 工作动画 "thinking with <effort> effort" 抠 reasoning effort 等级。
/// 例: "✶ Working… (4s · ↓82 tokens · thinking with xhigh effort)" → "xhigh"。
/// 零侵入(PTY 输出里就有), 不依赖 hook 写 transcript。
fn extract_effort(stripped: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(stripped).ok()?;
    let start = s.find("thinking with ")? + "thinking with ".len();
    let word: String = s[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (!word.is_empty()).then_some(word)
}

/// 在 OSC payload 后扫描终结符。返回 `(payload_end_relative, after_terminator_relative)`,
/// 或 None 若 buf 内未找到。
/// 终结符: BEL(`\x07`)或 ST(`\x1b\\`)。
fn find_osc_terminator(s: &[u8]) -> Option<(usize, usize)> {
    let mut j = 0;
    while j < s.len() {
        if s[j] == 0x07 {
            return Some((j, j + 1));
        }
        if s[j] == 0x1b && j + 1 < s.len() && s[j + 1] == b'\\' {
            return Some((j, j + 2));
        }
        j += 1;
    }
    None
}

/// 极简 ANSI/CSI 序列剥离
/// 只处理常见 CSI `\e[...m` 与 OSC `\e]...BEL` — 不求完整,够 stdout 规则匹配即可
fn strip_ansi(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0x1b && i + 1 < input.len() {
            match input[i + 1] {
                b'[' => {
                    // CSI 直到字母
                    let mut j = i + 2;
                    while j < input.len() && !(0x40..=0x7e).contains(&input[j]) {
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
                b']' => {
                    // OSC 直到 BEL 或 \e\\
                    let mut j = i + 2;
                    while j < input.len() && input[j] != 0x07 {
                        if input[j] == 0x1b && j + 1 < input.len() && input[j + 1] == b'\\' {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
                _ => {
                    i += 2;
                    continue;
                }
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_133_finished_sets_idle() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"hello");
        assert_eq!(d.current(), TaskStatus::Running);
        let _ = d.feed(b"\x1b]133;D;0\x1b\\");
        assert_eq!(d.current(), TaskStatus::Idle);
    }

    #[test]
    fn osc_133_prompt_ready_is_idle_not_waiting() {
        // FTCS 133;A=prompt 就绪 应判 Idle(灰), 不能是 WaitingInput(黄)。
        // shell 集成注入后每个 prompt 都发 D+A, 空 shell 不该恒黄。
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\");
        assert_eq!(d.current(), TaskStatus::Idle);
        assert!(d.idle_by_osc(), "prompt-ready 应是 OSC 确认的 Idle");
    }

    #[test]
    fn claude_trust_folder_detected() {
        // 实测首次进目录的"信任此文件夹"框
        let mut d = StatusDetector::new("claude");
        let _ = d.feed(b"1. Yes, I trust this folder");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn claude_prose_does_not_false_trigger_yellow() {
        // 校准后:agent 正文里的泛化确认语 / (y/n) 不再误触发黄灯。
        // (只认菜单独有措辞 → 减少 agent 工作期间的假 WaitingInput)
        let mut d = StatusDetector::new("claude");
        let _ = d.feed(b"I'll proceed. Do you want to proceed with option A? (y/n)");
        assert_eq!(
            d.current(),
            TaskStatus::Running,
            "泛化措辞不应误判为 WaitingInput"
        );
    }

    #[test]
    fn gemini_proceed_detected() {
        let mut d = StatusDetector::new("gemini");
        let _ = d.feed(b"Apply changes? ");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn cursor_accept_detected() {
        let mut d = StatusDetector::new("cursor-agent");
        let _ = d.feed(b"Keep changes? ");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn cline_approve_detected() {
        let mut d = StatusDetector::new("cline");
        let _ = d.feed(b"Approve? [y/n]");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn opencode_confirm_detected() {
        let mut d = StatusDetector::new("opencode");
        let _ = d.feed(b"Please confirm before continuing");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn copilot_allow_detected() {
        let mut d = StatusDetector::new("ghcs");
        let _ = d.feed(b"Allow this command to run?");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn kimi_chinese_prompt_detected() {
        // Kimi 是 Moonshot 国产 agent, 中文 prompt 是常态
        let mut d = StatusDetector::new("kimi");
        let _ = d.feed("是否继续执行?".as_bytes());
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn droid_continue_detected() {
        let mut d = StatusDetector::new("droid");
        let _ = d.feed(b"Continue? (y/n)");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn amp_approve_detected() {
        let mut d = StatusDetector::new("amp");
        let _ = d.feed(b"Approve and continue");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn unknown_command_no_rules() {
        // 普通 shell 不应被 agent 规则误命中
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"Do you want to proceed? (y/n)");
        // 没有 agent 规则,只有 idle/running 状态机, 当前应是 Running 而非 WaitingInput
        assert_eq!(d.current(), TaskStatus::Running);
    }

    #[test]
    fn pick_rules_handles_path_and_case() {
        // basename + lowercase 处理
        assert!(pick_rules("/usr/local/bin/claude").is_some());
        assert!(pick_rules("CLAUDE-CODE").is_some());
        assert!(pick_rules("cursor-agent").is_some());
        assert!(pick_rules("vim").is_none());
    }

    #[test]
    fn idle_after_silence() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"x");
        assert_eq!(d.current(), TaskStatus::Running);
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 50));
        assert_eq!(d.tick(), Some(TaskStatus::Idle));
    }

    // Stalled 状态机
    #[test]
    fn stall_not_triggered_when_disabled() {
        // 默认 stall_enabled=false (普通 shell), 久不输出也不升 Stalled
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"hello");
        d.mark_user_input();
        // 模拟超过 idle timeout
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 50));
        let _ = d.tick();
        assert_eq!(d.current(), TaskStatus::Idle);
        // 把 last_chunk_at 拉远 (借助小阈值)
        d.enable_stall_detection(50);
        d.mark_user_input(); // 用户提了问 + 没回应 → 才能升 Stalled
                             // 阈值开启前 last_chunk_at 已经 > 50ms, 立刻 tick 应升 Stalled
        std::thread::sleep(std::time::Duration::from_millis(60));
        let r = d.tick();
        assert_eq!(r, Some(TaskStatus::Stalled));
    }

    #[test]
    fn stall_cleared_by_new_output() {
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        let _ = d.feed(b"thinking...");
        d.mark_user_input(); // 模拟用户后续提问 (last_user_input > last_chunk_at)
                             // 必须等过 IDLE_TIMEOUT_MS (800ms) 才能让 Running -> Idle, 再让 Idle -> Stalled
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 60));
        let _ = d.tick(); // Running -> Idle
        let _ = d.tick(); // Idle -> Stalled (此时 elapsed 已 > 50ms 远超 stall 阈值)
        assert_eq!(d.current(), TaskStatus::Stalled);
        // 任何新输出 -> Running
        let r = d.feed(b"continuing");
        assert_eq!(r, Some(TaskStatus::Running));
        assert_eq!(d.current(), TaskStatus::Running);
    }

    #[test]
    fn stall_disabled_restores_idle() {
        // disable_stall_detection 在 Stalled 状态时应转回 Idle
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        let _ = d.feed(b"x");
        d.mark_user_input();
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 60));
        let _ = d.tick(); // -> Idle
        let _ = d.tick(); // -> Stalled
        assert_eq!(d.current(), TaskStatus::Stalled);
        let r = d.disable_stall_detection();
        assert_eq!(r, Some(TaskStatus::Idle));
        assert_eq!(d.current(), TaskStatus::Idle);
    }

    // 修复"纯 idle 终端误报 Stalled".
    // 新开窗口 + 无输入 → 从未 Running → 不应升 Stalled.
    #[test]
    fn never_running_never_stalls() {
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        // 从不 feed 任何输出 — has_been_running 始终 false
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 100));
        let r1 = d.tick();
        let r2 = d.tick();
        assert_eq!(
            r1, None,
            "tick should not promote never-running detector to Stalled"
        );
        assert_eq!(r2, None);
        assert_eq!(d.current(), TaskStatus::Idle);
    }

    // 核心场景 — claude/codex 跑完任务后等输入 (没用户操作) 不应升 Stalled.
    // 没有 mark_user_input 调用 = 用户没提任何问 = "去喝咖啡了" = 不是卡住.
    #[test]
    fn agent_finished_idle_without_user_input_never_stalls() {
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        // 模拟 agent 跑完任务输出
        let _ = d.feed(b"Done. Result: ...");
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 60));
        let _ = d.tick(); // Running -> Idle
        assert_eq!(d.current(), TaskStatus::Idle);
        // 用户从不提问. 远超 stall_threshold 也不升 Stalled.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let r = d.tick();
        assert_eq!(r, None, "agent finished + 用户未提问 不算 Stalled");
        assert_eq!(d.current(), TaskStatus::Idle);
    }

    // 核心场景 — 用户提问后 agent 沉默 → 才是真卡住.
    #[test]
    fn stall_fires_only_when_user_waited_after_silence() {
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        // agent 跑完任务输出, 用户读到 prompt
        let _ = d.feed(b"Done.");
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 60));
        let _ = d.tick(); // -> Idle
                          // 用户提了新问题, agent 没回应任何输出.
        d.mark_user_input();
        std::thread::sleep(std::time::Duration::from_millis(80)); // 远超 50ms 阈值
        let r = d.tick();
        assert_eq!(
            r,
            Some(TaskStatus::Stalled),
            "用户已提问但 agent 沉默 → Stalled"
        );
    }

    // agent echo 用户输入后停止 → 视为已响应, 不升 Stalled (保守策略).
    #[test]
    fn user_input_followed_by_agent_echo_does_not_stall() {
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        let _ = d.feed(b"Done.");
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 60));
        let _ = d.tick(); // -> Idle
                          // 用户提问, agent echo 回显 (=输出, last_chunk_at 又更新)
        d.mark_user_input();
        let _ = d.feed(b"user-message-echo");
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 60));
        let _ = d.tick(); // Running -> Idle
                          // 此时 last_user_input < last_chunk_at, 不再视为"用户在等"
        std::thread::sleep(std::time::Duration::from_millis(80));
        let r = d.tick();
        assert_eq!(
            r, None,
            "agent 有 echo 输出 → 已响应, 即便后续沉默也不升 Stalled"
        );
    }

    #[test]
    fn waiting_input_not_overridden_by_stall() {
        // WaitingInput 不应被 Stalled 覆盖 (已知在等用户, 不是卡)
        let mut d = StatusDetector::new("claude");
        d.enable_stall_detection(50);
        let _ = d.feed(b"3. No, and tell Claude what to do differently");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let r = d.tick();
        assert_eq!(r, None, "tick should not promote WaitingInput to Stalled");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    #[test]
    fn strip_csi() {
        let s = strip_ansi(b"\x1b[31mhello\x1b[0m world");
        assert_eq!(s, b"hello world");
    }

    /// OSC 限速 — 1 秒内 >10 个事件 应被丢弃,不影响 current_status
    #[test]
    fn osc_rate_limit_drops_excess() {
        let mut d = StatusDetector::new("zsh");
        // 一次喂 30 个 OSC 133;D(idle)在同一窗口内
        let mut buf = Vec::with_capacity(30 * 8);
        for _ in 0..30 {
            buf.extend_from_slice(b"\x1b]133;D\x1b\\");
        }
        let _ = d.feed(&buf);
        // 状态仍合法(最后一次 D 在第 11 次时被限速拒绝;但前 10 次合法已落地 idle)
        assert_eq!(d.current(), TaskStatus::Idle);
        // 关键断言:计数器封顶在限制值,未无限增长
        // (访问私有字段;使用 cfg(test) 内部已可见)
        assert!(d.osc_recent_count <= OSC_RATE_LIMIT);
    }

    /// idle_by_osc: OSC D 触发的 Idle 标记 true
    #[test]
    fn idle_by_osc_true_when_d_triggered() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"output");
        let _ = d.feed(b"\x1b]133;D;0\x1b\\");
        assert_eq!(d.current(), TaskStatus::Idle);
        assert!(d.idle_by_osc(), "OSC D 触发的 Idle 应标记 by_osc=true");
    }

    /// idle_by_osc: timeout 触发的 Idle 标记 false(ping 误判防护)
    #[test]
    fn idle_by_osc_false_when_timeout_triggered() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"ping output");
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 50));
        assert_eq!(d.tick(), Some(TaskStatus::Idle));
        assert!(!d.idle_by_osc(), "timeout 触发的 Idle 应标记 by_osc=false");
    }

    /// idle_by_osc: 转 Running 后再 timeout 不应继承上次的 osc 标记
    #[test]
    fn idle_by_osc_clears_on_new_command() {
        let mut d = StatusDetector::new("zsh");
        // 先 OSC D → Idle by_osc=true
        let _ = d.feed(b"\x1b]133;D;0\x1b\\");
        assert!(d.idle_by_osc());
        // 新 chunk → Running, 清掉
        let _ = d.feed(b"more output");
        assert_eq!(d.current(), TaskStatus::Running);
        // timeout → Idle 应该 by_osc=false
        std::thread::sleep(std::time::Duration::from_millis(IDLE_TIMEOUT_MS + 50));
        let _ = d.tick();
        assert!(!d.idle_by_osc(), "新命令后 timeout 不应继承上次 OSC D 标记");
    }

    /// OSC 133;D 退出码 payload 被捕获
    #[test]
    fn osc_133_d_captures_exit_code() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]133;D;42\x1b\\");
        assert_eq!(d.current(), TaskStatus::Idle);
        assert_eq!(d.last_exit_code(), Some(42));
    }

    /// OSC 133;D 无 payload 也合法,不污染 exit_code
    #[test]
    fn osc_133_d_without_payload_leaves_exit_code_none() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]133;D\x1b\\");
        assert_eq!(d.current(), TaskStatus::Idle);
        assert_eq!(d.last_exit_code(), None);
    }

    /// OSC 633;P;Cwd= 捕获 cwd
    #[test]
    fn osc_633_p_cwd_captured() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]633;P;Cwd=/Users/mt/dev2/VibeTerm\x1b\\");
        assert_eq!(d.current_cwd(), Some("/Users/mt/dev2/VibeTerm"));
    }

    /// OSC 633;E 捕获命令行
    #[test]
    fn osc_633_e_captures_commandline() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]633;E;ls -la /tmp\x1b\\");
        assert_eq!(d.last_command_line(), Some("ls -la /tmp"));
    }

    /// BEL 终结符也支持(替代 ST)
    #[test]
    fn osc_terminated_by_bel() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]133;D;7\x07");
        assert_eq!(d.current(), TaskStatus::Idle);
        assert_eq!(d.last_exit_code(), Some(7));
    }

    /// 跨 chunk 携带 cwd payload(carry 容量足够)
    #[test]
    fn osc_633_cwd_split_across_chunks() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]633;P;Cwd=/Users/mt/");
        assert_eq!(d.current_cwd(), None, "未见 ST,不应过早 commit");
        let _ = d.feed(b"workspace\x1b\\");
        assert_eq!(d.current_cwd(), Some("/Users/mt/workspace"));
    }

    /// 正常输出后跟 OSC,exit_code 仅被 D kind 触发
    #[test]
    fn osc_133_c_does_not_set_exit_code() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"\x1b]133;C\x1b\\");
        assert_eq!(d.current(), TaskStatus::Running);
        assert_eq!(d.last_exit_code(), None);
    }

    // ---- OSC 0/2 标题 spinner 状态信号(claude/codex 实测) ----

    #[test]
    fn braille_detection() {
        assert!(contains_braille("⠋ Claude Code".as_bytes()));
        assert!(contains_braille("⠹ vt-capture".as_bytes()));
        assert!(!contains_braille("✳ Claude Code".as_bytes())); // ✳ 不是 braille
        assert!(!contains_braille(b"vt-capture"));
    }

    /// claude 工作中标题(braille spinner)→ Running
    #[test]
    fn claude_title_spinner_sets_running() {
        let mut d = StatusDetector::new("claude");
        let _ = d.feed(b"\x1b]0;\xe2\xa0\x8b Claude Code\x07"); // "⠋ Claude Code"
        assert_eq!(d.current(), TaskStatus::Running);
    }

    /// spinner→静态(✳)且无 body 授权框 → 精确 turn-done(Idle + finalized)
    #[test]
    fn claude_title_spinner_stop_is_idle_done() {
        let mut d = StatusDetector::new("claude");
        let _ = d.feed(b"\x1b]0;\xe2\xa0\x8b Claude Code\x07"); // working
        assert_eq!(d.current(), TaskStatus::Running);
        let _ = d.feed(b"\x1b]0;\xe2\x9c\xb3 Claude Code\x07"); // "✳ Claude Code" 静态
        assert_eq!(d.current(), TaskStatus::Idle);
        assert!(d.idle_by_osc(), "标题 spinner→静态 应判为真完成(供通知)");
    }

    /// spinner 停 + body 同时有授权框 → WaitingInput 覆盖 Idle(不能误判完成)
    #[test]
    fn claude_title_stop_with_permission_body_is_waiting() {
        let mut d = StatusDetector::new("claude");
        let _ = d.feed(b"\x1b]0;\xe2\xa0\x8b Claude Code\x07"); // working
        let _ = d
            .feed(b"No, and tell Claude what to do differently\x1b]0;\xe2\x9c\xb3 Claude Code\x07");
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    /// claude 真实授权菜单措辞("No, and tell Claude…")命中
    #[test]
    fn claude_permission_menu_detected() {
        let mut d = StatusDetector::new("claude");
        let _ = d.feed("❯ 1. Yes\n  3. No, and tell Claude what to do differently".as_bytes());
        assert_eq!(d.current(), TaskStatus::WaitingInput);
    }

    /// 从 claude 工作动画嗅探 effort
    #[test]
    fn effort_sniffed_from_spinner() {
        assert_eq!(
            extract_effort("✶ Working… (4s · ↓82 tokens · thinking with xhigh effort)".as_bytes()),
            Some("xhigh".to_string())
        );
        assert_eq!(
            extract_effort(b"thinking with high effort"),
            Some("high".to_string())
        );
        assert_eq!(extract_effort(b"Working..."), None);

        // detector 用 shell 命令创建(agent 在 shell 里跑), 仍要能嗅探到 effort —
        // 这正是不按 agent_rules 把门的原因。
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed("✻ Cogitating (2s · thinking with max effort)".as_bytes());
        assert_eq!(d.last_effort(), Some("max"));
    }

    /// codex 标题 spinner → Running, 转静态(纯 cwd 名) → Idle
    #[test]
    fn codex_title_spinner_running_then_idle() {
        let mut d = StatusDetector::new("codex");
        let _ = d.feed(b"\x1b]0;\xe2\xa0\x8b vt-capture\x07"); // "⠋ vt-capture"
        assert_eq!(d.current(), TaskStatus::Running);
        let _ = d.feed(b"\x1b]0;vt-capture\x07"); // 静态 = idle
        assert_eq!(d.current(), TaskStatus::Idle);
    }

    /// 普通(非 spin)标题变更不应把 Running 弄成 Idle
    #[test]
    fn static_title_without_prior_spin_no_change() {
        let mut d = StatusDetector::new("zsh");
        let _ = d.feed(b"output");
        assert_eq!(d.current(), TaskStatus::Running);
        let _ = d.feed(b"\x1b]0;my project\x07");
        assert_eq!(d.current(), TaskStatus::Running);
    }

    /// codex 的 OSC 10;?/11;?(颜色查询)不应被误当标题, 不影响状态
    #[test]
    fn osc_color_query_ignored() {
        let mut d = StatusDetector::new("codex");
        let _ = d.feed(b"\x1b]0;\xe2\xa0\x8b x\x07"); // working
        let _ = d.feed(b"\x1b]10;?\x07\x1b]11;?\x07"); // 颜色查询
        assert_eq!(d.current(), TaskStatus::Running, "颜色查询不该改状态");
    }

    /// 窗口重置后,新一秒可以再接 10 个
    #[test]
    fn osc_rate_limit_resets_after_window() {
        let mut d = StatusDetector::new("zsh");
        let mut buf = Vec::new();
        for _ in 0..10 {
            buf.extend_from_slice(b"\x1b]133;D\x1b\\");
        }
        let _ = d.feed(&buf);
        assert_eq!(d.osc_recent_count, 10);

        std::thread::sleep(std::time::Duration::from_millis(OSC_RATE_WINDOW_MS + 50));
        // 下次 feed 触发窗口重置
        let _ = d.feed(b"\x1b]133;C\x1b\\");
        assert_eq!(d.current(), TaskStatus::Running);
        assert_eq!(d.osc_recent_count, 1);
    }
}
