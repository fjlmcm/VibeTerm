//! Claude transcript 全量扫描 — `~/.claude/projects/<dir>/*.jsonl` 的 assistant 行.
//!
//! 流式逐行 (BufReader), 内存 O(行长), 不限文件大小 (一次性聚合, 非 3s 轮询,
//! 长会话也要计入). 只读.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Entry;
use crate::claude::blocks::chrono_parse_iso;
use crate::claude::pricing::{cost_of, Usage};
use crate::claude::project::project_dir_to_cwd;

#[derive(Deserialize)]
struct Row {
    #[serde(rename = "type")]
    line_type: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    timestamp: Option<String>,
    model: Option<String>,
    usage: Option<RowUsage>,
    message: Option<RowMessage>,
}

#[derive(Deserialize)]
struct RowMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<RowUsage>,
}

#[derive(Deserialize, Default)]
struct RowUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// 遍历所有 project dir, 把窗口内的 assistant 用量行追加到 `out`.
pub(crate) fn scan(projects_root: &Path, since_ms: i64, out: &mut Vec<Entry>) {
    let Ok(dirs) = std::fs::read_dir(projects_root) else {
        return;
    };
    // 工作清单 (path, project, size) —— 大文件先排序, par_collect round-robin 后各线程更均衡.
    let mut work: Vec<(PathBuf, String, u64)> = Vec::new();
    for dir in dirs.flatten() {
        let dp = dir.path();
        if !dp.is_dir() {
            continue;
        }
        let project_path = dir
            .file_name()
            .to_str()
            .map(project_dir_to_cwd)
            .unwrap_or_default();
        let Ok(files) = std::fs::read_dir(&dp) else {
            continue;
        };
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            // 文件最后写入早于窗口 → 其最新条目也早于窗口, 整文件跳过.
            // 读不到 mtime(权限 / 网络盘)→ 当作"最近"仍扫描, 行级 ts 过滤兜底.
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(i64::MAX);
            if mtime < since_ms {
                continue;
            }
            work.push((fp, project_path.clone(), meta.len()));
        }
    }
    work.sort_by_key(|e| std::cmp::Reverse(e.2));
    let entries = super::par_collect(work, move |(path, project, _size)| {
        let mut v = Vec::new();
        scan_file(&path, since_ms, &project, &mut v);
        v
    });
    out.extend(entries);
}

fn scan_file(path: &Path, since_ms: i64, project_path: &str, out: &mut Vec<Entry>) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        // 快速跳过非 assistant 行, 省 serde 开销.
        if !trimmed.starts_with('{') || !trimmed.contains("\"type\":\"assistant\"") {
            continue;
        }
        let Ok(row) = serde_json::from_str::<Row>(trimmed) else {
            continue;
        };
        if row.line_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(ts_ms) = row.timestamp.as_deref().and_then(chrono_parse_iso) else {
            continue;
        };
        if ts_ms < since_ms {
            continue;
        }
        let raw = row
            .usage
            .or_else(|| row.message.as_ref().and_then(take_usage));
        let Some(raw) = raw else { continue };
        let model = row
            .model
            .or_else(|| row.message.as_ref().and_then(|m| m.model.clone()));
        // 跳过 Claude 本地合成消息 (model="<synthetic>") — 无真实 API 用量,
        // 计入会污染 message_count / cost_unknown_entries.
        if model.as_deref() == Some("<synthetic>") {
            continue;
        }
        let usage = Usage {
            input_tokens: raw.input_tokens,
            cache_creation_input_tokens: raw.cache_creation_input_tokens,
            cache_read_input_tokens: raw.cache_read_input_tokens,
            output_tokens: raw.output_tokens,
        };
        let ctx_at_call =
            usage.input_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
        let cost = model
            .as_deref()
            .and_then(|m| cost_of(m, usage, ctx_at_call));
        let tokens = usage.input_tokens
            + usage.cache_creation_input_tokens
            + usage.cache_read_input_tokens
            + usage.output_tokens;
        let dedup_key = row
            .message
            .as_ref()
            .and_then(|m| m.id.clone())
            .map(|id| (id, row.request_id.clone().unwrap_or_default()));
        out.push(Entry {
            ts_ms,
            is_claude: true,
            model,
            project_path: Some(project_path.to_string()),
            usage,
            tokens,
            cost,
            dedup_key,
        });
    }
}

/// 取 message.usage (借用 → 拥有), 避免 partial move 掉整个 message.
fn take_usage(m: &RowMessage) -> Option<RowUsage> {
    m.usage.as_ref().map(|u| RowUsage {
        input_tokens: u.input_tokens,
        cache_creation_input_tokens: u.cache_creation_input_tokens,
        cache_read_input_tokens: u.cache_read_input_tokens,
        output_tokens: u.output_tokens,
    })
}
