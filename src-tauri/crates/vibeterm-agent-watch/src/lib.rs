//! Agent 状态嗅探 — 纯被动文件监听, 不联网, 不写用户配置.
//!
//! 数据源:
//!   - Claude:
//!     - `~/.claude/usage_cache.json`  →  5h/7d quota (服务端给, 准确)
//!     - `~/.claude/projects/<sanitize(cwd)>/<sid>.jsonl`  →  context / model / cost
//!   - Codex:
//!     - `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`  →  全字段 (rate_limits 内联)
//!
//! 暴露统一的 `AgentSnapshot` enum, 由 IPC 层 emit 给前端.
//!
//! 设计:
//!   - watcher 独立 tokio task, 用 notify crate 监听 FS
//!   - 每次变更 debounce 100ms (避免 atomic write 触发多次)
//!   - 解析后通过 mpsc::UnboundedSender<AgentSnapshot> 推给主循环
//!   - 主循环维护 latest snapshot + emit IPC

use serde::{Deserialize, Serialize};

pub mod claude;
pub mod codex;
pub mod provider;
pub mod stats;

/// 上层订阅的统一事件 — Claude/Codex 任一更新都推一个这个.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "agent", rename_all = "lowercase")]
pub enum AgentSnapshot {
    Claude(ClaudeSnapshot),
    Codex(CodexSnapshot),
}

// ---- Claude ----

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClaudeSnapshot {
    /// 来自 usage_cache.json 的 5h / 7d quota — 服务端权威数据
    pub usage_cache: Option<UsageCache>,
    /// 来自当前活跃 session jsonl 的 context / model 信息 (v2 实现)
    pub session: Option<ClaudeSession>,
    /// 最后更新的 unix ms (snapshot 合成时刻)
    pub updated_at_ms: i64,
}

/// `~/.claude/usage_cache.json` 完整反序列化结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageCache {
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    /// Sonnet 独占 7d 配额 (付费/分级用户)
    pub seven_day_sonnet: Option<QuotaWindow>,
    /// Opus 独占 7d 配额
    pub seven_day_opus: Option<QuotaWindow>,
    pub seven_day_oauth_apps: Option<QuotaWindow>,
    pub extra_usage: Option<ExtraUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindow {
    /// 0..=100, 服务端给的已用百分比
    pub utilization: f64,
    /// ISO8601 (UTC); 注意 Claude 文件里可能是 null
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
    pub currency: Option<String>,
    pub disabled_reason: Option<String>,
}

/// Session 级信息 — 由 v2 的 project transcript watcher 填充
#[derive(Debug, Clone, Serialize, Default)]
pub struct ClaudeSession {
    pub session_id: String,
    pub project_path: String,
    pub model: Option<String>,
    /// 当前 context 使用量 (input + cache_read + cache_creation, 不含 output)
    pub context_tokens: Option<u64>,
    /// 模型上下文窗口上限 (根据 model_id 查表)
    pub context_window: Option<u64>,
    /// 累计 cost (USD)
    pub session_cost_usd: Option<f64>,
    /// Prompt cache 5min TTL 到期时刻 (unix ms) — None = 没用过 5min cache.
    /// Anthropic prompt cache 5m 跟 1h 是两个独立 TTL, 取最后一次 cache_creation
    /// 写入时刻 + TTL 长度作为到期时刻. 后续 cache_read 命中不刷新 TTL.
    pub cache_5m_until_ms: Option<i64>,
    /// Prompt cache 1h TTL 到期时刻 (unix ms) — None = 没用过 1h cache.
    pub cache_1h_until_ms: Option<i64>,
    /// 最新一次 hook 回传 (transcript 里的 attachment.response) 携带的 reasoning effort
    /// 等级 (`low` / `medium` / `high` / `xhigh` 等). Claude 仅在 hook 触发时把 effort
    /// 写进 transcript, 没 hook / 没取到 → None (widget 不显, 不臆造).
    pub effort: Option<String>,
}

// ---- Codex ----

#[derive(Debug, Clone, Serialize, Default)]
pub struct CodexSnapshot {
    pub session_id: String,
    pub cwd: String,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub cli_version: Option<String>,
    /// 当前 turn 的 total_tokens (跟 Codex CLI `tokens_in_context_window` 一致).
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// Context 占用百分比 — 严格按 Codex CLI 算法 (扣 BASELINE_TOKENS=12000).
    /// 公式: (total_tokens - 12000) / (context_window - 12000) * 100, clamp 0..100.
    /// 不算就 None (window 太小或缺数据). 前端 codex-ctx widget 直接用这个值,
    /// 不要自己 ratio, 否则跟 Codex CLI 显示对不上.
    pub context_used_pct: Option<f64>,
    /// rate_limits.primary — Codex 主配额 (window_minutes / used_percent / resets_at)
    pub primary_limit: Option<RateLimit>,
    /// rate_limits.secondary — 次配额 (一般是 5h 之类)
    pub secondary_limit: Option<RateLimit>,
    pub plan_type: Option<String>,
    pub updated_at_ms: i64,
    /// 最近几次 turn 的 tokens / min (用 token_count 事件时间戳算)
    pub tokens_per_min_recent: f64,
    /// "normal" / "moderate" / "high" — 跟 Claude 同阈值
    pub burn_rate_level: String,
    /// 最新 turn 的 reasoning effort (`xhigh` / `high` / `normal` / `low`)
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub used_percent: f64,
    /// 窗口长度 (分钟) — 10080 = 7 天, 300 = 5 小时.
    /// 上游 Codex 协议是 Option (free 计划 / 新模型常为 null), 必须容错:
    /// 否则 null 会让整条 token_count 反序列化失败, 连 context_window 一起丢。
    #[serde(default)]
    pub window_minutes: Option<u64>,
    /// unix seconds — 上游可能为 null → Option
    #[serde(default)]
    pub resets_at: Option<i64>,
}
