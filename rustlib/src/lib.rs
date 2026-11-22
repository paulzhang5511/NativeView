//! NativeView：纯原生 Android 视图框架（PRD `docs/prd.md`）。
//!
//! 模块结构（PRD §7.1）：
//! - [`platform`]：平台层（Android 手写 FFI 封装 / 主机测试桩）
//! - `backends` / `gfx` / `text` / `ui` / `anim` / `demo`：随任务落地
//!
//! 入口符号 `ANativeActivity_onCreate` 在 Task 5 落地。

pub mod anim;
pub mod backends;
pub mod demo;
pub mod gfx;
pub mod paint;
pub mod platform;
pub mod text;
pub mod ui;
