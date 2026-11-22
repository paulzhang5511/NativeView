//! 动效通道：时间戳状态机、各通道进度、按压与脉冲的覆盖层绘制。
//!
//! # 模型
//! 每个动效是 `Option<(起始时刻, …)>`：新操作直接覆写（可中断、不排队），
//! 因此不存在「动画队列」，也不会有多个动效抢同一属性。
//!
//! # 时钟
//! `clock` 是动画时钟，按「真实帧间隔 / 系统倍率」累积：
//! 倍率 1.0 时与真实时间同速；倍率 0（无障碍「移除动画」）时每帧推进 3000ms，
//! 任何动效都在下一帧直接落到终态，且不再请求帧。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::ui::dimen::dp;

use crate::demo::inkflow_anim as anim;

use super::layout::Layout;
use super::theme::*;
use super::{Inkflow, Pulse, STEP_WHEN_REMOVED, Screen};

impl Inkflow {
    /// 帧推进（宿主帧回调入口，单调毫秒）。
    ///
    /// # 返回
    /// true = 仍需重绘下一帧（还有未结束的动效）。
    ///
    /// # 为什么时钟不在这里归零
    /// `perform` 在两次帧回调之间发生，登记动画时用的是上一次 `clock`；
    /// 若这里把时钟重置，动画会从负进度起跳（表现为「闪一下」）。
    pub(crate) fn advance(&mut self, now: u64) -> bool {
        if now <= self.last_now {
            return self.any_anim();
        }
        let dt = now - self.last_now;
        self.last_now = now;
        let was_active = self.any_anim();
        // 动画时钟按倍率拉伸：真实 16ms 在 2× 慢镜头下只算 8ms 动画时间。
        self.clock = self.clock.saturating_add(self.step_ms(dt));
        // 新 toast：文本变化即视为一次新提示，无需在每个调用点手动打点。
        if self.toast != self.toast_seen {
            self.toast_seen = self.toast.clone();
            if self.toast.is_some() {
                self.toast_at = self.clock;
            }
        }
        was_active || self.any_anim()
    }

    /// 注入系统动画倍率（`animator_duration_scale`）：0 表示关闭动画。
    pub(crate) fn set_animator_scale(&mut self, scale: f32) {
        self.anim_scale = if scale.is_finite() && scale >= 0.0 {
            scale
        } else {
            0.0
        };
    }

    /// 把真实帧间隔换算成动画时间增量。
    pub(super) fn step_ms(&self, dt: u64) -> u64 {
        if self.anim_scale <= 0.0 {
            // 「移除动画」：一步跨过任何单通道时长，动画立即落到终态。
            return STEP_WHEN_REMOVED;
        }
        (dt as f64 / self.anim_scale as f64) as u64
    }

    /// 是否还有动效未结束（宿主据此决定是否持续请求帧）。
    pub(crate) fn needs_frames(&self) -> bool {
        self.any_anim()
    }

    /// 任一通道仍在计时窗口内。
    pub(super) fn any_anim(&self) -> bool {
        let c = self.clock;
        let page_total =
            anim::PAGE_MS.max(anim::PAGE_BACK_MS.max(anim::CARD_MS + 3 * anim::CARD_STAGGER_MS));
        self.page_anim
            .is_some_and(|(at, _)| anim::running(c, at, page_total))
            || self.sheet_anim.is_some_and(|(at, open, _)| {
                anim::running(
                    c,
                    at,
                    if open {
                        anim::SHEET_OPEN_MS
                    } else {
                        anim::SHEET_CLOSE_MS
                    },
                )
            })
            || self
                .press
                .is_some_and(|(_, at, _)| anim::running(c, at, anim::PRESS_MS))
            || self
                .pulse
                .is_some_and(|(_, _, at)| anim::running(c, at, anim::PULSE_MS))
            || (self.toast.is_some()
                && anim::running(
                    c,
                    self.toast_at,
                    anim::TOAST_IN_MS + anim::TOAST_HOLD_MS + anim::TOAST_OUT_MS,
                ))
            || self
                .filter_at
                .is_some_and(|at| anim::running(c, at, anim::FILTER_MS))
            || self.cards_at.is_some_and(|at| {
                anim::running(
                    c,
                    at,
                    anim::CARD_MS.max(anim::FILTER_MS) + 3 * anim::CARD_STAGGER_MS,
                )
            })
            || self
                .export_at
                .is_some_and(|at| anim::running(c, at, anim::EXPORT_MS + anim::EXPORT_STAGGER_MS))
            || self
                .flash_at
                .is_some_and(|at| anim::running(c, at, anim::FLASH_MS))
    }

    /// 切屏进度 0..=1；无切换动画时为 1（终态）。
    pub(super) fn page_shift(&self) -> f32 {
        let Some((at, forward)) = self.page_anim else {
            return 0.0;
        };
        if forward {
            anim::slide_in(self.clock, at, anim::PAGE_MS, dp(anim::PAGE_SHIFT_PX))
        } else {
            anim::slide_in(
                self.clock,
                at,
                anim::PAGE_BACK_MS,
                dp(anim::PAGE_BACK_SHIFT_PX),
            )
        }
    }

    /// 切屏遮罩不透明度：新页立刻可见，遮罩随进度淡出。
    pub(super) fn page_cover(&self) -> f32 {
        let Some((at, forward)) = self.page_anim else {
            return 0.0;
        };
        let (dur, a0) = if forward {
            (anim::PAGE_MS, anim::PAGE_COVER_ALPHA)
        } else {
            (anim::PAGE_BACK_MS, anim::PAGE_BACK_COVER_ALPHA)
        };
        a0 * (1.0 - anim::ease_out(anim::progress(self.clock, at, dur)))
    }

    /// 笔刷面板展开度 0..=1（0 = 完全收起，1 = 完全展开）。
    ///
    /// 从中途反向时以当前展开度为起点，避免「跳回满屏再退出」的突跳。
    pub(super) fn sheet_p(&self) -> f32 {
        let Some((at, open, from)) = self.sheet_anim else {
            return if self.screen == Screen::BrushSheet {
                1.0
            } else {
                0.0
            };
        };
        if open {
            from + (1.0 - from)
                * anim::ease_out(anim::progress(self.clock, at, anim::SHEET_OPEN_MS))
        } else {
            from * (1.0 - anim::ease_in(anim::progress(self.clock, at, anim::SHEET_CLOSE_MS)))
        }
    }

    /// 按压高亮的衰减系数（0..=1）；仅同屏有效。
    pub(super) fn press_fade(&self) -> f32 {
        let Some((_, at, screen)) = self.press else {
            return 0.0;
        };
        if screen != self.screen {
            return 0.0;
        }
        let p = anim::progress(self.clock, at, anim::PRESS_MS);
        if p >= 1.0 {
            0.0
        } else {
            anim::PRESS_ALPHA * (1.0 - anim::ease_out(p))
        }
    }

    /// 按压高亮：统一叠在命中控件上（不做几何变化，避免文字错位）。
    pub(super) fn draw_press(&self, c: &mut Canvas) {
        let Some((rect, _, screen)) = self.press else {
            return;
        };
        let a = self.press_fade();
        if a <= 0.001 || screen != self.screen {
            return;
        }
        fill_round(
            c,
            rect,
            dp(14.0),
            Color {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF,
                a: (a * 255.0) as u8,
            },
        );
    }

    /// 选中脉冲环：按类别取当前几何统一绘制，避免每个控件各写一遍。
    pub(super) fn draw_pulse_ring(&self, c: &mut Canvas, l: &Layout) {
        let Some((kind, idx, at)) = self.pulse else {
            return;
        };
        let p = anim::progress(self.clock, at, anim::PULSE_MS);
        if p >= 1.0 {
            return;
        }
        let rect = match kind {
            Pulse::Size => l.nc_sizes.get(idx).copied(),
            Pulse::Paper => l.nc_papers.get(idx).copied(),
            Pulse::Color => l.bd_swatches.get(idx).copied(),
            Pulse::Preset => l.sh_presets.get(idx).copied(),
            Pulse::Tool => l.bd_tools.get(idx).copied(),
            Pulse::Filter => l.lib_filters.get(idx).copied(),
        };
        let Some(rect) = rect else {
            return;
        };
        let grow = dp(anim::PULSE_RING_PX) * p;
        let a = anim::PULSE_ALPHA * (1.0 - p);
        let ring = r(
            rect.x as f32 - grow,
            rect.y as f32 - grow,
            rect.w as f32 + grow * 2.0,
            rect.h as f32 + grow * 2.0,
        );
        stroke_round(
            c,
            ring,
            dp(18.0),
            dp(1.6),
            Color {
                r: 0xB9,
                g: 0xA6,
                b: 0xFF,
                a: (a * 255.0) as u8,
            },
        );
    }

    /// 选中脉冲强度：仅当类别与下标都匹配、且仍在窗口内时非零。
    ///
    /// 注意：这里返回 sin 包络（中点峰值），与 `draw_pulse_ring` 的线性淡出
    /// 是两条不同曲线，不要合并——合并会改变脉冲的外观手感。
    #[cfg(test)]
    pub(super) fn pulse_p(&self, kind: Pulse, idx: usize) -> f32 {
        let Some((k, i, at)) = self.pulse else {
            return 0.0;
        };
        if k != kind || i != idx {
            return 0.0;
        }
        anim::pulse(anim::progress(self.clock, at, anim::PULSE_MS))
    }

    /// 作品库第 i 张卡的入场位移：错峰滑入，动画结束后为 0。
    pub(super) fn card_shift(&self, i: usize) -> f32 {
        // 筛选重入：不做错峰、更短更轻，避免「整屏重放」的廉价感。
        if let Some(at) = self
            .filter_at
            .filter(|at| anim::running(self.clock, *at, anim::FILTER_MS))
        {
            let p = anim::progress(self.clock, at, anim::FILTER_MS);
            return dp(anim::FILTER_SHIFT_PX) * (1.0 - anim::ease_out(p));
        }
        let Some(at) = self.cards_at else {
            return 0.0;
        };
        let p = anim::staggered(self.clock, at, i, anim::CARD_STAGGER_MS, anim::CARD_MS);
        dp(anim::CARD_SHIFT_PX) * (1.0 - anim::ease_out(p))
    }

    /// 记录一次选中脉冲（切换尺寸/纸色/色板/预设/工具/筛选时调用）。
    pub(super) fn mark_pulse(&mut self, kind: Pulse, idx: usize) {
        self.pulse = Some((kind, idx, self.clock));
    }

    /// toast 的不透明度系数（含淡入、停留、淡出）。
    pub(super) fn toast_p(&self) -> f32 {
        if self.toast.is_none() {
            return 0.0;
        }
        anim::envelope(
            self.clock,
            self.toast_at,
            anim::TOAST_IN_MS,
            anim::TOAST_HOLD_MS,
            anim::TOAST_OUT_MS,
        )
    }

    /// 导出成功后的入场进度（状态行与双按钮）。
    pub(super) fn export_p(&self) -> f32 {
        match self.export_at {
            Some(at) => anim::progress(self.clock, at, anim::EXPORT_MS),
            None => 1.0,
        }
    }

    /// 画布内容变化的闪烁强度（撤销 / 重做 / 切图层）。
    pub(super) fn flash_p(&self) -> f32 {
        let Some(at) = self.flash_at else {
            return 0.0;
        };
        let p = anim::progress(self.clock, at, anim::FLASH_MS);
        anim::FLASH_ALPHA * (1.0 - anim::ease_out(p))
    }
}
