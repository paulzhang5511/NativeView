//! 坐标换算：屏幕/窗口坐标 → 视图本地坐标（PRD §5.2，Task 17）。
//!
//! # 语义对齐（Android `getLocationInWindow` / `View.getLocalVisibleRect`）
//! - 视图本地坐标系原点 = 自身 (left, top)（含 translation 偏移）；
//! - 父到子换算：`local = window - Σ(祖先 left/top + translation)`；
//! - 滚动容器（ScrollView）在 V1 以内容平移实现滚动，触摸命中时
//!   需把窗口坐标转换进「内容坐标系」（child left/top 已含滚动偏移的
//!   补偿由命中测试时反向加回）；本模块只提供逐层原语，容器语义
//!   由 dispatch 链（Task 18）组合。

use crate::ui::view::View;

/// 视图本地坐标（相对自身左上角）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalPoint {
    /// x（右为正）。
    pub x: f32,
    /// y（下为正）。
    pub y: f32,
}

/// 窗口坐标 → 视图本地坐标（单层）。
///
/// # 参数
/// `view`：目标视图（已 layout）；`p`：窗口坐标系下的点。
///
/// # 语义
/// `local.x = p.x - (view.left + view.translation_x)`，
/// y 同理——与 Android 视图在窗口中的实际绘制位置一致
/// （draw 时背景矩形即按 left+translation 定位）。
pub fn to_local(view: &View, p: LocalPoint) -> LocalPoint {
    LocalPoint {
        x: p.x - (view.left() + view.translation_x()),
        y: p.y - (view.top() + view.translation_y()),
    }
}

/// 判断窗口坐标点是否落在视图 bounds 内（含 translation，
/// 不含父裁剪——命中测试由分发链逐层判断）。
///
/// # 边界语义
/// 与 Android `View.onTouchEvent` 的接收条件一致：命中区域为
/// [left+tx, right+tx) × [top+ty, bottom+ty)，边界值按左闭右开。
pub fn hit_test(view: &View, p: LocalPoint) -> bool {
    let x0 = view.left() + view.translation_x();
    let y0 = view.top() + view.translation_y();
    let x1 = view.right() + view.translation_x();
    let y1 = view.bottom() + view.translation_y();
    // GONE 不参与命中（从布局中消失）。
    if view.visibility() == crate::ui::view::Visibility::Gone {
        return false;
    }
    p.x >= x0 && p.x < x1 && p.y >= y0 && p.y < y1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::view::{View, Visibility};

    /// 基准：未平移视图的换算与命中。
    #[test]
    fn to_local_basic() {
        let mut v = View::new();
        v.layout(100.0, 200.0, 300.0, 400.0);
        let p = to_local(&v, LocalPoint { x: 150.0, y: 250.0 });
        assert_eq!(p, LocalPoint { x: 50.0, y: 50.0 });
        assert!(hit_test(&v, LocalPoint { x: 150.0, y: 250.0 }));
        assert!(!hit_test(&v, LocalPoint { x: 50.0, y: 250.0 }));
        assert!(!hit_test(&v, LocalPoint { x: 350.0, y: 250.0 }));
    }

    /// translation 参与换算（绘制位置与命中区域同步偏移）。
    #[test]
    fn translation_shifts_hit_bounds() {
        let mut v = View::new();
        v.layout(0.0, 0.0, 100.0, 100.0);
        v.set_translation_x(50.0);
        v.set_translation_y(50.0);
        // 实际绘制区域 (50,50)-(150,150)。
        assert!(hit_test(&v, LocalPoint { x: 50.0, y: 50.0 }));
        assert!(!hit_test(&v, LocalPoint { x: 49.9, y: 50.0 }));
        assert!(
            !hit_test(&v, LocalPoint { x: 150.0, y: 150.0 }),
            "right-open"
        );
        // 本地坐标 = 窗口坐标 - (left + translation)。
        let p = to_local(&v, LocalPoint { x: 60.0, y: 60.0 });
        assert_eq!(p, LocalPoint { x: 10.0, y: 10.0 });
    }

    /// GONE 不命中。
    #[test]
    fn gone_never_hit() {
        let mut v = View::new();
        v.layout(0.0, 0.0, 100.0, 100.0);
        v.set_visibility(Visibility::Gone);
        assert!(!hit_test(&v, LocalPoint { x: 50.0, y: 50.0 }));
    }

    /// INVISIBLE 仍命中（占位语义，Android 一致）。
    #[test]
    fn invisible_still_hit() {
        let mut v = View::new();
        v.layout(0.0, 0.0, 100.0, 100.0);
        v.set_visibility(Visibility::Invisible);
        assert!(hit_test(&v, LocalPoint { x: 50.0, y: 50.0 }));
    }
}
