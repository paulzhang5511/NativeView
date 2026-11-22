//! 笔刷参数表：4 条滑杆的下标、取值范围、读写与显示文案。
//!
//! # 为什么集中在这里
//! 滑杆下标（0..3）在面板绘制、手势命中、无障碍标签三处都要用，
//! 一旦散落就会「画的和改的不是同一条」。读写统一走本模块，
//! 新增参数只改这里。

use crate::paint::Brush;

/// 粗细滑杆下标。
pub const PARAM_SIZE: usize = 0;
/// 不透明度滑杆下标。
pub const PARAM_OPACITY: usize = 1;
/// 流量滑杆下标。
pub const PARAM_FLOW: usize = 2;
/// 平滑滑杆下标。
pub const PARAM_SMOOTHING: usize = 3;

/// 滑杆名（面板左侧文案，同时用于无障碍标签）。
pub const PARAM_LABELS: [&str; 4] = ["粗细", "不透明度", "流量", "平滑"];

pub(super) fn param_value(brush: &Brush, i: usize) -> f32 {
    match i {
        PARAM_SIZE => brush.size,
        PARAM_OPACITY => brush.opacity * 100.0,
        PARAM_FLOW => brush.flow * 100.0,
        PARAM_SMOOTHING => brush.smoothing * 100.0,
        _ => 0.0,
    }
}

pub(super) fn param_max(i: usize) -> f32 {
    if i == PARAM_SIZE { 120.0 } else { 100.0 }
}

pub(super) fn set_param(brush: &mut Brush, i: usize, value: f32) {
    match i {
        PARAM_SIZE => brush.size = value.clamp(1.0, 120.0),
        PARAM_OPACITY => brush.opacity = (value / 100.0).clamp(0.0, 1.0),
        PARAM_FLOW => brush.flow = (value / 100.0).clamp(0.0, 1.0),
        PARAM_SMOOTHING => brush.smoothing = (value / 100.0).clamp(0.0, 1.0),
        // 越界下标不可达（滑杆只在 0..=3 命中），静默忽略。
        _ => {}
    }
}

pub(super) fn param_text(brush: &Brush, i: usize) -> String {
    if i == PARAM_SIZE {
        format!("{:.0}px", brush.size)
    } else {
        format!("{:.0}%", param_value(brush, i))
    }
}
