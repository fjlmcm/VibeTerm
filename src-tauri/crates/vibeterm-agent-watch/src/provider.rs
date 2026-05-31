//! Provider 抽象层 —— 统一不同 agent (Claude / Codex / 未来) 的用量解析入口。
//!
//! 设计借鉴 CodexBar 的 ProviderFetchPipeline: 每个 provider 跑自己的"源降级链",
//! 并记录每步 [`SourceAttempt`](走了哪个源 / 成功否 / 为何降级), 供 /doctor 诊断。
//! 加新 agent = 实现 [`AgentProvider`] + 在 [`providers`] 注册一行。
//!
//! 本层**包裹**现有的 `claude::` / `codex::` 解析(不重写 parser), 只做统一建模 + 诊断。
//! 现有 per-provider IPC(get_claude_session_by_cwd 等)与前端不受影响, 零回归。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// 会话 context 占用(统一)。`window` 拿不到时 `used_pct` 为 None → 前端显 "—"(不臆造)。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ContextUsage {
    pub used_tokens: Option<u64>,
    pub window: Option<u64>,
    pub used_pct: Option<f64>,
    /// 窗口值来源(诊断用): "rollout"(Codex 权威) / "model-table"(Claude 前缀表兜底) / "none"
    pub window_source: &'static str,
}

/// 额度窗口(统一; 角色靠 `window_minutes` 判, 不靠 primary/secondary 位置)。
#[derive(Debug, Clone, Serialize)]
pub struct QuotaWindow {
    /// "5h" / "7d" / "weekly" 等
    pub label: String,
    pub used_pct: f64,
    pub window_minutes: Option<u64>,
    /// unix 秒; 拿不到为 None
    pub resets_at: Option<i64>,
}

/// 降级链单步诊断 —— 接 /doctor, 让"走了哪个源 / 为何降级"可见。
#[derive(Debug, Clone, Serialize)]
pub struct SourceAttempt {
    /// "transcript" / "usage_cache.json" / "rollout" / "model-table" 等
    pub source: String,
    pub ok: bool,
    pub note: String,
}

/// 统一的 agent 用量快照(provider 抽象输出)。
#[derive(Debug, Clone, Serialize)]
pub struct AgentUsage {
    pub kind: AgentKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub context: Option<ContextUsage>,
    pub quotas: Vec<QuotaWindow>,
    pub cost_usd: Option<f64>,
    /// 降级链诊断
    pub sources: Vec<SourceAttempt>,
}

/// Provider 抽象: 每个 agent 实现, 跑自己的源降级链。
/// 加 agent = 实现本 trait + 在 [`providers`] 注册。
pub trait AgentProvider: Send + Sync {
    fn kind(&self) -> AgentKind;
    /// 按 cwd 解析当前用量(内部跑源链, 记录 attempts)。无任何数据 → None。
    fn resolve_by_cwd(&self, cwd: &str) -> Option<AgentUsage>;
}

fn pct(used: Option<u64>, window: Option<u64>) -> Option<f64> {
    match (used, window) {
        (Some(t), Some(w)) if w > 0 => Some((t as f64 / w as f64 * 100.0).clamp(0.0, 100.0)),
        _ => None,
    }
}

// ---- Claude ----

pub struct ClaudeProvider;

impl AgentProvider for ClaudeProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn resolve_by_cwd(&self, cwd: &str) -> Option<AgentUsage> {
        let mut sources = Vec::new();

        // 源 1: transcript jsonl → model / context / cost
        let sess = crate::claude::project::read_for_cwd(cwd);
        sources.push(SourceAttempt {
            source: "transcript".into(),
            ok: sess.is_some(),
            note: if sess.is_some() {
                "会话 jsonl 解析成功(model/context/cost)".into()
            } else {
                "该 cwd 无活跃 Claude 会话 jsonl".into()
            },
        });
        let sess = sess?;

        let window_source = if sess.context_window.is_some() {
            // 当前 Claude 窗口由模型前缀表推断(transcript 不含权威窗口)
            "model-table"
        } else {
            "none"
        };
        let context = Some(ContextUsage {
            used_tokens: sess.context_tokens,
            window: sess.context_window,
            used_pct: pct(sess.context_tokens, sess.context_window),
            window_source,
        });

        // 源 2: usage_cache.json → 5h/7d 账号级配额(权威)
        let mut quotas = Vec::new();
        match crate::claude::usage_cache::read_once() {
            Some(uc) => {
                sources.push(SourceAttempt {
                    source: "usage_cache.json".into(),
                    ok: true,
                    note: "5h/7d 配额(服务端权威)".into(),
                });
                if let Some(w) = uc.five_hour {
                    quotas.push(QuotaWindow {
                        label: "5h".into(),
                        used_pct: w.utilization,
                        window_minutes: Some(300),
                        resets_at: None,
                    });
                }
                if let Some(w) = uc.seven_day {
                    quotas.push(QuotaWindow {
                        label: "7d".into(),
                        used_pct: w.utilization,
                        window_minutes: Some(10080),
                        resets_at: None,
                    });
                }
            }
            None => sources.push(SourceAttempt {
                source: "usage_cache.json".into(),
                ok: false,
                note: "无 ~/.claude/usage_cache.json(未登录/未用过)".into(),
            }),
        }

        Some(AgentUsage {
            kind: AgentKind::Claude,
            model: sess.model,
            effort: sess.effort,
            context,
            quotas,
            cost_usd: sess.session_cost_usd,
            sources,
        })
    }
}

// ---- Codex ----

pub struct CodexProvider;

impl AgentProvider for CodexProvider {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn resolve_by_cwd(&self, cwd: &str) -> Option<AgentUsage> {
        let mut sources = Vec::new();

        // 唯一源: rollout jsonl → 全字段(权威 model_context_window + rate_limits 内联)
        let snap = crate::codex::session::read_for_cwd(cwd);
        sources.push(SourceAttempt {
            source: "rollout".into(),
            ok: snap.is_some(),
            note: if snap.is_some() {
                "rollout token_count 解析成功(权威 model_context_window + rate_limits)".into()
            } else {
                "该 cwd 无 Codex rollout 会话".into()
            },
        });
        let snap = snap?;

        let context = Some(ContextUsage {
            used_tokens: snap.context_tokens,
            window: snap.context_window,
            // Codex 占用% 已按其 BASELINE 算好, 直接用; 缺则按 used/window 兜算
            used_pct: snap
                .context_used_pct
                .or_else(|| pct(snap.context_tokens, snap.context_window)),
            window_source: if snap.context_window.is_some() {
                "rollout"
            } else {
                "none"
            },
        });

        // 额度: rollout 内联的 primary/secondary, 角色靠 window_minutes 判
        let mut quotas = Vec::new();
        for limit in [snap.primary_limit.as_ref(), snap.secondary_limit.as_ref()]
            .into_iter()
            .flatten()
        {
            let label = match limit.window_minutes {
                Some(300) => "5h",
                Some(10080) => "7d",
                _ => "window",
            };
            quotas.push(QuotaWindow {
                label: label.into(),
                used_pct: limit.used_percent,
                window_minutes: limit.window_minutes,
                resets_at: limit.resets_at,
            });
        }

        Some(AgentUsage {
            kind: AgentKind::Codex,
            model: snap.model,
            effort: snap.effort,
            context,
            quotas,
            cost_usd: None,
            sources,
        })
    }
}

/// Provider 注册表 —— 加新 agent 在此追加一行。
pub fn providers() -> Vec<Box<dyn AgentProvider>> {
    vec![Box::new(ClaudeProvider), Box::new(CodexProvider)]
}

/// 解析指定 agent 在某 cwd 的统一用量(给 IPC / /doctor)。
pub fn resolve(kind: AgentKind, cwd: &str) -> Option<AgentUsage> {
    providers()
        .into_iter()
        .find(|p| p.kind() == kind)
        .and_then(|p| p.resolve_by_cwd(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_claude_and_codex() {
        let ps = providers();
        assert_eq!(ps.len(), 2);
        assert!(ps.iter().any(|p| p.kind() == AgentKind::Claude));
        assert!(ps.iter().any(|p| p.kind() == AgentKind::Codex));
    }

    #[test]
    fn pct_clamps_and_guards_zero_window() {
        assert_eq!(pct(Some(100), Some(1000)), Some(10.0));
        assert_eq!(pct(Some(100), Some(0)), None);
        assert_eq!(pct(None, Some(1000)), None);
        assert_eq!(pct(Some(2000), Some(1000)), Some(100.0)); // clamp
    }

    #[test]
    fn agent_kind_as_str() {
        assert_eq!(AgentKind::Claude.as_str(), "claude");
        assert_eq!(AgentKind::Codex.as_str(), "codex");
    }
}
