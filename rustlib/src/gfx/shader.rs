//! 着色器：线性/径向/扫描渐变（PRD §5.1.3 Android `Shader` L1 子集）。
//!
//! # 设计
//! - 渐变参数在录制期由 `Canvas` 烘焙矩阵（与几何一致），`Shader` 内
//!   坐标已是设备空间；
//! - 颜色停靠点用 `Arc<[ColorStop]>` 共享：`Paint` 为 Clone 语义，
//!   多条命令复用同一渐变时不复制数组；
//! - Android `TileMode` 对齐：CLAMP/REPEAT/MIRROR。

use std::sync::Arc;

use super::color::Color;
use super::matrix::Matrix;

/// 颜色停靠点：位置 ∈ [0,1]（0=起点，1=终点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub position: f32,
    pub color: Color,
}

/// 平铺模式（Android `Shader.TileMode`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TileMode {
    /// 边界色延伸（Android 默认）。
    #[default]
    Clamp,
    /// 重复渐变周期。
    Repeat,
    /// 镜像重复。
    Mirror,
}

/// 渐变着色器（V1 仅渐变：纯色由 `Paint.color` 承担）。
#[derive(Clone, Debug, PartialEq)]
pub enum Shader {
    /// 线性渐变：沿 (x0,y0)→(x1,y1) 插值。
    LinearGradient {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        stops: Arc<[ColorStop]>,
        tile: TileMode,
    },
    /// 径向渐变：以 (cx,cy) 为心、r 为半径，内→外插值。
    RadialGradient {
        cx: f64,
        cy: f64,
        r: f64,
        stops: Arc<[ColorStop]>,
        tile: TileMode,
    },
    /// 扫描渐变：绕 (cx,cy) 从起始角顺时针插值（Android 0 rad = 3 点钟方向）。
    SweepGradient {
        cx: f64,
        cy: f64,
        stops: Arc<[ColorStop]>,
    },
}

impl Shader {
    /// 线性渐变构造（Android `LinearGradient` 参数形态）。
    ///
    /// # 约束
    /// `positions` 为 `None` 时均匀分布（Android 语义）；
    /// 少于 2 个停靠点退化为纯色，仍合法（后端按首色处理）。
    pub fn linear(
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        colors: &[Color],
        positions: Option<&[f32]>,
        tile: TileMode,
    ) -> Self {
        Self::LinearGradient {
            x0,
            y0,
            x1,
            y1,
            stops: Self::normalize_stops(colors, positions),
            tile,
        }
    }

    /// 径向渐变构造（Android `RadialGradient` 参数形态）。
    pub fn radial(
        cx: f64,
        cy: f64,
        r: f64,
        colors: &[Color],
        positions: Option<&[f32]>,
        tile: TileMode,
    ) -> Self {
        Self::RadialGradient {
            cx,
            cy,
            r,
            stops: Self::normalize_stops(colors, positions),
            tile,
        }
    }

    /// 扫描渐变构造（Android `SweepGradient` 参数形态）。
    pub fn sweep(cx: f64, cy: f64, colors: &[Color], positions: Option<&[f32]>) -> Self {
        Self::SweepGradient {
            cx,
            cy,
            stops: Self::normalize_stops(colors, positions),
        }
    }

    /// 停靠点归一化：None 时均匀分布；越界/长度不齐时按 Android
    /// 容忍语义处理（positions 长度 != colors 长度时退化为均匀分布）。
    fn normalize_stops(colors: &[Color], positions: Option<&[f32]>) -> Arc<[ColorStop]> {
        let n = colors.len();
        if n == 0 {
            return Arc::from([]);
        }
        if n == 1 {
            // 单色：两个同色停靠点覆盖全程。
            return Arc::from([
                ColorStop {
                    position: 0.0,
                    color: colors[0],
                },
                ColorStop {
                    position: 1.0,
                    color: colors[0],
                },
            ]);
        }
        if let Some(pos) = positions {
            if pos.len() == n {
                return Arc::from(
                    colors
                        .iter()
                        .zip(pos.iter())
                        .map(|(c, &p)| ColorStop {
                            position: p,
                            color: *c,
                        })
                        .collect::<Vec<_>>(),
                );
            }
        }
        // 均匀分布：i/(n-1)。
        let step = 1.0 / (n as f32 - 1.0);
        Arc::from(
            colors
                .iter()
                .enumerate()
                .map(|(i, &c)| ColorStop {
                    position: i as f32 * step,
                    color: c,
                })
                .collect::<Vec<_>>(),
        )
    }

    /// 变换烘焙：几何参数随矩阵映射（停点表不变）。
    ///
    /// # 边界
    /// 径向半径仅按均匀缩放因子近似（`sx*sy` 开方），
    /// 非均匀缩放的椭圆化是 L2 差异（后端以圆消费）。
    pub fn bake(&self, m: &Matrix) -> Self {
        match self {
            Shader::LinearGradient {
                x0,
                y0,
                x1,
                y1,
                stops,
                tile,
            } => {
                let (x0, y0) = m.map_point(*x0, *y0);
                let (x1, y1) = m.map_point(*x1, *y1);
                Shader::LinearGradient {
                    x0,
                    y0,
                    x1,
                    y1,
                    stops: stops.clone(),
                    tile: *tile,
                }
            }
            Shader::RadialGradient {
                cx,
                cy,
                r,
                stops,
                tile,
            } => {
                let (cx, cy) = m.map_point(*cx, *cy);
                // 均匀缩放因子：矩阵行列式开方（|det|^0.5 = √(sx·sy)）。
                let det = m.a * m.d - m.b * m.c;
                let scale = det.abs().sqrt();
                Shader::RadialGradient {
                    cx,
                    cy,
                    r: r * scale,
                    stops: stops.clone(),
                    tile: *tile,
                }
            }
            Shader::SweepGradient { cx, cy, stops } => {
                let (cx, cy) = m.map_point(*cx, *cy);
                Shader::SweepGradient {
                    cx,
                    cy,
                    stops: stops.clone(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 均匀分布：None positions → i/(n-1)。
    #[test]
    fn stops_evenly_distributed() {
        let s = Shader::linear(
            0.0,
            0.0,
            100.0,
            0.0,
            &[Color::RED, Color::GREEN, Color::BLUE],
            None,
            TileMode::Clamp,
        );
        match s {
            Shader::LinearGradient { stops, .. } => {
                assert_eq!(stops.len(), 3);
                assert!((stops[0].position - 0.0).abs() < 1e-6);
                assert!((stops[1].position - 0.5).abs() < 1e-6);
                assert!((stops[2].position - 1.0).abs() < 1e-6);
            }
            _ => panic!("expected LinearGradient"),
        }
    }

    /// 显式 positions 保留原值。
    #[test]
    fn stops_explicit_positions() {
        let s = Shader::radial(
            50.0,
            50.0,
            30.0,
            &[Color::RED, Color::BLUE],
            Some(&[0.2, 0.8]),
            TileMode::Repeat,
        );
        match s {
            Shader::RadialGradient { stops, tile, r, .. } => {
                assert_eq!(stops[0].position, 0.2);
                assert_eq!(stops[1].position, 0.8);
                assert_eq!(tile, TileMode::Repeat);
                assert_eq!(r, 30.0);
            }
            _ => panic!("expected RadialGradient"),
        }
    }

    /// positions 长度不匹配 → 均匀分布回退（Android 容忍语义）。
    #[test]
    fn stops_mismatch_falls_back_even() {
        let s = Shader::linear(
            0.0,
            0.0,
            10.0,
            0.0,
            &[Color::RED, Color::GREEN],
            Some(&[0.1]), // 长度 1 != 2
            TileMode::Clamp,
        );
        match s {
            Shader::LinearGradient { stops, .. } => {
                assert!((stops[0].position - 0.0).abs() < 1e-6);
                assert!((stops[1].position - 1.0).abs() < 1e-6);
            }
            _ => panic!("expected LinearGradient"),
        }
    }

    /// 单色渐变：两个同色停靠点。
    #[test]
    fn single_color_doubles() {
        let s = Shader::sweep(0.0, 0.0, &[Color::WHITE], None);
        match s {
            Shader::SweepGradient { stops, .. } => {
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].color, Color::WHITE);
                assert_eq!(stops[1].color, Color::WHITE);
            }
            _ => panic!("expected SweepGradient"),
        }
    }

    /// 渐变烘焙：平移改变中心，半径按均匀缩放。
    #[test]
    fn gradient_baked_by_matrix() {
        let s = Shader::radial(
            10.0,
            10.0,
            5.0,
            &[Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        );
        let m = Matrix::translation(100.0, 50.0);
        match s.bake(&m) {
            Shader::RadialGradient { cx, cy, r, .. } => {
                assert_eq!(cx, 110.0);
                assert_eq!(cy, 60.0);
                assert_eq!(r, 5.0); // 平移不改半径
            }
            _ => panic!("expected RadialGradient"),
        }
        // 均匀缩放 ×2：半径 ×2。
        let m2 = Matrix::scaling(2.0, 2.0);
        match s.bake(&m2) {
            Shader::RadialGradient { r, .. } => assert!((r - 10.0).abs() < 1e-9),
            _ => panic!("expected RadialGradient"),
        }
    }

    /// 线性渐变端点烘焙。
    #[test]
    fn linear_endpoints_baked() {
        let s = Shader::linear(
            0.0,
            0.0,
            10.0,
            10.0,
            &[Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        );
        let m = Matrix::translation(1.0, 2.0);
        match s.bake(&m) {
            Shader::LinearGradient { x0, y0, x1, y1, .. } => {
                assert_eq!((x0, y0, x1, y1), (1.0, 2.0, 11.0, 12.0));
            }
            _ => panic!("expected LinearGradient"),
        }
    }
}
