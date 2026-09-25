//! 读剪贴板里的文件路径(优先于 bitmap 抓取,避免拿到文件 icon 缩略图)
//!
//! 思路借鉴 references/wezterm/window/src/os/macos/clipboard.rs:21-31。
//! macOS 平台:`NSPasteboard.readObjectsForClasses([NSURL])` 取文件 URL 转 path;
//! 其他平台暂返回空(后续可补 Wayland / Win32)。

/// 读剪贴板里的文件绝对路径列表。空列表表示剪贴板里没有文件 URL。
pub fn read_clipboard_files() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        macos::read()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::ClassType;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSArray, NSURL};

    pub fn read() -> Vec<String> {
        let pb = NSPasteboard::generalPasteboard();
        let classes = NSArray::from_slice(&[NSURL::class()]);
        // SAFETY: 纯只读 NSPasteboard API;class_array 仅含 NSURL,options 为空。
        let Some(items) = (unsafe { pb.readObjectsForClasses_options(&classes, None) }) else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|obj| obj.downcast::<NSURL>().ok())
            .filter(|url| url.isFileURL())
            .filter_map(|url| url.path())
            .map(|p| p.to_string())
            .collect()
    }
}
