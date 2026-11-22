//! 图片控件：ImageView + ScaleType 三种（PRD §5.1.7，Task 16）。

use crate::gfx::image::Image;
use crate::gfx::scene::{Rect, Scene};
use crate::ui::measure_spec::{MeasureSpec, Mode};
use crate::ui::view::View;

/// 图片缩放类型（Android 数值语义子集）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScaleType {
    /// 拉伸铺满（不保持宽高比）。
    #[default]
    FitXY,
    /// 保持比例适配居中。
    FitCenter,
    /// 保持比例短边铺满（溢出裁剪）。
    CenterCrop,
}

/// 图片视图。
pub struct ImageView {
    /// 自身 View 状态。
    pub view: View,
    /// 图像（None = 不绘制）。
    image: Option<Image>,
    /// 缩放类型。
    scale_type: ScaleType,
}

impl ImageView {
    /// 创建。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            image: None,
            scale_type: ScaleType::FitXY,
        }
    }

    /// 设置图像。
    pub fn set_image(&mut self, image: Option<Image>) {
        self.image = image;
        self.view.request_layout();
    }

    /// 当前图像。
    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    /// 设置缩放类型。
    pub fn set_scale_type(&mut self, t: ScaleType) {
        self.scale_type = t;
        self.view.request_layout();
    }

    /// 当前缩放类型。
    pub fn scale_type(&self) -> ScaleType {
        self.scale_type
    }

    /// 测量：WRAP 取图像固有尺寸；否则按 spec。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let (w, h) = match &self.image {
            Some(img) => (img.width() as f32, img.height() as f32),
            None => (0.0, 0.0),
        };
        let final_w = match width_spec.mode() {
            Mode::Exactly => width_spec.size() as f32,
            _ => w,
        };
        let final_h = match height_spec.mode() {
            Mode::Exactly => height_spec.size() as f32,
            _ => h,
        };
        self.view.set_measured_dimension(final_w, final_h);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 内容可见区域（ScaleType 变换后的目标矩形，供测试/绘制）。
    ///
    /// # 返回
    /// `None` = 无图像或零尺寸。
    pub fn content_bounds(&self) -> Rect {
        let (Some(img), cw, ch) = (self.image.as_ref(), self.view.width(), self.view.height())
        else {
            return Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            };
        };
        let iw = img.width() as f64;
        let ih = img.height() as f64;
        let vx = self.view.left() as f64;
        let vy = self.view.top() as f64;
        match self.scale_type {
            ScaleType::FitXY => Rect {
                x: vx,
                y: vy,
                w: cw as f64,
                h: ch as f64,
            },
            ScaleType::FitCenter => {
                if iw <= 0.0 || ih <= 0.0 {
                    return Rect {
                        x: vx,
                        y: vy,
                        w: 0.0,
                        h: 0.0,
                    };
                }
                let scale = (cw as f64 / iw).min(ch as f64 / ih);
                let w = iw * scale;
                let h = ih * scale;
                Rect {
                    x: vx + (cw as f64 - w) / 2.0,
                    y: vy + (ch as f64 - h) / 2.0,
                    w,
                    h,
                }
            }
            ScaleType::CenterCrop => {
                if iw <= 0.0 || ih <= 0.0 {
                    return Rect {
                        x: vx,
                        y: vy,
                        w: 0.0,
                        h: 0.0,
                    };
                }
                // 短边铺满：可见区域即 bounds（源图裁剪在绘制阶段）。
                Rect {
                    x: vx,
                    y: vy,
                    w: cw as f64,
                    h: ch as f64,
                }
            }
        }
    }

    /// 绘制：背景 + 图像（按 ScaleType 计算目标矩形，V1 以 DrawImage
    /// 平铺近似：FIT_XY 精确，FIT_CENTER/CENTER_CROP 由 bounds 居中近似，
    /// 精确采样缩放进 L2）。
    pub fn draw(&self, scene: &mut Scene) {
        self.view.draw(scene);
        let Some(img) = &self.image else { return };
        let dst = self.content_bounds();
        scene.commands.push(crate::gfx::scene::Command::DrawImage {
            image: img.clone(),
            x: dst.x,
            y: dst.y,
            scale: 1.0,
        });
    }
}

impl Default for ImageView {
    fn default() -> Self {
        Self::new()
    }
}

/// ImageView 的 Widget 实现（叶子：默认手势推进即可，无子树）。
impl crate::ui::widgets::widget::Widget for ImageView {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        ImageView::measure(self, width_spec, height_spec)
    }
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        ImageView::layout(self, left, top, right, bottom)
    }
    fn draw(&self, scene: &mut Scene) {
        ImageView::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
