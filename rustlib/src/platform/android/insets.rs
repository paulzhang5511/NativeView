//! 系统栏避让（edge-to-edge insets）：Kotlin 侧读取 WindowInsets 后
//! 经 JNI 注入，Rust 侧全局缓存供 Demo 布局消费。
//!
//! # 设计
//! - Android 15+ 强制 edge-to-edge（`EDGE_TO_EDGE_ENFORCED`），窗口 surface
//!   恒全屏，状态栏/导航栏遮挡内容由应用自行避让；
//! - NativeActivity 无 decor view 布局链拿不到 insets 回调，故由 Kotlin
//!   空壳 Activity 在 onCreate 时一次性读取并注入（路线 B 允许的最小桥）；
//! - V1 注入后不再变化（旋转重建时 Kotlin 会重走 onCreate 重新注入）。

use log::info;
use std::sync::atomic::{AtomicU32, Ordering};

/// 状态栏高度（px）。0 = 未注入（Demo 按 0 避让，全屏布局）。
static STATUS_BAR: AtomicU32 = AtomicU32::new(0);

/// 导航栏高度（px）。0 = 未注入。
static NAV_BAR: AtomicU32 = AtomicU32::new(0);

/// Kotlin 侧注入系统栏高度（JNI 入口调用；重复调用以最新为准）。
pub fn set_system_bar_insets(status: u32, nav: u32) {
    STATUS_BAR.store(status, Ordering::Relaxed);
    NAV_BAR.store(nav, Ordering::Relaxed);
    info!("system bar insets: status={}px, nav={}px", status, nav);
}

/// 状态栏高度（px；未注入时 0）。
pub fn status_bar() -> u32 {
    STATUS_BAR.load(Ordering::Relaxed)
}

/// 导航栏高度（px；未注入时 0）。
pub fn nav_bar() -> u32 {
    NAV_BAR.load(Ordering::Relaxed)
}
