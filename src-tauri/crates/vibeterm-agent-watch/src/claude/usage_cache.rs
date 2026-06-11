//! 监听 `~/.claude/usage_cache.json` — Claude Code 主进程定期写入的服务端 quota 快照.
//!
//! 文件格式参见 lib.rs 的 UsageCache. 文件由 Claude 用 `.lock` 做原子写入,
//! 我们只需要:
//!   1. 启动时尝试读一次 (用户已有 Claude session 时立即有值)
//!   2. notify watcher 监听变更, debounce 100ms 后重读
//!   3. 文件不存在不算错, 返回 None (用户没装 Claude 或新装)
//!
//! 不写文件, 不联网, 不需要权限.

use std::path::PathBuf;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::UsageCache;

/// 解析的事件 — None 表示文件不存在或读取失败 (前端应显示 "—").
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct UsageCacheUpdate {
    pub cache: Option<UsageCache>,
    pub mtime_ms: i64,
}

/// 返回 usage_cache.json 路径; 不保证存在.
pub fn cache_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".claude").join("usage_cache.json"))
}

/// usage_cache.json 通常 < 50KB. 4MB 是宽松上限, 防异常文件 OOM.
const USAGE_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// 同步读一次 cache 文件; 文件不存在或解析失败都返回 None.
/// 调用方应只在 Claude 可能已用过的场景下调用 (UI 显示降级).
pub fn read_once() -> Option<UsageCache> {
    let path = cache_path()?;
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > USAGE_CACHE_MAX_BYTES {
        tracing::warn!("usage_cache: refusing oversized {} bytes", meta.len());
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice::<UsageCache>(&bytes)
        .map_err(|e| {
            tracing::warn!("usage_cache parse failed: {e}");
            e
        })
        .ok()
}

fn read_with_mtime() -> UsageCacheUpdate {
    let Some(path) = cache_path() else {
        return UsageCacheUpdate {
            cache: None,
            mtime_ms: 0,
        };
    };
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => {
            return UsageCacheUpdate {
                cache: None,
                mtime_ms: 0,
            }
        }
    };
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let cache = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<UsageCache>(&b).ok());
    UsageCacheUpdate { cache, mtime_ms }
}

/// 启动 watcher; 通过 `tx` 推送 update. 后台 tokio task 持有 watcher 直到 sender 关闭.
/// 启动时立刻推一次当前值 (即使是 None) 让前端拿到初始状态.
pub fn spawn_watcher(tx: mpsc::UnboundedSender<UsageCacheUpdate>) {
    // 立刻推一次初值
    let initial = read_with_mtime();
    let _ = tx.send(initial);

    let Some(path) = cache_path() else {
        tracing::info!("usage_cache: no $HOME, watcher skipped");
        return;
    };
    // 监听 ~/.claude 目录 (文件可能尚未创建) 而非具体文件
    let watch_dir = match path.parent() {
        Some(p) => p.to_path_buf(),
        None => return,
    };

    // 用 std::thread 而非 tokio spawn_blocking: setup 阶段可能还没进 tokio runtime,
    // 且 watcher 是纯阻塞 loop, 不需要 tokio 调度.
    std::thread::spawn(move || {
        let (notify_tx, notify_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut watcher = match notify::recommended_watcher(notify_tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("usage_cache: watcher init failed: {e}");
                return;
            }
        };
        // 目录可能不存在 (用户从未跑过 Claude); 这种情况下直接返回, 不报错
        if !watch_dir.exists() {
            tracing::info!(
                "usage_cache: {} doesn't exist, watcher skipped",
                watch_dir.display()
            );
            return;
        }
        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::warn!("usage_cache: watch {} failed: {e}", watch_dir.display());
            return;
        }
        tracing::info!("usage_cache: watching {}", watch_dir.display());

        // 简单 debounce: 收到事件后等 100ms, 期间继续吸收事件
        let target_name = path.file_name().map(|n| n.to_os_string());
        loop {
            // 阻塞等第一个事件
            let first = match notify_rx.recv() {
                Ok(r) => r,
                Err(_) => return, // channel 关闭
            };
            if !is_relevant(&first, &target_name) {
                continue;
            }
            // debounce 窗口
            let deadline = std::time::Instant::now() + Duration::from_millis(100);
            while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
                if let Ok(ev) = notify_rx.recv_timeout(remaining) {
                    let _ = ev; // 吸收掉, 不关心内容
                } else {
                    break;
                }
            }
            let update = read_with_mtime();
            if tx.send(update).is_err() {
                tracing::info!("usage_cache: receiver dropped, stopping watcher");
                return;
            }
        }
    });
}

fn is_relevant(ev: &notify::Result<Event>, target_name: &Option<std::ffi::OsString>) -> bool {
    let Ok(ev) = ev else { return false };
    // 我们关心: usage_cache.json 的 Create / Modify / Remove
    // 但 atomic write 会触发 .lock 文件事件 — 这些不相关
    let Some(name) = target_name.as_ref() else {
        return true;
    };
    let matches_target = ev
        .paths
        .iter()
        .any(|p| p.file_name().map(|n| n == name).unwrap_or(false));
    matches_target
        && matches!(
            ev.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_uses_home() {
        let p = cache_path();
        assert!(p.is_some());
        let s = p.unwrap();
        assert!(s.ends_with(".claude/usage_cache.json"));
    }

    #[test]
    fn parses_sample_json() {
        let sample = r#"{"five_hour":{"utilization":6.0,"resets_at":"2026-05-27T17:20:00.627649+00:00"},"seven_day":{"utilization":19.0,"resets_at":"2026-06-01T09:00:00.627668+00:00"},"seven_day_oauth_apps":null,"seven_day_opus":null,"seven_day_sonnet":{"utilization":0.0,"resets_at":null},"extra_usage":{"is_enabled":false,"monthly_limit":null,"used_credits":null,"utilization":null,"currency":null,"disabled_reason":null}}"#;
        let parsed: UsageCache = serde_json::from_str(sample).expect("parse");
        assert_eq!(parsed.five_hour.as_ref().unwrap().utilization, 6.0);
        assert_eq!(parsed.seven_day.as_ref().unwrap().utilization, 19.0);
        assert_eq!(parsed.seven_day_sonnet.as_ref().unwrap().utilization, 0.0);
        assert!(parsed
            .seven_day_sonnet
            .as_ref()
            .unwrap()
            .resets_at
            .is_none());
        assert!(!parsed.extra_usage.as_ref().unwrap().is_enabled);
    }
}
