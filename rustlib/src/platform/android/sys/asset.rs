//! `android/asset_manager.h` 的手写绑定（NDK 27.3.13750724）。
//!
//! V1 只绑定读取路径（open/read/close），供 §6.4 资源加载使用。

#[repr(C)]
pub struct AAssetManager {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AAsset {
    _private: [u8; 0],
}

/// `AAssetManager_open` 的模式（头文件 enum，V1 只用 BUFFER 模式）。
pub const AASSET_MODE_UNKNOWN: i32 = 0;
pub const AASSET_MODE_RANDOM: i32 = 1;
pub const AASSET_MODE_STREAMING: i32 = 2;
pub const AASSET_MODE_BUFFER: i32 = 3;

#[cfg(target_os = "android")]
unsafe extern "C" {
    pub fn AAssetManager_open(
        mgr: *mut AAssetManager,
        filename: *const core::ffi::c_char,
        mode: i32,
    ) -> *mut AAsset;
    pub fn AAsset_read(asset: *mut AAsset, buf: *mut core::ffi::c_void, count: usize) -> i32;
    pub fn AAsset_close(asset: *mut AAsset);
    pub fn AAsset_getBuffer(asset: *mut AAsset) -> *const core::ffi::c_void;
    pub fn AAsset_getLength(asset: *mut AAsset) -> i64;
    pub fn AAsset_getRemainingLength(asset: *mut AAsset) -> i64;
    /// 打开资产为文件描述符（fd 交接给 mmap，API 14+）。
    /// 返回 fd；`out_start`/`out_end` 写入资产数据在底层文件中的区间。
    /// `off64_t` 在 Linux/bionic ABI 上为 i64。
    pub fn AAsset_openFileDescriptor64(
        asset: *mut AAsset,
        out_start: *mut i64,
        out_end: *mut i64,
    ) -> core::ffi::c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模式常量与 NDK 头文件逐一对照。
    #[test]
    fn mode_constants_match_ndk() {
        assert_eq!(AASSET_MODE_UNKNOWN, 0);
        assert_eq!(AASSET_MODE_RANDOM, 1);
        assert_eq!(AASSET_MODE_STREAMING, 2);
        assert_eq!(AASSET_MODE_BUFFER, 3);
    }
}
