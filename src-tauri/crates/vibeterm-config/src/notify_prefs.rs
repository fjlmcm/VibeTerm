//! notify.toml — 通知偏好
//!
//! 全局开关 + 两类事件分别开关 + 自定义声音 + 免打扰时段。
//!
//! ```toml
//! schema_version = 1
//! enabled = true
//!
//! [events.waiting_input]
//! enabled = true
//! sound = "Tink"            # macOS 系统声音 / ~/Library/Sounds/*.aiff 文件名
//!
//! [events.done]               # 现在语义 = "agent 完成 turn (via Stop hook)"
//! enabled = true               # 不再是 OSC 133 D shell 完成
//! sound = "Glass"
//!
//! [quiet_hours]
//! enabled = false
//! start = "22:00"
//! end = "08:00"
//! ```
//!
//! 历史:
//!  - Stalled 事件曾在 EventsPrefs 里, 但区分"agent 真挂了"vs"agent 完成等输入"
//!    在通用 TUI 协议层做不到, 误报严重, 后期移除.
//!  - `done` 字段保留, 但语义从"OSC 133/633 D shell 命令完成"改成"claude/codex
//!    Stop hook 完成". 用户的现有 notify.toml 不破坏, UI 标签改成"Agent 完成 (via hook)".
//!    旧的 notify.toml `[events.stalled]` 段会被 TOML parser 默默忽略, 向前兼容.
//!
//! 设计取舍:
//!   - sound 走系统通知子系统(tauri-plugin-notification),只接受字符串名;
//!     不引入自播 audio 栈(避免锁屏 / 勿扰 / 静音模式被绕过).
//!   - quiet_hours 允许跨夜(start > end 表示从 start 到次日 end).
//!   - 空字符串 sound → fallback 系统默认.

use serde::{Deserialize, Serialize};

/// 单个事件的通知偏好。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct EventNotifyPrefs {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 系统声音名(macOS: Glass/Tink/Sosumi/Hero/Pop 等,或 `~/Library/Sounds/<name>.aiff`)。
    /// 空字符串或 None → 走系统默认。
    #[serde(default)]
    pub sound: Option<String>,
}

impl EventNotifyPrefs {
    pub fn new(enabled: bool, sound: &str) -> Self {
        Self {
            enabled,
            sound: Some(sound.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct EventsPrefs {
    #[serde(default = "EventsPrefs::default_waiting_input")]
    pub waiting_input: EventNotifyPrefs,
    #[serde(default = "EventsPrefs::default_done")]
    pub done: EventNotifyPrefs,
}

impl Default for EventsPrefs {
    fn default() -> Self {
        Self {
            waiting_input: Self::default_waiting_input(),
            done: Self::default_done(),
        }
    }
}

impl EventsPrefs {
    /// 默认从 macOS 系统名 (Tink/Glass) 切换到 VibeTerm 自带库 —
    /// 跨平台一致 (Linux/Windows 也能放), 视听上更优.
    fn default_waiting_input() -> EventNotifyPrefs {
        EventNotifyPrefs::new(true, "tone20")
    }
    fn default_done() -> EventNotifyPrefs {
        EventNotifyPrefs::new(true, "ringtone2")
    }
}

/// 免打扰时段。start/end 为 24h "HH:MM";start > end 表示跨夜。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct QuietHours {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "QuietHours::default_start")]
    pub start: String,
    #[serde(default = "QuietHours::default_end")]
    pub end: String,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            start: Self::default_start(),
            end: Self::default_end(),
        }
    }
}

impl QuietHours {
    fn default_start() -> String {
        "22:00".to_string()
    }
    fn default_end() -> String {
        "08:00".to_string()
    }

    /// 判断 24h 制 "HH:MM" 是否落在 [start, end) 时段(含跨夜).
    /// 输入非法 → false(失败开放,不静默用户).
    pub fn contains(&self, hh_mm: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let now = parse_hh_mm(hh_mm);
        let s = parse_hh_mm(&self.start);
        let e = parse_hh_mm(&self.end);
        match (now, s, e) {
            (Some(n), Some(s), Some(e)) => {
                if s == e {
                    false
                } else if s < e {
                    n >= s && n < e
                } else {
                    // 跨夜:[s, 24:00) ∪ [00:00, e)
                    n >= s || n < e
                }
            }
            _ => false,
        }
    }
}

fn parse_hh_mm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(h * 60 + m)
}

fn default_true() -> bool {
    true
}

fn default_persistent_remind_secs() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct NotifyFile {
    #[serde(default = "NotifyFile::default_schema_version")]
    pub schema_version: u32,
    /// 全局总开关。off 时所有通知一律不弹。
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub events: EventsPrefs,
    #[serde(default)]
    pub quiet_hours: QuietHours,
    /// 主窗口在前台、但完成的不是当前选中任务时,仍轻提示(前端音效 + 任务列表行高亮,
    /// 不弹系统横幅 —— macOS 前台横幅本就常被吞)。关 → 维持"前台一律静默"。默认开。
    #[serde(default = "default_true")]
    pub notify_focused_other_task: bool,
    /// Dock 图标角标显示"未看完成数"(聚合状态 = Done 的任务数),用户切到该任务后自动减一。
    /// 安静的持续提醒,不响铃、不弹横幅,复用现有 seen/Done 模型。默认开。
    #[serde(default = "default_true")]
    pub dock_badge_unseen: bool,
    /// 间歇持续声音提醒:只要有"未看完成"任务且主窗口未聚焦,就每隔 PERSISTENT_REMIND 秒响
    /// **1 路**声音催用户回来看,直到未看完成数归零(全局单路,多任务不叠加)。打扰性最强,
    /// 仅特定场景(离机跑长任务)需要,默认关。一旦主窗口聚焦即停 —— 人回到 app 就不再催。
    #[serde(default)]
    pub persistent_unseen_sound: bool,
    /// 持续提醒的响铃间隔(秒)。默认 30。仅 `persistent_unseen_sound` 开时生效;用时 clamp 到 [5,3600]。
    #[serde(default = "default_persistent_remind_secs")]
    pub persistent_remind_secs: u64,
}

impl Default for EventNotifyPrefs {
    fn default() -> Self {
        Self::new(true, "")
    }
}

impl Default for NotifyFile {
    fn default() -> Self {
        Self {
            schema_version: Self::default_schema_version(),
            enabled: true,
            events: EventsPrefs::default(),
            quiet_hours: QuietHours::default(),
            notify_focused_other_task: true,
            dock_badge_unseen: true,
            persistent_unseen_sound: false,
            persistent_remind_secs: 30,
        }
    }
}

impl NotifyFile {
    fn default_schema_version() -> u32 {
        1
    }

    pub fn load() -> Self {
        let p = match super::notify_toml_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !p.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                tracing::warn!(err = %e, "notify parse failed, fallback default");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), super::ConfigError> {
        let p = super::notify_toml_path()?;
        let s = toml::to_string_pretty(self)?;
        super::atomic_write(&p, s.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_same_day() {
        let q = QuietHours {
            enabled: true,
            start: "10:00".into(),
            end: "12:00".into(),
        };
        assert!(q.contains("11:00"));
        assert!(q.contains("10:00"));
        assert!(!q.contains("12:00"));
        assert!(!q.contains("09:59"));
        assert!(!q.contains("13:00"));
    }

    #[test]
    fn quiet_hours_overnight() {
        let q = QuietHours {
            enabled: true,
            start: "22:00".into(),
            end: "08:00".into(),
        };
        assert!(q.contains("22:00"));
        assert!(q.contains("23:30"));
        assert!(q.contains("00:00"));
        assert!(q.contains("07:59"));
        assert!(!q.contains("08:00"));
        assert!(!q.contains("12:00"));
        assert!(!q.contains("21:59"));
    }

    #[test]
    fn quiet_hours_disabled() {
        let q = QuietHours {
            enabled: false,
            start: "00:00".into(),
            end: "23:59".into(),
        };
        assert!(!q.contains("12:00"));
    }

    #[test]
    fn quiet_hours_invalid_input() {
        let q = QuietHours {
            enabled: true,
            start: "22:00".into(),
            end: "08:00".into(),
        };
        assert!(!q.contains("garbage"));
        assert!(!q.contains("25:00"));
        assert!(!q.contains("12:99"));
    }

    #[test]
    fn defaults_round_trip() {
        let f = NotifyFile::default();
        let s = toml::to_string_pretty(&f).unwrap();
        let parsed: NotifyFile = toml::from_str(&s).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.events.done.sound.as_deref(), Some("ringtone2"));
        assert_eq!(parsed.events.waiting_input.sound.as_deref(), Some("tone20"));
        // 多 agent 提醒新字段默认值
        assert!(parsed.notify_focused_other_task, "前台非当前任务提示默认开");
        assert!(parsed.dock_badge_unseen, "Dock 角标默认开");
        assert!(!parsed.persistent_unseen_sound, "持续声音提醒默认关");
    }

    // 向前兼容: 旧 notify.toml 含 [events.stalled] 段不应报错, 字段被静默忽略.
    #[test]
    fn legacy_stalled_section_ignored() {
        let toml_with_stalled = r#"
            schema_version = 1
            enabled = true

            [events.waiting_input]
            enabled = true
            sound = "Tink"

            [events.done]
            enabled = true
            sound = "Glass"

            [events.stalled]
            enabled = true
            sound = "Sosumi"

            [quiet_hours]
            enabled = false
            start = "22:00"
            end = "08:00"
        "#;
        let parsed: NotifyFile = toml::from_str(toml_with_stalled).expect("legacy parse");
        assert!(parsed.enabled);
        assert_eq!(parsed.events.waiting_input.sound.as_deref(), Some("Tink"));
        // 旧 notify.toml 缺新字段 → 走 #[serde(default)],不破坏现有配置
        assert!(parsed.notify_focused_other_task, "旧 toml 缺字段 → 默认开");
        assert!(parsed.dock_badge_unseen, "旧 toml 缺字段 → 默认开");
        assert!(!parsed.persistent_unseen_sound, "旧 toml 缺字段 → 默认关");
    }
}
