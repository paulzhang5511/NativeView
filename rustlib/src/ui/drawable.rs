//! Drawable：L1 抽象 + Color/Gradient 实现（PRD §5.1.4，Task 16）。
//!
//! # 设计
//! - `Drawable` 以枚举承载（V1 固定三种实现，无动态分发需求）；
//! - `BitmapDrawable` 消费 `gfx::Image`（fd/mmap 共享，零拷贝）；
//! - `GradientDrawable` 为纯色/圆角/描边子集（非 Android 全量 shape）。

use crate::gfx::color::Color;
use crate::gfx::image::Image;
use crate::gfx::paint::Paint;
use crate::gfx::path::Path;
use crate::gfx::scene::{Command, Rect, Scene};
use crate::gfx::shader::{Shader, TileMode};

/// 渐变方向（Android `GradientDrawable.Orientation` 子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientOrientation {
    /// 左→右。
    Horizontal,
    /// 上→下。
    Vertical,
}

/// Drawable 实现（V1 封闭集合）。
#[derive(Clone, Debug, PartialEq)]
pub enum Drawable {
    /// 纯色矩形。
    Color(ColorDrawable),
    /// 位图（fd/mmap 共享）。
    Bitmap(BitmapDrawable),
    /// 纯色圆角/描边（渐变可选）。
    Gradient(GradientDrawable),
}

impl Drawable {
    /// 绘制到场景（bounds 为自身记录的矩形）。
    pub fn draw(&self, scene: &mut Scene) {
        let bounds = self.bounds();
        match self {
            Drawable::Color(d) => d.draw_into(scene, bounds),
            Drawable::Bitmap(d) => d.draw_into(scene, bounds),
            Drawable::Gradient(d) => d.draw_into(scene, bounds),
        }
    }

    /// 当前 bounds。
    pub fn bounds(&self) -> Rect {
        match self {
            Drawable::Color(d) => d.bounds,
            Drawable::Bitmap(d) => d.bounds,
            Drawable::Gradient(d) => d.bounds,
        }
    }

    /// 设置 bounds（宿主 View layout 时同步）。
    pub fn set_bounds(&mut self, rect: Rect) {
        match self {
            Drawable::Color(d) => d.bounds = rect,
            Drawable::Bitmap(d) => d.bounds = rect,
            Drawable::Gradient(d) => d.bounds = rect,
        }
    }

    /// 透明度 [0,255]（0 = 不绘制）。
    pub fn set_alpha(&mut self, alpha: u8) {
        match self {
            Drawable::Color(d) => d.alpha = alpha,
            Drawable::Bitmap(d) => d.alpha = alpha,
            Drawable::Gradient(d) => d.alpha = alpha,
        }
    }

    /// 可见性开关（false = 不绘制）。
    pub fn set_visible(&mut self, visible: bool) {
        match self {
            Drawable::Color(d) => d.visible = visible,
            Drawable::Bitmap(d) => d.visible = visible,
            Drawable::Gradient(d) => d.visible = visible,
        }
    }
}

/// 绘制前置检查：alpha=0 或不可见时跳过。
fn gated(alpha: u8, visible: bool) -> bool {
    alpha > 0 && visible
}

/// 纯色矩形。
#[derive(Clone, Debug, PartialEq)]
pub struct ColorDrawable {
    /// 填充色。
    pub color: Color,
    /// bounds。
    pub bounds: Rect,
    /// 透明度（乘进 color）。
    pub alpha: u8,
    /// 可见性。
    pub visible: bool,
}

impl ColorDrawable {
    /// 创建（默认全可见）。
    pub fn new(color: Color) -> Self {
        Self {
            color,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            alpha: 255,
            visible: true,
        }
    }

    /// 设置 bounds。
    pub fn set_bounds(&mut self, rect: Rect) {
        self.bounds = rect;
    }

    /// 设置透明度。
    pub fn set_alpha(&mut self, alpha: u8) {
        self.alpha = alpha;
    }

    /// 设置可见性。
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 绘制到场景。
    pub fn draw(&self, scene: &mut Scene) {
        self.draw_into(scene, self.bounds)
    }

    /// 绘制（bounds 外部传入）。
    fn draw_into(&self, scene: &mut Scene, bounds: Rect) {
        if !gated(self.alpha, self.visible) {
            return;
        }
        let mut color = self.color;
        color.a = scale_u8(color.a, self.alpha);
        let paint = Paint::default().color(color);
        scene.commands.push(Command::FillRect {
            rect: bounds,
            paint,
        });
    }
}

/// 位图。
#[derive(Clone, Debug, PartialEq)]
pub struct BitmapDrawable {
    /// 图像（共享像素）。
    pub image: Image,
    /// bounds。
    pub bounds: Rect,
    /// 透明度（V1 仅门控：0 跳过，非 0 原样绘制——alpha 混合留 L2）。
    pub alpha: u8,
    /// 可见性。
    pub visible: bool,
}

impl BitmapDrawable {
    /// 创建。
    pub fn new(image: Image) -> Self {
        Self {
            image,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            alpha: 255,
            visible: true,
        }
    }

    /// 设置 bounds。
    pub fn set_bounds(&mut self, rect: Rect) {
        self.bounds = rect;
    }

    /// 设置透明度。
    pub fn set_alpha(&mut self, alpha: u8) {
        self.alpha = alpha;
    }

    /// 设置可见性。
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 绘制到场景。
    pub fn draw(&self, scene: &mut Scene) {
        self.draw_into(scene, self.bounds)
    }

    /// 绘制：拉伸到 bounds（FIT_XY 语义；ScaleType 由 ImageView 处理）。
    fn draw_into(&self, scene: &mut Scene, bounds: Rect) {
        if !gated(self.alpha, self.visible) {
            return;
        }
        scene.commands.push(Command::DrawImage {
            image: self.image.clone(),
            x: bounds.x,
            y: bounds.y,
            scale: 1.0,
        });
    }
}

/// 纯色/圆角/描边（渐变可选）。
#[derive(Clone, Debug, PartialEq)]
pub struct GradientDrawable {
    /// 背景内容：纯色或渐变。
    pub fill: GradientFill,
    /// 圆角半径（0 = 直角）。
    pub corner_radius: f64,
    /// 描边色（`None` = 无描边）。
    pub stroke_color: Option<Color>,
    /// 描边宽。
    pub stroke_width: f32,
    /// bounds。
    pub bounds: Rect,
    /// 透明度。
    pub alpha: u8,
    /// 可见性。
    pub visible: bool,
}

/// 填充内容。
#[derive(Clone, Debug, PartialEq)]
pub enum GradientFill {
    /// 纯色。
    Solid(Color),
    /// 双色渐变（方向由 orientation 决定，bounds 对角线）。
    LinearGradient(GradientOrientation, Color, Color),
}

impl GradientDrawable {
    /// 纯色圆角背景。
    pub fn rounded(color: Color, corner_radius: f64) -> Self {
        Self {
            fill: GradientFill::Solid(color),
            corner_radius,
            stroke_color: None,
            stroke_width: 0.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            alpha: 255,
            visible: true,
        }
    }

    /// 双色渐变背景（无圆角无描边基础形态）。
    pub fn linear(orientation: GradientOrientation, colors: &[Color; 2]) -> Self {
        Self {
            fill: GradientFill::LinearGradient(orientation, colors[0], colors[1]),
            corner_radius: 0.0,
            stroke_color: None,
            stroke_width: 0.0,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            alpha: 255,
            visible: true,
        }
    }

    /// 设置 bounds。
    pub fn set_bounds(&mut self, rect: Rect) {
        self.bounds = rect;
    }

    /// 设置透明度。
    pub fn set_alpha(&mut self, alpha: u8) {
        self.alpha = alpha;
    }

    /// 设置可见性。
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 绘制到场景。
    pub fn draw(&self, scene: &mut Scene) {
        self.draw_into(scene, self.bounds)
    }

    /// 绘制：圆角矩形路径 + 填充/渐变/描边。
    fn draw_into(&self, scene: &mut Scene, bounds: Rect) {
        if !gated(self.alpha, self.visible) {
            return;
        }
        let path = if self.corner_radius > 0.0 {
            round_rect_path(bounds, self.corner_radius)
        } else {
            Path::rect(bounds.x, bounds.y, bounds.w, bounds.h)
        };
        let mut paint = Paint::default();
        match &self.fill {
            GradientFill::Solid(c) => {
                let mut c = *c;
                c.a = scale_u8(c.a, self.alpha);
                paint = paint.color(c);
            }
            GradientFill::LinearGradient(o, c0, c1) => {
                let mut c0 = *c0;
                let mut c1 = *c1;
                c0.a = scale_u8(c0.a, self.alpha);
                c1.a = scale_u8(c1.a, self.alpha);
                let (x0, y0, x1, y1) = match o {
                    GradientOrientation::Horizontal => {
                        (bounds.x, bounds.y, bounds.x + bounds.w, bounds.y)
                    }
                    GradientOrientation::Vertical => {
                        (bounds.x, bounds.y, bounds.x, bounds.y + bounds.h)
                    }
                };
                paint = paint.shader(Some(Shader::linear(
                    x0,
                    y0,
                    x1,
                    y1,
                    &[c0, c1],
                    None,
                    TileMode::Clamp,
                )));
            }
        }
        scene.commands.push(Command::FillPath {
            path: path.clone(),
            paint,
        });
        if let Some(sc) = self.stroke_color {
            let stroke_paint = Paint::default()
                .color(sc)
                .style(crate::gfx::paint::PaintStyle::Stroke)
                .stroke_width(self.stroke_width);
            scene.commands.push(Command::StrokePath {
                path,
                paint: stroke_paint,
            });
        }
    }
}

/// u8 alpha 乘法（0-255 域）。
fn scale_u8(a: u8, scale: u8) -> u8 {
    ((u16::from(a) * u16::from(scale)) / 255) as u8
}

/// 圆角矩形路径（四角各一段圆弧，kurbo arc 近似用 cubic_to）。
fn round_rect_path(r: Rect, radius: f64) -> Path {
    let x0 = r.x;
    let y0 = r.y;
    let x1 = r.x + r.w;
    let y1 = r.y + r.h;
    // 半径不超过短边一半（与 Android 自动收缩一致）。
    let rad = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    // 圆弧贝塞尔常数 0.5523。
    let k = rad * 0.5523;
    let mut p = Path::new();
    p.move_to(x0 + rad, y0);
    p.line_to(x1 - rad, y0);
    p.cubic_to(x1 - rad + k, y0, x1, y0 + rad - k, x1, y0 + rad);
    p.line_to(x1, y1 - rad);
    p.cubic_to(x1, y1 - rad + k, x1 - rad + k, y1, x1 - rad, y1);
    p.line_to(x0 + rad, y1);
    p.cubic_to(x0 + rad - k, y1, x0, y1 - rad + k, x0, y1 - rad);
    p.line_to(x0, y0 + rad);
    p.cubic_to(x0, y0 + rad - k, x0 + rad - k, y0, x0 + rad, y0);
    p.close();
    p
}
