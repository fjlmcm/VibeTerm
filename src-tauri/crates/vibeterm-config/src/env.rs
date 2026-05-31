//! env.toml — 公共环境变量 + 代理
//!
//! 结构:
//!   [env]               — 任意 user env(API key / BASE_URL ...)
//!   [proxy]
//!     enabled = false
//!     http     = "http://..."
//!     https    = "http://..."
//!     no_proxy = "..."
//!
//! 注入逻辑:
//!   proxy.enabled = true → 自动追加 HTTP_PROXY / HTTPS_PROXY / NO_PROXY env
//!   若 [env] 同名 key 已存在 → 用户显式优先(4 层 env 合并)

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvFile {
    #[serde(default = "EnvFile::default_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub proxy: Option<ProxySection>,
    /// 粘贴图片落盘配置(None / 全字段 None → 用代码默认)
    #[serde(default)]
    pub clipboard_images: Option<ClipboardImagesSection>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxySection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub http: Option<String>,
    #[serde(default)]
    pub https: Option<String>,
    #[serde(default)]
    pub no_proxy: Option<String>,
}

/// 粘贴图片临时落盘的位置 + 容量
///
/// - `dir`:绝对路径或 `~/...`;留空 → 用 config_dir 下 `clipboard-images/`
/// - `max_count`:文件数上限,留空 → 200
/// - `max_mb`:总字节上限(MB),留空 → 200
///
/// 全字段都不填等价于不写本 section,行为完全等同上一版本。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardImagesSection {
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub max_count: Option<usize>,
    #[serde(default)]
    pub max_mb: Option<u64>,
}

impl EnvFile {
    fn default_version() -> u32 {
        1
    }

    pub fn load() -> Result<Self, super::ConfigError> {
        let p: PathBuf = super::env_toml_path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(&p)?;
        Ok(toml::from_str(&s)?)
    }

    pub fn save(&self) -> Result<(), super::ConfigError> {
        let p = super::env_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        super::atomic_write(&p, s.as_bytes())
    }

    /// 把 env + proxy 转成 Vec<(K,V)>,供 PTY spawn 使用。
    /// 顺序:proxy(若 enabled)→ env(用户显式优先)
    pub fn to_env_pairs(&self) -> Vec<(String, String)> {
        let mut out: HashMap<String, String> = HashMap::new();
        if let Some(p) = &self.proxy {
            if p.enabled {
                if let Some(v) = &p.http {
                    out.insert("HTTP_PROXY".into(), v.clone());
                }
                if let Some(v) = &p.https {
                    out.insert("HTTPS_PROXY".into(), v.clone());
                }
                if let Some(v) = &p.no_proxy {
                    out.insert("NO_PROXY".into(), v.clone());
                }
            }
        }
        // [env] 显式优先(覆盖 proxy 注入的同名 key)
        for (k, v) in &self.env {
            // std::process::Command::env 对含 '=' 或 NUL 的 key、含 NUL 的 value 会 panic;
            // env.toml 用户可控,跳过非法对并记 warn,避免 PTY spawn 时崩溃。
            if k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0') {
                tracing::warn!(key = %k, "skipping invalid env var (contains '=' or NUL)");
                continue;
            }
            out.insert(k.clone(), v.clone());
        }
        out.into_iter().collect()
    }
}
