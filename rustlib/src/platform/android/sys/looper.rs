//! `android/looper.h` 的手写绑定（NDK 27.3.13750724）。

/// 不透明 Looper 句柄（借用语义；由 `ALooper_prepare`/`ALooper_forThread` 获得）。
#[repr(C)]
pub struct ALooper {
    _private: [u8; 0],
}

/// `ALooper_prepare` 的选项（头文件 enum）。
pub const ALOOPER_PREPARE_ALLOW_NON_CALLBACKS: i32 = 1;

/// `ALooper_pollOnce`/`pollAll` 的返回值（头文件 enum）。
pub const ALOOPER_POLL_WAKE: i32 = -1;
pub const ALOOPER_POLL_CALLBACK: i32 = -2;
pub const ALOOPER_POLL_TIMEOUT: i32 = -3;
pub const ALOOPER_POLL_ERROR: i32 = -4;

/// poll 事件掩码（头文件 enum，用于 `ALooper_addFd`）。
pub const ALOOPER_EVENT_INPUT: i32 = 1 << 0;
pub const ALOOPER_EVENT_OUTPUT: i32 = 1 << 1;
pub const ALOOPER_EVENT_ERROR: i32 = 1 << 2;

#[cfg(target_os = "android")]
unsafe extern "C" {
    pub fn ALooper_forThread() -> *mut ALooper;
    pub fn ALooper_prepare(opts: i32) -> *mut ALooper;
    pub fn ALooper_acquire(looper: *mut ALooper);
    pub fn ALooper_release(looper: *mut ALooper);
    /// `timeoutMillis` < 0 表示无限等待；0 表示立即返回。
    pub fn ALooper_pollOnce(
        timeout_millis: i32,
        out_fd: *mut i32,
        out_events: *mut i32,
        out_data: *mut *mut core::ffi::c_void,
    ) -> i32;
    /// 注册 fd 到 looper 轮询集合；callback=NULL 时用 ident 返回（头文件）。
    /// 可在任何线程调用（含其他线程的 looper）。
    pub fn ALooper_addFd(
        looper: *mut ALooper,
        fd: i32,
        ident: i32,
        events: i32,
        callback: Option<unsafe extern "C" fn(*mut core::ffi::c_void, i32) -> i32>,
        data: *mut core::ffi::c_void,
    ) -> i32;
    pub fn ALooper_wake(looper: *mut ALooper);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// poll 结果常量与头文件逐一对照。
    #[test]
    fn poll_constants_match_ndk() {
        assert_eq!(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS, 1);
        assert_eq!(ALOOPER_POLL_WAKE, -1);
        assert_eq!(ALOOPER_POLL_CALLBACK, -2);
        assert_eq!(ALOOPER_POLL_TIMEOUT, -3);
        assert_eq!(ALOOPER_POLL_ERROR, -4);
    }
}
