//! 笔迹采样：把稀疏的触摸点展开成密集轨迹，并把压感映射为笔尖半径。

/// 单个采样点（坐标系与触摸事件一致，调用方负责换算到画布像素）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrokePoint {
    /// 画布像素坐标 x。
    pub x: f32,
    /// 画布像素坐标 y。
    pub y: f32,
    /// 压力 [0,1]；设备无压感时恒为 1.0。
    pub pressure: f32,
    /// 事件时间（毫秒，相对手势起点）。
    pub time_ms: i64,
}

impl StrokePoint {
    /// 构造采样点；压力先夹到 [0,1]，负值视作 0。
    pub fn new(x: f32, y: f32, pressure: f32, time_ms: i64) -> Self {
        Self {
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            time_ms,
        }
    }
}

/// 相邻采样点距离超过该阈值就插值，保证快速划动时轨迹连续。
pub const MAX_GAP: f32 = 2.0;

/// 展开稀疏采样：相邻点距 > `max_gap` 时线性插入中间点。
///
/// # 不变量
/// - 首尾点原样保留（含压感），中间插值点的 pressure 与 time 线性插值；
/// - 输入为空返回空，单点原样返回；
/// - 不产生 NaN：重叠点（距离 0）直接跳过，不做除法。
pub fn densify(points: &[StrokePoint], max_gap: f32) -> Vec<StrokePoint> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let gap = if max_gap.is_finite() && max_gap > 0.1 {
        max_gap
    } else {
        0.1
    };
    let mut out = Vec::with_capacity(points.len() * 4);
    out.push(points[0]);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let steps = (dist / gap).ceil();
        // dist 为 0 或 NaN 时 steps 非有限，直接跳到终点，避免除零。
        if steps.is_finite() && steps >= 2.0 {
            let n = steps as usize;
            for i in 1..n {
                let t = i as f32 / n as f32;
                out.push(StrokePoint {
                    x: a.x + dx * t,
                    y: a.y + dy * t,
                    pressure: a.pressure + (b.pressure - a.pressure) * t,
                    time_ms: a.time_ms + ((b.time_ms - a.time_ms) as f32 * t) as i64,
                });
            }
        }
        out.push(b);
    }
    out
}

/// 指数滑动平均平滑轨迹。
///
/// `factor` 为 1.0 时输出退化为「首点重复」（最平滑），为 0 时保持原轨迹；
/// 首点始终原样保留，因此落笔起点不会漂移。
pub fn smooth(points: &[StrokePoint], factor: f32) -> Vec<StrokePoint> {
    let f = factor.clamp(0.0, 1.0);
    let mut out = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            out.push(*p);
            continue;
        }
        let prev = out[i - 1];
        // 权重语义：f=0 完全跟随新点（保留原轨迹），f=1 完全停留在上一点（最强平滑）。
        let follow = 1.0 - f;
        out.push(StrokePoint {
            x: prev.x + (p.x - prev.x) * follow,
            y: prev.y + (p.y - prev.y) * follow,
            pressure: prev.pressure + (p.pressure - prev.pressure) * follow,
            time_ms: p.time_ms,
        });
    }
    out
}

/// 压感 → 笔尖半径。
///
/// 映射为 `base_radius * (min_ratio + (1 - min_ratio) * pressure)`：
/// 压力 0 时仍保留 `min_ratio` 的半径，避免断笔；压力 1 时等于基础半径。
pub fn radius_for(pressure: f32, base_radius: f32, min_ratio: f32) -> f32 {
    let ratio = min_ratio.clamp(0.05, 0.95);
    let p = if pressure.is_finite() {
        pressure.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let base = if base_radius.is_finite() && base_radius > 0.0 {
        base_radius
    } else {
        1.0
    };
    base * (ratio + (1.0 - ratio) * p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32, p: f32) -> StrokePoint {
        StrokePoint::new(x, y, p, 0)
    }

    #[test]
    fn densify_inserts_points_when_gap_exceeds_threshold() {
        let points = [pt(0.0, 0.0, 0.5), pt(100.0, 0.0, 1.0)];
        let out = densify(&points, 2.0);
        assert!(out.len() >= 51, "100px 间距按 2px 插值应至少 51 点");
        assert_eq!(out.first().unwrap().x, 0.0);
        assert_eq!(out.last().unwrap().x, 100.0);
    }

    #[test]
    fn densify_keeps_short_segments_untouched() {
        let points = [pt(0.0, 0.0, 0.5), pt(1.0, 0.0, 0.5), pt(2.0, 0.0, 0.5)];
        let out = densify(&points, 2.0);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn densify_interpolates_pressure_linearly() {
        let points = [pt(0.0, 0.0, 0.2), pt(10.0, 0.0, 0.8)];
        let out = densify(&points, 2.0);
        for w in out.windows(2) {
            assert!(w[1].pressure >= w[0].pressure - 1e-4);
        }
        // 中间点压力应落在两端之间。
        let mid = out[out.len() / 2];
        assert!(mid.pressure > 0.2 && mid.pressure < 0.8, "{:?}", mid);
    }

    #[test]
    fn densify_single_or_empty_is_identity() {
        assert!(densify(&[], 2.0).is_empty());
        let one = [pt(3.0, 4.0, 1.0)];
        assert_eq!(densify(&one, 2.0), one.to_vec());
    }

    #[test]
    fn smooth_zero_keeps_original_track() {
        let points = [pt(0.0, 0.0, 1.0), pt(10.0, 10.0, 1.0), pt(20.0, 0.0, 1.0)];
        assert_eq!(smooth(&points, 0.0), points.to_vec());
    }

    #[test]
    fn smooth_one_collapses_to_start_point() {
        let points = [pt(0.0, 0.0, 1.0), pt(10.0, 10.0, 1.0), pt(20.0, 0.0, 1.0)];
        let out = smooth(&points, 1.0);
        assert_eq!(out[0], points[0]);
        for p in &out[1..] {
            assert!((p.x - 0.0).abs() < 1e-4 && (p.y - 0.0).abs() < 1e-4);
        }
    }

    #[test]
    fn smooth_half_keeps_start_and_damps_jitter() {
        let points = [pt(0.0, 0.0, 1.0), pt(10.0, 0.0, 1.0), pt(0.0, 0.0, 1.0)];
        let out = smooth(&points, 0.5);
        assert_eq!(out[0].x, 0.0);
        assert!(out[1].x > 0.0 && out[1].x < 10.0);
        assert!(out[2].x > 0.0, "回抖被平滑后不应回到 0");
    }

    #[test]
    fn radius_grows_with_pressure_and_has_floor() {
        let base = 10.0;
        let r0 = radius_for(0.0, base, 0.3);
        let r1 = radius_for(0.1, base, 0.3);
        let r5 = radius_for(0.5, base, 0.3);
        let rmax = radius_for(1.0, base, 0.3);
        assert!((r0 - 3.0).abs() < 1e-3);
        assert!(r1 >= r0 && r5 > r1 && rmax > r5);
        assert!((rmax - base).abs() < 1e-3);
    }

    #[test]
    fn radius_clamps_out_of_range_pressure() {
        assert_eq!(radius_for(-1.0, 10.0, 0.3), radius_for(0.0, 10.0, 0.3));
        assert_eq!(radius_for(2.0, 10.0, 0.3), radius_for(1.0, 10.0, 0.3));
        assert_eq!(radius_for(f32::NAN, 10.0, 0.3), radius_for(1.0, 10.0, 0.3));
    }
}
