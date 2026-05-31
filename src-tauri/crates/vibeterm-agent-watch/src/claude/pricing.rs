//! Claude 模型定价表 — 简化版 + 运行时可覆盖.
//!
//! 不学 ccusage 内嵌 LiteLLM 巨型 JSON (300KB+, 慢编译, 大部分用不上). 这里只列
//! Claude Code 实际触达的模型 (Opus / Sonnet / Haiku 当代几个), 价格 / M token.
//!
//! 超过 200,000 token 提示 (`1m` 上下文模式) 用一档涨价 (`*_above_200k`), 跟 Anthropic
//! pricing page 一致. 来源: https://www.anthropic.com/pricing (2026 年 5 月快照).
//!
//! **两层价格**:
//!   1. `builtin()` —— 编译进二进制的离线快照, 永远兜底 (无网络 / 未更新时用).
//!   2. `OVERRIDE` —— 运行时覆盖表, 由设置·更新页"更新模型价格"手动拉取后
//!      `set_pricing_override` 注入 (主 app 落 `config_dir/pricing.json`). 命中优先于 builtin.
//!
//! 未匹配的模型返回 None — widget 显示 "—" 而非乱估.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

// ---- 运行时价格覆盖 (手动更新) ----

/// 价格覆盖表 — 与 repo 根 / `config_dir/pricing.json` 格式一致.
/// 字段名对齐 `Pricing`, serde 直接反序列化.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingTable {
    /// 价格快照日期, 如 "2026-05".
    pub updated_at: String,
    /// 数据来源描述 (展示用), 如 "Anthropic official pricing".
    pub source: String,
    pub models: PricingModels,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingModels {
    pub opus: Pricing,
    pub sonnet: Pricing,
    pub haiku: Pricing,
}

/// 当前价格来源状态 — 给设置·更新页显示.
#[derive(Debug, Clone, Serialize)]
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

/// 覆盖表查询 (与 builtin 同样 contains 匹配). 覆盖表只含 opus/sonnet/haiku 三档.
fn override_lookup(lower: &str) -> Option<Pricing> {
    let g = OVERRIDE.read().unwrap_or_else(|e| e.into_inner());
    let t = g.as_ref()?;
    if lower.contains("opus") {
        Some(t.models.opus)
    } else if lower.contains("sonnet") {
        Some(t.models.sonnet)
    } else if lower.contains("haiku") {
        Some(t.models.haiku)
    } else {
        None
    }
}

/// 模型 id → 定价. 先查运行时覆盖, 未命中回退内置快照. 匹配规则: 子串 (opus / sonnet / haiku).
pub fn pricing_for(model: &str) -> Option<Pricing> {
    let lower = model.to_ascii_lowercase();
    override_lookup(&lower).or_else(|| builtin(&lower))
}

/// 内置离线快照 (编译期常量, 永远兜底). 来源: Anthropic pricing (2026 年 5 月).
fn builtin(lower: &str) -> Option<Pricing> {
    if lower.contains("opus") {
        // Opus 4.x: $15 / $75
        return Some(Pricing {
            input_per_mtok: 15.0,
            output_per_mtok: 75.0,
            cache_creation_per_mtok: 18.75,
            cache_read_per_mtok: 1.50,
            input_above_200k_per_mtok: Some(22.5),
            output_above_200k_per_mtok: Some(112.5),
            cache_creation_above_200k_per_mtok: Some(28.125),
            cache_read_above_200k_per_mtok: Some(2.25),
        });
    }
    if lower.contains("sonnet") {
        // Sonnet 4.5: $3 / $15
        return Some(Pricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_creation_per_mtok: 3.75,
            cache_read_per_mtok: 0.30,
            input_above_200k_per_mtok: Some(6.0),
            output_above_200k_per_mtok: Some(22.5),
            cache_creation_above_200k_per_mtok: Some(7.5),
            cache_read_above_200k_per_mtok: Some(0.60),
        });
    }
    if lower.contains("haiku") {
        // Haiku 4.5: $1 / $5
        return Some(Pricing {
            input_per_mtok: 1.0,
            output_per_mtok: 5.0,
            cache_creation_per_mtok: 1.25,
            cache_read_per_mtok: 0.10,
            input_above_200k_per_mtok: None,
            output_above_200k_per_mtok: None,
            cache_creation_above_200k_per_mtok: None,
            cache_read_above_200k_per_mtok: None,
        });
    }
    None
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

    #[test]
    fn opus_pricing_matches_anthropic() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        let p = pricing_for("claude-opus-4-7").unwrap();
        assert_eq!(p.input_per_mtok, 15.0);
        assert_eq!(p.output_per_mtok, 75.0);
    }

    #[test]
    fn sonnet_below_200k_normal_rate() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
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
            output_tokens: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };
        let cost = cost_of("claude-sonnet-4-5", u, 300_000).unwrap();
        assert!((cost - 6.0).abs() < 0.01);
    }

    #[test]
    fn unknown_model_returns_none() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        assert!(pricing_for("some-future-model").is_none());
        assert!(cost_of("some-future-model", Usage::default(), 0).is_none());
    }

    #[test]
    fn override_takes_precedence_then_clears() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        clear_pricing_override();
        // 注入一个与内置明显不同的 opus 价(99/199), 验证覆盖生效.
        let bumped = Pricing {
            input_per_mtok: 99.0,
            output_per_mtok: 199.0,
            cache_creation_per_mtok: 1.0,
            cache_read_per_mtok: 1.0,
            input_above_200k_per_mtok: None,
            output_above_200k_per_mtok: None,
            cache_creation_above_200k_per_mtok: None,
            cache_read_above_200k_per_mtok: None,
        };
        set_pricing_override(PricingTable {
            updated_at: "2099-01".into(),
            source: "test".into(),
            models: PricingModels {
                opus: bumped,
                sonnet: bumped,
                haiku: bumped,
            },
        });
        assert_eq!(pricing_for("claude-opus-4-7").unwrap().input_per_mtok, 99.0);
        assert_eq!(pricing_status().source, "override");
        assert_eq!(pricing_status().updated_at.as_deref(), Some("2099-01"));

        // 清除后回内置快照.
        clear_pricing_override();
        assert_eq!(pricing_for("claude-opus-4-7").unwrap().input_per_mtok, 15.0);
        assert_eq!(pricing_status().source, "builtin");
    }
}
