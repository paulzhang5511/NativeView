//! 文字绘制辅助。
//!
//! # 为什么单独一层
//! `Canvas` 的命令必须整体排在文字命令之前（否则后追加的背景矩形会盖掉字形），
//! 所以图形与文字分成两个 Scene；本模块只负责往文字 Scene 追加已排版的行。

use crate::gfx::color::Color;
use crate::gfx::scene::{Rect, Scene};
use crate::text::layout::{TextAlign, TextStyleRequest};
use crate::text::render::emit_layout;

use super::Inkflow;

impl Inkflow {
    /// 绘制一行文本（宽度受限自动折行），返回占用高度。
    pub(super) fn text_block(
        &mut self,
        scene: &mut Scene,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: TextAlign,
        max_w: f32,
    ) -> f32 {
        let Some(cx) = self.text.as_mut() else {
            return 0.0;
        };
        let req = TextStyleRequest {
            text: s,
            font_size: size,
            max_advance: Some(max_w),
            align,
            color,
            line_spacing_multiplier: 1.25,
        };
        let t = std::time::Instant::now();
        let Some(layout) = cx.layout(&req) else {
            return 0.0;
        };
        crate::platform::perf::add_build_sub_us(
            crate::platform::perf::BUILD_SUB_TEXT,
            t.elapsed(),
        );
        let w = layout.width();
        let ax = match align {
            TextAlign::Left => x as f64,
            TextAlign::Center => (x + ((max_w - w) / 2.0)) as f64,
            TextAlign::Right => (x + max_w - w) as f64,
        };
        let h = layout.height();
        emit_layout(scene, &layout, ax, y as f64);
        h
    }
    /// 在矩形内水平+垂直居中绘制单行文本。
    pub(super) fn text_center(
        &mut self,
        scene: &mut Scene,
        s: &str,
        rect: Rect,
        size: f32,
        color: Color,
    ) {
        let Some(cx) = self.text.as_mut() else {
            return;
        };
        let req = TextStyleRequest {
            text: s,
            font_size: size,
            max_advance: Some(rect.w as f32),
            align: TextAlign::Center,
            color,
            line_spacing_multiplier: 1.2,
        };
        let t = std::time::Instant::now();
        let Some(layout) = cx.layout(&req) else {
            return;
        };
        crate::platform::perf::add_build_sub_us(
            crate::platform::perf::BUILD_SUB_TEXT,
            t.elapsed(),
        );
        let h = layout.height() as f64;
        let y = rect.y + ((rect.h - h) / 2.0).max(0.0);
        emit_layout(scene, &layout, rect.x, y);
    }
}
