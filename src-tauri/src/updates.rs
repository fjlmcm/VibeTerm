//! 软件版本检查:用户手动触发或启用自动检查时调用。
//! 只读 GitHub 发布信息,无上传、遥测或自动安装。

use tauri::AppHandle;
use vibeterm_ipc::{IpcError, IpcResult};

// REST API: 仅在发现新版本时拿一次正式发布说明(release body 是发布后人工注入的).
// 未认证限流 60 次/小时/IP, 代理共享出口下常年耗尽 → 不能当版本检查主路径.
pub(crate) const GH_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/fjlmcm/VibeTerm/releases/latest";
// 版本检查主路径: updater 的 latest.json 是 release 资产(经 CDN 下载), 无 REST API 限流.
pub(crate) const GH_LATEST_JSON_URL: &str =
    "https://github.com/fjlmcm/VibeTerm/releases/latest/download/latest.json";
pub(crate) const GH_RELEASE_TAG_BASE: &str = "https://github.com/fjlmcm/VibeTerm/releases/tag/";

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

/// release 拉取结果: 区分有 release / 仓库尚无 release(404)/ 限流(403/429)/ 网络错误.
pub(crate) enum ReleaseFetch {
    Body(String),
    NoRelease,
    /// GitHub 限流(未认证 60 次/小时/IP, 代理共享出口下很容易耗尽).
    /// 单独归类是为了前端给"稍后重试"而非"检查网络"的文案 —— 这种失败网络是通的.
    RateLimited(String),
    Err(String),
}

/// GET GitHub latest release, 把 404(尚无任何 release)/ 403·429(限流)与真正的网络错误分开.
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
        Err(ureq::Error::Status(code @ (403 | 429), _)) => {
            ReleaseFetch::RateLimited(format!("status {code}"))
        }
        Err(e) => ReleaseFetch::Err(format!("{e}")),
    }
}

/// updater latest.json 里版本检查用得到的字段.
#[derive(serde::Deserialize)]
pub(crate) struct LatestJson {
    pub(crate) version: String,
    pub(crate) notes: Option<String>,
    pub(crate) pub_date: Option<String>,
}

/// 解析 latest.json, 版本号去 'v' 前缀(tauri 生成的是裸版本号, 容错带前缀的).
pub(crate) fn parse_latest_json(body: &str) -> Result<LatestJson, String> {
    let mut latest: LatestJson = serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;
    latest.version = latest.version.trim_start_matches('v').to_string();
    if latest.version.is_empty() {
        return Err("empty version".into());
    }
    Ok(latest)
}

/// 检查软件更新(手动触发或启用自动检查时调用).
/// 主路径 GET updater 的 latest.json(release 资产, 无 REST API 限流)比较版本;
/// 发现新版本时才请求一次 REST API 拿正式发布说明(release body, 发布后人工注入),
/// 拿不到(含限流)静默回落 latest.json 自带的模板 notes —— notes 是锦上添花, 不挡版本检查.
/// 仓库尚无任何 release 时(404)视为"已是最新", 不报错.
#[tauri::command]
pub(crate) async fn check_app_update(app: AppHandle) -> IpcResult<AppUpdateInfo> {
    let current = app.package_info().version.to_string();
    let body = match tokio::task::spawn_blocking(|| http_get_release(GH_LATEST_JSON_URL))
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
        // ":rate_limited:" 是与前端的约定标记(settings-update.tsx 据此切"稍后重试"文案)
        ReleaseFetch::RateLimited(e) => {
            return Err(IpcError::Unknown {
                trace_id: format!("check_app_update:rate_limited:{e}"),
            });
        }
        ReleaseFetch::Err(e) => {
            return Err(IpcError::Unknown {
                trace_id: format!("check_app_update:net:{e}"),
            });
        }
    };
    let latest = parse_latest_json(&body).map_err(|e| IpcError::Unknown {
        trace_id: format!("check_app_update:parse:{e}"),
    })?;
    let has_update = version_gt(&latest.version, &current);

    let mut notes = latest.notes;
    let mut release_url = format!("{GH_RELEASE_TAG_BASE}v{}", latest.version);
    if has_update {
        #[derive(serde::Deserialize)]
        struct GhRelease {
            html_url: String,
            body: Option<String>,
        }
        let fetched = tokio::task::spawn_blocking(|| http_get_release(GH_LATEST_RELEASE_URL))
            .await
            .unwrap_or_else(|e| ReleaseFetch::Err(format!("join: {e}")));
        if let ReleaseFetch::Body(b) = fetched {
            if let Ok(rel) = serde_json::from_str::<GhRelease>(&b) {
                if rel.body.as_deref().is_some_and(|s| !s.trim().is_empty()) {
                    notes = rel.body;
                }
                release_url = rel.html_url;
            }
        }
    }
    Ok(AppUpdateInfo {
        current,
        latest: Some(latest.version),
        has_update,
        release_url: Some(release_url),
        notes,
        published_at: latest.pub_date,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// tauri 生成的 latest.json:裸版本号 + notes + pub_date
    #[test]
    fn parses_updater_latest_json() {
        let body = r###"{"version":"1.1.2","notes":"## VibeTerm v1.1.2","pub_date":"2026-06-12T09:51:32.630Z","platforms":{"darwin-aarch64":{"signature":"x","url":"https://example.com/a.tar.gz"}}}"###;
        let l = parse_latest_json(body).unwrap();
        assert_eq!(l.version, "1.1.2");
        assert_eq!(l.notes.as_deref(), Some("## VibeTerm v1.1.2"));
        assert_eq!(l.pub_date.as_deref(), Some("2026-06-12T09:51:32.630Z"));
    }

    /// 容错带 v 前缀的版本号;空版本 / 非 JSON 报错
    #[test]
    fn latest_json_version_normalization_and_errors() {
        assert_eq!(
            parse_latest_json(r#"{"version":"v2.0.0"}"#)
                .unwrap()
                .version,
            "2.0.0"
        );
        assert!(parse_latest_json(r#"{"version":""}"#).is_err());
        assert!(parse_latest_json("not json").is_err());
    }

    #[test]
    fn version_gt_semverish() {
        assert!(version_gt("1.1.2", "1.1.1"));
        assert!(version_gt("1.2", "1.1.9"));
        assert!(version_gt("2.0.0", "1.9.9"));
        assert!(!version_gt("1.1.2", "1.1.2"));
        assert!(!version_gt("1.1.1", "1.1.2"));
        // 缺段记 0、非数字尾巴截断
        assert!(version_gt("1.1.2", "1.1"));
        assert!(!version_gt("1.1.2-beta", "1.1.2"));
    }
}
