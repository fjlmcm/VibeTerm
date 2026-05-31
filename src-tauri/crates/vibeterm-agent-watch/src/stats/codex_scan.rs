//! Codex rollout 全量扫描 — `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` 的 token_count 事件.
//!
//! 每个 rollout 内: `session_meta.cwd` → 项目, `turn_context.model` → 当前模型 (附到其后的
//! token_count), `event_msg/token_count` → 本 turn token (input+cached+output+reasoning).
//! Codex 无公开定价表, cost 恒 None. 只读, 流式.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::Entry;
use crate::claude::blocks::chrono_parse_iso;
use crate::claude::pricing::Usage;

/// 遍历 `<root>/YYYY/MM/DD/*.jsonl`, 窗口内 token_count 事件追加到 `out`.
pub(crate) fn scan(codex_root: &Path, since_ms: i64, out: &mut Vec<Entry>) {
    let files = rollout_files(codex_root, since_ms);
    let entries = super::par_collect(files, move |path| {
        let mut v = Vec::new();
        scan_rollout(&path, since_ms, &mut v);
        v
    });
    out.extend(entries);
}

/// 收集 mtime 在窗口内的 rollout 文件路径 (3 层日期目录).
fn rollout_files(root: &Path, since_ms: i64) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    let Ok(years) = std::fs::read_dir(root) else {
        return files;
    };
    for y in years.flatten() {
        let Ok(months) = std::fs::read_dir(y.path()) else {
            continue;
        };
        for m in months.flatten() {
            let Ok(days) = std::fs::read_dir(m.path()) else {
                continue;
            };
            for d in days.flatten() {
                let Ok(entries) = std::fs::read_dir(d.path()) else {
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
                        .and_then(|md| md.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .and_then(|dd| i64::try_from(dd.as_millis()).ok())
                        // 读不到 mtime → 当作"最近"仍扫描, 行级 ts 过滤兜底.
                        .unwrap_or(i64::MAX);
                    if mtime < since_ms {
                        continue;
                    }
                    files.push(fp);
                }
            }
        }
    }
    files
}

fn scan_rollout(path: &Path, since_ms: i64, out: &mut Vec<Entry>) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session_meta") => {
                if let Some(c) = v.pointer("/payload/cwd").and_then(|x| x.as_str()) {
                    cwd = Some(c.to_string());
                }
            }
            Some("turn_context") => {
                if let Some(m) = v.pointer("/payload/model").and_then(|x| x.as_str()) {
                    model = Some(m.to_string());
                }
            }
            Some("event_msg") => {
                if v.pointer("/payload/type").and_then(|x| x.as_str()) != Some("token_count") {
                    continue;
                }
                let Some(ts_ms) = v
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(chrono_parse_iso)
                else {
                    continue;
                };
                if ts_ms < since_ms {
                    continue;
                }
                let Some(last) = v.pointer("/payload/info/last_token_usage") else {
                    continue;
                };
                let get = |k: &str| last.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                // 用 Codex 自己的 turn 总量 total_tokens (= input + output + reasoning,
                // cached_input ⊆ input). 直接加 cached 会双重计数, 故不再单独累加;
                // 字段缺失 (而非值为 0) 时才回退 input+output+reasoning.
                let tokens = last
                    .get("total_tokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or_else(|| {
                        get("input_tokens")
                            .saturating_add(get("output_tokens"))
                            .saturating_add(get("reasoning_output_tokens"))
                    });
                out.push(Entry {
                    ts_ms,
                    is_claude: false,
                    model: model.clone(),
                    project_path: cwd.clone(),
                    usage: Usage::default(),
                    tokens,
                    cost: None,
                    dedup_key: None,
                });
            }
            _ => {}
        }
    }
}
