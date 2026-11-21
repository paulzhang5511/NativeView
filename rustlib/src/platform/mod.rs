//! 平台层：Android 专属实现与主机测试桩。
//!
//! # 分层铁律（PRD §4）
//! - 只有本模块允许出现 `extern "C"` FFI 与 Android 专属类型；
//! - 上层模块（backends/gfx/text/ui）一律通过安全封装访问平台能力；
//! - `android` 子模块仅在 `target_os = "android"` 下编译，主机侧走 `host` 桩。

pub mod android;
pub mod host;
pub mod perf;
