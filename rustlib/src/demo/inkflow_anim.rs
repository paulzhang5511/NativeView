//! Inkflow 交互动效的时间曲线、时长常量与进度计算。
//!
//! 设计约束：
//! - 全部是纯函数，主机侧可直接单测（不依赖渲染与设备）；
//! - 时长集中在本模块，绘制层只取「已算好的进度」，避免魔法数散落；
//! - 进度统一钳到 `0..=1`，动画被中断时直接以新起点覆写，不排队。

/// 前进切屏（库 → 新建 → 画板 → 导出）。
pub const PAGE_MS: u64 = 240;
/// 返回切屏（画板 / 新建 / 导出 → 上一级）。
pub const PAGE_BACK_MS: u64 = 220;
/// 前进切屏时整页内容的起始下移量（px）。
pub const PAGE_SHIFT_PX: f32 = 40.0;
/// 返回切屏时整页内容的起始上移量（px，负值表示自上滑入）。
pub const PAGE_BACK_SHIFT_PX: f32 = -24.0;
/// 切屏遮罩的初始不透明度。
pub const PAGE_COVER_ALPHA: f32 = 0.22;
pub const PAGE_BACK_COVER_ALPHA: f32 = 0.16;

/// 笔刷面板滑入 / 滑出时长。
pub const SHEET_OPEN_MS: u64 = 320;
pub const SHEET_CLOSE_MS: u64 = 220;
/// 面板背后压暗的最大不透明度。
pub const SHEET_SCRIM_ALPHA: f32 = 0.28;

/// 作品库单张卡片入场时长与相邻错峰。
pub const CARD_MS: u64 = 300;
pub const CARD_STAGGER_MS: u64 = 50;
/// 卡片入场下移量。
pub const CARD_SHIFT_PX: f32 = 20.0;
/// 筛选切换后卡片区重入时长与位移。
pub const FILTER_MS: u64 = 200;
pub const FILTER_SHIFT_PX: f32 = 10.0;

/// 选中脉冲时长。
pub const PULSE_MS: u64 = 260;
/// 脉冲描边外扩量（px）。
pub const PULSE_RING_PX: f32 = 3.0;
/// 脉冲起始不透明度。
pub const PULSE_ALPHA: f32 = 0.55;

/// 按压高亮时长与最大不透明度。
pub const PRESS_MS: u64 = 380;
pub const PRESS_ALPHA: f32 = 0.10;

/// toast：淡入 / 停留 / 淡出。
pub const TOAST_IN_MS: u64 = 180;
pub const TOAST_HOLD_MS: u64 = 1400;
pub const TOAST_OUT_MS: u64 = 200;
/// toast 入场位移（自上方落入）。
pub const TOAST_SHIFT_PX: f32 = -8.0;

/// 导出成功后的按钮与状态行入场。
pub const EXPORT_MS: u64 = 240;
pub const EXPORT_STAGGER_MS: u64 = 60;
pub const EXPORT_SHIFT_PX: f32 = 12.0;

/// 画布内容变化（撤销 / 重做 / 切图层）的闪烁。
pub const FLASH_MS: u64 = 140;
pub const FLASH_ALPHA: f32 = 0.10;

/// 线性进度：`now` 早于 `since` 时为 0，超过 `dur` 钳到 1。
///
/// # 边界
/// `dur == 0` 视为「瞬时完成」，返回 1.0，避免除零。
pub fn progress(now: u64, since: u64, dur: u64) -> f32 {
    if dur == 0 {
        return 1.0;
    }
    (now.saturating_sub(since) as f32 / dur as f32).clamp(0.0, 1.0)
}

/// ease-out-cubic：起步快、收尾慢，入场标准曲线。
pub fn ease_out(p: f32) -> f32 {
    let t = p.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// ease-in-cubic：起步慢、收尾快，退场曲线。
pub fn ease_in(p: f32) -> f32 {
    let t = p.clamp(0.0, 1.0);
    t * t * t
}

/// 脉冲包络：0 → 1 → 0，中点最大（sin 半周期）。
///
/// 当前只被单测覆盖（`pulse_p` 走这条曲线，环绘制另用线性淡出）：
/// 保留是为了让「峰值型包络」的数值在屏幕接入前先被锁定。
#[allow(dead_code)]
pub fn pulse(p: f32) -> f32 {
    let t = p.clamp(0.0, 1.0);
    // f32 下 sin(π) 约为 -8.7e-8，终点会得到负值；钳到 0 避免负 alpha 与断言抖动。
    (t * std::f32::consts::PI).sin().max(0.0)
}

/// 错峰进度：第 `i` 个元素延后 `i * stagger` 毫秒启动。
pub fn staggered(now: u64, since: u64, i: usize, stagger: u64, dur: u64) -> f32 {
    progress(now, since + i as u64 * stagger, dur)
}

/// 三段包络：淡入 → 停留 → 淡出，返回 0..=1 的不透明度系数。
pub fn envelope(now: u64, since: u64, in_ms: u64, hold_ms: u64, out_ms: u64) -> f32 {
    let t = now.saturating_sub(since);
    if t < in_ms {
        ease_out(progress(now, since, in_ms))
    } else if t < in_ms + hold_ms {
        1.0
    } else {
        1.0 - ease_in(progress(now, since + in_ms + hold_ms, out_ms))
    }
}

/// 位移：从 `from` 缓动到 0（入场用），返回当前偏移。
pub fn slide_in(now: u64, since: u64, dur: u64, from: f32) -> f32 {
    from * (1.0 - ease_out(progress(now, since, dur)))
}

/// 位移：从 0 缓动到 `to`（退场用），返回当前偏移。
///
/// 与 `slide_in` 成对保留：退场屏尚未接入，数值先由单测守护。
#[allow(dead_code)]
pub fn slide_out(now: u64, since: u64, dur: u64, to: f32) -> f32 {
    to * ease_in(progress(now, since, dur))
}

/// 是否仍在 `dur` 毫秒内（用于 `needs_frames` 的存续判断）。
pub fn running(now: u64, since: u64, dur: u64) -> bool {
    now.saturating_sub(since) < dur
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_clamps_before_and_after_window() {
        // 起始前不得为负，结束后必须钳到 1，否则绘制层会拿到反向位移。
        assert_eq!(progress(100, 200, 100), 0.0);
        assert_eq!(progress(200, 200, 100), 0.0);
        assert_eq!(progress(250, 200, 100), 0.5);
        assert_eq!(progress(300, 200, 100), 1.0);
        assert_eq!(progress(900, 200, 100), 1.0);
        // 零时长视为瞬时完成。
        assert_eq!(progress(200, 200, 0), 1.0);
    }

    #[test]
    fn easing_curves_hit_endpoints_and_stay_monotonic() {
        for f in [ease_out as fn(f32) -> f32, ease_in] {
            assert!((f(0.0) - 0.0).abs() < 1e-6);
            assert!((f(1.0) - 1.0).abs() < 1e-6);
            let mut prev = 0.0;
            for step in 0..=10 {
                let v = f(step as f32 / 10.0);
                assert!(v >= prev - 1e-6, "curve must be monotonic");
                prev = v;
            }
        }
        // ease-out 起步必须比线性快，ease-in 起步必须更慢。
        assert!(ease_out(0.25) > 0.25);
        assert!(ease_in(0.25) < 0.25);
    }

    #[test]
    fn pulse_peaks_at_middle_and_vanishes_at_ends() {
        assert!((pulse(0.0)).abs() < 1e-6);
        assert!((pulse(0.5) - 1.0).abs() < 1e-6);
        assert!(pulse(1.0).abs() < 1e-6);
    }

    #[test]
    fn staggered_delays_each_item() {
        // 第 0 张立刻开始，第 3 张要等到 150ms 后才起步。
        assert_eq!(staggered(1000, 1000, 0, 50, 300), 0.0);
        assert_eq!(staggered(1000, 1000, 3, 50, 300), 0.0);
        assert!((staggered(1150, 1000, 3, 50, 300) - 0.0).abs() < 1e-6);
        assert!(staggered(1300, 1000, 3, 50, 300) > 0.0);
        assert_eq!(staggered(1500, 1000, 3, 50, 300), 1.0);
    }

    #[test]
    fn envelope_fades_in_holds_then_fades_out() {
        // 0ms 起步为 0，180ms 已全亮，停留期恒为 1，结尾回到 0。
        assert_eq!(envelope(1000, 1000, 180, 1400, 200), 0.0);
        assert_eq!(envelope(1200, 1000, 180, 1400, 200), 1.0);
        assert_eq!(envelope(2000, 1000, 180, 1400, 200), 1.0);
        // 淡出段起点仍为 1（刚开始淡出），走完 200ms 才归零。
        assert_eq!(envelope(2580, 1000, 180, 1400, 200), 1.0);
        assert_eq!(envelope(2780, 1000, 180, 1400, 200), 0.0);
        assert!(envelope(2680, 1000, 180, 1400, 200) < 1.0);
    }

    #[test]
    fn slide_in_starts_at_from_and_ends_at_zero() {
        assert_eq!(slide_in(500, 500, 240, 24.0), 24.0);
        assert_eq!(slide_in(740, 500, 240, 24.0), 0.0);
        // 中段必须落在 (0, from) 之间。
        let mid = slide_in(620, 500, 240, 24.0);
        assert!(mid > 0.0 && mid < 24.0);
        // 退场方向相反。
        assert_eq!(slide_out(500, 500, 220, 600.0), 0.0);
        assert_eq!(slide_out(720, 500, 220, 600.0), 600.0);
    }

    #[test]
    fn running_flags_only_inside_window() {
        assert!(running(500, 500, 240));
        assert!(running(600, 500, 240));
        assert!(!running(740, 500, 240));
    }
}
