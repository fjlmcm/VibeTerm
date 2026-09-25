//! Claude 内置模型数据表(model id → 上下文窗口).
//!
//! 数据源是 LiteLLM 社区表(model_prices_and_context_window.json),
//! 不内嵌它的全量 JSON(300KB+, 绝大部分用不上), 只抽 anthropic 原生 claude 条目的
//! `max_input_tokens`. 内嵌快照 `litellm_snapshot.json` 编译进二进制,运行时只读,
//! 由 `scripts/update-model-data.py` 生成,每次发版前刷新一次。
//!
//! **匹配规则**:
//!   - 归一化: lowercase + 去掉 `[1m]` 后缀(transcript 可能带, LiteLLM key 不带)
//!   - 先精确命中, 再最长前缀命中(条目 key 是 model id 的前缀且边界处非字母数字,
//!     处理 `claude-opus-4-8-20991231` 这类带日期后缀的 id)
//!
//! 未匹配的模型返回 None,由调用方决定降级策略.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// LiteLLM 单条目里我们用得到的字段. 其余字段忽略; 个别异形条目 (如 sample_spec)
/// 反序列化失败直接跳过.
#[derive(Deserialize)]
struct LitellmEntry {
    litellm_provider: Option<String>,
    max_input_tokens: Option<u64>,
}

/// LiteLLM 原始条目 map → model id → 上下文窗口. 只收 anthropic 原生 claude 条目,
/// 缺 `max_input_tokens` 的跳过.
fn convert_entries(map: &serde_json::Map<String, serde_json::Value>) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for (key, v) in map {
        if !key.to_ascii_lowercase().starts_with("claude") {
            continue;
        }
        let Ok(e) = serde_json::from_value::<LitellmEntry>(v.clone()) else {
            continue;
        };
        if e.litellm_provider.as_deref() != Some("anthropic") {
            continue;
        }
        if let Some(ctx) = e.max_input_tokens {
            out.insert(key.to_ascii_lowercase(), ctx);
        }
    }
    out
}

/// 内嵌快照文件的包装格式.
#[derive(Deserialize)]
struct SnapshotFile {
    entries: serde_json::Map<String, serde_json::Value>,
}

/// 内置数据表 — 解析内嵌快照, 进程内只做一次. 快照损坏(不应发生, 有测试守门)时 None.
fn builtin_table() -> Option<&'static BTreeMap<String, u64>> {
    static BUILTIN: OnceLock<Option<BTreeMap<String, u64>>> = OnceLock::new();
    BUILTIN
        .get_or_init(|| {
            let snap: SnapshotFile =
                serde_json::from_str(include_str!("litellm_snapshot.json")).ok()?;
            Some(convert_entries(&snap.entries))
        })
        .as_ref()
}

/// 归一化 model id: lowercase + 去掉 `[1m]` 后缀.
fn normalize(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase();
    lower
        .strip_suffix("[1m]")
        .map(|s| s.to_string())
        .unwrap_or(lower)
}

/// 表内查找: 精确命中优先, 否则最长前缀命中(边界处须非字母数字,
/// 防 `claude-fable-50` 误中 `claude-fable-5`).
fn lookup_in(models: &BTreeMap<String, u64>, norm: &str) -> Option<u64> {
    if let Some(ctx) = models.get(norm) {
        return Some(*ctx);
    }
    models
        .iter()
        .filter(|(k, _)| {
            norm.len() > k.len()
                && norm.starts_with(k.as_str())
                && !norm.as_bytes()[k.len()].is_ascii_alphanumeric()
        })
        .max_by_key(|(k, _)| k.len())
        .map(|(_, ctx)| *ctx)
}

/// 模型 id → 上下文窗口 (tokens). 数据缺失时 None, 由调用方做物理推断兜底.
pub fn context_window_of(model: &str) -> Option<u64> {
    let norm = normalize(model);
    builtin_table().and_then(|models| lookup_in(models, &norm))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内嵌快照必须可解析且条目充足 — 守门 scripts/update-model-data.py 的产物.
    #[test]
    fn builtin_snapshot_parses() {
        let t = builtin_table().expect("builtin snapshot must parse");
        assert!(t.len() >= 10, "got {} models", t.len());
    }

    /// 上下文窗口来自数据 — fable/opus-4.8/sonnet-4.6 是 1M, sonnet-4.5/opus-4.5 是 200k.
    #[test]
    fn context_windows_from_data() {
        assert_eq!(context_window_of("claude-fable-5"), Some(1_000_000));
        assert_eq!(context_window_of("claude-opus-4-8"), Some(1_000_000));
        assert_eq!(context_window_of("claude-sonnet-4-6"), Some(1_000_000));
        assert_eq!(context_window_of("claude-sonnet-4-5"), Some(200_000));
        assert_eq!(context_window_of("claude-opus-4-5"), Some(200_000));
        assert_eq!(context_window_of("not-a-model"), None);
    }

    /// 日期后缀 id 走最长前缀命中; 边界检查防误中.
    #[test]
    fn prefix_and_suffix_matching() {
        // 精确条目本就存在
        assert_eq!(
            context_window_of("claude-haiku-4-5-20251001"),
            Some(200_000)
        );
        // 未来日期后缀 → 前缀命中
        assert_eq!(
            context_window_of("claude-opus-4-8-20991231"),
            Some(1_000_000)
        );
        // [1m] 后缀归一化
        assert_eq!(context_window_of("claude-sonnet-4-6[1m]"), Some(1_000_000));
        assert_eq!(context_window_of("claude-opus-4-7[1m]"), Some(1_000_000));
        // 边界: claude-fable-50 不得误中 claude-fable-5
        assert!(context_window_of("claude-fable-50").is_none());
    }

    /// 内嵌快照条目转换:跳过异形条目、非 Anthropic 模型与缺窗口的条目.
    #[test]
    fn converts_litellm_entries() {
        let body = r#"{
            "sample_spec": {"max_tokens": "set to max output tokens"},
            "claude-test-9": {"litellm_provider": "anthropic", "max_input_tokens": 1000000},
            "claude-no-window": {"litellm_provider": "anthropic"},
            "anthropic.claude-test-9": {"litellm_provider": "bedrock_converse", "max_input_tokens": 1000000},
            "gpt-x": {"litellm_provider": "openai", "max_input_tokens": 400000}
        }"#;
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(body).unwrap();
        let models = convert_entries(&map);
        assert_eq!(models.len(), 1, "只收 anthropic 原生 claude 条目");
        assert_eq!(models.get("claude-test-9"), Some(&1_000_000));
    }
}
