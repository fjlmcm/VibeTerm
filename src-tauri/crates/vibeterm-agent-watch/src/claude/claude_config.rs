//! 读 `~/.claude.json` — Claude Code CLI 全局 state 文件(只读).
//!
//! 用途: 解析用户订阅 plan 显示名(`plan_label`),数据来自 `oauthAccount`.

use std::path::PathBuf;

/// `~/.claude.json` 通常 < 1MB. 16MB 是宽松上限, 超过基本是异常或攻击载荷.
/// 防止深嵌套 JSON 触发 serde_json 栈溢出 / 内存暴涨.
const CLAUDE_CONFIG_MAX_BYTES: u64 = 16 * 1024 * 1024;

fn claude_config_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude.json"))
}

/// 同步读 ~/.claude.json. ~90KB 文件, 偶尔读不会引入显著开销.
/// 失败返回 None (新装用户 / 文件不存在 / 超大异常).
fn read_config() -> Option<serde_json::Value> {
    let path = claude_config_path()?;
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > CLAUDE_CONFIG_MAX_BYTES {
        tracing::warn!(
            "claude_config: refusing oversized ~/.claude.json ({} bytes > cap {})",
            meta.len(),
            CLAUDE_CONFIG_MAX_BYTES
        );
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 解析用户 Claude 订阅 plan, 返回简短显示名 (例如 `Max 20x` / `Pro` / `Free`).
/// 数据来自 `~/.claude.json.oauthAccount.{organizationType, organizationRateLimitTier}`.
/// 未登录或读不到返回 None.
pub fn plan_label() -> Option<String> {
    let config = read_config()?;
    let acct = config.get("oauthAccount")?;
    let org_type = acct
        .get("organizationType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tier = acct
        .get("organizationRateLimitTier")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // tier 形如 default_claude_max_20x / default_claude_max_5x / default_claude_pro
    let parse_max_multi = || -> Option<&'static str> {
        if tier.contains("20x") {
            Some("Max 20x")
        } else if tier.contains("5x") {
            Some("Max 5x")
        } else {
            None
        }
    };
    let label = match org_type {
        "claude_max" => parse_max_multi().unwrap_or("Max").to_string(),
        "claude_pro" => "Pro".to_string(),
        "claude_free" => "Free".to_string(),
        "claude_team" => "Team".to_string(),
        "claude_enterprise" => "Enterprise".to_string(),
        other if !other.is_empty() => {
            // 兜底: 去掉 "claude_" 前缀 + 首字母大写
            let stripped = other.strip_prefix("claude_").unwrap_or(other);
            let mut chars = stripped.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        }
        _ => return None,
    };
    Some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_in_home() {
        let p = claude_config_path().unwrap();
        assert!(p.ends_with(".claude.json"));
    }
}
