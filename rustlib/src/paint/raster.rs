//! 笔刷光栅化：把笔尖与轨迹落到 RGBA 缓冲（src-over，非预乘）。
//!
//! # 不变量
//! - 缓冲为 RGBA8888 非预乘、行优先；所有写入都裁剪在声明尺寸内，越界像素直接丢弃；
//! - 抖动与颗粒使用「坐标哈希 + 种子」的确定性伪随机，同一 seed 逐像素可复现
//!   （测试与回放都依赖这一点）；
//! - 橡皮只把 alpha 衰减到 0，不改 RGB，避免擦除处出现黑色尖边。

use super::brush::{Brush, BrushKind};
use super::stroke::radius_for;

/// 脏矩形（左闭右开），供历史快照与局部重绘使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl DirtyRect {
    /// 空矩形（不可见时返回）。
    pub fn empty() -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
        }
    }

    /// 是否没有任何像素受影响。
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// 与另一矩形求并集（用于合并一笔的多个打点）。
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// 像素面积。
    pub fn area(&self) -> u64 {
        if self.is_empty() {
            return 0;
        }
        (self.x1 - self.x0) as u64 * (self.y1 - self.y0) as u64
    }

    /// 直接从缓冲示意（`width/height` 为整层尺寸）。
    pub fn from_pixels(width: u32, height: u32, x: u32, y: u32) -> Self {
        Self {
            x0: x.min(width),
            y0: y.min(height),
            x1: (x + 1).min(width),
            y1: (y + 1).min(height),
        }
    }
}

/// 32 位确定性伪随机（xorshift），用于抖动，避免引入 `rand` 依赖。
fn next_rand(state: &mut u32) -> f32 {
    let mut s = *state;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    *state = s;
    (s & 0x00FF_FFFF) as f32 / 16_777_215.0
}

/// 像素级噪声 [0,1)，颗粒感用。
fn pixel_noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ seed.wrapping_mul(83_492_791);
    h ^= h >> 15;
    h = h.wrapping_mul(2_246_822_519);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / 16_777_215.0
}

/// 非预乘 src-over 合成一个像素。
fn blend_over(dst: &mut [u8], src: [u8; 4]) {
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    if sa >= 1.0 {
        dst[0] = src[0];
        dst[1] = src[1];
        dst[2] = src[2];
        dst[3] = 255;
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        dst[0] = 0;
        dst[1] = 0;
        dst[2] = 0;
        dst[3] = 0;
        return;
    }
    for c in 0..3 {
        let v = (src[c] as f32 * sa + dst[c] as f32 * da * (1.0 - sa)) / out_a;
        dst[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// 擦除一个像素：按覆盖率衰减 alpha。
fn erase(dst: &mut [u8], coverage: f32) {
    let da = dst[3] as f32 * (1.0 - coverage.clamp(0.0, 1.0));
    dst[3] = da.round().clamp(0.0, 255.0) as u8;
}

/// 单点笔尖打点。
///
/// # 参数
/// - `radius`：笔尖半径（像素），小于 0.5 按 0.5 处理；
/// - `color`：RGBA，alpha 为先算好的单点浓度（调用方乘 `opacity/flow`）；
/// - `seed`：抖动种子，同 seed 结果确定；
/// - 返回受影响区域的脏矩形，完全越界时为空。
#[allow(clippy::too_many_arguments)]
pub fn stamp(
    buf: &mut [u8],
    w: u32,
    h: u32,
    cx: f32,
    cy: f32,
    radius: f32,
    color: [u8; 4],
    kind: BrushKind,
    seed: u32,
) -> DirtyRect {
    if w == 0 || h == 0 {
        return DirtyRect::empty();
    }
    let need = (w as u64 * h as u64 * 4) as usize;
    if buf.len() < need {
        return DirtyRect::empty();
    }
    let tuning = kind.tuning();
    let base_r = if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        0.5
    };
    let mut state = seed ^ 0x9E37_79B9;

    // 抖动：喷枪/水彩偏移中心并轻度缩放半径，制造不规则边缘。
    let (cx, cy, r) = if tuning.jitter > 0.0 {
        let jx = next_rand(&mut state) - 0.5;
        let jy = next_rand(&mut state) - 0.5;
        let js = next_rand(&mut state) - 0.5;
        (
            cx + jx * base_r * tuning.jitter * 2.0,
            cy + jy * base_r * tuning.jitter * 2.0,
            (base_r * (1.0 + js * tuning.jitter * 0.6)).max(0.5),
        )
    } else {
        (cx, cy, base_r)
    };

    let x0 = (cx - r - 1.0).floor().max(0.0) as u32;
    let y0 = (cy - r - 1.0).floor().max(0.0) as u32;
    let x1 = ((cx + r + 1.0).ceil() as i64).clamp(0, w as i64) as u32;
    let y1 = ((cy + r + 1.0).ceil() as i64).clamp(0, h as i64) as u32;
    if x1 <= x0 || y1 <= y0 {
        return DirtyRect::empty();
    }

    // 边缘过渡带宽：硬度越高越窄，最窄 1px（保证抗锯齿）。
    let aa = (r * (1.0 - tuning.hardness)).max(1.0);
    let alpha_scale = color[3] as f32 / 255.0;

    for py in y0..y1 {
        for px in x0..x1 {
            let dx = px as f32 + 0.5 - cx;
            let dy = py as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let cover = ((r + 0.5 - d) / aa).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let mut a = cover;
            if tuning.grain > 0.0 {
                let n = pixel_noise(px, py, seed);
                a *= 1.0 - tuning.grain * n;
            }
            let a = a * alpha_scale;
            if a <= 0.004 {
                continue;
            }
            let idx = ((py as u64 * w as u64 + px as u64) * 4) as usize;
            if kind == BrushKind::Eraser {
                erase(&mut buf[idx..idx + 4], a);
            } else {
                let sa = (a * 255.0).round().clamp(1.0, 255.0) as u8;
                blend_over(&mut buf[idx..idx + 4], [color[0], color[1], color[2], sa]);
            }
        }
    }
    DirtyRect { x0, y0, x1, y1 }
}

/// 沿轨迹连续打点，返回整笔脏矩形。
///
/// 打点间距取 `max(0.7, 半径 * 0.18)`，保证快速划动时不出现断点；
/// 压感逐点线性插值后经 [`radius_for`] 映射半径。
pub fn stroke(
    buf: &mut [u8],
    w: u32,
    h: u32,
    points: &[(f32, f32, f32)],
    brush: &Brush,
    color: [u8; 4],
    seed: u32,
) -> DirtyRect {
    if points.is_empty() {
        return DirtyRect::empty();
    }
    let tuning = brush.kind.tuning();
    let a = (color[3] as f32 / 255.0) * brush.opacity * brush.flow * tuning.flow_scale;
    let sa = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    let color = [color[0], color[1], color[2], sa];
    let base = brush.radius();
    let mut dirty = DirtyRect::empty();

    // 单点（tap）：留下一个圆点。
    if points.len() == 1 {
        let (x, y, p) = points[0];
        return dirty.union(stamp(
            buf,
            w,
            h,
            x,
            y,
            radius_for(p, base, 0.35),
            color,
            brush.kind,
            seed,
        ));
    }

    for (i, pair) in points.windows(2).enumerate() {
        let (ax, ay, ap) = pair[0];
        let (bx, by, bp) = pair[1];
        let dist = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        let rad = radius_for((ap + bp) * 0.5, base, 0.35);
        let spacing = (rad * 0.18).max(0.7);
        let steps = (dist / spacing).ceil();
        let steps = if steps.is_finite() {
            steps.max(1.0) as usize
        } else {
            1
        };

        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let x = ax + (bx - ax) * t;
            let y = ay + (by - ay) * t;
            let p = ap + (bp - ap) * t;
            // 每笔起点固定打点，避免 steps=1 且 t=0 时漏掉位置
            let point_seed =
                seed ^ (i as u32).wrapping_mul(2_654_435_761) ^ (s as u32).wrapping_mul(40_503);
            dirty = dirty.union(stamp(
                buf,
                w,
                h,
                x,
                y,
                radius_for(p, base, 0.35),
                color,
                brush.kind,
                point_seed,
            ));
        }
    }
    // 终点补一枪，保证抬手处有满半径落点。
    let (lx, ly, lp) = points[points.len() - 1];
    dirty = dirty.union(stamp(
        buf,
        w,
        h,
        lx,
        ly,
        radius_for(lp, base, 0.35),
        color,
        brush.kind,
        seed ^ 0xA5A5,
    ));
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(w: u32, h: u32) -> Vec<u8> {
        vec![0u8; (w * h * 4) as usize]
    }

    fn alpha_at(buf: &[u8], w: u32, x: u32, y: u32) -> u8 {
        buf[((y * w + x) * 4 + 3) as usize]
    }

    fn px(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn stamp_center_is_opaque() {
        let (w, h) = (64u32, 64u32);
        let mut buf = buffer(w, h);
        stamp(
            &mut buf,
            w,
            h,
            32.0,
            32.0,
            8.0,
            [255, 80, 40, 255],
            BrushKind::Pen,
            7,
        );
        assert_eq!(alpha_at(&buf, w, 32, 32), 255);
        assert_eq!(px(&buf, w, 32, 32), [255, 80, 40, 255]);
    }

    #[test]
    fn stamp_edge_fades() {
        let (w, h) = (64u32, 64u32);
        let mut buf = buffer(w, h);
        stamp(
            &mut buf,
            w,
            h,
            32.0,
            32.0,
            8.0,
            [255, 255, 255, 255],
            BrushKind::Soft,
            1,
        );
        let center = alpha_at(&buf, w, 32, 32);
        let near_edge = alpha_at(&buf, w, 39, 32);
        assert!(
            near_edge < center,
            "边缘 {} 应小于中心 {}",
            near_edge,
            center
        );
        assert!(near_edge > 0, "软笔边缘应仍有残余，不应硬切");
    }

    #[test]
    fn stamp_clips_when_out_of_bounds() {
        let (w, h) = (16u32, 16u32);
        let mut buf = buffer(w, h);
        let r = stamp(
            &mut buf,
            w,
            h,
            -6.0,
            -6.0,
            4.0,
            [255, 0, 0, 255],
            BrushKind::Pen,
            3,
        );
        assert!(r.is_empty(), "完全越界的打点不应产生脏矩形");
        assert!(buf.iter().all(|b| *b == 0));
    }

    #[test]
    fn stamp_partially_outside_keeps_visible_part() {
        let (w, h) = (16u32, 16u32);
        let mut buf = buffer(w, h);
        let r = stamp(
            &mut buf,
            w,
            h,
            0.0,
            8.0,
            6.0,
            [0, 255, 0, 255],
            BrushKind::Pen,
            3,
        );
        assert!(!r.is_empty());
        assert!(alpha_at(&buf, w, 0, 8) > 0, "贴边打点应保留可见部分");
    }

    #[test]
    fn blend_over_accumulates_alpha() {
        let (w, h) = (8u32, 8u32);
        let mut buf = buffer(w, h);
        stamp(
            &mut buf,
            w,
            h,
            4.0,
            4.0,
            3.0,
            [0, 0, 255, 128],
            BrushKind::Pen,
            0,
        );
        let first = alpha_at(&buf, w, 4, 4);
        assert!(
            first > 100 && first < 160,
            "首层 alpha 应约 128，实际 {}",
            first
        );
        stamp(
            &mut buf,
            w,
            h,
            4.0,
            4.0,
            3.0,
            [0, 0, 255, 128],
            BrushKind::Pen,
            0,
        );
        let second = alpha_at(&buf, w, 4, 4);
        assert!(second > first, "叠加后 alpha 应上升");
        assert!(second > 180, "两次 50% 叠加应接近 191，实际 {}", second);
    }

    #[test]
    fn stroke_fills_gap_between_points() {
        let (w, h) = (64u32, 8u32);
        let mut buf = buffer(w, h);
        let brush = Brush::preset(BrushKind::Pen);
        let r = stroke(
            &mut buf,
            w,
            h,
            &[(4.0, 4.0, 1.0), (60.0, 4.0, 1.0)],
            &brush,
            [255, 0, 0, 255],
            9,
        );
        assert!(!r.is_empty());
        assert!(alpha_at(&buf, w, 30, 4) > 0, "两点之间应被连续打点覆盖");
        assert!(
            alpha_at(&buf, w, 2, 4) > 0,
            "起点笔尖应落笔（半径 3 覆盖 x=1..7）"
        );
    }

    #[test]
    fn stroke_is_deterministic_for_same_seed() {
        let (w, h) = (48u32, 48u32);
        let brush = Brush::preset(BrushKind::Airbrush);
        let mut a = buffer(w, h);
        let mut b = buffer(w, h);
        let pts = [(10.0, 10.0, 1.0), (38.0, 38.0, 1.0)];
        stroke(&mut a, w, h, &pts, &brush, [200, 200, 255, 200], 42);
        stroke(&mut b, w, h, &pts, &brush, [200, 200, 255, 200], 42);
        assert_eq!(a, b, "同 seed 的喷枪必须逐像素一致");
    }

    #[test]
    fn eraser_clears_alpha() {
        let (w, h) = (16u32, 16u32);
        let mut buf = buffer(w, h);
        stamp(
            &mut buf,
            w,
            h,
            8.0,
            8.0,
            6.0,
            [255, 0, 0, 255],
            BrushKind::Pen,
            0,
        );
        assert_eq!(alpha_at(&buf, w, 8, 8), 255);
        stamp(
            &mut buf,
            w,
            h,
            8.0,
            8.0,
            6.0,
            [0, 0, 0, 255],
            BrushKind::Eraser,
            0,
        );
        assert_eq!(alpha_at(&buf, w, 8, 8), 0, "橡皮应把覆盖区 alpha 清零");
    }

    #[test]
    fn stamp_respects_buffer_length() {
        // 声明尺寸小于实际缓冲时必须在声明范围内工作，不允许读越界。
        let w = 4u32;
        let h = 3u32;
        let mut buf = buffer(w, h);
        stamp(
            &mut buf,
            w,
            h,
            2.0,
            1.0,
            2.0,
            [1, 2, 3, 255],
            BrushKind::Pen,
            0,
        );
        assert_eq!(buf.len(), (w * h * 4) as usize);
    }

    #[test]
    fn dirty_rect_union_covers_both() {
        let a = DirtyRect {
            x0: 1,
            y0: 2,
            x1: 5,
            y1: 6,
        };
        let b = DirtyRect {
            x0: 0,
            y0: 4,
            x1: 3,
            y1: 9,
        };
        let u = a.union(b);
        assert_eq!(
            u,
            DirtyRect {
                x0: 0,
                y0: 2,
                x1: 5,
                y1: 9
            }
        );
        assert_eq!(u.area(), 35);
    }
}
