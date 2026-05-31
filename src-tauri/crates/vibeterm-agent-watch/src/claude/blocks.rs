//! 5 小时滚动块识别 — 移植自 ccusage `blocks.rs`.
//!
//! Anthropic 的 5h block 规则:
//!   - block 起点向下取整到整点 (`floor_to_hour`)
//!   - 距 block 起点 > 5h → 关闭旧 block, 开新的
//!   - 距上一条 entry > 5h → 也关闭 (出现"空档")
//!   - 当前 block "活跃" = now - last_entry < 5h 且 now < end
//!
//! VibeTerm 只需要"当前活跃 block"统计 (累积 token + 剩余时间), 不需要历史块.
//! 所以这里做了简化版: 单遍扫 jsonl 求 active block, O(N).

use std::path::Path;

use serde::Serialize;

use super::project::projects_root;

const FIVE_HOURS_MS: i64 = 5 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct ActiveBlock {
    /// block 起点 (unix ms), 向下取整到整点
    pub start_at_ms: i64,
    /// block 终点 = start + 5h
    pub end_at_ms: i64,
    /// 最后一条 entry 时间 (用来判断 active)
    pub last_entry_at_ms: i64,
    /// 已用的 token 之和 (input + cache_creation + cache_read + output)
    pub tokens_used: u64,
    /// 当前 block 已经过了多久 (now - start)
    pub elapsed_ms: i64,
    /// 剩余时间 (end - now), 已过期为 0
    pub remaining_ms: i64,
    /// 已用比 (elapsed / 5h × 100, 用作时间维度的进度条)
    pub elapsed_pct: f64,
    /// burn rate — 整个 block 平均 tokens / min
    pub tokens_per_min_avg: f64,
    /// burn rate — 最近 N 条 entry (N≈10) 的 tokens / min, 反映"当下速度"
    pub tokens_per_min_recent: f64,
    /// 等级 (匹配 ccusage 阈值): "normal" / "moderate" / "high"
    pub burn_rate_level: String,
    /// block 内累计 cost (USD). 模型未匹配 pricing 表则为 None.
    /// 注: 这是按 hardcoded pricing × tokens 估算, **不是 Anthropic 权威值**.
    /// 仅保留字段方便未来按需重启, 当前 UI 不显示.
    pub cost_usd: Option<f64>,
}

fn floor_to_hour_ms(ms: i64) -> i64 {
    let hour_ms = 60 * 60 * 1000;
    (ms / hour_ms) * hour_ms
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// jsonl 一条 assistant 行的解析结果 — 用于 block / cost 计算
struct Entry {
    ts_ms: i64,
    total_tokens: u64,
    usage: super::pricing::Usage,
    model: Option<String>,
}

/// 解析 jsonl 文件, 算当前活跃 5h block. 文件不存在或全空返回 None.
pub fn active_block_for_file(path: &Path) -> Option<ActiveBlock> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut entries: Vec<Entry> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') || !trimmed.contains("\"type\":\"assistant\"") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()).or_else(|| {
            v.get("message")
                .and_then(|m| m.get("timestamp"))
                .and_then(|t| t.as_str())
        }) else {
            continue;
        };
        let Some(ts_ms) = chrono_parse_iso(ts) else {
            continue;
        };
        let Some(usage_v) = v
            .get("usage")
            .or_else(|| v.get("message").and_then(|m| m.get("usage")))
        else {
            continue;
        };
        let input = usage_v
            .get("input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cache_creation = usage_v
            .get("cache_creation_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cache_read = usage_v
            .get("cache_read_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let output = usage_v
            .get("output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let model = v
            .get("model")
            .and_then(|m| m.as_str())
            .or_else(|| {
                v.get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
            })
            .map(|s| s.to_string());
        let total = input + cache_creation + cache_read + output;
        entries.push(Entry {
            ts_ms,
            total_tokens: total,
            usage: super::pricing::Usage {
                input_tokens: input,
                cache_creation_input_tokens: cache_creation,
                cache_read_input_tokens: cache_read,
                output_tokens: output,
            },
            model,
        });
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by_key(|e| e.ts_ms);

    let now = now_ms();
    let mut current_start: Option<i64> = None;
    let mut current_last: i64 = 0;
    let mut current_tokens: u64 = 0;
    let mut current_cost: f64 = 0.0;
    let mut current_cost_unknown = false;
    let mut current_entries: Vec<(i64, u64)> = Vec::new();

    for e in &entries {
        match current_start {
            Some(start) => {
                let since_start = e.ts_ms - start;
                let since_last = e.ts_ms - current_last;
                if since_start > FIVE_HOURS_MS || since_last > FIVE_HOURS_MS {
                    current_start = Some(floor_to_hour_ms(e.ts_ms));
                    current_tokens = 0;
                    current_cost = 0.0;
                    current_cost_unknown = false;
                    current_entries.clear();
                }
            }
            None => {
                current_start = Some(floor_to_hour_ms(e.ts_ms));
            }
        }
        current_last = e.ts_ms;
        current_tokens += e.total_tokens;
        current_entries.push((e.ts_ms, e.total_tokens));
        // cost — 没 model 或没 pricing 表则该 entry 跳过, 总和标 unknown
        let ctx_at_call = e.usage.input_tokens
            + e.usage.cache_creation_input_tokens
            + e.usage.cache_read_input_tokens;
        match e
            .model
            .as_deref()
            .and_then(|m| super::pricing::cost_of(m, e.usage, ctx_at_call))
        {
            Some(c) => current_cost += c,
            None => current_cost_unknown = true,
        }
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
    // 最近 N=10 条 entry — 时间跨度从第一条到最后一条
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
    // 只要 block 内存在任何未知 pricing 的条目, 累加值就是"部分成本", 无法区分于完整成本,
    // 直接返回 None 避免把偏低的部分成本当成权威值回传 (混合场景也算未知).
    let cost_usd = if current_cost_unknown {
        None
    } else {
        Some(current_cost)
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
        cost_usd,
    })
}

/// 按 cwd 取 active block — 找该 project 下 mtime 最新 jsonl, 跑算法.
///
/// **安全**: canonicalize 后必须仍在 `projects_root()` 内, 防止前端构造路径
/// + symlink 把读取引向 ~/.ssh 等敏感目录 (与 `project::read_for_cwd` 同一防护).
pub fn active_block_for_cwd(cwd: &str) -> Option<ActiveBlock> {
    let root = projects_root()?;
    let project_dir_name = super::project::cwd_to_project_dir(cwd);
    let dir = root.join(&project_dir_name);
    let canon_dir = std::fs::canonicalize(&dir).ok()?;
    let canon_root = std::fs::canonicalize(&root).ok()?;
    if !canon_dir.starts_with(&canon_root) {
        tracing::warn!(
            "claude block: refusing symlink escape {} → {}",
            dir.display(),
            canon_dir.display()
        );
        return None;
    }
    let (jsonl, _) = super::project::latest_jsonl_in(&canon_dir)?;
    active_block_for_file(&jsonl)
}

/// 最小化的 ISO8601 → unix ms 解析. 不引入 chrono 依赖 (该 crate 已小, 避免膨胀).
/// 仅支持 RFC3339 形如 `2026-05-27T15:23:25.486Z` 或 `2026-05-27T15:23:25+00:00`.
pub(crate) fn chrono_parse_iso(s: &str) -> Option<i64> {
    // 简单 parser: yyyy-MM-ddTHH:mm:ss(.fff)?(Z|±HH:MM)
    // 用 std 实现, 不依赖 chrono.
    let bytes = s.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year: i64 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let month: u32 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    let hour: u32 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
    let minute: u32 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
    let sec: u32 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
    // 可选小数秒
    let mut idx = 19usize;
    let mut frac_ms: i64 = 0;
    if bytes.get(idx) == Some(&b'.') {
        idx += 1;
        let start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        // 取前 3 位作 ms
        let frac_str = &s[start..idx];
        let take = frac_str.get(..3.min(frac_str.len()))?;
        frac_ms = take.parse().ok()?;
        if take.len() < 3 {
            frac_ms *= 10i64.pow(3 - take.len() as u32);
        }
    }
    // 时区
    let tz_offset_min: i64 = if idx < bytes.len() {
        let c = bytes[idx];
        if c == b'Z' {
            0
        } else if c == b'+' || c == b'-' {
            if idx + 5 >= bytes.len() {
                return None;
            }
            let sign = if c == b'+' { 1i64 } else { -1i64 };
            let hh: i64 = std::str::from_utf8(&bytes[idx + 1..idx + 3])
                .ok()?
                .parse()
                .ok()?;
            let mm: i64 = std::str::from_utf8(&bytes[idx + 4..idx + 6])
                .ok()?
                .parse()
                .ok()?;
            sign * (hh * 60 + mm)
        } else {
            0
        }
    } else {
        0
    };

    // 转 unix ms (假设输入是 UTC + offset)
    // 用 chrono 算太重, 自己实现一个 days_from_civil (天文台公式)
    let days = days_from_civil(year, month, day);
    let epoch_days = days - days_from_civil(1970, 1, 1);
    let mut total_sec =
        epoch_days * 86400 + (hour as i64) * 3600 + (minute as i64) * 60 + sec as i64;
    total_sec -= tz_offset_min * 60;
    Some(total_sec * 1000 + frac_ms)
}

/// Howard Hinnant 公历日数算法 (`days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u64;
    let m = m as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse_basic() {
        // 2026-01-01T00:00:00Z = 1764547200 unix sec
        let ms = chrono_parse_iso("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(ms, 1_767_225_600_000);
    }

    #[test]
    fn iso_parse_with_offset_and_frac() {
        // 2026-05-27T17:20:00.627+00:00
        let ms = chrono_parse_iso("2026-05-27T17:20:00.627+00:00").unwrap();
        // 跟 2026-05-27T17:20:00.627Z 一致
        let ms_z = chrono_parse_iso("2026-05-27T17:20:00.627Z").unwrap();
        assert_eq!(ms, ms_z);
        assert_eq!(ms % 1000, 627);
    }

    #[test]
    fn iso_parse_positive_offset() {
        // 2026-01-01T08:00:00+08:00 = 2026-01-01T00:00:00Z
        let a = chrono_parse_iso("2026-01-01T08:00:00+08:00").unwrap();
        let b = chrono_parse_iso("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn floor_to_hour_basic() {
        // 2026-01-01T12:34:56Z → 2026-01-01T12:00:00Z
        let ms = chrono_parse_iso("2026-01-01T12:34:56Z").unwrap();
        let floored = floor_to_hour_ms(ms);
        let expected = chrono_parse_iso("2026-01-01T12:00:00Z").unwrap();
        assert_eq!(floored, expected);
    }

    #[test]
    fn active_block_single_session() {
        // 写一个临时 jsonl, 两条 assistant 在同一小时
        let jsonl = r#"{"type":"assistant","timestamp":"2026-05-27T15:23:25.486Z","model":"claude-opus-4-7","usage":{"input_tokens":100,"cache_creation_input_tokens":50,"cache_read_input_tokens":200,"output_tokens":40}}
{"type":"assistant","timestamp":"2026-05-27T15:25:00.000Z","model":"claude-opus-4-7","usage":{"input_tokens":200,"cache_creation_input_tokens":0,"cache_read_input_tokens":390,"output_tokens":80}}
"#;
        let tmp = std::env::temp_dir().join("vt-test-block.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let blk = active_block_for_file(&tmp).unwrap();
        // 起点是 2026-05-27T15:00:00 (floor to hour)
        let expected_start = chrono_parse_iso("2026-05-27T15:00:00Z").unwrap();
        assert_eq!(blk.start_at_ms, expected_start);
        // tokens = sum of all input+cache+output
        // 100+50+200+40 + 200+0+390+80 = 1060
        assert_eq!(blk.tokens_used, 1060);
    }
}
