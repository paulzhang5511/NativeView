//! 路径（PRD §5.1.3：`Path` L1 子集）。
//!
//! # 设计
//! - 路径元素（moveTo/lineTo/quadTo/cubicTo/close）为后端无关的中间表示，
//!   各渲染后端负责翻译为自己的几何类型（vello 各家 Rect/Path API 不同）；
//! - V1 不做 `Path.op` 布尔运算（PRD §16 待决 4：kurbo 无稳定布尔运算），
//!   预留 `FillType` 供 even-odd 填充。

/// 填充规则（PRD §5.1.3 `setFillType`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillType {
    /// 非零环绕（默认，与 Android `Path.FillType.WINDING` 一致）。
    #[default]
    Winding,
    /// 奇偶规则（`EVEN_ODD`）。
    EvenOdd,
}

/// 路径元素（按追加顺序记录）。
///
/// # 不变量
/// 首元素必为 `MoveTo`（构造器保证）；`Close` 后再追加 `Line/Quad/Cubic`
/// 隐含重新开始一段（与 Skia 语义一致）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathElement {
    /// 移动到 (x, y)（开新段）。
    MoveTo { x: f64, y: f64 },
    /// 直线到 (x, y)。
    LineTo { x: f64, y: f64 },
    /// 二次贝塞尔：控制点 (cx, cy)，终点 (x, y)。
    QuadTo { cx: f64, cy: f64, x: f64, y: f64 },
    /// 三次贝塞尔：控制点 1、2，终点。
    CubicTo {
        c1x: f64,
        c1y: f64,
        c2x: f64,
        c2y: f64,
        x: f64,
        y: f64,
    },
    /// 闭合当前段（回到最近 MoveTo 点）。
    Close,
}

/// 可复用路径（builder 模式，与 Android `Path` 对齐）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    /// 有序路径元素。
    elements: Vec<PathElement>,
    /// 填充规则。
    fill_type: FillType,
}

impl Path {
    /// 空路径。
    pub fn new() -> Self {
        Self::default()
    }

    /// 矩形快捷构造（等价 moveTo+lineTo×3+close）。
    pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Self {
        let mut p = Path::new();
        p.move_to(x, y);
        p.line_to(x + w, y);
        p.line_to(x + w, y + h);
        p.line_to(x, y + h);
        p.close();
        p
    }

    /// 圆快捷构造（中心 + 半径，四段三次贝塞尔近似，kappa=0.5522847498）。
    pub fn circle(cx: f64, cy: f64, r: f64) -> Self {
        let k = r * 0.5522847498;
        let mut p = Path::new();
        p.move_to(cx + r, cy);
        p.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
        p.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
        p.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
        p.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
        p.close();
        p
    }

    /// 椭圆快捷构造（外接矩形，四段三次贝塞尔）。
    pub fn oval(x: f64, y: f64, w: f64, h: f64) -> Self {
        let kx = w * 0.5 * 0.5522847498;
        let ky = h * 0.5 * 0.5522847498;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (rx, ry) = (w / 2.0, h / 2.0);
        let mut p = Path::new();
        p.move_to(cx + rx, cy);
        p.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
        p.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
        p.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
        p.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
        p.close();
        p
    }

    /// 圆角矩形快捷构造（四角等半径，每角两段三次贝塞尔近似）。
    ///
    /// # 边界
    /// 半径自动钳制到 `min(w, h) / 2`，避免小尺寸角半径溢出。
    pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Self {
        let r = r.clamp(0.0, w.min(h) / 2.0);
        let k = r * 0.5522847498;
        let (right, bottom) = (x + w, y + h);
        let mut p = Path::new();
        p.move_to(x + r, y);
        p.line_to(right - r, y);
        p.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
        p.line_to(right, bottom - r);
        p.cubic_to(
            right,
            bottom - r + k,
            right - r + k,
            bottom,
            right - r,
            bottom,
        );
        p.line_to(x + r, bottom);
        p.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
        p.line_to(x, y + r);
        p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
        p.close();
        p
    }

    /// moveTo（后续 lineTo 等的段起点）。
    pub fn move_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.elements.push(PathElement::MoveTo { x, y });
        self
    }

    /// lineTo。
    pub fn line_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.elements.push(PathElement::LineTo { x, y });
        self
    }

    /// quadTo（二次贝塞尔）。
    pub fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) -> &mut Self {
        self.elements.push(PathElement::QuadTo { cx, cy, x, y });
        self
    }

    /// cubicTo（三次贝塞尔）。
    pub fn cubic_to(
        &mut self,
        c1x: f64,
        c1y: f64,
        c2x: f64,
        c2y: f64,
        x: f64,
        y: f64,
    ) -> &mut Self {
        self.elements.push(PathElement::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
        });
        self
    }

    /// close（闭合当前段）。
    pub fn close(&mut self) -> &mut Self {
        self.elements.push(PathElement::Close);
        self
    }

    /// setFillType。
    pub fn set_fill_type(&mut self, ft: FillType) -> &mut Self {
        self.fill_type = ft;
        self
    }

    /// 重置为空路径（Android `reset`）。
    pub fn reset(&mut self) {
        self.elements.clear();
        self.fill_type = FillType::Winding;
    }

    /// 是否为空（无任何元素）。
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// 原地平移全部元素（Scene::translate 的路径分支）。
    ///
    /// # 性能
    /// 单次遍历，无分配。
    pub fn translate(&mut self, dx: f64, dy: f64) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for el in &mut self.elements {
            match el {
                PathElement::MoveTo { x, y } | PathElement::LineTo { x, y } => {
                    *x += dx;
                    *y += dy;
                }
                PathElement::QuadTo { cx, cy, x, y } => {
                    *cx += dx;
                    *cy += dy;
                    *x += dx;
                    *y += dy;
                }
                PathElement::CubicTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => {
                    *c1x += dx;
                    *c1y += dy;
                    *c2x += dx;
                    *c2y += dy;
                    *x += dx;
                    *y += dy;
                }
                PathElement::Close => {}
            }
        }
    }

    /// 只读访问路径元素（后端消费接口）。
    pub fn elements(&self) -> &[PathElement] {
        &self.elements
    }

    /// 填充规则。
    pub fn fill_type(&self) -> FillType {
        self.fill_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// moveTo/lineTo 顺序记录正确。
    #[test]
    fn elements_recorded_in_order() {
        let mut p = Path::new();
        p.move_to(0.0, 0.0).line_to(10.0, 0.0).line_to(10.0, 10.0);
        assert_eq!(
            p.elements(),
            [
                PathElement::MoveTo { x: 0.0, y: 0.0 },
                PathElement::LineTo { x: 10.0, y: 0.0 },
                PathElement::LineTo { x: 10.0, y: 10.0 },
            ]
        );
    }

    /// 矩形构造 = moveTo + 3×lineTo + close。
    #[test]
    fn rect_shape() {
        let p = Path::rect(1.0, 2.0, 3.0, 4.0);
        assert_eq!(p.elements().len(), 5);
        assert_eq!(p.elements()[0], PathElement::MoveTo { x: 1.0, y: 2.0 });
        assert_eq!(*p.elements().last().unwrap(), PathElement::Close);
    }

    /// 圆构造：moveTo + 4×cubic + close = 6 元素。
    #[test]
    fn circle_shape() {
        let p = Path::circle(0.0, 0.0, 5.0);
        assert_eq!(p.elements().len(), 6);
        let cubics = p
            .elements()
            .iter()
            .filter(|e| matches!(e, PathElement::CubicTo { .. }))
            .count();
        assert_eq!(cubics, 4);
    }

    /// 椭圆构造元素数与圆一致（6）。
    #[test]
    fn oval_shape() {
        let p = Path::oval(0.0, 0.0, 10.0, 20.0);
        assert_eq!(p.elements().len(), 6);
    }

    /// FillType 设置与默认值。
    #[test]
    fn fill_type_setter() {
        let mut p = Path::new();
        assert_eq!(p.fill_type(), FillType::Winding);
        p.set_fill_type(FillType::EvenOdd);
        assert_eq!(p.fill_type(), FillType::EvenOdd);
    }

    /// reset 清空元素并恢复默认 FillType。
    #[test]
    fn reset_clears() {
        let mut p = Path::rect(0.0, 0.0, 1.0, 1.0);
        p.set_fill_type(FillType::EvenOdd);
        p.reset();
        assert!(p.is_empty());
        assert_eq!(p.fill_type(), FillType::Winding);
    }
}
