//! `android/native_window.h` 的手写绑定（NDK 27.3.13750724）。

use core::ffi::c_int;

/// 不透明窗口句柄；Rust 侧永不构造实例，只持有指针（借用语义）。
#[repr(C)]
pub struct ANativeWindow {
    _private: [u8; 0],
}

/// `ANativeWindow_lock` 返回的缓冲区描述。
///
/// # 不变量
/// - `stride` 是一行占用的**像素数**（不是字节数），可能 > width；
///   逐行写入时必须按 stride 步进，否则画面斜切（PRD §12.R7）。
/// - `bits` 指向缓冲区起点，借用语义：unlock 后立即失效，禁止持有。
#[repr(C)]
pub struct ANativeWindow_Buffer {
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub format: i32,
    pub bits: *mut u8,
    pub reserved: [u32; 6],
}

/// `WINDOW_FORMAT_*`（legacy 枚举，与 AHARDWAREBUFFER_FORMAT_* 数值一致）。
pub const WINDOW_FORMAT_RGBA_8888: c_int = 1;
pub const WINDOW_FORMAT_RGBX_8888: c_int = 2;
pub const WINDOW_FORMAT_RGB_565: c_int = 4;

#[cfg(target_os = "android")]
unsafe extern "C" {
    pub fn ANativeWindow_acquire(window: *mut ANativeWindow);
    pub fn ANativeWindow_release(window: *mut ANativeWindow);
    pub fn ANativeWindow_getWidth(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_getHeight(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_getFormat(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_setBuffersGeometry(
        window: *mut ANativeWindow,
        width: i32,
        height: i32,
        format: i32,
    ) -> i32;
    pub fn ANativeWindow_lock(
        window: *mut ANativeWindow,
        out_buffer: *mut ANativeWindow_Buffer,
        in_out_dirty_bounds: *mut super::rect::ARect,
    ) -> i32;
    pub fn ANativeWindow_unlockAndPost(window: *mut ANativeWindow) -> i32;
    // __INTRODUCED_IN(28)：minSdk=29 下可用，无需门控。
    pub fn ANativeWindow_setBuffersDataSpace(window: *mut ANativeWindow, data_space: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    /// 缓冲区布局：4×i32 + 1 指针 + reserved[6]，64 位上 = 16 + 8 + 24 = 48 字节。
    /// reserved[6] 缺失或长度错误都会导致 lock 写越界（PRD §9 FFI 布局校验）。
    #[test]
    fn buffer_layout_matches_ndk() {
        assert_eq!(size_of::<ANativeWindow_Buffer>(), 48);
        assert_eq!(align_of::<ANativeWindow_Buffer>(), 8);
        let b: ANativeWindow_Buffer = unsafe { std::mem::zeroed() };
        assert_eq!(b.reserved.len(), 6);
    }

    /// 窗口格式常量与 graphics.h 对应值一致（AHardwareBuffer_Format）。
    #[test]
    fn format_constants_match_ndk() {
        assert_eq!(WINDOW_FORMAT_RGBA_8888, 1);
        assert_eq!(WINDOW_FORMAT_RGBX_8888, 2);
        assert_eq!(WINDOW_FORMAT_RGB_565, 4);
    }
}
