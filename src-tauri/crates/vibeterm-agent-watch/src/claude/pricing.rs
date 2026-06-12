//! Claude 模型数据表(价格 + 上下文窗口)— 按模型 id 匹配 + 运行时可覆盖.
//!
//! 数据源是 LiteLLM 社区表(model_prices_and_context_window.json, ccusage 同源),
//! 不内嵌它的全量 JSON(300KB+, 绝大部分用不上), 只抽 anthropic 原生 claude 条目:
//!
//! **两层数据**:
//!   1. `builtin()` —— 内嵌快照 `litellm_snapshot.json`(编译进二进制, 永远兜底).
//!      由 `scripts/update-model-data.py` 生成, **每次发版前刷新一次**(发版流程约定).
//!   2. `OVERRIDE` —— 运行时覆盖表, 设置·更新页"更新模型价格"拉取同一数据源后
//!      `set_pricing_override` 注入(主 app 落 `config_dir/pricing.json`). 按模型命中
//!      优先于 builtin; 覆盖表里没有的模型仍回落 builtin.
//!
//! **匹配规则**(代替旧版 opus/sonnet/haiku 三档子串匹配 —— 那个区分不出 deprecated
//! 旧价, 也分不出 4.1 与 4.5+ 的 3 倍价差):
//!   - 归一化: lowercase + 去掉 `[1m]` 后缀(transcript 可能带, LiteLLM key 不带)
//!   - 先精确命中, 再最长前缀命中(条目 key 是 model id 的前缀且边界处非字母数字,
//!     处理 `claude-opus-4-8-20991231` 这类带日期后缀的 id)
//!
//! 未匹配的模型返回 None — widget 显示 "—" 而非乱估.

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
pub struct ModelInfo {
    pub pricing: Pricing,
    /// 上下文窗口上限 (tokens), 来自 LiteLLM `max_input_tokens`. 缺数据时 None.
    pub context_window: Option<u64>,
}

// ---- 数据表 + 运行时覆盖 ----

/// 模型数据表 — 也是 `config_dir/pricing.json` 的 schema(v2, 按模型 id 存).
/// v1(opus/sonnet/haiku 三档)的旧 pricing.json 会反序列化失败 → 启动时忽略并
/// 回落 builtin, 用户点一次"更新模型价格"即重建为 v2.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PricingTable {
    /// 数据快照日期, 如 "2026-06-13".
    pub updated_at: String,
    /// 数据来源描述 (展示用), 如 "LiteLLM (BerriAI/litellm)".
    pub source: String,
    pub models: BTreeMap<String, ModelInfo>,
}

/// 当前价格来源状态 — 给设置·更新页显示.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PricingStatus {
    /// "builtin" | "override"
    pub source: String,
    /// override 表的 updated_at (builtin 时 None)
    pub updated_at: Option<String>,
    /// override 表的 source 描述 (builtin 时 None)
    pub origin: Option<String>,
}

/// 运行时覆盖表. None = 用内置快照. `RwLock::new` 是 const fn → 可直接 static.
static OVERRIDE: RwLock<Option<PricingTable>> = RwLock::new(None);

/// 注入/替换覆盖表 (主 app 加载 `config_dir/pricing.json` 或更新成功后调用).
pub fn set_pricing_override(table: PricingTable) {
    *OVERRIDE.write().unwrap_or_else(|e| e.into_inner()) = Some(table);
}

/// 清除覆盖, 回退内置快照 (设置·更新页"还原内置默认").
pub fn clear_pricing_override() {
    *OVERRIDE.write().unwrap_or_else(|e| e.into_inner()) = None;
}

/// 当前价格来源状态.
pub fn pricing_status() -> PricingStatus {
    let g = OVERRIDE.read().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some(t) => PricingStatus {
            source: "override".into(),
            updated_at: Some(t.updated_at.clone()),
            origin: Some(t.source.clone()),
        },
        None => PricingStatus {
            source: "builtin".into(),
            updated_at: None,
            origin: None,
        },
    }
}

// ---- LiteLLM 解析(builtin 快照与运行时更新共用同一转换器) ----

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

/// 解析 LiteLLM 原始全表(运行时"更新模型价格"用). 抽出的模型数过少视为源格式变更.
pub fn parse_litellm(body: &str, source: &str, updated_at: String) -> Result<PricingTable, String> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;
    let models = convert_entries(&map);
    if models.len() < 10 {
        return Err(format!(
            "only {} anthropic claude entries — source format changed?",
            models.len()
        ));
    }
    Ok(PricingTable {
        updated_at,
        source: source.to_string(),
        models,
    })
}

// ---- 内嵌快照(scripts/update-model-data.py 生成, 发版前刷新) ----

/// 内嵌快照文件的包装格式.
#[derive(Deserialize)]
struct SnapshotFile {
    snapshot_date: String,
    source: String,
    entries: serde_json::Map<String, serde_json::Value>,
}

/// 内置数据表 — 解析内嵌快照, 进程内只做一次. 快照损坏(不应发生, 有测试守门)时 None.
fn builtin_table() -> Option<&'static PricingTable> {
    static BUILTIN: OnceLock<Option<PricingTable>> = OnceLock::new();
    BUILTIN
        .get_or_init(|| {
            let snap: SnapshotFile =
                serde_json::from_str(include_str!("litellm_snapshot.json")).ok()?;
            Some(PricingTable {
                updated_at: snap.snapshot_date,
                source: snap.source,
                models: convert_entries(&snap.entries),
            })
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

/// 模型 id → 条目. 覆盖表按模型命中优先; 覆盖表里没有的模型回落内置快照.
pub fn model_info_for(model: &str) -> Option<ModelInfo> {
    let norm = normalize(model);
    let from_override = {
        let g = OVERRIDE.read().unwrap_or_else(|e| e.into_inner());
        g.as_ref().and_then(|t| lookup_in(&t.models, &norm))
    };
    from_override.or_else(|| builtin_table().and_then(|t| lookup_in(&t.models, &norm)))
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
    use std::sync::Mutex;

    // OVERRIDE 是全局可变 state;cargo test 默认多线程并行, 用串行锁避免互相干扰.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    /// 内嵌快照必须可解析且条目充足 — 守门 scripts/update-model-data.py 的产物.
    #[test]
    fn builtin_snapshot_parses() {
        let t = builtin_table().expect("builtin snapshot must parse");
        assert!(t.models.len() >= 10, "got {} models", t.models.len());
        assert!(!t.updated_at.is_empty());
    }

    /// 按模型区分价格 — 旧版 substring 匹配做不到的 (4.1 与 4.5+ 差 3 倍).
    #[test]
    fn per_model_prices_match_anthropic() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
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
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
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
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
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
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        let u = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = cost_of("claude-sonnet-4-5", u, 50_000).unwrap();
        assert!((cost - 3.0).abs() < 0.01);
    }

    #[test]
    fn sonnet_above_200k_double_rate() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
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
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        let u = Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = cost_of("claude-fable-5", u, 300_000).unwrap();
        assert!((cost - 10.0).abs() < 0.01);
    }

    #[test]
    fn unknown_model_returns_none() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        assert!(pricing_for("some-future-model").is_none());
        assert!(cost_of("some-future-model", Usage::default(), 0).is_none());
    }

    fn test_table(model: &str, input_per_mtok: f64) -> PricingTable {
        let mut models = BTreeMap::new();
        models.insert(
            model.to_string(),
            ModelInfo {
                pricing: Pricing {
                    input_per_mtok,
                    output_per_mtok: input_per_mtok * 2.0,
                    cache_creation_per_mtok: 1.0,
                    cache_read_per_mtok: 1.0,
                    input_above_200k_per_mtok: None,
                    output_above_200k_per_mtok: None,
                    cache_creation_above_200k_per_mtok: None,
                    cache_read_above_200k_per_mtok: None,
                },
                context_window: Some(500_000),
            },
        );
        PricingTable {
            updated_at: "2099-01".into(),
            source: "test".into(),
            models,
        }
    }

    /// 覆盖表按模型命中优先; 覆盖表没有的模型回落 builtin, 不得被整表吞掉.
    #[test]
    fn override_per_model_with_builtin_fallback() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        set_pricing_override(test_table("claude-opus-4-7", 99.0));
        assert_eq!(pricing_for("claude-opus-4-7").unwrap().input_per_mtok, 99.0);
        assert_eq!(context_window_of("claude-opus-4-7"), Some(500_000));
        // override 缺 fable → 回落 builtin
        assert_eq!(pricing_for("claude-fable-5").unwrap().input_per_mtok, 10.0);
        assert_eq!(context_window_of("claude-fable-5"), Some(1_000_000));
        assert_eq!(pricing_status().source, "override");
        assert_eq!(pricing_status().updated_at.as_deref(), Some("2099-01"));

        clear_pricing_override();
        assert_eq!(pricing_for("claude-opus-4-7").unwrap().input_per_mtok, 5.0);
        assert_eq!(pricing_status().source, "builtin");
    }

    /// 旧 v1 pricing.json(opus/sonnet/haiku 三档)必须解析失败 → 启动时忽略回落 builtin.
    #[test]
    fn v1_pricing_json_fails_to_parse() {
        let v1 = r#"{"updated_at":"2026-05-01","source":"LiteLLM","models":{"opus":{"input_per_mtok":15.0,"output_per_mtok":75.0,"cache_creation_per_mtok":18.75,"cache_read_per_mtok":1.5,"input_above_200k_per_mtok":null,"output_above_200k_per_mtok":null,"cache_creation_above_200k_per_mtok":null,"cache_read_above_200k_per_mtok":null},"sonnet":{},"haiku":{}}}"#;
        assert!(serde_json::from_str::<PricingTable>(v1).is_err());
    }

    /// 运行时解析 LiteLLM 原始全表(含异形条目)— 与内嵌快照同一转换器.
    #[test]
    fn parse_litellm_raw_body() {
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
        // 条目过少会被拒 — 这里直接测转换器
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(body).unwrap();
        let models = convert_entries(&map);
        assert_eq!(models.len(), 1, "只收 anthropic 原生 claude 条目");
        let mi = models.get("claude-test-9").unwrap();
        assert_eq!(mi.pricing.input_per_mtok, 10.0);
        assert_eq!(mi.pricing.cache_read_per_mtok, 1.0);
        assert_eq!(mi.context_window, Some(1_000_000));
        // 全表入口: 条目不足报错
        assert!(parse_litellm(body, "test", "2026-06-13".into()).is_err());
    }
}
