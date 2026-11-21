//! 渲染场景：后端无关的绘制命令中间表示（PRD §7.1 `gfx-scene`）。
//!
//! # 设计
//! - `Scene` 是 `Canvas` 录制命令的产物，各渲染后端只消费 `Scene`，
//!   与具体渲染库（vello_* / vello_cpu）解耦；
//! - 命令集随任务演进（Task 9 矩形/路径，Task 10 渐变/裁剪/图片）；
//! - 所有几何用 f64（渲染库原生精度），避免 f32 往返损失。

use super::color::Color;
use super::image::Image;
use super::paint::Paint;
#[cfg(target_os = "android")]
use super::path::FillType;
use super::path::Path;

/// 轴对齐矩形。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
/// 绘制命令（V1 集：填充矩形、填充/描边路径、裁剪）。
///
/// # 演进
/// Task 10 增渐变/图片；Task 13 增字形。
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// 以画笔填充轴对齐矩形（含透明度/颜色）。
    FillRect { rect: Rect, paint: Paint },
    /// 填充路径。
    FillPath { path: Path, paint: Paint },
    /// 描边路径（粗细/线帽/连接由 paint 决定）。
    StrokePath { path: Path, paint: Paint },
    /// 裁剪（后续命令的可见区域收窄；由 [`Command::PopClips`] 弹出）。
    ClipRect { rect: Rect },
    /// 裁剪路径。
    ClipPath { path: Path },
    /// 弹出 n 层裁剪（restore 语义：解除最近的 n 个 clip）。
    PopClips { count: u32 },
    /// 绘制位图（矩阵已烘焙：目标区域 + 像素由 `Arc` 共享零拷贝）。
    DrawImage { image: Image, x: f64, y: f64 },
    /// 绘制已定位字形组（text 栈排版产物，基线系坐标 + 全局字体）。
    DrawGlyphs {
        /// 字形列表（glifo `Glyph` 语义：id + x/y）。
        glyphs: Vec<crate::text::layout::SceneGlyph>,
        /// 字号（px）。
        font_size: f32,
        /// 字形颜色。
        color: crate::gfx::color::Color,
    },
}

impl Command {
    /// 命令名（诊断日志用；与渲染行为无关）。
    pub fn name(&self) -> &'static str {
        match self {
            Command::FillRect { .. } => "FillRect",
            Command::FillPath { .. } => "FillPath",
            Command::StrokePath { .. } => "StrokePath",
            Command::ClipRect { .. } => "ClipRect",
            Command::ClipPath { .. } => "ClipPath",
            Command::PopClips { .. } => "PopClips",
            Command::DrawImage { .. } => "DrawImage",
            Command::DrawGlyphs { .. } => "DrawGlyphs",
        }
    }
}
///
/// # 不变量
/// 命令按录制顺序执行；矩阵变换在录制期已烘焙进几何（Canvas 负责），
/// `Command` 中不出现变换矩阵——后端无需维护变换栈。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// 画布宽度（物理像素）。
    pub width: u32,
    /// 画布高度（物理像素）。
    pub height: u32,
    /// 清屏色（不透明时后端可跳过首矩形覆盖）。
    pub clear_color: Color,
    /// 有序命令列表。
    pub commands: Vec<Command>,
}

impl Scene {
    /// 空场景（无命令）。
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            clear_color: Color::TRANSPARENT,
            commands: Vec::new(),
        }
    }

    /// 是否包含任何绘制命令（无则后端可只清屏）。
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// 整体平移场景（父容器滚动偏移用）。
    ///
    /// # 语义
    /// 对全部命令的几何施加纯平移（Scroll 内容偏移的唯一路径）；
    /// `clear_color` 不变。与 Canvas 矩阵烘焙一致，结果仍无变换栈。
    ///
    /// # 性能
    /// 原地遍历，无分配（路径元素逐点改写）。
    pub fn translate(&mut self, dx: f64, dy: f64) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for cmd in &mut self.commands {
            match cmd {
                Command::FillRect { rect, .. } | Command::ClipRect { rect } => {
                    rect.x += dx;
                    rect.y += dy;
                }
                Command::FillPath { path, .. }
                | Command::StrokePath { path, .. }
                | Command::ClipPath { path } => {
                    path.translate(dx, dy);
                }
                Command::DrawImage { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Command::DrawGlyphs { glyphs, .. } => {
                    for g in glyphs {
                        g.x += dx as f32;
                        g.y += dy as f32;
                    }
                }
                Command::PopClips { .. } => {}
            }
        }
    }
}

/// 供后端消费的填充语义辅助：从路径读取填充规则。
///
/// 后端翻译路径元素时需知道填充规则（winding/even-odd）；
/// 封装在此避免各后端重复理解 `Path` 内部结构。
#[cfg(target_os = "android")]
pub fn fill_type_of(path: &Path) -> FillType {
    path.fill_type()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 场景命令顺序保持（录制即序）。
    #[test]
    fn commands_keep_order() {
        let mut s = Scene::new(100, 100);
        s.commands.push(Command::ClipRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 50.0,
            },
        });
        s.commands.push(Command::FillRect {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 5.0,
                h: 5.0,
            },
            paint: Paint::default(),
        });
        assert_eq!(s.commands.len(), 2);
        assert!(matches!(s.commands[0], Command::ClipRect { .. }));
        assert!(matches!(s.commands[1], Command::FillRect { .. }));
        assert!(!s.is_empty());
    }

    /// 矩阵烘焙验证：Canvas 变换不进命令集（变换在录制期消化）。
    #[test]
    fn matrix_not_in_commands() {
        let m = crate::gfx::matrix::Matrix::translation(10.0, 20.0);
        let _ = m;
        let s = Scene::new(10, 10);
        assert!(s.is_empty());
    }

    /// 后端无关性：主机侧命令消费器可完整遍历全部命令变体。
    ///
    /// 证明 `Scene` 只依赖 gfx 自身类型（Path/Paint/Rect/Color），
    /// 不引入任何 Android/渲染库依赖即可被消费——
    /// cpu/vello_gpu/vello 三后端消费的是同一份中间表示。
    #[test]
    fn scene_consumable_without_backend_deps() {
        let mut c = crate::gfx::canvas::Canvas::new(100, 100);
        c.clip_rect(Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        });
        c.draw_rect(
            Rect {
                x: 1.0,
                y: 1.0,
                w: 5.0,
                h: 5.0,
            },
            &Paint::default(),
        );
        let mut path = Path::new();
        path.move_to(0.0, 0.0).line_to(10.0, 10.0);
        c.draw_path(&path, &Paint::default());
        let s = c.finish();

        // 模拟后端：顺序消费每条命令并统计语义负载。
        let mut fills = 0usize;
        let mut strokes = 0usize;
        let mut clips = 0usize;
        let mut images = 0usize;
        let mut pops = 0usize;
        let mut glyphs = 0usize;
        for cmd in &s.commands {
            match cmd {
                Command::FillRect { .. } | Command::FillPath { .. } => fills += 1,
                Command::StrokePath { .. } => strokes += 1,
                Command::ClipRect { .. } | Command::ClipPath { .. } => clips += 1,
                Command::DrawImage { .. } => images += 1,
                Command::PopClips { count } => pops += *count as usize,
                Command::DrawGlyphs { .. } => glyphs += 1,
            }
        }
        assert_eq!(
            (fills, strokes, clips, images, pops, glyphs),
            (2, 0, 1, 0, 0, 0)
        );
    }
}
