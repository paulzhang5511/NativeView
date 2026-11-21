//! 平台安全封装层：日志后端。
//!
//! 自写 `log::Log` 实现，桥接到 `__android_log_write`（PRD §6.4.5：
//! 禁用 `android_logger` crate，避免第三方 NDK 绑定依赖）。

use log::{LevelFilter, Log, Metadata, Record};

#[cfg(any(target_os = "android", test))]
use super::sys::log::{
    ANDROID_LOG_DEBUG, ANDROID_LOG_ERROR, ANDROID_LOG_INFO, ANDROID_LOG_VERBOSE, ANDROID_LOG_WARN,
};

#[cfg(any(target_os = "android", test))]
use log::Level;
/// Logcat 标签；固定常量，禁止拼接（AGENTS §2.1）。
const TAG: &str = "velloview";

/// 桥接 `log` → `__android_log_write` 的后端。
pub struct AndroidLogger;

impl AndroidLogger {
    /// 注册为全局日志后端。
    ///
    /// # 注意
    /// 进程内只能成功注册一次；重复调用静默忽略（start/stop 周期中可能重入）。
    pub fn install() {
        let logger = Box::leak(Box::new(Self));
        // set_logger 失败（已注册）不是错误：保留首个实例即可。
        let _ = log::set_logger(logger);
        // 默认 Info：逐命令/逐帧诊断日志走 Debug/Trace，避免主线程 logcat
        // 同步写把帧预算吃光（真机排查可临时用 `set_max_level(Debug)`）。
        log::set_max_level(LevelFilter::Info);
    }

    /// log 级别 → Logcat 优先级映射。
    /// 主机侧仅被单测调用；Android 侧经 `log()` 走此路径。
    #[cfg(any(target_os = "android", test))]
    fn priority(level: Level) -> i32 {
        match level {
            Level::Error => ANDROID_LOG_ERROR,
            Level::Warn => ANDROID_LOG_WARN,
            Level::Info => ANDROID_LOG_INFO,
            Level::Debug => ANDROID_LOG_DEBUG,
            Level::Trace => ANDROID_LOG_VERBOSE,
        }
    }

    /// 任意文本安全转 C 字符串：内部 NUL 截断、非法 UTF-8 由 `log` 保证不会出现。
    fn to_cstr(s: &str) -> std::ffi::CString {
        // CString::new 失败的唯一原因是内含 NUL，此时截断到 NUL 前；
        // 日志路径禁止 panic（PRD §6.4.5）。
        std::ffi::CString::new(s).unwrap_or_else(|e| {
            std::ffi::CString::new(&s[..e.nul_position()]).expect("truncated string is NUL-free")
        })
    }
}

impl Log for AndroidLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let tag = Self::to_cstr(TAG);
        let text = Self::to_cstr(&record.args().to_string());
        // SAFETY: tag/text 均为合法 NUL 结尾字符串；失败仅返回负值，不崩溃。
        #[cfg(target_os = "android")]
        unsafe {
            super::sys::log::__android_log_write(
                Self::priority(record.level()),
                tag.as_ptr(),
                text.as_ptr(),
            );
        }
        #[cfg(not(target_os = "android"))]
        {
            // 主机桩：打印到 stderr，便于 cargo test 观察日志路径。
            eprintln!("[velloview] {} {}", record.level(), text.to_string_lossy());
            let _ = tag;
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 级别映射逐一对照 android_LogPriority。
    #[test]
    fn level_mapping_matches_ndk() {
        assert_eq!(AndroidLogger::priority(Level::Error), 6);
        assert_eq!(AndroidLogger::priority(Level::Warn), 5);
        assert_eq!(AndroidLogger::priority(Level::Info), 4);
        assert_eq!(AndroidLogger::priority(Level::Debug), 3);
        assert_eq!(AndroidLogger::priority(Level::Trace), 2);
    }

    /// 内部 NUL 截断不 panic（日志铁律）。
    #[test]
    fn embedded_nul_truncated() {
        let c = AndroidLogger::to_cstr("a\0b");
        assert_eq!(c.to_bytes(), b"a");
    }
}
