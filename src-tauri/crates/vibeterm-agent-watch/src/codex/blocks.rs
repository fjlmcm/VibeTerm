//! Codex 5 小时滚动块识别 — 移植自 ccusage `blocks.rs` (跟 claude/blocks.rs 同算法).
//!
//! 关键差异点:
//!   - 数据源是 `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, 不是 Claude 的 projects/<cwd>/*.jsonl
//!   - 每条事件: type=event_msg, payload.type=token_count, payload.info.last_token_usage
//!   - 5h block 是按**账号**算的 (跟 Anthropic 一致), 跨 rollout 跨 session, 不按 cwd 过滤
//!   - Codex 没有 cache_creation 字段, 用 input + cached_input + output + reasoning_output
//!   - cost: Codex 没公开价格表, 暂返回 None (UI 端隐藏 cost 行)
//!
//! 算法 (跟 claude::blocks::active_block_for_file 一致):
//!   - 起点 floor 到整点
//!   - since_start > 5h 或 since_last > 5h 则关闭旧块, 起点 floor 当前 entry
//!   - 当前 active = 最后一条 entry 在 5h 内
//!   - burn rate 取最近 N=10 条 entry 的 tokens/min

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::claude::blocks::{chrono_parse_iso, ActiveBlock};

use super::session::sessions_root;

const FIVE_HOURS_MS: i64 = 5 * 60 * 60 * 1000;
const ROLLOUT_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// 扫最近 N 天目录 — 5h block 边界最多跨 1 天, 但留余地兜底容错时钟漂移.
const RECENT_DAY_LIMIT: usize = 3;

fn floor_to_hour_ms(ms: i64) -> i64 {
    let hour_ms = 60 * 60 * 1000;
    (ms / hour_ms) * hour_ms
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// 一条 token_count 事件 → block 算法用的 entry.
struct Entry {
    ts_ms: i64,
    /// 本 turn 总 token 用量 (input + cached + output + reasoning_output)
    total_tokens: u64,
}

/// 扫近 RECENT_DAY_LIMIT 天 rollout 文件, 按 mtime 倒序返回路径列表.
/// (跟 codex::session 的扫描逻辑同, 但暴露文件列表而不是先解析全文)
fn recent_rollouts() -> Vec<PathBuf> {
    let Some(root) = sessions_root() else {
        return Vec::new();
    };
    if !root.exists() {
        return Vec::new();
    }
    let mut day_dirs: Vec<(PathBuf, i64)> = Vec::new();
    if let Ok(ys) = std::fs::read_dir(&root) {
        for y in ys.flatten() {
            let yp = y.path();
            if !yp.is_dir() {
                continue;
            }
            let Ok(ms) = std::fs::read_dir(&yp) else {
                continue;
            };
            for m in ms.flatten() {
                let mp = m.path();
                if !mp.is_dir() {
                    continue;
                }
                let Ok(ds) = std::fs::read_dir(&mp) else {
                    continue;
                };
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
                        .and_then(|d| i64::try_from(d.as_millis()).ok())
                        .unwrap_or(0);
                    day_dirs.push((dp, mtime));
                }
            }
        }
    }
    day_dirs.sort_by_key(|e| std::cmp::Reverse(e.1));
    day_dirs.truncate(RECENT_DAY_LIMIT);

    let mut files: Vec<(PathBuf, i64)> = Vec::new();
    for (dir, _) in day_dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for f in entries.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let mtime = f
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0);
            files.push((fp, mtime));
        }
    }
    files.sort_by_key(|e| std::cmp::Reverse(e.1));
    files.into_iter().map(|(p, _)| p).collect()
}

/// 从单个 rollout 流式提取 token_count entries. 文件过大跳过.
fn extract_entries_from(path: &Path, out: &mut Vec<Entry>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() > ROLLOUT_MAX_BYTES {
        tracing::debug!(
            "codex blocks: skip oversized {} ({} bytes)",
            path.display(),
            meta.len()
        );
        return;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let reader = BufReader::new(file);
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        // 快速跳过非 token_count 行
        if !trimmed.contains("\"token_count\"") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("event_msg") {
            continue;
        }
        let payload = match v.get("payload") {
            Some(p) => p,
            None => continue,
        };
        if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let ts = match v.get("timestamp").and_then(|t| t.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let ts_ms = match chrono_parse_iso(ts) {
            Some(ms) => ms,
            None => continue,
        };
        let last = match payload.get("info").and_then(|i| i.get("last_token_usage")) {
            Some(u) => u,
            None => continue,
        };
        let input = last
            .get("input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cached = last
            .get("cached_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let output = last
            .get("output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let reasoning = last
            .get("reasoning_output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let total = input
            .saturating_add(cached)
            .saturating_add(output)
            .saturating_add(reasoning);
        out.push(Entry {
            ts_ms,
            total_tokens: total,
        });
    }
}

/// 算当前活跃 5h block. 扫所有最近 rollout, 汇集 token_count entries, 跑算法.
/// `cwd` 参数仅用于 IPC 跟 Claude 对称 — 实际 Codex 配额按账号算, 不按 cwd 过滤.
pub fn active_block_for_cwd(_cwd: &str) -> Option<ActiveBlock> {
    let files = recent_rollouts();
    if files.is_empty() {
        return None;
    }
    let mut entries: Vec<Entry> = Vec::new();
    for f in files {
        extract_entries_from(&f, &mut entries);
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by_key(|e| e.ts_ms);

    let now = now_ms();
    let mut current_start: Option<i64> = None;
    let mut current_last: i64 = 0;
    let mut current_tokens: u64 = 0;
    let mut current_entries: Vec<(i64, u64)> = Vec::new();

    for e in &entries {
        match current_start {
            Some(start) => {
                let since_start = e.ts_ms - start;
                let since_last = e.ts_ms - current_last;
                if since_start > FIVE_HOURS_MS || since_last > FIVE_HOURS_MS {
                    current_start = Some(floor_to_hour_ms(e.ts_ms));
                    current_tokens = 0;
                    current_entries.clear();
                }
            }
            None => {
                current_start = Some(floor_to_hour_ms(e.ts_ms));
            }
        }
        current_last = e.ts_ms;
        current_tokens = current_tokens.saturating_add(e.total_tokens);
        current_entries.push((e.ts_ms, e.total_tokens));
    }

    let start = current_start?;
    let end = start + FIVE_HOURS_MS;
    let elapsed = (now - start).max(0);
    let remaining = (end - now).max(0);
    let elapsed_min = (elapsed as f64) / 60_000.0;
    let avg = if elapsed_min > 0.0 {
        current_tokens as f64 / elapsed_min
    } else {
        0.0
    };
    let recent_n = current_entries.len().min(10);
    let recent_slice = &current_entries[current_entries.len() - recent_n..];
    let recent = if recent_n >= 2 {
        let first_ts = recent_slice.first().map(|(t, _)| *t).unwrap_or(0);
        let last_ts = recent_slice.last().map(|(t, _)| *t).unwrap_or(0);
        let span_min = ((last_ts - first_ts).max(1) as f64) / 60_000.0;
        let tokens_sum: u64 = recent_slice.iter().map(|(_, t)| t).sum();
        if span_min > 0.0 {
            tokens_sum as f64 / span_min
        } else {
            0.0
        }
    } else {
        avg
    };
    let level = if recent < 2000.0 {
        "normal"
    } else if recent < 5000.0 {
        "moderate"
    } else {
        "high"
    };
    Some(ActiveBlock {
        start_at_ms: start,
        end_at_ms: end,
        last_entry_at_ms: current_last,
        tokens_used: current_tokens,
        elapsed_ms: elapsed,
        remaining_ms: remaining,
        elapsed_pct: ((elapsed as f64 / FIVE_HOURS_MS as f64) * 100.0).min(100.0),
        tokens_per_min_avg: avg,
        tokens_per_min_recent: recent,
        burn_rate_level: level.to_string(),
        cost_usd: None, // Codex 没公开价格表
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_rollout(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn extract_token_count_entries() {
        let tmp = std::env::temp_dir().join("vt-codex-blocks-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let body = r#"{"timestamp":"2026-05-27T20:00:00.000Z","type":"session_meta","payload":{"id":"x","cwd":"/x"}}
{"timestamp":"2026-05-27T20:05:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":10,"reasoning_output_tokens":5,"total_tokens":165},"model_context_window":200000}}}
{"timestamp":"2026-05-27T20:10:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":200,"output_tokens":20,"total_tokens":220},"last_token_usage":{"input_tokens":200,"cached_input_tokens":100,"output_tokens":20,"reasoning_output_tokens":10,"total_tokens":330},"model_context_window":200000}}}
"#;
        let p = write_rollout(&tmp, "rollout-test.jsonl", body);
        let mut entries = Vec::new();
        extract_entries_from(&p, &mut entries);
        assert_eq!(entries.len(), 2);
        // 第一条: 100 + 50 + 10 + 5 = 165
        assert_eq!(entries[0].total_tokens, 165);
        // 第二条: 200 + 100 + 20 + 10 = 330
        assert_eq!(entries[1].total_tokens, 330);
    }
}
