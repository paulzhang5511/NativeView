//! `android/log.h` 的手写绑定（NDK 27.3.13750724）。
//!
//! 用于自写 `log::Log` 后端（PRD §6.4.5），替代被禁止的 `android_logger`
//! （其依赖第三方 NDK 绑定 `android_log-sys`）。

use core::ffi::c_int;

/// `android_LogPriority` 枚举值（与头文件逐一对应）。
pub const ANDROID_LOG_UNKNOWN: c_int = 0;
pub const ANDROID_LOG_DEFAULT: c_int = 1;
pub const ANDROID_LOG_VERBOSE: c_int = 2;
pub const ANDROID_LOG_DEBUG: c_int = 3;
pub const ANDROID_LOG_INFO: c_int = 4;
pub const ANDROID_LOG_WARN: c_int = 5;
pub const ANDROID_LOG_ERROR: c_int = 6;
pub const ANDROID_LOG_FATAL: c_int = 7;
pub const ANDROID_LOG_SILENT: c_int = 8;

#[cfg(target_os = "android")]
#[link(name = "log")]
unsafe extern "C" {
    /// 写入一条日志；返回 1 表示已写入，`-EPERM` 表示被过滤。
    ///
    /// # 安全约束
    /// `tag` 与 `text` 必须是有效的 NUL 结尾 C 字符串；
    /// 日志路径永不 panic（转换失败时静默丢弃，PRD §6.4.5）。
    pub fn __android_log_write(
        prio: c_int,
        tag: *const core::ffi::c_char,
        text: *const core::ffi::c_char,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 优先级常量与 android_LogPriority 枚举逐一对照。
    #[test]
    fn log_priority_constants_match_ndk() {
        assert_eq!(ANDROID_LOG_UNKNOWN, 0);
        assert_eq!(ANDROID_LOG_DEFAULT, 1);
        assert_eq!(ANDROID_LOG_VERBOSE, 2);
        assert_eq!(ANDROID_LOG_DEBUG, 3);
        assert_eq!(ANDROID_LOG_INFO, 4);
        assert_eq!(ANDROID_LOG_WARN, 5);
        assert_eq!(ANDROID_LOG_ERROR, 6);
        assert_eq!(ANDROID_LOG_FATAL, 7);
        assert_eq!(ANDROID_LOG_SILENT, 8);
    }
}
