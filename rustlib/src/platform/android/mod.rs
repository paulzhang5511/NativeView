//! `android` 平台模块：NativeActivity 生命周期、窗口、输入、资产、日志。
//!
//! # 结构
//! - [`sys`]：手写 NDK C 绑定（仅声明，无业务逻辑，PRD §6.4.4）；
//! - `activity.rs` / `window.rs` / …：安全封装层（对上不暴露裸指针）。

pub mod activity;
pub mod asset;
/// 显示密度（dp/sp 换算基准；Kotlin 侧注入，未注入为 1.0）。
pub mod density;
pub mod input;
/// 系统栏避让（Kotlin 侧注入 insets；仅 Android 有意义但保持平台可编译）。
pub mod insets;
pub mod log;
pub mod sys;
pub mod sysprop;
pub mod window;

/// 入口装配与主循环：仅 Android 产物编译（依赖 NDK 符号真实现）。
#[cfg(target_os = "android")]
pub mod glue;

/// JNI 桥入口（Kotlin 侧调用）：仅 Android 产物编译。
#[cfg(target_os = "android")]
pub mod jni_bridge;

/// IME 平台桥（Rust ↔ Kotlin 隐形 EditText）：仅 Android 产物编译。
#[cfg(target_os = "android")]
pub mod ime_bridge;

/// 无障碍平台桥（快照推送 + 点击回推）：仅 Android 产物编译。
#[cfg(target_os = "android")]
pub mod a11y_bridge;
