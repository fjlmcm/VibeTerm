//! Claude project transcript 监听 — `~/.claude/projects/<sanitize(cwd)>/<sid>.jsonl`.
//!
//! 解析最后一条 `type=assistant` 消息, 拿到:
//!   - model (顶层 `.model` 或 `.message.model`)
//!   - context_tokens = input_tokens + cache_creation_input_tokens + cache_read_input_tokens
//!     (不含 output_tokens — 这是上下文窗口占用算法, 参考 ccusage commands/mod.rs:572-611)
//!
//! 设计:
//!   - 全局监听 `~/.claude/projects/` (RecursiveMode::Recursive)
//!   - 任何 jsonl 写入 → debounce 200ms → 找全局 mtime 最新的 jsonl
//!   - 该 jsonl 是"当前活跃 session", 解析后 emit ClaudeSession
//!
//! v4 会改成"按 cwd 过滤", 当前 v2 全局取最新.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::ClaudeSession;

const PROJECTS_SUBDIR: &str = "projects";

/// 单个 jsonl 文件大小软上限 — 超过则只解析尾部 `TAIL_BYTES`(见 parse_last_assistant),
/// 不整读(整读 143MB 这种长会话每 3s 一刷太慢, 原来直接 return None 又导致状态栏全空).
const JSONL_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 超限文件只读末尾这么多字节 —— 末尾即最新 assistant(model/ctx/cost)+ 最近 effort,
/// 单 turn 通常 < 1MB, 8MB 足够覆盖最近若干 turn, 且 3s 一扫够快.
const TAIL_BYTES: u64 = 8 * 1024 * 1024;

pub fn projects_root() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude").join(PROJECTS_SUBDIR))
}

/// 把 cwd 编码为 project dir 名: `/Users/mt/dev2/VibeTerm` → `-Users-mt-dev2-VibeTerm`.
/// 仅替换 `/` 为 `-`, 保留点和大小写 (实测 `dev2.VibeTerm` → `-Users-mt-dev2.VibeTerm` 这种).
pub fn cwd_to_project_dir(cwd: &str) -> String {
    cwd.replace('/', "-")
}

/// 找一个目录下 mtime 最新的 *.jsonl, 返回路径 + mtime_ms.
pub(crate) fn latest_jsonl_in(dir: &Path) -> Option<(PathBuf, i64)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(PathBuf, i64)> = None;
    for ent in entries.flatten() {
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(m) = ent.metadata() else { continue };
        let mtime = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        match &best {
            Some((_, bm)) if *bm >= mtime => {}
            _ => best = Some((p, mtime)),
        }
    }
    best
}

/// 全局扫所有 project dir, 找 mtime 最新的 jsonl. 返回 (path, project_dir_name, mtime).
fn find_active_session_file() -> Option<(PathBuf, String, i64)> {
    let root = projects_root()?;
    let entries = std::fs::read_dir(&root).ok()?;
    let mut best: Option<(PathBuf, String, i64)> = None;
    for ent in entries.flatten() {
        let p = ent.path();
        if !p.is_dir() {
            continue;
        }
        let name = p.file_name()?.to_string_lossy().into_owned();
        let Some((jsonl, mtime)) = latest_jsonl_in(&p) else {
            continue;
        };
        match &best {
            Some((_, _, bm)) if *bm >= mtime => {}
            _ => best = Some((jsonl, name, mtime)),
        }
    }
    best
}

// --- JSONL 解析 ---

#[derive(Debug, Deserialize)]
struct AssistantLine<'a> {
    #[serde(rename = "type")]
    line_type: Option<&'a str>,
    // 新格式: 顶层 model + usage
    model: Option<String>,
    usage: Option<UsageBlock>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    // 旧格式: 嵌套在 message.{model, usage}
    message: Option<Message>,
    // 顶层 timestamp (ISO8601) — prompt-cache TTL 算法用
    timestamp: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct Message {
    model: Option<String>,
    usage: Option<UsageBlock>,
}

#[derive(Debug, Deserialize)]
struct UsageBlock {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    /// Anthropic prompt cache 细分 — 5m / 1h 两种 TTL.
    /// 每条 assistant 行只填该 turn 新写入 cache 的 token 数 (cache_creation_input_tokens 总和).
    /// 后续 turn 命中已有 cache (read) 时这俩都是 0, 但 cache_read_input_tokens 才会涨.
    #[serde(default)]
    cache_creation: Option<CacheCreationDetail>,
}

#[derive(Debug, Deserialize, Default)]
struct CacheCreationDetail {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

const FIVE_MIN_MS: i64 = 5 * 60 * 1000;
const ONE_HOUR_MS: i64 = 60 * 60 * 1000;

/// 流式扫 jsonl 找最后一条 type=assistant, 解析 context_tokens + model + session_id +
/// prompt-cache TTL 到期时刻.
/// 超过 `JSONL_MAX_BYTES` 的文件直接跳过 (防 OOM).
fn parse_last_assistant(path: &Path) -> Option<ParsedSession> {
    let meta = std::fs::metadata(path).ok()?;
    let mut file = std::fs::File::open(path).ok()?;
    // 超大文件(长会话, 远超 cap): 不整读(慢, 3s 一刷), 只 seek 到尾部 TAIL_BYTES 解析。
    // 文件末尾即最新 assistant(model/ctx/cost)+ 最近 effort/attachment, 状态栏够用。
    // (原来直接 return None → model/ctx/effort 全没, 是长会话状态栏空白 + effort 抓不到的真因。)
    // 早期 /effort 命令可能落在窗口外, 但 live effort 由嗅探层 task.effort 兜底。
    let skip_partial_first = meta.len() > JSONL_MAX_BYTES;
    if skip_partial_first {
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(meta.len() - TAIL_BYTES)).ok()?;
    }
    let reader = BufReader::new(file);
    let mut line_iter = reader.lines().map_while(Result::ok);
    if skip_partial_first {
        let _ = line_iter.next(); // seek 后首行多半是半行, 丢弃
    }
    let mut last: Option<ParsedSession> = None;
    // 跟踪最后一条写入 5m/1h cache 的 timestamp — TTL 从该时刻起算.
    let mut last_5m_write_ms: Option<i64> = None;
    let mut last_1h_write_ms: Option<i64> = None;
    // 跟踪最后一条 hook 回传 (attachment) 携带的 reasoning effort 等级.
    let mut last_effort: Option<String> = None;
    // 跟踪最近一次 /effort 命令选定的会话级 effort 模式(含 "ultracode"/"max"). 这是
    // 拿到 ultracode 的唯一途径: ultracode 底层 effort=xhigh, attachment 区分不出.
    let mut last_effort_command: Option<String> = None;
    for line in line_iter {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        // attachment(async_hook_response) 行带 Claude hook 回传的 effort 等级,
        // 不是 assistant 行, 单独抽取后跳过 (晚出现的覆盖早的 → 最终拿最新).
        if trimmed.contains("\"type\":\"attachment\"") {
            if let Some(level) = extract_effort_level(trimmed) {
                last_effort = Some(level);
            }
            continue;
        }
        // /effort 命令回显(user 事件) → 用户选定的会话级 effort 模式. "this session only"
        // 的选择持续整个会话, 故最近一次即当前模式. 唯一能拿到 ultracode 的途径.
        if trimmed.contains("\"type\":\"user\"") && trimmed.contains("Set effort level to") {
            if let Some(level) = extract_effort_command(trimmed) {
                last_effort_command = Some(level);
            }
            continue;
        }
        // 快速跳过非 assistant 行 (节省 serde 开销)
        if !trimmed.contains("\"type\":\"assistant\"") {
            continue;
        }
        let parsed: AssistantLine = match serde_json::from_str(trimmed) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if parsed.line_type != Some("assistant") {
            continue;
        }
        let model = parsed
            .model
            .or_else(|| parsed.message.as_ref().and_then(|m| m.model.clone()));
        let ts_ms = parsed.timestamp.and_then(super::blocks::chrono_parse_iso);
        let usage = parsed
            .usage
            .or_else(|| parsed.message.and_then(|m| m.usage));
        let Some(usage) = usage else {
            continue;
        };
        // 更新 cache TTL 起点 — 仅当本 turn 写入新 cache (ephemeral_*_input_tokens > 0)
        if let (Some(ts), Some(cc)) = (ts_ms, &usage.cache_creation) {
            if cc.ephemeral_5m_input_tokens > 0 {
                last_5m_write_ms = Some(ts);
            }
            if cc.ephemeral_1h_input_tokens > 0 {
                last_1h_write_ms = Some(ts);
            }
        }
        let ctx =
            usage.input_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
        last = Some(ParsedSession {
            model,
            context_tokens: ctx,
            session_id: parsed.session_id.map(|s| s.to_string()),
            cache_5m_until_ms: None,
            cache_1h_until_ms: None,
            effort: None,
        });
    }
    // 最后填 TTL 到期时刻 — 仅在 last 存在时填
    if let Some(ref mut p) = last {
        p.cache_5m_until_ms = last_5m_write_ms.map(|t| t + FIVE_MIN_MS);
        p.cache_1h_until_ms = last_1h_write_ms.map(|t| t + ONE_HOUR_MS);
        // /effort 命令选定的模式优先(含 ultracode), 回退 attachment 的 effort.level.
        p.effort = last_effort_command.or(last_effort);
    }
    last
}

struct ParsedSession {
    model: Option<String>,
    context_tokens: u64,
    session_id: Option<String>,
    cache_5m_until_ms: Option<i64>,
    cache_1h_until_ms: Option<i64>,
    effort: Option<String>,
}

/// 从一行 transcript 里抠出 `"effort":{"level":"<x>"}` 的 level 值.
/// Claude hook 回传 (attachment.response) 把当前 reasoning effort 等级写进 transcript;
/// 这里用轻量子串提取, 避免对每条大 attachment 行做完整 serde 解析.
fn extract_effort_level(line: &str) -> Option<String> {
    const KEY: &str = "\"effort\":{\"level\":\"";
    let start = line.find(KEY)? + KEY.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    let level = &rest[..end];
    if level.is_empty() {
        None
    } else {
        Some(level.to_string())
    }
}

/// 从 `/effort` 命令的回显行抠出用户选定的 effort 模式(low/medium/high/xhigh/max/ultracode).
/// 仅认 `local-command-stdout` 之后的 `Set effort level to <level>`, 避免误吃讨论文本.
/// 这是拿到 "ultracode" 的唯一途径 —— ultracode 底层 effort=xhigh, attachment 的
/// effort.level 区分不出, 只有用户的 /effort 选择本身带字面 ultracode.
fn extract_effort_command(line: &str) -> Option<String> {
    let stdout_at = line.find("local-command-stdout")?;
    const KEY: &str = "Set effort level to ";
    let start = line[stdout_at..].find(KEY)? + stdout_at + KEY.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    let level = &rest[..end];
    if level.is_empty() {
        None
    } else {
        Some(level.to_string())
    }
}

/// Anthropic GA 1M-context model 前缀 — 跟 openclaw `ANTHROPIC_GA_1M_MODEL_PREFIXES` 对齐.
/// 这些模型的 model id (不论带不带 `[1m]` 后缀) 都对应 1M context window.
/// 老模型 (opus-4 / 4.1 / 4.5, sonnet-4 / 4.5, haiku-*) 仍是 200k.
///
/// 重要: jsonl 里 `message.model` 永远是裸 id (如 `claude-opus-4-7`), 没有 `[1m]`.
/// 我们必须基于 prefix 列表判断, **不能依赖 `[1m]` 后缀也不能依赖 lastModelUsage**
/// (那是历史记录, 不反映当前 session 状态).
const ANTHROPIC_GA_1M_PREFIXES: &[&str] = &[
    "claude-opus-4-6",
    "claude-opus-4.6",
    "claude-opus-4-7",
    "claude-opus-4.7",
    "claude-opus-4-8",
    "claude-opus-4.8",
    "claude-sonnet-4-6",
    "claude-sonnet-4.6",
];

/// 模型 → 上下文窗口上限. 仅依赖 model id + 实测 ctx, 不依赖 cwd / lastModelUsage.
/// 优先级:
///   1. model id 以 GA 1M 前缀开头 (含 `[1m]` 显式后缀同样命中, 因 startsWith)
///      → 1,000,000
///   2. 观测 ctx > 200,000 → 1M (物理推断, 兜底新 model 没被列入 prefix 表)
///   3. 缺省 200k
///
/// `cwd` / `observed_ctx` 仍接受为参数, 是为了未来扩展 (例如用户手动 override),
/// 但本版本不用 cwd.
pub fn context_window_for(model: &str, _cwd: Option<&str>, observed_ctx: u64) -> u64 {
    let lower = model.to_ascii_lowercase();
    // 兼容裸 id 跟 [1m] 显式后缀: 都走 startsWith
    if ANTHROPIC_GA_1M_PREFIXES
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return 1_000_000;
    }
    if observed_ctx > 200_000 {
        return 1_000_000;
    }
    200_000
}

/// 解析 project dir 名 (`-Users-mt-dev2-VibeTerm`) → 原 cwd (`/Users/mt/dev2/VibeTerm`).
/// 简单恢复 (`-` → `/`); 注意路径里如果原本含 `-` 字符会被破坏, 接受不完美.
pub fn project_dir_to_cwd(dir_name: &str) -> String {
    dir_name.replace('-', "/")
}

/// 由 latest jsonl 组装出 ClaudeSession; 文件不可读或没 assistant 行 → None.
/// `cwd_hint`: 已知真 cwd 时传入 (调用方持有), 用于查 ~/.claude.json 做 1M 确定性识别.
fn build_snapshot(path: &Path, project_dir: &str, cwd_hint: Option<&str>) -> Option<ClaudeSession> {
    let parsed = parse_last_assistant(path)?;
    let session_id = parsed.session_id.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let project_path = cwd_hint
        .map(|s| s.to_string())
        .unwrap_or_else(|| project_dir_to_cwd(project_dir));
    let context_window = parsed
        .model
        .as_deref()
        .map(|m| context_window_for(m, Some(project_path.as_str()), parsed.context_tokens))
        .unwrap_or_else(|| {
            if parsed.context_tokens > 200_000 {
                1_000_000
            } else {
                200_000
            }
        });
    Some(ClaudeSession {
        session_id,
        project_path,
        model: parsed.model,
        context_tokens: Some(parsed.context_tokens),
        context_window: Some(context_window),
        session_cost_usd: None,
        cache_5m_until_ms: parsed.cache_5m_until_ms,
        cache_1h_until_ms: parsed.cache_1h_until_ms,
        effort: parsed.effort,
    })
}

/// 同步拉一次"当前活跃 Claude session" — 前端启动时调用.
pub fn read_once() -> Option<ClaudeSession> {
    find_active_session_file()
        .and_then(|(path, project_dir, _)| build_snapshot(&path, &project_dir, None))
}

/// 跨所有 project dir 累加过去 24 小时 (滚动窗口) jsonl 中的 token 总量.
/// 返回 (input + cache_creation + cache_read + output) 之和.
/// 用于状态栏 24h-tokens widget — 重度用户跟踪近 24h 消耗.
///
/// **流式实现**: 用 BufReader 行迭代器, 单文件硬上限 `JSONL_MAX_BYTES`,
/// 防止大 session 文件 (可达数百 MB) 触发 OOM.
pub fn total_tokens_last_24h() -> u64 {
    let Some(root) = projects_root() else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return 0;
    };
    let now_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    )
    .unwrap_or(i64::MAX);
    let cutoff_ms = now_ms - 86_400_000;
    let mut total: u64 = 0;
    for ent in entries.flatten() {
        let p = ent.path();
        if !p.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&p) else {
            continue;
        };
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            if meta.len() > JSONL_MAX_BYTES {
                tracing::debug!(
                    "claude project: skip oversized {} ({} bytes)",
                    fp.display(),
                    meta.len()
                );
                continue;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
                .unwrap_or(0);
            if mtime < cutoff_ms {
                continue;
            }
            total = total.saturating_add(scan_file_tokens(&fp, cutoff_ms));
        }
    }
    total
}

/// 兼容旧名 (deprecated alias) — 语义即过去 24h.
#[deprecated(note = "use total_tokens_last_24h")]
pub fn total_tokens_today() -> u64 {
    total_tokens_last_24h()
}

/// 流式扫单个 jsonl 文件, 累加 cutoff_ms 之后的 assistant 行 usage tokens.
fn scan_file_tokens(fp: &Path, cutoff_ms: i64) -> u64 {
    let Ok(file) = std::fs::File::open(fp) else {
        return 0;
    };
    let reader = BufReader::new(file);
    let mut sub: u64 = 0;
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') || !trimmed.contains("\"type\":\"assistant\"") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ts = v.get("timestamp").and_then(|t| t.as_str()).or_else(|| {
            v.get("message")
                .and_then(|m| m.get("timestamp"))
                .and_then(|t| t.as_str())
        });
        // 缺时间戳或解析失败一律视为"不在 24h 内"而跳过, 否则旧/脏数据会被错误计入.
        let ts_ms = ts.and_then(super::blocks::chrono_parse_iso).unwrap_or(0);
        if ts_ms < cutoff_ms {
            continue;
        }
        let usage = v
            .get("usage")
            .or_else(|| v.get("message").and_then(|m| m.get("usage")));
        if let Some(u) = usage {
            let input = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
            let cache_creation = u
                .get("cache_creation_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let cache_read = u
                .get("cache_read_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let output = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
            sub = sub.saturating_add(input + cache_creation + cache_read + output);
        }
    }
    sub
}

/// 按 cwd 查 Claude session — 用于状态栏的"当前终端"语义.
/// cwd 编码后即 project dir 名 (`/Users/mt/dev2/VibeTerm` → `-Users-mt-dev2-VibeTerm`).
/// 取该 dir 下 mtime 最新的 jsonl, 解析最后 assistant 行.
///
/// **安全**: canonicalize 后必须仍在 `projects_root()` 内, 防止前端构造路径
/// + symlink 把 watcher 引向 ~/.ssh 等敏感目录.
pub fn read_for_cwd(cwd: &str) -> Option<ClaudeSession> {
    let root = projects_root()?;
    let project_dir_name = cwd_to_project_dir(cwd);
    let dir = root.join(&project_dir_name);
    let canon_dir = std::fs::canonicalize(&dir).ok()?;
    let canon_root = std::fs::canonicalize(&root).ok()?;
    if !canon_dir.starts_with(&canon_root) {
        tracing::warn!(
            "claude project: refusing symlink escape {} → {}",
            dir.display(),
            canon_dir.display()
        );
        return None;
    }
    let (path, _) = latest_jsonl_in(&canon_dir)?;
    // 传 cwd_hint 让 build_snapshot 查 ~/.claude.json 做 1M 确定性识别
    build_snapshot(&path, &project_dir_name, Some(cwd))
}

/// 启动 watcher; 立即推一次初值, 之后变更触发重算.
///
/// 选用无界 channel: 生产端速率受下方 200ms debounce 锁死在约 5 条/秒上界, 单条消息小,
/// 消费端 (IPC emit) 处理极快, 不会无界增长; 用有界 channel 反而会引入丢消息/阻塞.
pub fn spawn_watcher(tx: mpsc::UnboundedSender<Option<ClaudeSession>>) {
    // 启动时拉一次
    let _ = tx.send(read_once());

    let Some(root) = projects_root() else {
        return;
    };

    std::thread::spawn(move || {
        let (n_tx, n_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut watcher = match notify::recommended_watcher(n_tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("claude project: watcher init failed: {e}");
                return;
            }
        };
        if !root.exists() {
            tracing::info!(
                "claude project: {} doesn't exist, watcher skipped",
                root.display()
            );
            return;
        }
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            tracing::warn!("claude project: watch failed: {e}");
            return;
        }
        tracing::info!("claude project: watching {}", root.display());

        loop {
            let first = match n_rx.recv() {
                Ok(r) => r,
                Err(_) => return,
            };
            if !is_relevant(&first) {
                continue;
            }
            // debounce
            let deadline = std::time::Instant::now() + Duration::from_millis(200);
            while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
                if n_rx.recv_timeout(remaining).is_err() {
                    break;
                }
            }
            let snapshot = find_active_session_file()
                .and_then(|(path, project_dir, _)| build_snapshot(&path, &project_dir, None));
            if tx.send(snapshot).is_err() {
                return;
            }
        }
    });
}

fn is_relevant(ev: &notify::Result<Event>) -> bool {
    let Ok(ev) = ev else { return false };
    // 任意 .jsonl 的 Create / Modify
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
    fn cwd_encoding() {
        assert_eq!(
            cwd_to_project_dir("/Users/mt/dev2/VibeTerm"),
            "-Users-mt-dev2-VibeTerm"
        );
    }

    #[test]
    fn context_window_inference_paths() {
        // 老模型 (200k) — 不在 GA 1M 列表
        assert_eq!(
            context_window_for("claude-sonnet-4-5", None, 50_000),
            200_000
        );
        assert_eq!(
            context_window_for("claude-haiku-4-5-20251001", None, 50_000),
            200_000
        );
        assert_eq!(
            context_window_for("claude-opus-4-5", None, 100_000),
            200_000
        );
        // GA 1M 模型 — 裸 id 即 1M
        assert_eq!(
            context_window_for("claude-opus-4-6", None, 50_000),
            1_000_000
        );
        assert_eq!(
            context_window_for("claude-opus-4-7", None, 50_000),
            1_000_000
        );
        // opus-4-8(当前 GA 模型, 1M)— ctx < 200k 时也必须判 1M, 否则 ctx% 虚高 5 倍
        assert_eq!(
            context_window_for("claude-opus-4-8", None, 50_000),
            1_000_000
        );
        assert_eq!(
            context_window_for("claude-sonnet-4-6", None, 50_000),
            1_000_000
        );
        // [1m] 显式后缀 — 跟裸 id 一样命中 (startsWith)
        assert_eq!(
            context_window_for("claude-opus-4-7[1m]", None, 50_000),
            1_000_000
        );
        // observed > 200k 兜底 — 即使 model 不在 1M 列表也判 1M (新 model 未列入)
        assert_eq!(
            context_window_for("claude-future-model-xyz", None, 300_000),
            1_000_000
        );
        // observed ≤ 200k 且不在 1M 列表 → 200k
        assert_eq!(
            context_window_for("claude-future-model-xyz", None, 150_000),
            200_000
        );
    }

    #[test]
    fn parses_assistant_line() {
        let jsonl = r#"{"type":"user","message":{"content":"hi"}}
{"type":"assistant","sessionId":"abc-123","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":382,"cache_read_input_tokens":279307,"output_tokens":260}}
{"type":"assistant","sessionId":"abc-123","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":556,"cache_read_input_tokens":279689,"output_tokens":236}}
"#;
        let tmp = std::env::temp_dir().join("vt-test-claude-proj.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let parsed = parse_last_assistant(&tmp).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-7"));
        // 取最后一条: 1 + 556 + 279689 = 280246
        assert_eq!(parsed.context_tokens, 280_246);
        assert_eq!(parsed.session_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn extract_effort_level_parses_hook_attachment() {
        assert_eq!(
            extract_effort_level(r#"{"effort":{"level":"high"}}"#).as_deref(),
            Some("high")
        );
        assert_eq!(
            extract_effort_level(r#"{"x":1,"effort":{"level":"xhigh"},"y":2}"#).as_deref(),
            Some("xhigh")
        );
        // /effort max 期间 hook payload 实测会原样写 "max"(本会话 6 条实证)
        assert_eq!(
            extract_effort_level(r#"{"effort":{"level":"max"}}"#).as_deref(),
            Some("max")
        );
        assert_eq!(
            extract_effort_level(r#"{"text":"talking about effort"}"#),
            None
        );
        assert_eq!(extract_effort_level(r#"{"effort":{"level":""}}"#), None);
    }

    #[test]
    fn parses_effort_from_attachment_latest_wins() {
        // attachment(async_hook_response) 行带 Claude hook 回传的 effort; 晚出现的覆盖早的.
        let jsonl = concat!(
            r#"{"type":"assistant","sessionId":"s1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_read_input_tokens":1000,"output_tokens":10}}"#,
            "\n",
            r#"{"type":"attachment","attachment":{"type":"async_hook_response","response":{"session_id":"s1","permission_mode":"default","effort":{"level":"high"},"hook_event_name":"Stop"}}}"#,
            "\n",
            r#"{"type":"attachment","attachment":{"type":"async_hook_response","response":{"effort":{"level":"xhigh"},"hook_event_name":"Stop"}}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"s1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_read_input_tokens":2000,"output_tokens":10}}"#,
            "\n",
        );
        let tmp = std::env::temp_dir().join("vt-test-claude-effort.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let parsed = parse_last_assistant(&tmp).unwrap();
        assert_eq!(parsed.effort.as_deref(), Some("xhigh"));
        // attachment 行不破坏 assistant 解析
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(parsed.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn no_effort_when_no_attachment() {
        let jsonl = r#"{"type":"assistant","sessionId":"s2","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_read_input_tokens":500,"output_tokens":5}}
"#;
        let tmp = std::env::temp_dir().join("vt-test-claude-noeffort.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let parsed = parse_last_assistant(&tmp).unwrap();
        assert_eq!(parsed.effort, None);
    }

    #[test]
    fn extract_effort_command_parses_slash_effort() {
        let line = r#"{"type":"user","message":{"content":"<command-name>/effort</command-name>\n<local-command-stdout>Set effort level to ultracode (this session only): xhigh + dynamic workflow orchestration</local-command-stdout>"}}"#;
        assert_eq!(extract_effort_command(line).as_deref(), Some("ultracode"));
        assert_eq!(
            extract_effort_command(
                r#"<local-command-stdout>Set effort level to max (this session only): ...</local-command-stdout>"#
            )
            .as_deref(),
            Some("max")
        );
        // 无 local-command-stdout 标记 → 不认, 避免误吃讨论文本
        assert_eq!(
            extract_effort_command(r#"{"text":"聊 Set effort level to ultracode 这个话题"}"#),
            None
        );
    }

    #[test]
    fn effort_command_overrides_attachment_for_ultracode() {
        // /effort ultracode 时 attachment.effort.level 仍是 xhigh(ultracode 底层即 xhigh),
        // 但 /effort 命令回显带字面 ultracode → 应优先取 ultracode.
        let jsonl = concat!(
            r#"{"type":"assistant","sessionId":"s1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_read_input_tokens":1000,"output_tokens":10}}"#,
            "\n",
            r#"{"type":"user","message":{"content":"<command-name>/effort</command-name>\n<local-command-stdout>Set effort level to ultracode (this session only): xhigh + dynamic workflow orchestration</local-command-stdout>"}}"#,
            "\n",
            r#"{"type":"attachment","attachment":{"type":"async_hook_response","response":{"effort":{"level":"xhigh"},"hook_event_name":"Stop"}}}"#,
            "\n",
        );
        let tmp = std::env::temp_dir().join("vt-test-claude-effort-cmd.jsonl");
        std::fs::write(&tmp, jsonl).unwrap();
        let parsed = parse_last_assistant(&tmp).unwrap();
        assert_eq!(parsed.effort.as_deref(), Some("ultracode"));
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-7"));
    }
}
