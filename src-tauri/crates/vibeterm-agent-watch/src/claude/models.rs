//! Claude 模型 id → 默认上下文窗口.
//!
//! 只需要回答"200k 还是 1M"一个问题, 按版本号规则判定, 不再内嵌任何外部数据表:
//!   - `[1m]` 后缀(Claude Code 显式开 1M 会话)→ 1M
//!   - fable / mythos 系列 → 1M
//!   - 版本 >= 4.6(opus-4-6 / sonnet-4-6 / opus-4-7 / 4-8 / 5.x …)→ 1M
//!   - 其余(sonnet-4-5 / opus-4-5 / haiku-4-5 / 4.1 / 3.x …)→ 200k
//!
//! 之所以按"默认"而非 API 上限: transcript 里裸 id 就是 Claude Code 的默认会话,
//! 1M 会话一定带 `[1m]`. 照抄 LiteLLM 的 max_input_tokens(开 beta 后的上限)会让
//! ctx% 低 5 倍. 解析不出版本号(非 claude 模型 / 无数字段)返回 None, 由调用方按
//! 已观测的上下文用量做物理推断兜底.

const CTX_200K: u64 = 200_000;
const CTX_1M: u64 = 1_000_000;

/// 从 `claude-…` id 里取 (major, minor). 版本段是 1~2 位数字(8 位数字是日期后缀, 不算);
/// 第一个版本段是 major, 紧随其后的版本段是 minor, 没有则 0.
/// 兼容两种排列: `claude-opus-4-8-20260416` 与老式 `claude-3-7-sonnet-20250219`.
fn parse_version(norm: &str) -> Option<(u32, u32)> {
    let is_ver = |s: &str| (1..=2).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    let mut segs = norm.strip_prefix("claude-")?.split('-');
    let major = segs.find(|s| is_ver(s))?.parse().ok()?;
    let minor = segs
        .next()
        .filter(|s| is_ver(s))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

/// 模型 id → 默认上下文窗口 (tokens). 无法判定时 None.
pub fn context_window_of(model: &str) -> Option<u64> {
    let lower = model.trim().to_ascii_lowercase();
    if !lower.starts_with("claude") {
        return None;
    }
    let (norm, one_m_suffix) = match lower.strip_suffix("[1m]") {
        Some(s) => (s, true),
        None => (lower.as_str(), false),
    };
    if one_m_suffix || norm.starts_with("claude-fable") || norm.starts_with("claude-mythos") {
        return Some(CTX_1M);
    }
    let (major, minor) = parse_version(norm)?;
    Some(if (major, minor) >= (4, 6) {
        CTX_1M
    } else {
        CTX_200K
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_rule_1m_vs_200k() {
        // 1M: 4.6 及以后、5.x、fable / mythos
        for m in [
            "claude-opus-4-6",
            "claude-opus-4-6-20260205",
            "claude-sonnet-4-6",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-opus-4-8-20991231",
            "claude-opus-5",
            "claude-opus-5-5",
            "claude-sonnet-5",
            "claude-fable-5",
            "claude-fable-5-1",
            "claude-mythos-5-1",
            "claude-mythos-preview",
        ] {
            assert_eq!(context_window_of(m), Some(CTX_1M), "{m}");
        }
        // 200k: 4.5 及以前(含老式 3.x 排列)
        for m in [
            "claude-sonnet-4-5",
            "claude-sonnet-4-5-20250929",
            "claude-opus-4-5",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-1",
            "claude-opus-4-1-20250805",
            "claude-4-opus-20250514",
            "claude-3-7-sonnet-20250219",
            "claude-3-haiku-20240307",
        ] {
            assert_eq!(context_window_of(m), Some(CTX_200K), "{m}");
        }
    }

    #[test]
    fn one_m_suffix_and_unknown() {
        // [1m] 后缀无条件 1M, 即便基础型号默认 200k
        assert_eq!(context_window_of("claude-sonnet-4-5[1m]"), Some(CTX_1M));
        assert_eq!(context_window_of("Claude-Sonnet-4-6[1M]"), Some(CTX_1M));
        // 解析不出版本 / 非 claude → None
        assert!(context_window_of("not-a-model").is_none());
        assert!(context_window_of("claude-code-tool").is_none());
        assert!(context_window_of("").is_none());
    }

    #[test]
    fn version_parsing_ignores_date_suffix() {
        assert_eq!(parse_version("claude-opus-4-8-20260416"), Some((4, 8)));
        assert_eq!(parse_version("claude-3-7-sonnet-20250219"), Some((3, 7)));
        assert_eq!(parse_version("claude-3-haiku-20240307"), Some((3, 0)));
        assert_eq!(parse_version("claude-opus-5"), Some((5, 0)));
        assert_eq!(parse_version("claude-sonnet-20250101"), None);
    }
}
