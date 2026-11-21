//! 画布：命令录制的核心入口（PRD §5.1.3）。
//!
//! # 设计
//! - `Canvas` 不渲染：只把绘制调用翻译成 [`Scene`] 命令（矩阵在录制期烘焙），
//!   渲染由后端完成（`ui` → `gfx` → `backends` 单向依赖）；
//! - 状态栈（save/restore）与 Android `Canvas` 语义一致：
//!   `save` 快照（矩阵 + 裁剪深度），`restore` 弹出；
//! - 变换在录制期应用于几何：点变换 + 非均匀缩放对描边宽度的影响
//!   取近似（沿主对角缩放因子），V1 不做逐段补偿（记录为 L2 差异）。

use super::color::Color;
use super::image::Image;
use super::matrix::Matrix;
use super::paint::Paint;
use super::path::Path;
use super::scene::{Command, Rect, Scene};

/// 画布状态栈的一层快照。
#[derive(Clone, Debug)]
struct State {
    matrix: Matrix,
    /// save 时刻已压栈的裁剪层数（restore 时弹出超出部分）。
    clip_depth: u32,
}

/// 画布（录制模式）。
///
/// # 生命周期
/// 借用中的 `Scene`；`finish` 交还。录制期间命令持续追加。
pub struct Canvas {
    scene: Scene,
    /// 当前变换矩阵（命令录制时烘焙用）。
    matrix: Matrix,
    /// save 栈（不含当前状态；save 压栈当前状态后继续）。
    stack: Vec<State>,
    /// 未配对 restore 数量防御（多余 restore 按 Android 语义忽略）。
    _marker: (),
    /// 当前活跃裁剪层数（clip 命令计数，restore 时弹出超出 save 时刻的部分）。
    clip_depth: u32,
}

impl Canvas {
    /// 创建画布（清屏色默认透明；用 `set_clear_color` 覆盖）。
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            scene: Scene::new(width, height),
            matrix: Matrix::IDENTITY,
            stack: Vec::new(),
            _marker: (),
            clip_depth: 0,
        }
    }

    /// 设置清屏色（建议在录制开头调用）。
    pub fn set_clear_color(&mut self, color: Color) {
        self.scene.clear_color = color;
    }

    /// 结束录制，交还场景。
    pub fn finish(self) -> Scene {
        self.scene
    }

    // ---- 状态栈（PRD §5.1.3 save/restore）----

    /// 压栈当前状态（矩阵 + 裁剪深度）。
    pub fn save(&mut self) {
        self.stack.push(State {
            matrix: self.matrix,
            clip_depth: self.clip_depth,
        });
    }

    /// 弹出最近一次 save（矩阵恢复 + 弹出超出 save 时刻的裁剪层；
    /// Android 语义：多余 restore 忽略）。
    pub fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            // 弹出 save 后新增的裁剪层（clip_depth 差值 > 0 时发 PopClips 命令）。
            let extra = self.clip_depth.saturating_sub(state.clip_depth);
            if extra > 0 {
                self.scene.commands.push(Command::PopClips { count: extra });
                self.clip_depth = state.clip_depth;
            }
            self.matrix = state.matrix;
        }
    }

    // ---- 变换（PRD §5.1.3 translate/scale/rotate/skew + concat）----

    /// 平移（右乘：在已有变换之上先应用平移）。
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.matrix = self.matrix.concat(&Matrix::translation(dx, dy));
    }

    /// 缩放。
    pub fn scale(&mut self, sx: f64, sy: f64) {
        self.matrix = self.matrix.concat(&Matrix::scaling(sx, sy));
    }

    /// 旋转（弧度，绕当前原点）。
    pub fn rotate(&mut self, radians: f64) {
        self.matrix = self.matrix.concat(&Matrix::rotation(radians));
    }

    /// 倾斜（skewX：x' = x + k·y；skewY：y' = k·x + y）。
    pub fn skew(&mut self, kx: f64, ky: f64) {
        let sk = Matrix {
            a: 1.0,
            b: ky,
            c: kx,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        };
        self.matrix = self.matrix.concat(&sk);
    }

    /// 拼接矩阵（右乘）。
    pub fn concat(&mut self, m: &Matrix) {
        self.matrix = self.matrix.concat(m);
    }

    // ---- 绘制（PRD §5.1.3 drawRect/drawPath...）----

    /// 矩形绘制（fill 与/或 stroke 按 paint.style；shader 随矩阵烘焙）。
    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        let baked = self.bake_rect(rect);
        let paint = self.bake_paint(paint);
        match paint.style {
            super::paint::PaintStyle::Fill | super::paint::PaintStyle::FillAndStroke => {
                self.scene.commands.push(Command::FillRect {
                    rect: baked,
                    paint: paint.clone(),
                });
                if paint.style == super::paint::PaintStyle::FillAndStroke {
                    // 描边矩形转为路径描边（复用同一路径，粗细由后端）。
                    let path = Path::rect(baked.x, baked.y, baked.w, baked.h);
                    self.scene
                        .commands
                        .push(Command::StrokePath { path, paint });
                }
            }
            super::paint::PaintStyle::Stroke => {
                // 描边矩形转为路径描边（复用同一路径，粗细由后端）。
                let path = Path::rect(baked.x, baked.y, baked.w, baked.h);
                self.scene
                    .commands
                    .push(Command::StrokePath { path, paint });
            }
        }
    }

    /// 路径绘制（fill 与/或 stroke 按 paint.style 与 path.fill_type）。
    pub fn draw_path(&mut self, path: &Path, paint: &Paint) {
        let baked = self.bake_path(path);
        let paint = self.bake_paint(paint);
        match paint.style {
            super::paint::PaintStyle::Fill | super::paint::PaintStyle::FillAndStroke => {
                self.scene.commands.push(Command::FillPath {
                    path: baked.clone(),
                    paint: paint.clone(),
                });
                if paint.style == super::paint::PaintStyle::FillAndStroke {
                    self.scene
                        .commands
                        .push(Command::StrokePath { path: baked, paint });
                }
            }
            super::paint::PaintStyle::Stroke => {
                self.scene
                    .commands
                    .push(Command::StrokePath { path: baked, paint });
            }
        }
    }

    /// 位图绘制（矩阵烘焙为左上角目标点；V1 无混合模式，整图不透明搬运）。
    pub fn draw_image(&mut self, image: &Image, x: f64, y: f64) {
        let (x, y) = self.matrix.map_point(x, y);
        self.scene.commands.push(Command::DrawImage {
            image: image.clone(),
            x,
            y,
        });
    }

    // ---- 裁剪（PRD §5.1.3 clipRect/clipPath）----

    /// 矩形裁剪（几何随当前矩阵烘焙——Android clipRect 支持矩阵变换的语义）。
    pub fn clip_rect(&mut self, rect: Rect) {
        let baked = self.bake_rect(rect);
        self.scene.commands.push(Command::ClipRect { rect: baked });
        self.clip_depth += 1;
    }

    /// 路径裁剪（同样烘焙矩阵）。
    pub fn clip_path(&mut self, path: &Path) {
        let baked = self.bake_path(path);
        self.scene.commands.push(Command::ClipPath { path: baked });
        self.clip_depth += 1;
    }

    // ---- 几何烘焙（私有）----

    /// 矩形四角随矩阵变换（旋转后不再是轴对齐：退化为路径表示交给后端）。
    fn bake_rect(&self, rect: Rect) -> Rect {
        // V1 快路径：纯平移/缩放时轴对齐保持。
        if self.matrix.b == 0.0 && self.matrix.c == 0.0 {
            let (x0, y0) = self.matrix.map_point(rect.x, rect.y);
            let (x1, y1) = self.matrix.map_point(rect.x + rect.w, rect.y + rect.h);
            return Rect {
                x: x0.min(x1),
                y: y0.min(y1),
                w: (x1 - x0).abs(),
                h: (y1 - y0).abs(),
            };
        }
        // 旋转/倾斜：包络矩形（保守放大，L2 近似记录）。
        let corners = [
            self.matrix.map_point(rect.x, rect.y),
            self.matrix.map_point(rect.x + rect.w, rect.y),
            self.matrix.map_point(rect.x, rect.y + rect.h),
            self.matrix.map_point(rect.x + rect.w, rect.y + rect.h),
        ];
        let xs = [corners[0].0, corners[1].0, corners[2].0, corners[3].0];
        let ys = [corners[0].1, corners[1].1, corners[2].1, corners[3].1];
        Rect {
            x: xs.iter().copied().fold(f64::INFINITY, f64::min),
            y: ys.iter().copied().fold(f64::INFINITY, f64::min),
            w: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - xs.iter().copied().fold(f64::INFINITY, f64::min),
            h: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - ys.iter().copied().fold(f64::INFINITY, f64::min),
        }
    }

    /// 画笔烘焙：shader 渐变几何随当前矩阵变换，颜色等其余参数原样。
    ///
    /// # 性能
    /// 纯色（shader=None）直通；渐变克隆 Arc 停靠点表（不复制像素）。
    fn bake_paint(&self, paint: &Paint) -> Paint {
        match &paint.shader {
            None => paint.clone(),
            Some(shader) => Paint {
                shader: Some(shader.bake(&self.matrix)),
                ..paint.clone()
            },
        }
    }

    /// 路径元素逐点随矩阵变换。
    fn bake_path(&self, path: &Path) -> Path {
        let m = &self.matrix;
        let mut out = Path::new();
        for el in path.elements() {
            match el {
                super::path::PathElement::MoveTo { x, y } => {
                    let (x, y) = m.map_point(*x, *y);
                    out.move_to(x, y);
                }
                super::path::PathElement::LineTo { x, y } => {
                    let (x, y) = m.map_point(*x, *y);
                    out.line_to(x, y);
                }
                super::path::PathElement::QuadTo { cx, cy, x, y } => {
                    let (cx, cy) = m.map_point(*cx, *cy);
                    let (x, y) = m.map_point(*x, *y);
                    out.quad_to(cx, cy, x, y);
                }
                super::path::PathElement::CubicTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => {
                    let (c1x, c1y) = m.map_point(*c1x, *c1y);
                    let (c2x, c2y) = m.map_point(*c2x, *c2y);
                    let (x, y) = m.map_point(*x, *y);
                    out.cubic_to(c1x, c1y, c2x, c2y, x, y);
                }
                super::path::PathElement::Close => {
                    out.close();
                }
            }
        }
        out.set_fill_type(path.fill_type());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::paint::PaintStyle;

    /// 状态栈：save/restore 矩阵恢复。
    #[test]
    fn save_restore_matrix() {
        let mut c = Canvas::new(100, 100);
        c.translate(10.0, 20.0);
        c.save();
        c.translate(5.0, 5.0);
        c.draw_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            &Paint::default(),
        );
        c.restore();
        c.draw_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 2.0,
                h: 2.0,
            },
            &Paint::default(),
        );
        let s = c.finish();
        // 第一矩形：(15,25)；第二矩形：(10,20)
        match &s.commands[0] {
            Command::FillRect { rect, .. } => {
                assert_eq!(rect.x, 15.0);
                assert_eq!(rect.y, 25.0);
            }
            _ => panic!("expected FillRect"),
        }
        match &s.commands[1] {
            Command::FillRect { rect, .. } => {
                assert_eq!(rect.x, 10.0);
                assert_eq!(rect.y, 20.0);
            }
            _ => panic!("expected FillRect"),
        }
    }

    /// 变换组合：translate → rotate 的复合次序。
    ///
    /// 旋转 90° 后矩形不再轴对齐，录制为包络矩形（L2 近似）：
    /// 原矩形 (10,0)-(15,5) 旋转后四角 x∈[-5,0]（包络宽 5），
    /// 再平移 (50,50) → x∈[45,50]。
    #[test]
    fn transform_composition() {
        let mut c = Canvas::new(100, 100);
        c.translate(50.0, 50.0);
        c.rotate(std::f64::consts::FRAC_PI_2);
        c.draw_rect(
            Rect {
                x: 10.0,
                y: 0.0,
                w: 5.0,
                h: 5.0,
            },
            &Paint::default(),
        );
        let s = c.finish();
        // 原矩形角点 (10,0)-(15,5) 旋转后 x∈[-5,0]、y∈[10,15]；
        // 平移 (50,50) 后包络 x∈[45,50]、y∈[60,65]。
        match &s.commands[0] {
            Command::FillRect { rect, .. } => {
                assert!((rect.x - 45.0).abs() < 1e-9, "x={}", rect.x);
                assert!((rect.y - 60.0).abs() < 1e-9, "y={}", rect.y);
                assert!((rect.w - 5.0).abs() < 1e-9, "w={}", rect.w);
                assert!((rect.h - 5.0).abs() < 1e-9, "h={}", rect.h);
            }
            _ => panic!("expected FillRect"),
        }
    }

    /// 多余 restore 忽略（Android 语义），不 panic 不截断。
    #[test]
    fn extra_restore_ignored() {
        let mut c = Canvas::new(100, 100);
        c.translate(10.0, 0.0);
        c.draw_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            &Paint::default(),
        );
        c.restore(); // 无 save：忽略
        let s = c.finish();
        assert_eq!(s.commands.len(), 1);
    }

    /// clip + restore：restore 产生 PopClips 命令弹出裁剪层。
    #[test]
    fn clip_popped_by_restore() {
        let mut c = Canvas::new(100, 100);
        c.save();
        c.clip_rect(Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        });
        c.draw_rect(
            Rect {
                x: 10.0,
                y: 10.0,
                w: 5.0,
                h: 5.0,
            },
            &Paint::default(),
        );
        c.restore();
        let s = c.finish();
        // 裁剪 + 绘制 + PopClips 三条命令。
        assert_eq!(s.commands.len(), 3);
        assert!(matches!(s.commands[0], Command::ClipRect { .. }));
        assert!(matches!(s.commands[1], Command::FillRect { .. }));
        assert!(matches!(s.commands[2], Command::PopClips { count: 1 }));
    }

    /// stroke 风格矩形 → 转路径描边命令。
    #[test]
    fn stroke_rect_becomes_stroke_path() {
        let mut c = Canvas::new(100, 100);
        let paint = Paint::default().style(PaintStyle::Stroke).stroke_width(2.0);
        c.draw_rect(
            Rect {
                x: 1.0,
                y: 1.0,
                w: 8.0,
                h: 8.0,
            },
            &paint,
        );
        let s = c.finish();
        assert!(matches!(s.commands[0], Command::StrokePath { .. }));
    }

    /// 路径矩阵烘焙：moveTo/lineTo 坐标平移。
    #[test]
    fn path_geometry_baked() {
        let mut c = Canvas::new(100, 100);
        c.translate(5.0, 7.0);
        let mut path = Path::new();
        path.move_to(0.0, 0.0).line_to(3.0, 4.0);
        c.draw_path(&path, &Paint::default());
        let s = c.finish();
        match &s.commands[0] {
            Command::FillPath { path, .. } => {
                let els = path.elements();
                assert_eq!(
                    els[0],
                    crate::gfx::path::PathElement::MoveTo { x: 5.0, y: 7.0 }
                );
                assert_eq!(
                    els[1],
                    crate::gfx::path::PathElement::LineTo { x: 8.0, y: 11.0 }
                );
            }
            _ => panic!("expected FillPath"),
        }
    }

    /// 渐变随矩阵烘焙：线性渐变端点平移（录制期消化，命令内已是设备坐标）。
    #[test]
    fn shader_baked_by_canvas_matrix() {
        let shader = crate::gfx::shader::Shader::linear(
            0.0,
            0.0,
            10.0,
            0.0,
            &[Color::RED, Color::BLUE],
            None,
            crate::gfx::shader::TileMode::Clamp,
        );
        let mut c = Canvas::new(100, 100);
        c.translate(20.0, 30.0);
        c.draw_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            &Paint::default().shader(Some(shader)),
        );
        let s = c.finish();
        match &s.commands[0] {
            Command::FillRect { paint, .. } => match &paint.shader {
                Some(crate::gfx::shader::Shader::LinearGradient { x0, x1, .. }) => {
                    assert_eq!(*x0, 20.0);
                    assert_eq!(*x1, 30.0);
                }
                _ => panic!("expected LinearGradient"),
            },
            _ => panic!("expected FillRect"),
        }
    }

    /// draw_image：目标点随矩阵烘焙；命令持有 Image 共享句柄。
    #[test]
    fn draw_image_bakes_position() {
        let img = crate::gfx::image::Image::decode(&test_png()).unwrap();
        let mut c = Canvas::new(100, 100);
        c.translate(10.0, 20.0);
        c.draw_image(&img, 1.0, 2.0);
        let s = c.finish();
        match &s.commands[0] {
            Command::DrawImage { image, x, y } => {
                assert_eq!(*x, 11.0);
                assert_eq!(*y, 22.0);
                assert_eq!(image.width(), img.width());
                assert_eq!(image.height(), img.height());
            }
            _ => panic!("expected DrawImage"),
        }
    }

    /// 生成 1x1 红色 PNG 测试字节。
    fn test_png() -> Vec<u8> {
        let img = image::RgbaImage::from_raw(1, 1, vec![0xFF, 0x00, 0x00, 0xFF]).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        png.into_inner()
    }
}
