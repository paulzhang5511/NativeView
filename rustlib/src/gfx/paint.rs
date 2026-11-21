//! 画笔（PRD §5.1.3：`Paint` L1 子集）。
//!
//! # 设计
//! - 纯值类型（Clone），与 `Canvas` 状态栈配合（save 快照 paint）；
//! - 颜色与描边参数为 L1 集；shader（渐变）由 `Shader` 承担，
//!   经 `Arc` 共享停靠点表，clone 不复制像素/数组。

use super::color::Color;
use super::shader::Shader;

/// 画笔样式（PRD §5.1.3 `setStyle`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintStyle {
    /// 只填充。
    #[default]
    Fill,
    /// 只描边。
    Stroke,
    /// 填充 + 描边。
    FillAndStroke,
}

/// 线帽样式（PRD §5.1.3 `setStrokeCap`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeCap {
    /// 平头（无线延伸）。
    #[default]
    Butt,
    /// 圆头（延伸半径）。
    Round,
    /// 方头（延伸半宽）。
    Square,
}

/// 线连接样式（PRD §5.1.3 `setStrokeJoin`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrokeJoin {
    /// 尖角。
    #[default]
    Miter,
    /// 斜切。
    Bevel,
    /// 圆角。
    Round,
}

/// 画笔：颜色、样式、描边、抗锯齿、着色器。
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    /// 主色（描边用色/填充色）。
    pub color: Color,
    /// 绘制样式（填充/描边/两者）。
    pub style: PaintStyle,
    /// 描边宽度（像素，<=0 时 stroke 视为无效，按 Skia 语义 hairline 不提供）。
    pub stroke_width: f32,
    /// 线帽。
    pub stroke_cap: StrokeCap,
    /// 线连接。
    pub stroke_join: StrokeJoin,
    /// 抗锯齿开关。
    pub antialias: bool,
    /// 着色器（渐变）：`None` 时用主色填充。
    pub shader: Option<Shader>,
}

impl Default for Paint {
    /// Android 默认值对齐：黑色、FILL、描边宽 0、BUTT/MITER、AA 关。
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            style: PaintStyle::Fill,
            stroke_width: 0.0,
            stroke_cap: StrokeCap::Butt,
            stroke_join: StrokeJoin::Miter,
            antialias: false,
            shader: None,
        }
    }
}

impl Paint {
    /// setColor。
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// setStyle。
    pub fn style(mut self, style: PaintStyle) -> Self {
        self.style = style;
        self
    }

    /// setStrokeWidth。
    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }

    /// setAntiAlias。
    pub fn antialias(mut self, aa: bool) -> Self {
        self.antialias = aa;
        self
    }

    /// setAlpha（保留 RGB，仅改 alpha 通道；与 Android `Paint.setAlpha` 语义一致）。
    pub fn alpha(mut self, alpha: u8) -> Self {
        self.color.a = alpha;
        self
    }

    /// setShader（渐变；None 清除）。
    pub fn shader(mut self, shader: Option<Shader>) -> Self {
        self.shader = shader;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::shader::TileMode;

    /// 默认值表（与 Android Paint 默认对齐）。
    #[test]
    fn defaults_match_android() {
        let p = Paint::default();
        assert_eq!(p.color, Color::BLACK);
        assert_eq!(p.style, PaintStyle::Fill);
        assert_eq!(p.stroke_width, 0.0);
        assert_eq!(p.stroke_cap, StrokeCap::Butt);
        assert_eq!(p.stroke_join, StrokeJoin::Miter);
        assert!(!p.antialias);
    }

    /// builder 链式设置。
    #[test]
    fn builder_setters() {
        let p = Paint::default()
            .color(Color::RED)
            .style(PaintStyle::Stroke)
            .stroke_width(4.0)
            .antialias(true);
        assert_eq!(p.color, Color::RED);
        assert_eq!(p.style, PaintStyle::Stroke);
        assert_eq!(p.stroke_width, 4.0);
        assert!(p.antialias);
    }

    /// setAlpha 仅改 alpha 通道。
    #[test]
    fn alpha_only_touches_alpha() {
        let p = Paint::default().color(Color::WHITE).alpha(0x80);
        assert_eq!(p.color.r, 0xFF);
        assert_eq!(p.color.g, 0xFF);
        assert_eq!(p.color.b, 0xFF);
        assert_eq!(p.color.a, 0x80);
    }

    /// shader 共享：clone Paint 不复制渐变停靠点表。
    #[test]
    fn shader_shared_on_clone() {
        let shader = Shader::linear(
            0.0,
            0.0,
            10.0,
            0.0,
            &[Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        );
        let p = Paint::default().shader(Some(shader));
        let q = p.clone();
        match (p.shader, q.shader) {
            (
                Some(Shader::LinearGradient { stops: a, .. }),
                Some(Shader::LinearGradient { stops: b, .. }),
            ) => {
                // Arc 指针相等：同一停靠点缓冲。
                assert!(std::ptr::eq(a.as_ptr(), b.as_ptr()));
            }
            _ => panic!("expected LinearGradient"),
        }
    }

    /// shader(None) 清除渐变。
    #[test]
    fn shader_cleared() {
        let shader = Shader::sweep(0.0, 0.0, &[Color::RED], None);
        let p = Paint::default().shader(Some(shader)).shader(None);
        assert!(p.shader.is_none());
    }
}
