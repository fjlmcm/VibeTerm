//! clipboard image 保存 + FIFO 淘汰
//!
//! 用 tmpdir 模拟 clipboard_images_dir;直接走 save_clipboard_image_in
//! (注入路径 + now_ms),不触碰真用户 config。

use std::time::Duration;

use vibeterm_config::{
    enforce_clipboard_images_caps, save_clipboard_image_at, save_clipboard_image_in,
    save_clipboard_image_in_with_caps, ClipboardImagesSection, EnvFile, CLIPBOARD_IMAGES_MAX_BYTES,
    CLIPBOARD_IMAGES_MAX_COUNT,
};

#[test]
fn save_writes_file_and_returns_absolute_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let path = save_clipboard_image_in(dir.path(), b"PNGBYTES", 1_700_000_000_000).expect("save");
    assert!(path.is_absolute());
    assert!(path.ends_with("1700000000000.png"));
    let content = std::fs::read(&path).expect("read back");
    assert_eq!(content, b"PNGBYTES");
}

#[test]
fn evicts_oldest_when_count_exceeds_cap() {
    let dir = tempfile::tempdir().expect("tmpdir");
    // 预填 CLIPBOARD_IMAGES_MAX_COUNT 张占位
    for i in 0..CLIPBOARD_IMAGES_MAX_COUNT {
        let p = dir.path().join(format!("{i}.png"));
        std::fs::write(&p, b"x").unwrap();
        // 用 i 作为 mtime 排序键 — 用 filetime 微调
        let mt = filetime::FileTime::from_unix_time(i as i64 + 1, 0);
        filetime::set_file_mtime(&p, mt).unwrap();
    }
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(before, CLIPBOARD_IMAGES_MAX_COUNT);

    // 再加一张 → 应淘汰最旧(0.png),保持总数 == MAX_COUNT
    let new_path = save_clipboard_image_in(dir.path(), b"NEW", 9_999).expect("save");
    let after_count = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(after_count, CLIPBOARD_IMAGES_MAX_COUNT);
    assert!(new_path.exists());
    assert!(!dir.path().join("0.png").exists(), "最旧的应被淘汰");
}

#[test]
fn evicts_when_bytes_exceed_cap() {
    let dir = tempfile::tempdir().expect("tmpdir");
    // 单文件 = 半个上限,3 个就过线
    let half: Vec<u8> = vec![0u8; (CLIPBOARD_IMAGES_MAX_BYTES / 2) as usize];
    for i in 0..2 {
        let p = dir.path().join(format!("{i}.png"));
        std::fs::write(&p, &half).unwrap();
        let mt = filetime::FileTime::from_unix_time(i as i64 + 1, 0);
        filetime::set_file_mtime(&p, mt).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    // 加第三个 same size → 至少 evict 一个,剩余 size 必 ≤ cap
    let p3 = save_clipboard_image_in(dir.path(), &half, 9_999).expect("save");
    assert!(p3.exists());
    let total: u64 = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(
        total <= CLIPBOARD_IMAGES_MAX_BYTES,
        "总字节应 ≤ cap;实际 = {total}"
    );
}

#[test]
fn env_toml_parses_new_clipboard_images_section() {
    let toml_str = r#"
schema_version = 1

[clipboard_images]
dir = "~/Pictures/VibeTerm"
max_count = 50
max_mb = 30
"#;
    let f: EnvFile = toml::from_str(toml_str).expect("parse");
    let sec = f.clipboard_images.expect("section");
    assert_eq!(sec.dir.as_deref(), Some("~/Pictures/VibeTerm"));
    assert_eq!(sec.max_count, Some(50));
    assert_eq!(sec.max_mb, Some(30));
}

#[test]
fn env_toml_clipboard_images_section_default_is_none() {
    let toml_str = r#"schema_version = 1"#;
    let f: EnvFile = toml::from_str(toml_str).expect("parse");
    assert!(f.clipboard_images.is_none());
}

#[test]
fn env_toml_partial_clipboard_section_allowed() {
    let toml_str = r#"
[clipboard_images]
max_count = 10
"#;
    let f: EnvFile = toml::from_str(toml_str).expect("parse");
    let sec = f.clipboard_images.unwrap();
    assert_eq!(sec.dir, None);
    assert_eq!(sec.max_count, Some(10));
    assert_eq!(sec.max_mb, None);
}

#[test]
fn save_with_custom_caps_respects_injected_limits() {
    let dir = tempfile::tempdir().expect("tmpdir");
    // 注入超严苛 caps:max_count = 1,新文件必须淘汰所有旧的
    for i in 0..3 {
        let p = dir.path().join(format!("old_{i}.png"));
        std::fs::write(&p, b"x").unwrap();
        let mt = filetime::FileTime::from_unix_time(i as i64 + 1, 0);
        filetime::set_file_mtime(&p, mt).unwrap();
    }
    let new_path = save_clipboard_image_in_with_caps(
        dir.path(),
        b"NEW",
        9_999,
        1, /* max_count */
        100 * 1024,
    )
    .expect("save");
    let pngs: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("png"))
        .collect();
    assert_eq!(pngs.len(), 1, "max_count=1 应只剩一个文件");
    assert!(new_path.exists());
}

// 让旧 ClipboardImagesSection 直接构造 → save 端到端
#[test]
fn clipboard_images_section_default_is_all_none() {
    let s: ClipboardImagesSection = Default::default();
    assert!(s.dir.is_none());
    assert!(s.max_count.is_none());
    assert!(s.max_mb.is_none());
}

#[test]
fn save_at_writes_exact_target_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let target = dir.path().join("abc123.png");
    save_clipboard_image_at(&target, b"HASHCONTENT").expect("save_at");
    assert!(target.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"HASHCONTENT");
}

#[test]
fn enforce_caps_separately_evicts_to_limits() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let bytes = vec![0u8; 1024];
    for i in 0..5 {
        let p = dir.path().join(format!("file_{i}.png"));
        std::fs::write(&p, &bytes).unwrap();
        let mt = filetime::FileTime::from_unix_time(i as i64 + 1, 0);
        filetime::set_file_mtime(&p, mt).unwrap();
    }
    let removed = enforce_clipboard_images_caps(dir.path(), 3, u64::MAX).expect("enforce");
    assert_eq!(removed, 2, "应淘汰 5-3=2 个");
    let remaining: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("png"))
        .collect();
    assert_eq!(remaining.len(), 3);
    assert!(!dir.path().join("file_0.png").exists(), "最旧的应被淘汰");
    assert!(!dir.path().join("file_1.png").exists());
}
