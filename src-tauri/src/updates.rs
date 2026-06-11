//! 手动更新检查(软件版本 / 模型价格)。
//! 🔴 零侵入红线:仅用户点按钮时联网;纯 GET 两个固定 HTTPS 端点;无上传/遥测/后台轮询。
//! 从 main.rs 拆出(行为不变)。

use tauri::AppHandle;
use vibeterm_ipc::{IpcError, IpcResult};

use crate::atomic_write;

// ===== 手动更新检查(软件版本 / 模型价格)=====
// 🔴 零侵入红线: 仅此处、仅用户点按钮时联网; 纯 GET 两个固定 HTTPS 端点;
// 无任何上传 / 遥测; 绝无后台轮询 / 启动自动检查. 价格 override 只落 VibeTerm config 目录.

// 价格源: LiteLLM 社区维护的公开价格表(权威、含 200k 分档、ccusage 同源).
pub(crate) const PRICING_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
pub(crate) const GH_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/fjlmcm/VibeTerm/releases/latest";

/// 同步 GET 一个 HTTPS 文本资源, 带超时 + UA. 仅供手动更新检查用(跑在 spawn_blocking 里).
pub(crate) fn http_get_text(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(6))
        .timeout_read(std::time::Duration::from_secs(12))
        .build();
    agent
        .get(url)
        .set("User-Agent", "VibeTerm")
        .call()
        .map_err(|e| format!("request failed: {e}"))?
        .into_string()
        .map_err(|e| format!("read body failed: {e}"))
}

/// 价格表 sanity 校验: 单价有限、非负、且 < 上限, 防脏数据污染成本估算.
pub(crate) fn validate_pricing(
    t: &vibeterm_agent_watch::claude::pricing::PricingTable,
) -> Result<(), String> {
    use vibeterm_agent_watch::claude::pricing::Pricing;
    let check = |p: &Pricing, name: &str| -> Result<(), String> {
        for v in [
            p.input_per_mtok,
            p.output_per_mtok,
            p.cache_creation_per_mtok,
            p.cache_read_per_mtok,
        ] {
            if !(v.is_finite() && (0.0..100_000.0).contains(&v)) {
                return Err(format!("{name}: price out of range: {v}"));
            }
        }
        Ok(())
    };
    check(&t.models.opus, "opus")?;
    check(&t.models.sonnet, "sonnet")?;
    check(&t.models.haiku, "haiku")?;
    if t.updated_at.is_empty() {
        return Err("missing updated_at".into());
    }
    Ok(())
}

/// 当前模型价格来源状态(内置快照 or 已手动更新的覆盖). 给设置·更新页显示.
#[tauri::command]
pub(crate) async fn get_pricing_status(
) -> IpcResult<vibeterm_agent_watch::claude::pricing::PricingStatus> {
    Ok(vibeterm_agent_watch::claude::pricing::pricing_status())
}

/// 当前日期 YYYY-MM-DD(价格快照时间戳, 本地时区).
pub(crate) fn pricing_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 从 LiteLLM 价格表适配出 opus/sonnet/haiku 当代价格.
/// LiteLLM 单价是 per-token, 这里 ×1e6 转 per-Mtok 对齐 VibeTerm 的 Pricing.
/// 同家族不同版本价格一致 → 取 anthropic 原生、优先当代(4.x)的代表条目.
pub(crate) fn parse_litellm_pricing(
    body: &str,
) -> Result<vibeterm_agent_watch::claude::pricing::PricingTable, String> {
    use vibeterm_agent_watch::claude::pricing::{Pricing, PricingModels, PricingTable};
    let map: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;
    let pick = |family: &str| -> Result<Pricing, String> {
        let mut chosen: Option<&serde_json::Value> = None;
        let mut chosen_modern = false;
        for (k, v) in &map {
            let kl = k.to_ascii_lowercase();
            if !kl.contains(family) {
                continue;
            }
            if v.get("litellm_provider").and_then(|x| x.as_str()) != Some("anthropic") {
                continue;
            }
            if v.get("input_cost_per_token")
                .and_then(|x| x.as_f64())
                .is_none()
            {
                continue;
            }
            // 优先 4.x 当代(claude-opus-4-x / claude-4-opus), 跳过 3.x 老价.
            let modern = kl.contains("-4") || kl.contains("4-");
            if chosen.is_none() || (modern && !chosen_modern) {
                chosen = Some(v);
                chosen_modern = modern;
            }
        }
        let v = chosen.ok_or_else(|| format!("no anthropic {family} entry"))?;
        let mtok = |key: &str| -> Option<f64> {
            v.get(key).and_then(|x| x.as_f64()).map(|n| n * 1_000_000.0)
        };
        let req = |key: &str| -> Result<f64, String> {
            mtok(key).ok_or_else(|| format!("{family}.{key} missing"))
        };
        Ok(Pricing {
            input_per_mtok: req("input_cost_per_token")?,
            output_per_mtok: req("output_cost_per_token")?,
            cache_creation_per_mtok: mtok("cache_creation_input_token_cost").unwrap_or(0.0),
            cache_read_per_mtok: mtok("cache_read_input_token_cost").unwrap_or(0.0),
            input_above_200k_per_mtok: mtok("input_cost_per_token_above_200k_tokens"),
            output_above_200k_per_mtok: mtok("output_cost_per_token_above_200k_tokens"),
            cache_creation_above_200k_per_mtok: mtok(
                "cache_creation_input_token_cost_above_200k_tokens",
            ),
            cache_read_above_200k_per_mtok: mtok("cache_read_input_token_cost_above_200k_tokens"),
        })
    };
    Ok(PricingTable {
        updated_at: pricing_today(),
        source: "LiteLLM (BerriAI/litellm)".to_string(),
        models: PricingModels {
            opus: pick("opus")?,
            sonnet: pick("sonnet")?,
            haiku: pick("haiku")?,
        },
    })
}

/// 手动更新模型价格: GET LiteLLM 价格表 → 适配 opus/sonnet/haiku → 校验 → 原子写 config → 注入覆盖.
/// 仅用户在设置·更新页点击时触发. 失败不影响内置快照.
#[tauri::command]
pub(crate) async fn update_model_pricing(
) -> IpcResult<vibeterm_agent_watch::claude::pricing::PricingStatus> {
    use vibeterm_agent_watch::claude::pricing::{pricing_status, set_pricing_override};
    let body = tokio::task::spawn_blocking(|| http_get_text(PRICING_URL))
        .await
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("update_model_pricing:join:{e}"),
        })?
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("update_model_pricing:net:{e}"),
        })?;
    let table = parse_litellm_pricing(&body).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:adapt:{e}"),
    })?;
    validate_pricing(&table).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:invalid:{e}"),
    })?;
    let path = vibeterm_config::pricing_json_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:path:{e}"),
    })?;
    let pretty = serde_json::to_string_pretty(&table).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:ser:{e}"),
    })?;
    atomic_write(&path, pretty.as_bytes()).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:write:{e}"),
    })?;
    set_pricing_override(table);
    Ok(pricing_status())
}

/// 还原内置默认价格: 删 override 文件 + 清缓存.
#[tauri::command]
pub(crate) async fn reset_model_pricing(
) -> IpcResult<vibeterm_agent_watch::claude::pricing::PricingStatus> {
    if let Ok(path) = vibeterm_config::pricing_json_path() {
        let _ = std::fs::remove_file(&path);
    }
    vibeterm_agent_watch::claude::pricing::clear_pricing_override();
    Ok(vibeterm_agent_watch::claude::pricing::pricing_status())
}

/// 软件版本检查结果(仅展示 + 给下载链接, 不下载安装).
#[derive(serde::Serialize, specta::Type)]
pub(crate) struct AppUpdateInfo {
    current: String,
    latest: Option<String>,
    has_update: bool,
    release_url: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
}

/// release 拉取结果: 区分有 release / 仓库尚无 release(404)/ 网络错误.
pub(crate) enum ReleaseFetch {
    Body(String),
    NoRelease,
    Err(String),
}

/// GET GitHub latest release, 把 404(尚无任何 release)与真正的网络错误分开.
pub(crate) fn http_get_release(url: &str) -> ReleaseFetch {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(6))
        .timeout_read(std::time::Duration::from_secs(12))
        .build();
    match agent.get(url).set("User-Agent", "VibeTerm").call() {
        Ok(r) => match r.into_string() {
            Ok(s) => ReleaseFetch::Body(s),
            Err(e) => ReleaseFetch::Err(format!("read body: {e}")),
        },
        Err(ureq::Error::Status(404, _)) => ReleaseFetch::NoRelease,
        Err(e) => ReleaseFetch::Err(format!("{e}")),
    }
}

/// 手动检查软件更新: GET GitHub latest release, 比较版本. 仅显示 + 给 release 链接.
/// 仓库尚无任何 release 时(404)视为"已是最新", 不报错.
#[tauri::command]
pub(crate) async fn check_app_update(app: AppHandle) -> IpcResult<AppUpdateInfo> {
    let current = app.package_info().version.to_string();
    let body = match tokio::task::spawn_blocking(|| http_get_release(GH_LATEST_RELEASE_URL))
        .await
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("check_app_update:join:{e}"),
        })? {
        ReleaseFetch::Body(b) => b,
        ReleaseFetch::NoRelease => {
            return Ok(AppUpdateInfo {
                current,
                latest: None,
                has_update: false,
                release_url: None,
                notes: None,
                published_at: None,
            });
        }
        ReleaseFetch::Err(e) => {
            return Err(IpcError::Unknown {
                trace_id: format!("check_app_update:net:{e}"),
            });
        }
    };
    #[derive(serde::Deserialize)]
    struct GhRelease {
        tag_name: String,
        html_url: String,
        body: Option<String>,
        published_at: Option<String>,
    }
    let rel: GhRelease = serde_json::from_str(&body).map_err(|e| IpcError::Unknown {
        trace_id: format!("check_app_update:parse:{e}"),
    })?;
    let latest_ver = rel.tag_name.trim_start_matches('v').to_string();
    let has_update = version_gt(&latest_ver, &current);
    Ok(AppUpdateInfo {
        current,
        latest: Some(latest_ver),
        has_update,
        release_url: Some(rel.html_url),
        notes: rel.body,
        published_at: rel.published_at,
    })
}

/// semver-ish 比较 a > b: 按 '.' 分段取前导数字比较, 缺段记 0.
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|x| {
                x.trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}
