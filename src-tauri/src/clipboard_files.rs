//! 读剪贴板里的文件路径(优先于 bitmap 抓取,避免拿到文件 icon 缩略图)
//!
//! 模式 1:1 借鉴 references/wezterm/window/src/os/macos/clipboard.rs:21-31。
//! macOS 平台:`NSPasteboard.propertyListForType(NSFilenamesPboardType)` 返
//! NSArray<NSString>;其他平台暂返回空(后续可补 Wayland / Win32)。
//!
//! cocoa 0.26 全面 deprecate(推荐 objc2-* 系列),但 API 在可见未来仍工作;
//! 局部 `#[allow(deprecated)]` 豁免 -D warnings。

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
#[allow(deprecated)]
mod macos {
    use cocoa::appkit::{NSFilenamesPboardType, NSPasteboard};
    use cocoa::base::{id, nil};
    use cocoa::foundation::{NSArray, NSString};

    pub fn read() -> Vec<String> {
        // SAFETY: 纯只读 NSPasteboard API 调用,不持有跨 await 的可变 obj-c ref;
        // 返回的 NSString 通过 UTF8String → CStr → Rust String 拷贝出来。
        unsafe {
            let pb: id = NSPasteboard::generalPasteboard(nil);
            if pb.is_null() {
                return Vec::new();
            }
            let plist: id = pb.propertyListForType(NSFilenamesPboardType);
            if plist.is_null() {
                return Vec::new();
            }
            let n = plist.count();
            let mut out = Vec::with_capacity(n as usize);
            for i in 0..n {
                let s: id = plist.objectAtIndex(i);
                if s.is_null() {
                    continue;
                }
                let cstr = s.UTF8String();
                if cstr.is_null() {
                    continue;
                }
                if let Ok(rust) = std::ffi::CStr::from_ptr(cstr).to_str() {
                    out.push(rust.to_owned());
                }
            }
            out
        }
    }
}
