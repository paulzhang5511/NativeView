//! NDK C 绑定声明层（手写 FFI，PRD §6.4）。
//!
//! # 约束（铁律）
//! - 本目录只做 `extern "C"` 声明，禁止任何业务逻辑；
//! - 所有 `#[link]` 集中在本文件，不下放到各子模块；
//! - 不透明类型用手写空结构（`[u8; 0]`），禁止 `*mut c_void` 代替；
//! - 回调结构体字段顺序必须与 NDK 27.3.13750724 头文件逐行一致，
//!   顺序错误会导致回调错位（崩溃而非编译失败）；
//! - 每个绑定模块必须带 `size_of`/`align_of` 布局断言测试；
//! - 本模块仅在 `target_os = "android"` 下编译进产物，主机侧测试
//!   依赖类型是 `repr(C)` 的确定性，因此布局断言在主机上同样运行。
//!
//! # 版本基线
//! NDK 27.3.13750724，minSdk = 29；凡 `__INTRODUCED_IN(30/31/…)` 的符号，
//! 调用前必须用 `ANativeActivity.sdkVersion` 门控（见各符号注释）。

#![allow(non_camel_case_types, non_snake_case, dead_code)]

pub mod activity;
pub mod asset;
pub mod input;
pub mod keycodes;
pub mod log;
pub mod looper;
pub mod rect;
pub mod sysprop;
pub mod window;

// `libandroid` 提供的 NDK 符号集合（native_activity / native_window /
// input / looper / asset_manager 等）；链接声明只允许出现在这里。
//
// Rust 2024 edition 的 `unsafe extern` 语法；实际符号在各子模块声明。
// `libandroid` 由 bionic 动态链接，无需显式链接参数即可解析符号；
// 各子模块的符号声明自带 `#[link]`（如 log.rs 指向 `liblog`）。
#[cfg(target_os = "android")]
unsafe extern "C" {}
