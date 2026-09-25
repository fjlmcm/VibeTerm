//! Claude 内置模型数据表(价格 + 上下文窗口)— 按模型 id 匹配.
//!
//! 数据源是 LiteLLM 社区表(model_prices_and_context_window.json, ccusage 同源),
//! 不内嵌它的全量 JSON(300KB+, 绝大部分用不上), 只抽 anthropic 原生 claude 条目:
//!
//! 内嵌快照 `litellm_snapshot.json` 编译进二进制,运行时只读。
//! 由 `scripts/update-model-data.py` 生成,每次发版前刷新一次。
//!
//! **匹配规则**(代替旧版 opus/sonnet/haiku 三档子串匹配 —— 那个区分不出 deprecated
//! 旧价, 也分不出 4.1 与 4.5+ 的 3 倍价差):
//!   - 归一化: lowercase + 去掉 `[1m]` 后缀(transcript 可能带, LiteLLM key 不带)
//!   - 先精确命中, 再最长前缀命中(条目 key 是 model id 的前缀且边界处非字母数字,
//!     处理 `claude-opus-4-8-20991231` 这类带日期后缀的 id)
//!
//! 未匹配的模型返回 None,由调用方决定降级策略.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_creation_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    /// 超 200k 提示档的涨价 (None = 同价). 1M context 用户 token 跨过 200k 后开始适用.
    pub input_above_200k_per_mtok: Option<f64>,
    pub output_above_200k_per_mtok: Option<f64>,
    pub cache_creation_above_200k_per_mtok: Option<f64>,
    pub cache_read_above_200k_per_mtok: Option<f64>,
}

/// 单模型条目: 价格 + 上下文窗口.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    pub pricing: Pricing,
    /// 上下文窗口上限 (tokens), 来自 LiteLLM `max_input_tokens`. 缺数据时 None.
    pub context_window: Option<u64>,
}

// ---- LiteLLM 内嵌快照解析 ----

/// LiteLLM 单条目里我们用得到的字段. 其余字段忽略; 个别异形条目 (如 sample_spec)
/// 反序列化失败直接跳过.
#[derive(Deserialize)]
struct LitellmEntry {
    litellm_provider: Option<String>,
    max_input_tokens: Option<u64>,
    input_cost_per_token: Option<f64>,
    output_cost_per_token: Option<f64>,
    cache_creation_input_token_cost: Option<f64>,
    cache_read_input_token_cost: Option<f64>,
    input_cost_per_token_above_200k_tokens: Option<f64>,
    output_cost_per_token_above_200k_tokens: Option<f64>,
    cache_creation_input_token_cost_above_200k_tokens: Option<f64>,
    cache_read_input_token_cost_above_200k_tokens: Option<f64>,
}

/// LiteLLM 原始条目 map → 按模型 id 的数据表. 只收 anthropic 原生 claude 条目.
/// LiteLLM 单价是 per-token, ×1e6 转 per-Mtok 对齐 `Pricing`.
fn convert_entries(
    map: &serde_json::Map<String, serde_json::Value>,
) -> BTreeMap<String, ModelInfo> {
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
        let (Some(input), Some(output)) = (e.input_cost_per_token, e.output_cost_per_token) else {
            continue;
        };
        let mtok = |x: Option<f64>| x.map(|n| n * 1_000_000.0);
        out.insert(
            key.to_ascii_lowercase(),
            ModelInfo {
                pricing: Pricing {
                    input_per_mtok: input * 1_000_000.0,
                    output_per_mtok: output * 1_000_000.0,
                    cache_creation_per_mtok: mtok(e.cache_creation_input_token_cost).unwrap_or(0.0),
                    cache_read_per_mtok: mtok(e.cache_read_input_token_cost).unwrap_or(0.0),
                    input_above_200k_per_mtok: mtok(e.input_cost_per_token_above_200k_tokens),
                    output_above_200k_per_mtok: mtok(e.output_cost_per_token_above_200k_tokens),
                    cache_creation_above_200k_per_mtok: mtok(
                        e.cache_creation_input_token_cost_above_200k_tokens,
                    ),
                    cache_read_above_200k_per_mtok: mtok(
                        e.cache_read_input_token_cost_above_200k_tokens,
                    ),
                },
                context_window: e.max_input_tokens,
            },
        );
    }
    out
}

// ---- 内嵌快照(scripts/update-model-data.py 生成, 发版前刷新) ----

/// 内嵌快照文件的包装格式.
#[derive(Deserialize)]
struct SnapshotFile {
    entries: serde_json::Map<String, serde_json::Value>,
}

/// 内置数据表 — 解析内嵌快照, 进程内只做一次. 快照损坏(不应发生, 有测试守门)时 None.
fn builtin_table() -> Option<&'static BTreeMap<String, ModelInfo>> {
    static BUILTIN: OnceLock<Option<BTreeMap<String, ModelInfo>>> = OnceLock::new();
    BUILTIN
        .get_or_init(|| {
            let snap: SnapshotFile =
                serde_json::from_str(include_str!("litellm_snapshot.json")).ok()?;
            Some(convert_entries(&snap.entries))
        })
        .as_ref()
}

// ---- 模型查找 ----

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
fn lookup_in(models: &BTreeMap<String, ModelInfo>, norm: &str) -> Option<ModelInfo> {
    if let Some(mi) = models.get(norm) {
        return Some(*mi);
    }
    models
        .iter()
        .filter(|(k, _)| {
            norm.len() > k.len()
                && norm.starts_with(k.as_str())
                && !norm.as_bytes()[k.len()].is_ascii_alphanumeric()
        })
        .max_by_key(|(k, _)| k.len())
        .map(|(_, mi)| *mi)
}

/// 模型 id → 内嵌快照条目.
pub fn model_info_for(model: &str) -> Option<ModelInfo> {
    let norm = normalize(model);
    builtin_table().and_then(|models| lookup_in(models, &norm))
}

/// 模型 id → 定价.
pub fn pricing_for(model: &str) -> Option<Pricing> {
    model_info_for(model).map(|mi| mi.pricing)
}

/// 模型 id → 上下文窗口 (tokens). 数据缺失时 None, 由调用方做物理推断兜底.
pub fn context_window_of(model: &str) -> Option<u64> {
    model_info_for(model).and_then(|mi| mi.context_window)
}

/// 单次 message 用量.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub output_tokens: u64,
}

/// 估算单次 message 成本 (USD). `context_size_at_call` 用来判断是否套用 above-200k 档.
pub fn cost_of(model: &str, u: Usage, context_size_at_call: u64) -> Option<f64> {
    let p = pricing_for(model)?;
    let use_200k = context_size_at_call > 200_000;
    let pick = |normal: f64, above: Option<f64>| -> f64 {
        if use_200k {
            above.unwrap_or(normal)
        } else {
            normal
        }
    };
    let cost = (u.input_tokens as f64 / 1_000_000.0)
        * pick(p.input_per_mtok, p.input_above_200k_per_mtok)
        + (u.output_tokens as f64 / 1_000_000.0)
            * pick(p.output_per_mtok, p.output_above_200k_per_mtok)
        + (u.cache_creation_input_tokens as f64 / 1_000_000.0)
            * pick(
                p.cache_creation_per_mtok,
                p.cache_creation_above_200k_per_mtok,
            )
        + (u.cache_read_input_tokens as f64 / 1_000_000.0)
            * pick(p.cache_read_per_mtok, p.cache_read_above_200k_per_mtok);
    Some(cost)
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

    /// 按模型区分价格 — 旧版 substring 匹配做不到的 (4.1 与 4.5+ 差 3 倍).
    #[test]
    fn per_model_prices_match_anthropic() {
        let opus48 = pricing_for("claude-opus-4-8").unwrap();
        assert_eq!(opus48.input_per_mtok, 5.0);
        assert_eq!(opus48.output_per_mtok, 25.0);
        let opus41 = pricing_for("claude-opus-4-1").unwrap();
        assert_eq!(opus41.input_per_mtok, 15.0);
        assert_eq!(opus41.output_per_mtok, 75.0);
        let fable = pricing_for("claude-fable-5").unwrap();
        assert_eq!(fable.input_per_mtok, 10.0);
        assert_eq!(fable.output_per_mtok, 50.0);
        assert_eq!(fable.cache_read_per_mtok, 1.0);
        let sonnet46 = pricing_for("claude-sonnet-4-6").unwrap();
        assert_eq!(sonnet46.input_per_mtok, 3.0);
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
            pricing_for("claude-haiku-4-5-20251001")
                .unwrap()
                .input_per_mtok,
            1.0
        );
        // 未来日期后缀 → 前缀命中
        assert_eq!(
            pricing_for("claude-opus-4-8-20991231")
                .unwrap()
                .input_per_mtok,
            5.0
        );
        // [1m] 后缀归一化
        assert_eq!(
            pricing_for("claude-sonnet-4-6[1m]").unwrap().input_per_mtok,
            3.0
        );
        assert_eq!(context_window_of("claude-opus-4-7[1m]"), Some(1_000_000));
        // 边界: claude-fable-50 不得误中 claude-fable-5
        assert!(pricing_for("claude-fable-50").is_none());
    }

    #[test]
    fn sonnet_below_200k_normal_rate() {
        let u = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = cost_of("claude-sonnet-4-5", u, 50_000).unwrap();
        assert!((cost - 3.0).abs() < 0.01);
    }

    #[test]
    fn sonnet_above_200k_double_rate() {
        let u = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = cost_of("claude-sonnet-4-5", u, 300_000).unwrap();
        assert!((cost - 6.0).abs() < 0.01);
    }

    /// fable 1M 窗口全程标准价 — 300k 上下文也不得套 above-200k 档.
    #[test]
    fn fable_no_long_context_surcharge() {
        let u = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = cost_of("claude-fable-5", u, 300_000).unwrap();
        assert!((cost - 10.0).abs() < 0.01);
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(pricing_for("some-future-model").is_none());
        assert!(cost_of("some-future-model", Usage::default(), 0).is_none());
    }

    /// 内嵌快照条目转换:跳过异形条目与非 Anthropic 模型.
    #[test]
    fn converts_litellm_entries() {
        let body = r#"{
            "sample_spec": {"max_tokens": "set to max output tokens"},
            "claude-test-9": {
                "litellm_provider": "anthropic",
                "max_input_tokens": 1000000,
                "input_cost_per_token": 1e-05,
                "output_cost_per_token": 5e-05,
                "cache_read_input_token_cost": 1e-06
            },
            "anthropic.claude-test-9": {
                "litellm_provider": "bedrock_converse",
                "input_cost_per_token": 1e-05,
                "output_cost_per_token": 5e-05
            },
            "gpt-x": {"litellm_provider": "openai", "input_cost_per_token": 1e-06, "output_cost_per_token": 2e-06}
        }"#;
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(body).unwrap();
        let models = convert_entries(&map);
        assert_eq!(models.len(), 1, "只收 anthropic 原生 claude 条目");
        let mi = models.get("claude-test-9").unwrap();
        assert_eq!(mi.pricing.input_per_mtok, 10.0);
        assert_eq!(mi.pricing.cache_read_per_mtok, 1.0);
        assert_eq!(mi.context_window, Some(1_000_000));
    }
}
