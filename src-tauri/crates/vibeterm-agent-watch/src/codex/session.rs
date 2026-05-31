//! Codex session 监听 — `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//!
//! 一个 rollout 文件 = 一次 Codex session, 行格式:
//!   - `type=session_meta` (首行): id / cwd / model_provider / cli_version
//!   - `type=turn_context`: model / effort
//!   - `type=event_msg`, `payload.type=token_count`: 上下文 + rate_limits
//!
//! v3 实现: 全局取最新 rollout (any cwd), 解析最新 token_count 给前端.
//! v4 会按 cwd 过滤.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{CodexSnapshot, RateLimit};

/// 单个 rollout jsonl 文件大小硬上限. Codex rollout 平时几 MB, 16MB 已宽松.
const ROLLOUT_MAX_BYTES: u64 = 16 * 1024 * 1024;

pub fn sessions_root() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".codex").join("sessions"))
}

/// 扫近 N 天目录, 找 mtime 最新的 rollout-*.jsonl.
/// 不全盘扫 (深目录开销), 仅看最近 2 天.
fn find_latest_rollout() -> Option<PathBuf> {
    let root = sessions_root()?;
    if !root.exists() {
        return None;
    }
    let mut candidates: Vec<(PathBuf, i64)> = Vec::new();
    // 进入 YYYY 目录 -> MM 目录 -> DD 目录 -> rollout.jsonl
    // 只看 mtime 最新的若干个 DD 目录, 收集其中所有 jsonl
    let mut day_dirs: Vec<(PathBuf, i64)> = Vec::new();
    if let Ok(ys) = std::fs::read_dir(&root) {
        for y in ys.flatten() {
            let yp = y.path();
            if !yp.is_dir() {
                continue;
            }
            if let Ok(ms) = std::fs::read_dir(&yp) {
                for m in ms.flatten() {
                    let mp = m.path();
                    if !mp.is_dir() {
                        continue;
                    }
                    if let Ok(ds) = std::fs::read_dir(&mp) {
                        for d in ds.flatten() {
                            let dp = d.path();
                            if !dp.is_dir() {
                                continue;
                            }
                            let mtime = d
                                .metadata()
                                .ok()
                                .and_then(|m| m.modified().ok())
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            day_dirs.push((dp, mtime));
                        }
                    }
                }
            }
        }
    }
    // 取最近 3 个日期目录
    day_dirs.sort_by_key(|e| std::cmp::Reverse(e.1));
    day_dirs.truncate(3);

    for (dir, _) in day_dirs {
        if let Ok(files) = std::fs::read_dir(&dir) {
            for f in files.flatten() {
                let fp = f.path();
                if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let mtime = f
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                candidates.push((fp, mtime));
            }
        }
    }
    candidates
        .into_iter()
        .max_by_key(|(_, m)| *m)
        .map(|(p, _)| p)
}

// --- JSONL 解析 ---

#[derive(Debug, Deserialize)]
struct CodexLine {
    #[serde(rename = "type")]
    line_type: String,
    #[serde(default)]
    timestamp: Option<String>,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SessionMeta {
    id: String,
    cwd: String,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    cli_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnContext {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenCountPayload {
    #[serde(rename = "type")]
    sub_type: String,
    info: TokenCountInfo,
    rate_limits: Option<RateLimits>,
}

#[derive(Debug, Deserialize)]
struct TokenCountInfo {
    /// 累积值 — 整个 session 所有 turn 加起来的 token (大、随时间单调增长).
    /// 不是 context 占用, 长 session 里能涨到几百 M tokens.
    #[allow(dead_code)]
    total_token_usage: TurnTokens,
    /// 上一个 turn 的 token 用量 — `input_tokens` 是发给模型的上下文大小,
    /// 这才是真正的 "当前 context 窗口占用". output_tokens 是模型回复, 进入下次 context.
    last_token_usage: TurnTokens,
    model_context_window: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TurnTokens {
    input_tokens: u64,
    #[serde(default)]
    #[allow(dead_code)]
    cached_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    /// 整个 turn 的 token 总量 (input + output + reasoning). Codex CLI 把它作为
    /// "tokens_in_context_window", 所以 ctx 占用应该用这个值, 不是 input_tokens.
    #[serde(default)]
    total_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct RateLimits {
    primary: Option<RateLimit>,
    secondary: Option<RateLimit>,
    plan_type: Option<String>,
}

/// 解析 rollout 文件, 提取最关键字段.
/// 用 BufReader 流式读, 单文件硬上限 `ROLLOUT_MAX_BYTES` 防 OOM.
fn build_snapshot(path: &Path) -> Option<CodexSnapshot> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > ROLLOUT_MAX_BYTES {
        tracing::debug!(
            "codex session: skip oversized {} ({} bytes)",
            path.display(),
            meta.len()
        );
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut session_id = String::new();
    let mut cwd = String::new();
    let mut model_provider: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut model: Option<String> = None;
    let mut effort: Option<String> = None;
    let mut context_tokens: Option<u64> = None;
    let mut context_window: Option<u64> = None;
    let mut primary: Option<RateLimit> = None;
    // (ts_ms, turn_tokens=input+output) — 用于算 burn rate
    let mut token_events: Vec<(i64, u64)> = Vec::new();
    let mut secondary: Option<RateLimit> = None;
    let mut plan_type: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let parsed: CodexLine = match serde_json::from_str(trimmed) {
            Ok(p) => p,
            Err(_) => continue,
        };
        match parsed.line_type.as_str() {
            "session_meta" => {
                if let Ok(m) = serde_json::from_value::<SessionMeta>(parsed.payload) {
                    session_id = m.id;
                    cwd = m.cwd;
                    model_provider = m.model_provider;
                    cli_version = m.cli_version;
                }
            }
            "turn_context" => {
                if let Ok(t) = serde_json::from_value::<TurnContext>(parsed.payload) {
                    if t.model.is_some() {
                        model = t.model;
                    }
                    if t.effort.is_some() {
                        effort = t.effort;
                    }
                }
            }
            "event_msg" => {
                let ts_ms = parsed.timestamp.as_deref().and_then(parse_iso);
                if let Ok(tc) = serde_json::from_value::<TokenCountPayload>(parsed.payload) {
                    if tc.sub_type != "token_count" {
                        continue;
                    }
                    // 用 last_token_usage.total_tokens — 跟 Codex CLI `tokens_in_context_window`
                    // 一致 (codex-rs/tui/src/token_usage.rs:42). 这是 input+output+reasoning
                    // 全部的"当前 context 占用".
                    context_tokens = Some(tc.info.last_token_usage.total_tokens);
                    context_window = tc.info.model_context_window;
                    // 累积 (ts, turn_tokens) 给 burn rate 用 — turn_tokens = 本 turn 的 input + output
                    if let Some(ts) = ts_ms {
                        let turn = tc.info.last_token_usage.input_tokens
                            + tc.info.last_token_usage.output_tokens;
                        token_events.push((ts, turn));
                    }
                    if let Some(rl) = tc.rate_limits {
                        primary = rl.primary;
                        secondary = rl.secondary;
                        plan_type = rl.plan_type;
                    }
                }
            }
            _ => {}
        }
    }

    if session_id.is_empty() {
        return None;
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    // burn rate: 最近 N=10 条 token_count 事件的 turn_tokens / 跨度分钟
    let n = token_events.len().min(10);
    let slice = &token_events[token_events.len() - n..];
    let (tokens_per_min_recent, burn_level) = if n >= 2 {
        let first_ts = slice.first().map(|(t, _)| *t).unwrap_or(0);
        let last_ts = slice.last().map(|(t, _)| *t).unwrap_or(0);
        let span_min = ((last_ts - first_ts).max(1) as f64) / 60_000.0;
        let sum: u64 = slice.iter().map(|(_, t)| t).sum();
        let rate = if span_min > 0.0 {
            sum as f64 / span_min
        } else {
            0.0
        };
        let level = if rate < 2000.0 {
            "normal"
        } else if rate < 5000.0 {
            "moderate"
        } else {
            "high"
        };
        (rate, level.to_string())
    } else {
        (0.0, "normal".to_string())
    };

    let context_used_pct = compute_codex_context_used_pct(context_tokens, context_window);

    Some(CodexSnapshot {
        session_id,
        cwd,
        model,
        model_provider,
        cli_version,
        context_tokens,
        context_window,
        context_used_pct,
        primary_limit: primary,
        secondary_limit: secondary,
        plan_type,
        updated_at_ms: now_ms,
        tokens_per_min_recent,
        burn_rate_level: burn_level,
        effort,
    })
}

/// Codex CLI 上下文百分比 — 严格按 `codex-rs/tui/src/token_usage.rs` 算法.
///   - BASELINE_TOKENS = 12000 (系统 prompt 假设占用), 从分子分母都扣掉
///   - 边缘: window <= baseline 返回 None
///
/// 参考: <https://github.com/openai/codex> codex-rs/tui/src/token_usage.rs
fn compute_codex_context_used_pct(tokens: Option<u64>, window: Option<u64>) -> Option<f64> {
    const BASELINE_TOKENS: u64 = 12_000;
    let t = tokens?;
    let w = window?;
    if w <= BASELINE_TOKENS {
        return None;
    }
    let effective_window = w - BASELINE_TOKENS;
    let used = t.saturating_sub(BASELINE_TOKENS);
    let used_pct = (used as f64 / effective_window as f64) * 100.0;
    Some(used_pct.clamp(0.0, 100.0))
}

fn parse_iso(s: &str) -> Option<i64> {
    crate::claude::blocks::chrono_parse_iso(s)
}

/// 同步拉一次 Codex 当前活跃 snapshot.
pub fn read_once() -> Option<CodexSnapshot> {
    find_latest_rollout().and_then(|p| build_snapshot(&p))
}

/// 按 cwd 查 Codex session — 扫近 3 天 rollout, 找首行 session_meta.cwd 匹配的 mtime 最新.
pub fn read_for_cwd(cwd: &str) -> Option<CodexSnapshot> {
    let root = sessions_root()?;
    if !root.exists() {
        return None;
    }
    // 复用 find_latest_rollout 的目录扫描, 但取所有候选并按 mtime 倒序逐个匹配
    let mut day_dirs: Vec<(PathBuf, i64)> = Vec::new();
    if let Ok(ys) = std::fs::read_dir(&root) {
        for y in ys.flatten() {
            let yp = y.path();
            if !yp.is_dir() {
                continue;
            }
            if let Ok(ms) = std::fs::read_dir(&yp) {
                for m in ms.flatten() {
                    let mp = m.path();
                    if !mp.is_dir() {
                        continue;
                    }
                    if let Ok(ds) = std::fs::read_dir(&mp) {
                        for d in ds.flatten() {
                            let dp = d.path();
                            if !dp.is_dir() {
                                continue;
                            }
                            let mtime = d
                                .metadata()
                                .ok()
                                .and_then(|m| m.modified().ok())
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            day_dirs.push((dp, mtime));
                        }
                    }
                }
            }
        }
    }
    day_dirs.sort_by_key(|e| std::cmp::Reverse(e.1));
    day_dirs.truncate(3);

    let mut candidates: Vec<(PathBuf, i64)> = Vec::new();
    for (dir, _) in day_dirs {
        if let Ok(files) = std::fs::read_dir(&dir) {
            for f in files.flatten() {
                let fp = f.path();
                if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let mtime = f
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                candidates.push((fp, mtime));
            }
        }
    }
    candidates.sort_by_key(|e| std::cmp::Reverse(e.1));
    // 按 mtime 倒序遍历, 找到第一个 session_meta.cwd 匹配的就返回
    for (path, _) in candidates {
        if let Some(snap) = build_snapshot(&path) {
            if snap.cwd == cwd {
                return Some(snap);
            }
        }
    }
    None
}

/// 选用无界 channel: 生产端速率受下方 200ms debounce 锁死在约 5 条/秒上界, 单条消息小,
/// 消费端 (IPC emit) 处理极快, 不会无界增长; 用有界 channel 反而会引入丢消息/阻塞.
pub fn spawn_watcher(tx: mpsc::UnboundedSender<Option<CodexSnapshot>>) {
    let _ = tx.send(read_once());

    let Some(root) = sessions_root() else { return };
    std::thread::spawn(move || {
        let (n_tx, n_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut watcher = match notify::recommended_watcher(n_tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("codex session: watcher init failed: {e}");
                return;
            }
        };
        if !root.exists() {
            tracing::info!(
                "codex session: {} doesn't exist, watcher skipped",
                root.display()
            );
            return;
        }
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            tracing::warn!("codex session: watch failed: {e}");
            return;
        }
        tracing::info!("codex session: watching {}", root.display());

        loop {
            let first = match n_rx.recv() {
                Ok(r) => r,
                Err(_) => return,
            };
            if !is_relevant(&first) {
                continue;
            }
            let deadline = std::time::Instant::now() + Duration::from_millis(200);
            while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
                if n_rx.recv_timeout(remaining).is_err() {
                    break;
                }
            }
            let snapshot = read_once();
            if tx.send(snapshot).is_err() {
                return;
            }
        }
    });
}

fn is_relevant(ev: &notify::Result<Event>) -> bool {
    let Ok(ev) = ev else { return false };
    let has_jsonl = ev
        .paths
        .iter()
        .any(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"));
    has_jsonl && matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_rollout() {
        let jsonl = r#"{"timestamp":"2026-05-27T20:21:51.000Z","type":"session_meta","payload":{"id":"abc-123","timestamp":"2026-05-27T20:21:51.000Z","cwd":"/Users/test","originator":"codex-tui","cli_version":"0.133.0","model_provider":"openai"}}
{"timestamp":"2026-05-27T20:22:00.000Z","type":"turn_context","payload":{"turn_id":"t1","cwd":"/Users/test","current_date":"2026-05-27","model":"gpt-5.5","effort":"xhigh"}}
{"timestamp":"2026-05-27T20:23:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":22337,"cached_input_tokens":6528,"output_tokens":144,"total_tokens":22481},"last_token_usage":{"input_tokens":22337,"output_tokens":144,"total_tokens":22481},"model_context_window":258400},"rate_limits":{"limit_id":"codex","primary":{"used_percent":32.0,"window_minutes":10080,"resets_at":1780144255},"secondary":null,"plan_type":"free"}}}
"#;
        let tmp = std::env::temp_dir().join("vt-test-codex-rollout.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let snap = build_snapshot(&tmp).unwrap();
        assert_eq!(snap.session_id, "abc-123");
        assert_eq!(snap.cwd, "/Users/test");
        assert_eq!(snap.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(snap.model_provider.as_deref(), Some("openai"));
        assert_eq!(snap.cli_version.as_deref(), Some("0.133.0"));
        // context_tokens 用 last_token_usage.total_tokens (跟 Codex CLI 对齐)
        assert_eq!(snap.context_tokens, Some(22481));
        assert_eq!(snap.context_window, Some(258400));
        // context_used_pct: (22481 - 12000) / (258400 - 12000) * 100 = 10481 / 246400 ≈ 4.25%
        let pct = snap.context_used_pct.unwrap();
        assert!((pct - 4.25).abs() < 0.05, "got {pct}");
        assert_eq!(snap.plan_type.as_deref(), Some("free"));
        let p = snap.primary_limit.as_ref().unwrap();
        assert_eq!(p.used_percent, 32.0);
        assert_eq!(p.window_minutes, Some(10080));
        assert_eq!(p.resets_at, Some(1780144255));
        assert!(snap.secondary_limit.is_none());
    }

    #[test]
    fn rate_limit_null_window_does_not_drop_token_count() {
        // free 计划 / 新模型常出现 window_minutes/resets_at 为 null。
        // 必须容错: 否则整条 token_count 反序列化失败, 连 context_window 一起丢。
        let jsonl = concat!(
            r#"{"timestamp":"2026-05-27T20:21:51.000Z","type":"session_meta","payload":{"id":"s9","timestamp":"2026-05-27T20:21:51.000Z","cwd":"/x","cli_version":"0.140.0","model_provider":"openai"}}"#,
            "\n",
            r#"{"timestamp":"2026-05-27T20:22:00.000Z","type":"turn_context","payload":{"turn_id":"t1","cwd":"/x","current_date":"2026-05-27","model":"gpt-5.5"}}"#,
            "\n",
            r#"{"timestamp":"2026-05-27T20:23:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110},"last_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110},"model_context_window":258400},"rate_limits":{"limit_id":"codex","primary":{"used_percent":45.0,"window_minutes":null,"resets_at":null},"secondary":null,"plan_type":"free"}}}"#,
            "\n",
        );
        let tmp = std::env::temp_dir().join("vt-test-codex-nullwin.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let snap = build_snapshot(&tmp).unwrap();
        // 关键: context_window 没被 null rate_limit 拖累
        assert_eq!(snap.context_window, Some(258400));
        let p = snap.primary_limit.as_ref().unwrap();
        assert_eq!(p.used_percent, 45.0);
        assert_eq!(p.window_minutes, None);
        assert_eq!(p.resets_at, None);
    }
}
