//! 历史使用量聚合 — 跨会话扫描 transcript / rollout, 产出按天 / 按模型 / 按项目的统计.
//!
//! 用于"使用统计面板". 与状态栏的"当前活跃 session"快照不同, 这里做**全量历史聚合**:
//!   - Claude: 遍历 `~/.claude/projects/<dir>/*.jsonl` 全部 assistant 行,
//!     按 `(message.id, requestId)` 去重 (避免 resume / fork 重复计数),
//!     用离线定价表 `claude::pricing::cost_of` 估算成本.
//!   - Codex: 遍历 `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` 的 token_count 事件,
//!     仅有 token 无成本 (Codex 无公开价格表).
//!
//! **零侵入**: 全程只读, 不写 `~/.claude` / `~/.codex`. 核心是纯函数
//! [`aggregate_from`] (接受根目录路径), 单测指向 tempdir, 绝不碰用户真实数据.
//!
//! 定价 / 块算法移植自 ccusage (<https://github.com/ryoppippi/ccusage>, MIT,
//! Copyright (c) 2025 ryoppippi). 见 `claude::pricing` / `claude::blocks` 头注释.

mod claude_scan;
mod codex_scan;

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

use crate::claude::pricing::Usage;

/// 单条用量记录 — claude / codex 扫描的中间产物, 聚合前的统一形态.
pub(crate) struct Entry {
    pub ts_ms: i64,
    pub is_claude: bool,
    pub model: Option<String>,
    pub project_path: Option<String>,
    /// Claude 的 token 细分 (定价用); Codex 全 0 (不参与成本).
    pub usage: Usage,
    /// 本条总 token (Claude: input+cache_creation+cache_read+output; Codex: input+cached+output+reasoning).
    pub tokens: u64,
    /// 估算成本 (USD). Claude 模型匹配定价表则 Some, 否则 None; Codex 恒 None.
    pub cost: Option<f64>,
    /// 去重键 `(message_id, request_id)`. None = 无 id (旧格式 / Codex), 不去重.
    pub dedup_key: Option<(String, String)>,
}

/// 聚合总览.
#[derive(Debug, Clone, Serialize, Default, specta::Type)]
pub struct UsageStats {
    /// 统计窗口 (天).
    pub range_days: u32,
    /// 生成时刻 unix ms.
    pub generated_at_ms: i64,
    pub totals: Totals,
    /// 按本地日期升序.
    pub daily: Vec<DailyStat>,
    /// 按 token 降序.
    pub by_model: Vec<ModelStat>,
    /// 按 token 降序.
    pub by_project: Vec<ProjectStat>,
}

#[derive(Debug, Clone, Serialize, Default, specta::Type)]
pub struct Totals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    /// Claude 总 token.
    pub claude_tokens: u64,
    /// Codex 总 token.
    pub codex_tokens: u64,
    /// Claude 估算总成本 (USD). 无任何可定价条目则 None.
    pub cost_usd: Option<f64>,
    /// 模型未匹配定价表的 Claude 条目数 — UI 据此提示"成本含 N 条未计价".
    pub cost_unknown_entries: u64,
    /// 计入的 Claude 消息数 (去重后).
    pub message_count: u64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct DailyStat {
    /// 本地时区 `YYYY-MM-DD`.
    pub date: String,
    pub claude_tokens: u64,
    pub codex_tokens: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ModelStat {
    pub model: String,
    pub total_tokens: u64,
    /// 该模型估算成本; 无定价 (Codex / 未知模型) 则 None.
    pub cost_usd: Option<f64>,
    pub message_count: u64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ProjectStat {
    pub project_path: String,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
    pub message_count: u64,
}

/// unix ms → 本地时区 `YYYY-MM-DD` (用户日历的"天", 非 UTC).
fn local_date(ts_ms: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ts_ms).single() {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => "????-??-??".to_string(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

// 聚合中每个桶的可变累加器 — cost 用 (sum, priced_count) 表达"有无可定价条目".
#[derive(Default)]
struct CostAcc {
    sum: f64,
    priced: u64,
}
impl CostAcc {
    fn add(&mut self, cost: Option<f64>) {
        if let Some(c) = cost {
            self.sum += c;
            self.priced += 1;
        }
    }
    /// 有可定价条目 → Some(总和); 否则 None (区分"$0" 与"无定价数据").
    fn finish(&self) -> Option<f64> {
        if self.priced > 0 {
            Some(self.sum)
        } else {
            None
        }
    }
}

/// 把 per-item 的解析并行到多核 (按 CPU 数, 上限 8), 每项产出 Vec<R> 合并返回.
/// 文件扫描天然 per-file 可并行 —— 重度用户 3GB+ transcript 单线程要 6-7s.
pub(crate) fn par_collect<T, R>(items: Vec<T>, f: impl Fn(T) -> Vec<R> + Sync) -> Vec<R>
where
    T: Send,
    R: Send,
{
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    if n <= 1 || items.len() <= 1 {
        return items.into_iter().flat_map(f).collect();
    }
    let mut chunks: Vec<Vec<T>> = (0..n).map(|_| Vec::new()).collect();
    for (i, it) in items.into_iter().enumerate() {
        chunks[i % n].push(it);
    }
    let f = &f;
    let mut out: Vec<R> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                s.spawn(move || {
                    let mut local: Vec<R> = Vec::new();
                    for it in chunk {
                        local.extend(f(it));
                    }
                    local
                })
            })
            .collect();
        for h in handles {
            if let Ok(v) = h.join() {
                out.extend(v);
            }
        }
    });
    out
}

/// 纯函数: 给定 Claude projects 根 + Codex sessions 根 + 起始时刻, 产出聚合.
/// **只读**; 单测指向 tempdir.
pub fn aggregate_from(projects_root: &Path, codex_root: &Path, since_ms: i64) -> UsageStats {
    let mut entries: Vec<Entry> = Vec::new();
    claude_scan::scan(projects_root, since_ms, &mut entries);
    codex_scan::scan(codex_root, since_ms, &mut entries);
    aggregate_entries(&entries, since_ms)
}

fn aggregate_entries(entries: &[Entry], since_ms: i64) -> UsageStats {
    use std::collections::{HashMap, HashSet};

    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut totals = Totals::default();
    let mut total_cost = CostAcc::default();

    // date -> (claude_tokens, codex_tokens, cost)
    let mut daily: HashMap<String, (u64, u64, CostAcc)> = HashMap::new();
    // model -> (tokens, cost, count)
    let mut by_model: HashMap<String, (u64, CostAcc, u64)> = HashMap::new();
    // project -> (tokens, cost, count)
    let mut by_project: HashMap<String, (u64, CostAcc, u64)> = HashMap::new();

    for e in entries {
        // 缓存可能覆盖更大范围 (MAX_RANGE_DAYS), 这里按请求的 since 过滤.
        if e.ts_ms < since_ms {
            continue;
        }
        // Claude 按 (message_id, request_id) 去重; 无键 (Codex / 旧格式) 不去重.
        if let Some(key) = &e.dedup_key {
            if !seen.insert(key.clone()) {
                continue;
            }
        }

        let date = local_date(e.ts_ms);
        let model = e.model.clone().unwrap_or_else(|| "unknown".to_string());
        let project = e
            .project_path
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let d = daily
            .entry(date)
            .or_insert_with(|| (0, 0, CostAcc::default()));
        let m = by_model
            .entry(model)
            .or_insert_with(|| (0, CostAcc::default(), 0));
        let p = by_project
            .entry(project)
            .or_insert_with(|| (0, CostAcc::default(), 0));

        // token 累加一律 saturating_add — 防 debug 溢出 panic / release 静默回绕
        // (真实数据 claude_tokens 已达 ~1.3e10 级, 远未及 u64::MAX, 但保持一致与稳健).
        if e.is_claude {
            totals.input_tokens = totals.input_tokens.saturating_add(e.usage.input_tokens);
            totals.output_tokens = totals.output_tokens.saturating_add(e.usage.output_tokens);
            totals.cache_creation_tokens = totals
                .cache_creation_tokens
                .saturating_add(e.usage.cache_creation_input_tokens);
            totals.cache_read_tokens = totals
                .cache_read_tokens
                .saturating_add(e.usage.cache_read_input_tokens);
            totals.claude_tokens = totals.claude_tokens.saturating_add(e.tokens);
            totals.message_count += 1;
            if e.cost.is_none() {
                totals.cost_unknown_entries += 1;
            }
            total_cost.add(e.cost);
            d.0 = d.0.saturating_add(e.tokens);
            d.2.add(e.cost);
        } else {
            totals.codex_tokens = totals.codex_tokens.saturating_add(e.tokens);
            d.1 = d.1.saturating_add(e.tokens);
        }

        m.0 = m.0.saturating_add(e.tokens);
        m.1.add(e.cost);
        m.2 += 1;
        p.0 = p.0.saturating_add(e.tokens);
        p.1.add(e.cost);
        p.2 += 1;
    }

    totals.cost_usd = total_cost.finish();

    let mut daily: Vec<DailyStat> = daily
        .into_iter()
        .map(|(date, (ct, xt, cost))| DailyStat {
            date,
            claude_tokens: ct,
            codex_tokens: xt,
            cost_usd: cost.finish(),
        })
        .collect();
    daily.sort_by_key(|e| e.date.clone());

    let mut by_model: Vec<ModelStat> = by_model
        .into_iter()
        .map(|(model, (tokens, cost, count))| ModelStat {
            model,
            total_tokens: tokens,
            cost_usd: cost.finish(),
            message_count: count,
        })
        .collect();
    by_model.sort_by_key(|e| std::cmp::Reverse(e.total_tokens));

    let mut by_project: Vec<ProjectStat> = by_project
        .into_iter()
        .map(|(project_path, (tokens, cost, count))| ProjectStat {
            project_path,
            total_tokens: tokens,
            cost_usd: cost.finish(),
            message_count: count,
        })
        .collect();
    by_project.sort_by_key(|e| std::cmp::Reverse(e.total_tokens));

    let range_days = ((now_ms() - since_ms).max(0) / 86_400_000) as u32;
    UsageStats {
        range_days,
        generated_at_ms: now_ms(),
        totals,
        daily,
        by_model,
        by_project,
    }
}

/// 一次全量扫描的结果缓存 — 避免每次开面板 / 切范围都重扫 3GB+ transcript.
struct Cached {
    scanned_at_ms: i64,
    /// 本次扫描覆盖的最早时刻 (entries 含 >= 此 ts 的全部记录).
    min_since_ms: i64,
    entries: Vec<Entry>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
/// 缓存一次扫满的最大范围 (天) —— 7/30/90 都从同一份 entries 过滤, 切范围不重扫.
const MAX_RANGE_DAYS: i64 = 90;
/// 缓存有效期 —— 同一面板会话内切范围 / 60s 内重开秒出; 超时重扫拿新数据.
const CACHE_TTL_MS: i64 = 60_000;

/// 薄 IO 包装: 聚合真实 `~/.claude/projects` + `~/.codex/sessions` 最近 `days` 天.
/// 命中缓存(覆盖该范围且未过期)→ 直接从内存 entries 聚合(毫秒级);
/// 否则全量并行扫满 `MAX_RANGE_DAYS` 并缓存. 找不到目录时退化为空统计.
pub fn collect(days: u32) -> UsageStats {
    let now = now_ms();
    let since = now - i64::from(days) * 86_400_000;

    if let Some(stats) = try_cache(now, since) {
        return stats;
    }

    let scan_since = now - MAX_RANGE_DAYS * 86_400_000;
    let projects_root = crate::claude::project::projects_root().unwrap_or_default();
    let codex_root = crate::codex::session::sessions_root().unwrap_or_default();
    let mut entries: Vec<Entry> = Vec::new();
    claude_scan::scan(&projects_root, scan_since, &mut entries);
    codex_scan::scan(&codex_root, scan_since, &mut entries);

    let stats = aggregate_entries(&entries, since);
    let mut guard = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    *guard = Some(Cached {
        scanned_at_ms: now,
        min_since_ms: scan_since,
        entries,
    });
    stats
}

/// 命中且覆盖请求范围 + 未过期 → 从缓存 entries 直接聚合.
fn try_cache(now: i64, since: i64) -> Option<UsageStats> {
    let guard = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    let c = guard.as_ref()?;
    if now - c.scanned_at_ms < CACHE_TTL_MS && c.min_since_ms <= since {
        Some(aggregate_entries(&c.entries, since))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 造一个隔离的 fixture: tempdir 下 projects/<dir>/<sid>.jsonl + codex/YYYY/MM/DD/rollout.jsonl.
    /// **绝不碰真实 ~/.claude / ~/.codex** (零侵入红线).
    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn aggregates_dedup_daily_model_project() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join("projects");
        let codex = tmp.path().join("codex");

        // 项目 A: 两条不同 message, 同一天 (用未来时间戳避免被 since 过滤).
        write(
            &projects.join("-Users-x-projA").join("s1.jsonl"),
            concat!(
                r#"{"type":"assistant","timestamp":"2099-01-02T10:00:00Z","requestId":"r1","model":"claude-opus-4-7","message":{"id":"m1","usage":{"input_tokens":100,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":50}}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2099-01-02T11:00:00Z","requestId":"r2","model":"claude-opus-4-7","message":{"id":"m2","usage":{"input_tokens":200,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}}"#,
                "\n",
            ),
        );
        // 项目 A 的 resume 副本: m1 重复出现 (应被去重), 外加未知模型 m3.
        write(
            &projects.join("-Users-x-projA").join("s1-resume.jsonl"),
            concat!(
                r#"{"type":"assistant","timestamp":"2099-01-02T10:00:00Z","requestId":"r1","model":"claude-opus-4-7","message":{"id":"m1","usage":{"input_tokens":100,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":50}}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2099-01-03T09:00:00Z","requestId":"r9","model":"claude-future-zzz","message":{"id":"m3","usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":5}}}"#,
                "\n",
                // 本地合成消息 — 应被过滤, 不计入 message_count / model 列表.
                r#"{"type":"assistant","timestamp":"2099-01-03T09:30:00Z","requestId":"rs","model":"<synthetic>","message":{"id":"msyn","usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}}"#,
                "\n",
            ),
        );
        // Codex rollout: 1 个 token_count, model gpt-5.5.
        write(
            &codex
                .join("2099")
                .join("01")
                .join("02")
                .join("rollout-x.jsonl"),
            concat!(
                r#"{"timestamp":"2099-01-02T12:00:00Z","type":"session_meta","payload":{"id":"cx","cwd":"/Users/x/projB"}}"#,
                "\n",
                r#"{"timestamp":"2099-01-02T12:01:00Z","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
                "\n",
                r#"{"timestamp":"2099-01-02T12:02:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"last_token_usage":{"input_tokens":300,"cached_input_tokens":100,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":325}}}}"#,
                "\n",
            ),
        );

        let stats = aggregate_from(&projects, &codex, 0);

        // 去重: m1 只算一次 → Claude 3 条消息 (m1, m2, m3); <synthetic> 被过滤不计.
        assert_eq!(stats.totals.message_count, 3);
        // <synthetic> 不出现在 by_model.
        assert!(!stats.by_model.iter().any(|m| m.model == "<synthetic>"));
        // Claude token: m1(150) + m2(200) + m3(15) = 365.
        assert_eq!(stats.totals.claude_tokens, 365);
        // 未知模型 m3 计入 cost_unknown_entries.
        assert_eq!(stats.totals.cost_unknown_entries, 1);
        // Codex: 用 total_tokens 字段 (325), 不是 input+cached+output+reasoning (425) — 验证不重复计 cached.
        assert_eq!(stats.totals.codex_tokens, 325);
        // 成本: opus m1+m2 有价, m3 无价; total cost_usd 应为 Some(>0).
        assert!(stats.totals.cost_usd.unwrap() > 0.0);

        // 按天: 01-02 有 m1+m2 (claude) + codex; 01-03 有 m3.
        let d0102 = stats.daily.iter().find(|d| d.date == "2099-01-02").unwrap();
        assert_eq!(d0102.claude_tokens, 350); // m1(150)+m2(200)
        assert_eq!(d0102.codex_tokens, 325);
        let d0103 = stats.daily.iter().find(|d| d.date == "2099-01-03").unwrap();
        assert_eq!(d0103.claude_tokens, 15);
        // 01-03 仅未知模型 → cost None.
        assert!(d0103.cost_usd.is_none());

        // 按模型: opus / future-zzz / gpt-5.5 三个.
        let opus = stats
            .by_model
            .iter()
            .find(|m| m.model.contains("opus"))
            .unwrap();
        assert_eq!(opus.total_tokens, 350);
        assert!(opus.cost_usd.unwrap() > 0.0);
        let codex_m = stats
            .by_model
            .iter()
            .find(|m| m.model == "gpt-5.5")
            .unwrap();
        assert_eq!(codex_m.total_tokens, 325);
        assert!(codex_m.cost_usd.is_none()); // Codex 无定价

        // 按项目: projA (claude) + /Users/x/projB (codex).
        assert!(stats
            .by_project
            .iter()
            .any(|p| p.project_path == "/Users/x/projB" && p.total_tokens == 325));
    }

    #[test]
    fn filters_entries_before_since() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = tmp.path().join("projects");
        // 一条 2020 年的老数据 — since_ms 取 2099 起点, 应被过滤掉.
        write(
            &projects.join("-Users-x-old").join("s.jsonl"),
            concat!(
                r#"{"type":"assistant","timestamp":"2020-01-01T00:00:00Z","requestId":"r","model":"claude-opus-4-7","message":{"id":"old","usage":{"input_tokens":999,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}}"#,
                "\n",
            ),
        );
        let codex = tmp.path().join("codex");
        let since = local_ms("2099-01-01T00:00:00Z");
        let stats = aggregate_from(&projects, &codex, since);
        assert_eq!(stats.totals.message_count, 0);
        assert_eq!(stats.totals.claude_tokens, 0);
    }

    fn local_ms(iso: &str) -> i64 {
        crate::claude::blocks::chrono_parse_iso(iso).unwrap()
    }

    #[test]
    fn missing_dirs_yield_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let stats = aggregate_from(
            &tmp.path().join("nope-projects"),
            &tmp.path().join("nope-codex"),
            0,
        );
        assert_eq!(stats.totals.message_count, 0);
        assert!(stats.daily.is_empty());
        assert!(stats.by_model.is_empty());
    }
}
