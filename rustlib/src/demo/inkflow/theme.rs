//! Inkflow 视觉令牌与基础绘制工具。
//!
//! # 职责
//! - 颜色令牌：只在这里定义，各屏模块禁止硬编码色值，改主题只改这一处；
//! - 几何与绘制小工具：圆角矩形 / 圆 / 全屏遮罩 / 矩形纵向平移（动效用）。
//!
//! # 可见性
//! 全部 `pub(super)`：仅 `demo::inkflow` 子树可见，不对外暴露。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::gfx::paint::{Paint, PaintStyle};
use crate::gfx::path::Path;
use crate::gfx::scene::Rect;

pub(super) fn bg() -> Color {
    Color {
        r: 0x12,
        g: 0x0F,
        b: 0x18,
        a: 255,
    }
}

pub(super) fn surface() -> Color {
    Color {
        r: 0x1E,
        g: 0x1A,
        b: 0x26,
        a: 255,
    }
}

pub(super) fn surface_hi() -> Color {
    Color {
        r: 0x2A,
        g: 0x24,
        b: 0x36,
        a: 255,
    }
}

pub(super) fn violet() -> Color {
    Color {
        r: 0x7C,
        g: 0x5C,
        b: 0xFF,
        a: 255,
    }
}

/// 紫色高亮描边色（选中行的 1.5dp 外描边）。
pub(super) fn violet_hi() -> Color {
    Color {
        r: 0xB9,
        g: 0xA6,
        b: 0xFF,
        a: 255,
    }
}

pub(super) fn text_main() -> Color {
    Color {
        r: 0xF2,
        g: 0xEE,
        b: 0xF8,
        a: 255,
    }
}

pub(super) fn text_dim() -> Color {
    Color {
        r: 0x9A,
        g: 0x92,
        b: 0xAD,
        a: 255,
    }
}

pub(super) fn text_faint() -> Color {
    Color {
        r: 0x6B,
        g: 0x64,
        b: 0x7C,
        a: 255,
    }
}

pub(super) fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        x: x as f64,
        y: y as f64,
        w: w.max(0.0) as f64,
        h: h.max(0.0) as f64,
    }
}

/// 矩形纵向平移（动效层用；命中测试仍用原矩形）。
pub(super) fn shift_rect(rect: Rect, dy: f32) -> Rect {
    if dy == 0.0 {
        return rect;
    }
    r(
        rect.x as f32,
        rect.y as f32 + dy,
        rect.w as f32,
        rect.h as f32,
    )
}

/// 命中矩形内是否包含点。
pub(super) fn has(rect: Rect, x: f32, y: f32, slack: f32) -> bool {
    let (x, y) = (x as f64, y as f64);
    x >= rect.x - slack as f64
        && x <= rect.x + rect.w + slack as f64
        && y >= rect.y - slack as f64
        && y <= rect.y + rect.h + slack as f64
}

/// 圆角填充。
pub(super) fn fill_round(c: &mut Canvas, rect: Rect, radius: f32, color: Color) {
    c.draw_path(
        &Path::rounded_rect(rect.x, rect.y, rect.w, rect.h, radius as f64),
        &Paint::default().color(color).antialias(true),
    );
}

/// 圆角描边。
pub(super) fn stroke_round(c: &mut Canvas, rect: Rect, radius: f32, width: f32, color: Color) {
    c.draw_path(
        &Path::rounded_rect(rect.x, rect.y, rect.w, rect.h, radius as f64),
        &Paint::default()
            .color(color)
            .antialias(true)
            .style(PaintStyle::Stroke)
            .stroke_width(width),
    );
}

/// 全屏压暗层：`base` 是 RGB 基准（近黑），`alpha` 为不透明度。
///
/// 只用于动效遮罩：不参与命中测试，也不改变任何控件的可见性语义。
pub(super) fn veil(c: &mut Canvas, w: f32, h: f32, base: u8, alpha: f32) {
    veil_rect(c, r(0.0, 0.0, w, h), base, alpha);
}

/// 区域内压暗（同一套颜色语义，供画布内容闪烁复用）。
pub(super) fn veil_rect(c: &mut Canvas, rect: Rect, base: u8, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    c.draw_rect(
        rect,
        &Paint::default().color(Color {
            r: base,
            g: base,
            b: base.saturating_add(8),
            a: (alpha.clamp(0.0, 1.0) * 255.0) as u8,
        }),
    );
}

/// 实心圆。
pub(super) fn fill_circle(c: &mut Canvas, cx: f64, cy: f64, radius: f64, color: Color) {
    c.draw_path(
        &Path::circle(cx, cy, radius),
        &Paint::default().color(color).antialias(true),
    );
}
