//! 配置加载 / 持久化 / 主题
//!
//! 范围:
//!   - 配置目录 + atomic write helper
//!   - config.toml 读 / 主题激活项
//!   - themes/*.toml 读 + 内置 2 套(Vibe / Catppuccin Mocha)
//!   - notify watcher(50ms debounce)

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub mod actions;
pub mod env;
pub mod keybindings;
pub mod notify_prefs;
pub mod prompts;
pub mod statusline;
pub mod theme;

pub use env::{ClipboardImagesSection, EnvFile, ProxySection};
pub use keybindings::{KeybindingEntry, KeybindingsFile};
pub use notify_prefs::{EventNotifyPrefs, EventsPrefs, NotifyFile, QuietHours};
pub use prompts::{PromptEntry, PromptsFile};
pub use statusline::{ProfileConfig, StatusLineFile, StatusLineItem, StatusLineItemDetail};
pub use theme::{Theme, ThemeShell, ThemeTerminal};

// ---- 错误 ----
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("notify: {0}")]
    Notify(#[from] notify::Error),
    #[error("no data dir")]
    NoDataDir,
    #[error("invalid: {0}")]
    Invalid(String),
}

// ---- 配置目录 ----
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    // 测试友好:VIBETERM_CONFIG_DIR 环境变量可重定向 — 仅 debug 构建生效,
    // 防 release 二进制因继承的环境变量(被恶意 shell / dotenv 注入)
    // 导致 save_statusline_config 写到任意攻击者控制路径.
    #[cfg(debug_assertions)]
    if let Ok(override_path) = std::env::var("VIBETERM_CONFIG_DIR") {
        let dir = PathBuf::from(override_path);
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        return Ok(dir);
    }
    let base = dirs::data_dir().ok_or(ConfigError::NoDataDir)?;
    let dir = base.join("VibeTerm");
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn themes_dir() -> Result<PathBuf, ConfigError> {
    let d = config_dir()?.join("themes");
    if !d.exists() {
        std::fs::create_dir_all(&d)?;
    }
    Ok(d)
}

pub fn tasks_json_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("tasks.json"))
}

pub fn config_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn env_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("env.toml"))
}

pub fn keybindings_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("keybindings.toml"))
}

pub fn prompts_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("prompts.toml"))
}

pub fn actions_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("actions.toml"))
}

pub fn statusline_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("statusline.toml"))
}

pub fn notify_toml_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("notify.toml"))
}

/// 模型价格覆盖文件(设置·更新页"更新模型价格"手动拉取后落盘处)。
/// 不存在 = 用编译进二进制的内置价格快照。仅 VibeTerm 自己的 config 目录,不碰 agent 配置。
pub fn pricing_json_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("pricing.json"))
}

/// 粘贴图片临时目录(自动创建)。
///
/// 优先级:env.toml `[clipboard_images].dir` > `<config_dir>/clipboard-images/`。
/// 支持 `~/...` 前缀展开;相对路径相对 `config_dir()`。
pub fn clipboard_images_dir() -> Result<PathBuf, ConfigError> {
    let d = resolve_clipboard_images_dir()?;
    if !d.exists() {
        std::fs::create_dir_all(&d)?;
    }
    Ok(d)
}

fn resolve_clipboard_images_dir() -> Result<PathBuf, ConfigError> {
    let env_file = EnvFile::load().unwrap_or_default();
    let configured = env_file
        .clipboard_images
        .as_ref()
        .and_then(|c| c.dir.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(s) = configured {
        let candidate = expand_user_path(s);
        return validate_clipboard_images_dir(candidate).or_else(|_| default_clipboard_images_dir());
    }
    default_clipboard_images_dir()
}

/// 粘贴图片的默认目录,**必须无空格** —— 路径会被裸注入 PTY 喂给 agent CLI,只有无空格裸路径
/// 才能被 claude code / codex 自动识别为图片附件。config_dir 在 macOS 是
/// `~/Library/Application Support/VibeTerm`(含空格),裸注入会让 claude code 把断裂的路径当
/// 普通文本(codex 容错能 unquote 带引号路径,claude code 不能)。故改落到无空格的 cache 目录
/// `~/Library/Caches/com.vibeterm.desktop/clipboard-images`(Linux: `~/.cache/...`)。
/// 注:用户名本身含空格(`/Users/John Doe/...`)的极少数情况无法靠换目录消除,属已知边界。
fn default_clipboard_images_dir() -> Result<PathBuf, ConfigError> {
    if let Some(base) = dirs::cache_dir() {
        return Ok(base.join("com.vibeterm.desktop").join("clipboard-images"));
    }
    // cache_dir 不可用(极罕见)→ 退回 config_dir,至少能落盘(可能含空格)。
    Ok(config_dir()?.join("clipboard-images"))
}

/// 校验用户配置的粘贴图片目录:必须位于 home_dir 或 config_dir 之内,
/// 防止渲染进程被攻陷后经 save_env_file 把落盘/清理路径重定向到任意敏感目录。
///
/// 目录可能尚未创建,故对"最近的已存在祖先"做 canonicalize 后再用 starts_with
/// 校验(对齐 git/cwd 路径校验的 canonicalize + starts_with 范式),
/// 这样既能解析 `..`/符号链接,又不要求目标目录已存在。
fn validate_clipboard_images_dir(candidate: PathBuf) -> Result<PathBuf, ConfigError> {
    let mut allowed: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        if let Ok(c) = home.canonicalize() {
            allowed.push(c);
        }
    }
    if let Ok(cfg) = config_dir() {
        if let Ok(c) = cfg.canonicalize() {
            allowed.push(c);
        }
    }
    if allowed.is_empty() {
        // 无法确定任何允许前缀时,拒绝用户路径(交由调用方回退默认目录)。
        return Err(ConfigError::Invalid(
            "cannot resolve allowed base dirs for clipboard_images.dir".into(),
        ));
    }

    // 找到 candidate 中最近的已存在祖先并 canonicalize,再拼回剩余的未创建部分。
    let mut existing = candidate.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    let canon_base = existing.canonicalize().map_err(ConfigError::Io)?;
    let suffix = candidate.strip_prefix(existing).unwrap_or(&candidate);
    let resolved = canon_base.join(suffix);

    if allowed.iter().any(|base| resolved.starts_with(base)) {
        Ok(resolved)
    } else {
        tracing::warn!(
            dir = ?candidate,
            "clipboard_images.dir escapes home/config dir, falling back to default",
        );
        Err(ConfigError::Invalid(
            "clipboard_images.dir must reside under home or config dir".into(),
        ))
    }
}

fn expand_user_path(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(s)
}

/// 临时图片清理上限(代码默认 200 张 / 200MB,FIFO 淘汰)。
/// 用户可通过 env.toml `[clipboard_images].max_count` / `max_mb` 覆盖。
pub const CLIPBOARD_IMAGES_MAX_COUNT: usize = 200;
pub const CLIPBOARD_IMAGES_MAX_BYTES: u64 = 200 * 1024 * 1024;

/// 从 env.toml 读取生效的上限,缺省字段取常量。
pub fn clipboard_images_caps() -> (usize, u64) {
    let env_file = EnvFile::load().unwrap_or_default();
    let section = env_file.clipboard_images.unwrap_or_default();
    let max_count = section.max_count.unwrap_or(CLIPBOARD_IMAGES_MAX_COUNT);
    let max_bytes = section
        .max_mb
        .map(|mb| mb.saturating_mul(1024).saturating_mul(1024))
        .unwrap_or(CLIPBOARD_IMAGES_MAX_BYTES);
    (max_count, max_bytes)
}

/// 保存粘贴的图片到 clipboard_images_dir,自动 FIFO 清理。
///
/// 用代码默认上限。函数可以注入 `dir` 与 `now_ms`,方便单测;
/// 生产入口见 [`save_clipboard_image_default`]。
pub fn save_clipboard_image_in(
    dir: &Path,
    bytes: &[u8],
    now_ms: u128,
) -> Result<PathBuf, ConfigError> {
    save_clipboard_image_in_with_caps(
        dir,
        bytes,
        now_ms,
        CLIPBOARD_IMAGES_MAX_COUNT,
        CLIPBOARD_IMAGES_MAX_BYTES,
    )
}

/// 与 [`save_clipboard_image_in`] 相同,但 caps 可注入(供生产路径从
/// env.toml 读出后传入)。
pub fn save_clipboard_image_in_with_caps(
    dir: &Path,
    bytes: &[u8],
    now_ms: u128,
    max_count: usize,
    max_bytes: u64,
) -> Result<PathBuf, ConfigError> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let new_len = bytes.len() as u64;

    // 列已存在文件 + mtime
    let mut entries: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("png") {
            continue;
        }
        let md = entry.metadata()?;
        let mtime = md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        entries.push((p, mtime, md.len()));
    }
    entries.sort_by_key(|(_, t, _)| *t);

    let mut total: u64 = entries.iter().map(|(_, _, l)| *l).sum();
    let mut count = entries.len();
    let mut i = 0;
    while (count + 1 > max_count || total + new_len > max_bytes) && i < entries.len() {
        let (p, _, l) = &entries[i];
        if let Err(e) = std::fs::remove_file(p) {
            // 淘汰受阻:新文件仍会写入(best-effort 软配额),记录当前/目标差距便于诊断。
            tracing::warn!(
                path = ?p,
                err = %e,
                count, max_count, total, new_len, max_bytes,
                "clipboard-image evict failed; caps may be temporarily exceeded",
            );
            break;
        }
        total = total.saturating_sub(*l);
        count = count.saturating_sub(1);
        i += 1;
    }

    let target = dir.join(format!("{now_ms}.png"));
    std::fs::write(&target, bytes)?;
    Ok(target)
}

/// 生产入口:目录与上限都从 env.toml 解析(缺省走常量)。
pub fn save_clipboard_image_default(bytes: &[u8]) -> Result<PathBuf, ConfigError> {
    let dir = clipboard_images_dir()?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        // 时钟早于 Unix 纪元时取偏移绝对值,避免退化为固定 0.png 造成覆盖。
        .unwrap_or_else(|e| e.duration())
        .as_millis();
    let (max_count, max_bytes) = clipboard_images_caps();
    save_clipboard_image_in_with_caps(&dir, bytes, now_ms, max_count, max_bytes)
}

/// 直接写到指定路径(命名由调用方决定,例如内容 hash);
/// 不做 FIFO 清理,清理由 [`enforce_clipboard_images_caps`] 单独调度。
pub fn save_clipboard_image_at(target: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(target, bytes)?;
    Ok(())
}

/// 对 `dir` 下 *.png 应用 FIFO 上限(按 mtime 升序删除最旧)。
/// 与 [`save_clipboard_image_at`] 解耦:hash 命名场景下也能复用同一清理逻辑。
pub fn enforce_clipboard_images_caps(
    dir: &Path,
    max_count: usize,
    max_bytes: u64,
) -> Result<usize, ConfigError> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut entries: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("png") {
            continue;
        }
        let md = entry.metadata()?;
        let mtime = md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        entries.push((p, mtime, md.len()));
    }
    entries.sort_by_key(|(_, t, _)| *t);

    let mut total: u64 = entries.iter().map(|(_, _, l)| *l).sum();
    let mut count = entries.len();
    let mut removed = 0usize;
    let mut i = 0;
    while (count > max_count || total > max_bytes) && i < entries.len() {
        let (p, _, l) = &entries[i];
        if let Err(e) = std::fs::remove_file(p) {
            tracing::warn!(path = ?p, err = %e, "clipboard-image evict failed");
            break;
        }
        total = total.saturating_sub(*l);
        count = count.saturating_sub(1);
        removed += 1;
        i += 1;
    }
    Ok(removed)
}

// ---- atomic write ----
// macOS / Linux:tempfile + fsync + rename(POSIX 原子)
// Windows:
//   - 优先用 tempfile.persist()(底层走 MoveFileEx with REPLACE_EXISTING)
//   - 失败 → 3 次 retry 间隔 50ms(OneDrive / 杀软 / 网络盘短暂占用)
//   - 仍失败 → fallback "truncate + write"(非原子,记 warn log)
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Invalid(format!("no parent: {path:?}")))?;
    use std::io::Write;

    #[cfg(not(target_os = "windows"))]
    {
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|e| ConfigError::Io(e.error))?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        // Windows path
        for attempt in 0..3 {
            let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
            tmp.write_all(bytes)?;
            tmp.as_file().sync_all()?;
            match tmp.persist(path) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::warn!(
                        attempt,
                        err = %e.error,
                        "atomic rename failed (likely OneDrive / AV / locked), retrying"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    // tmp 自动 drop 清理
                }
            }
        }
        // Fallback: truncate + write(非原子,极端场景)
        tracing::warn!(
            ?path,
            "atomic rename retries exhausted, falling back to truncate+write"
        );
        let mut f = std::fs::File::create(path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    }
}

// ---- config.toml ----
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "Config::default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "Config::default_active_theme")]
    pub active_theme: String,
    #[serde(default)]
    pub follow_system_theme: bool,
    #[serde(default)]
    pub language: Option<String>, // "zh-CN" | "en" | "ja"
    /// zsh shell 集成自动注入(spawn 时临时 ZDOTDIR,让 OSC 133 标记可靠发出)。
    /// 默认开;纯临时 env 注入,不写用户 dotfiles。
    #[serde(default = "Config::default_shell_integration")]
    pub shell_integration: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: Self::default_schema_version(),
            active_theme: Self::default_active_theme(),
            follow_system_theme: false,
            language: None,
            shell_integration: Self::default_shell_integration(),
        }
    }
}

impl Config {
    fn default_schema_version() -> u32 {
        1
    }
    fn default_active_theme() -> String {
        "gruvbox".into()
    }
    fn default_shell_integration() -> bool {
        true
    }

    pub fn load() -> Result<Self, ConfigError> {
        let p = config_toml_path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(&p)?;
        Ok(toml::from_str(&s)?)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let p = config_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        atomic_write(&p, s.as_bytes())
    }
}

// ---- 主题加载(内置 + 用户文件)----
pub fn load_all_themes() -> Vec<Theme> {
    let mut out = theme::builtins();
    // 用户自定义
    if let Ok(dir) = themes_dir() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                match std::fs::read_to_string(&p) {
                    Ok(s) => match toml::from_str::<Theme>(&s) {
                        Ok(t) => out.push(t),
                        Err(e) => tracing::warn!(path = ?p, err = %e, "theme parse failed"),
                    },
                    Err(e) => tracing::warn!(path = ?p, err = %e, "theme read failed"),
                }
            }
        }
    }
    out
}

pub fn get_theme(id: &str) -> Theme {
    // load_all_themes() 永远以内置主题打头(load_all_themes 内 `theme::builtins()`),
    // 故首元素可作为无 panic 的最终兜底;若未来 builtins() 为空则回退到 builtins() 首项,
    // 仍为空时构造一个内联默认主题,彻底消除生产路径上的 expect/panic。
    let themes = load_all_themes();
    let fallback = themes.first().cloned();
    themes
        .into_iter()
        .find(|t| t.id == id)
        .or(fallback)
        .unwrap_or_else(|| {
            theme::builtins()
                .into_iter()
                .next()
                .unwrap_or_else(theme::vibe)
        })
}

// ---- 文件监听 watcher(50ms debounce)----
pub struct ConfigWatcher {
    _watcher: notify::RecommendedWatcher,
    _last_change: Arc<Mutex<std::time::Instant>>,
}

impl ConfigWatcher {
    /// 创建 watcher;changed 回调在去抖后调用。
    pub fn start<F>(on_change: F) -> Result<Self, ConfigError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        use notify::{Event, EventKind, RecursiveMode, Watcher};
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = config_dir()?;
        let last_change = Arc::new(Mutex::new(std::time::Instant::now()));
        let last_change_w = last_change.clone();
        let cb = Arc::new(on_change);
        // 去抖在途标志:同一时刻最多一个 debounce 线程,避免每个 FS 事件都
        // spawn 新线程导致突发期线程尖峰。在途线程会复读最新时间戳,
        // 因此活动平息 50ms 后仍会触发一次回调。
        let pending = Arc::new(AtomicBool::new(false));

        let mut watcher: notify::RecommendedWatcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| match res {
                Ok(ev) => match ev.kind {
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                        let mut t = last_change_w.lock().unwrap();
                        *t = std::time::Instant::now();
                        drop(t);
                        // 已有在途去抖线程则只更新时间戳,不再 spawn。
                        if pending.swap(true, Ordering::SeqCst) {
                            return;
                        }
                        let lc = last_change_w.clone();
                        let cb2 = cb.clone();
                        let pending2 = pending.clone();
                        std::thread::spawn(move || {
                            // 自旋等待至最后一次事件后静默满 50ms 再触发。
                            loop {
                                std::thread::sleep(Duration::from_millis(50));
                                if lc.lock().unwrap().elapsed() >= Duration::from_millis(50) {
                                    break;
                                }
                            }
                            pending2.store(false, Ordering::SeqCst);
                            cb2();
                        });
                    }
                    _ => {}
                },
                Err(e) => tracing::warn!(err = %e, "watcher error"),
            })?;

        // 监听父目录(alacritty 模式)
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            _last_change: last_change,
        })
    }
}
