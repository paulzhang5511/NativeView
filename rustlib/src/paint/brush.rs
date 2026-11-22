//! 笔刷预设与参数。
//!
//! # 不变量
//! - `size` 是笔尖直径（画布像素），恒在 `[MIN_SIZE, MAX_SIZE]`；
//! - `opacity` 是整笔的不透明度上限，`flow` 是单次打点的浓度；
//! - `smoothing` 交给 [`crate::paint::stroke::smooth`]，本模块只存值。

/// 笔刷类型。决定光栅化的硬度、边缘衰减与抖动。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushKind {
    /// 硬边圆笔，边缘 1px 抗锯齿。
    Pen,
    /// 马克笔：高不透明度、方形感边缘。
    Marker,
    /// 铅笔：细、略带颗粒。
    Pencil,
    /// 软笔：边缘柔和衰减。
    Soft,
    /// 喷枪：半径内随机散布。
    Airbrush,
    /// 水彩：极低硬度，多次叠加。
    Watercolor,
    /// 荧光：高饱和、低流量，叠加发亮。
    Neon,
    /// 橡皮：以底色擦除（alpha 置 0）。
    Eraser,
}

/// 光栅化参数：由笔刷类型固定，不暴露给用户。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// 边缘硬度 [0,1]：1 为硬边，0 为全半径渐变。
    pub hardness: f32,
    /// 单点浓度的缩放系数（水彩 < 1，马克笔 > 1）。
    pub flow_scale: f32,
    /// 抖动幅度 [0,1]：喷枪/水彩会偏移打点位置。
    pub jitter: f32,
    /// 颗粒强度 [0,1]：铅笔按像素噪声降低 alpha。
    pub grain: f32,
}

impl BrushKind {
    /// 该类型的固定光栅参数。
    pub fn tuning(self) -> Tuning {
        match self {
            BrushKind::Pen => Tuning {
                hardness: 0.9,
                flow_scale: 1.0,
                jitter: 0.0,
                grain: 0.0,
            },
            BrushKind::Marker => Tuning {
                hardness: 0.75,
                flow_scale: 1.15,
                jitter: 0.0,
                grain: 0.0,
            },
            BrushKind::Pencil => Tuning {
                hardness: 0.85,
                flow_scale: 0.85,
                jitter: 0.02,
                grain: 0.55,
            },
            BrushKind::Soft => Tuning {
                hardness: 0.2,
                flow_scale: 1.0,
                jitter: 0.0,
                grain: 0.0,
            },
            BrushKind::Airbrush => Tuning {
                hardness: 0.05,
                flow_scale: 0.55,
                jitter: 0.35,
                grain: 0.2,
            },
            BrushKind::Watercolor => Tuning {
                hardness: 0.1,
                flow_scale: 0.4,
                jitter: 0.12,
                grain: 0.15,
            },
            BrushKind::Neon => Tuning {
                hardness: 0.35,
                flow_scale: 0.8,
                jitter: 0.0,
                grain: 0.0,
            },
            BrushKind::Eraser => Tuning {
                hardness: 0.8,
                flow_scale: 1.0,
                jitter: 0.0,
                grain: 0.0,
            },
        }
    }

    /// 中文名（UI 面板展示用；不进 `strings.xml`，属于设计稿固定文案）。
    pub fn label(self) -> &'static str {
        match self {
            BrushKind::Pen => "钢笔",
            BrushKind::Marker => "马克笔",
            BrushKind::Pencil => "铅笔",
            BrushKind::Soft => "软笔",
            BrushKind::Airbrush => "喷枪",
            BrushKind::Watercolor => "水彩",
            BrushKind::Neon => "荧光",
            BrushKind::Eraser => "橡皮",
        }
    }
}

/// 笔刷粗细下限（像素直径）。
pub const MIN_SIZE: f32 = 1.0;
/// 笔刷粗细上限（像素直径）。
pub const MAX_SIZE: f32 = 200.0;

/// 当前笔刷参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// 笔刷类型。
    pub kind: BrushKind,
    /// 直径（像素）。
    pub size: f32,
    /// 整笔不透明度 [0,1]。
    pub opacity: f32,
    /// 单点浓度 [0,1]。
    pub flow: f32,
    /// 轨迹平滑系数 [0,1]。
    pub smoothing: f32,
}

impl Brush {
    /// 按类型构造默认参数；所有数值已夹到合法区间。
    pub fn preset(kind: BrushKind) -> Self {
        let (size, opacity, flow, smoothing) = match kind {
            BrushKind::Pen => (6.0, 1.0, 1.0, 0.35),
            BrushKind::Marker => (24.0, 0.85, 1.0, 0.45),
            BrushKind::Pencil => (4.0, 0.9, 0.8, 0.25),
            BrushKind::Soft => (40.0, 0.75, 0.9, 0.55),
            BrushKind::Airbrush => (64.0, 0.45, 0.35, 0.6),
            BrushKind::Watercolor => (48.0, 0.5, 0.3, 0.5),
            BrushKind::Neon => (16.0, 1.0, 0.7, 0.4),
            BrushKind::Eraser => (32.0, 1.0, 1.0, 0.4),
        };
        Self {
            kind,
            size,
            opacity,
            flow,
            smoothing,
        }
    }

    /// 笔尖半径（像素）；恒定为正，供光栅化使用。
    pub fn radius(&self) -> f32 {
        (self.size * 0.5).max(0.5)
    }

    /// 把参数夹回合法区间；链式修改后调用，保证不变量。
    pub fn clamped(self) -> Self {
        Self {
            kind: self.kind,
            size: self.size.clamp(MIN_SIZE, MAX_SIZE),
            opacity: self.opacity.clamp(0.0, 1.0),
            flow: self.flow.clamp(0.0, 1.0),
            smoothing: self.smoothing.clamp(0.0, 1.0),
        }
    }
}

/// 笔刷面板中的一个预设项。
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    /// 稳定 id（状态持久化与单测锚点）。
    pub id: &'static str,
    /// 展示名。
    pub label: &'static str,
    /// 点击后应用的参数。
    pub brush: Brush,
}

/// 面板上的 8 个固定预设（用户不可增删）。
pub const PRESETS: [Preset; 8] = [
    Preset {
        id: "pen",
        label: "钢笔",
        brush: Brush {
            kind: BrushKind::Pen,
            size: 6.0,
            opacity: 1.0,
            flow: 1.0,
            smoothing: 0.35,
        },
    },
    Preset {
        id: "marker",
        label: "马克笔",
        brush: Brush {
            kind: BrushKind::Marker,
            size: 24.0,
            opacity: 0.85,
            flow: 1.0,
            smoothing: 0.45,
        },
    },
    Preset {
        id: "pencil",
        label: "铅笔",
        brush: Brush {
            kind: BrushKind::Pencil,
            size: 4.0,
            opacity: 0.9,
            flow: 0.8,
            smoothing: 0.25,
        },
    },
    Preset {
        id: "soft",
        label: "软笔",
        brush: Brush {
            kind: BrushKind::Soft,
            size: 40.0,
            opacity: 0.75,
            flow: 0.9,
            smoothing: 0.55,
        },
    },
    Preset {
        id: "airbrush",
        label: "喷枪",
        brush: Brush {
            kind: BrushKind::Airbrush,
            size: 64.0,
            opacity: 0.45,
            flow: 0.35,
            smoothing: 0.6,
        },
    },
    Preset {
        id: "watercolor",
        label: "水彩",
        brush: Brush {
            kind: BrushKind::Watercolor,
            size: 48.0,
            opacity: 0.5,
            flow: 0.3,
            smoothing: 0.5,
        },
    },
    Preset {
        id: "neon",
        label: "荧光",
        brush: Brush {
            kind: BrushKind::Neon,
            size: 16.0,
            opacity: 1.0,
            flow: 0.7,
            smoothing: 0.4,
        },
    },
    Preset {
        id: "eraser",
        label: "橡皮",
        brush: Brush {
            kind: BrushKind::Eraser,
            size: 32.0,
            opacity: 1.0,
            flow: 1.0,
            smoothing: 0.4,
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_radius_is_half_size() {
        let b = Brush::preset(BrushKind::Pen);
        assert!((b.radius() - 3.0).abs() < 1e-4);
    }

    #[test]
    fn clamped_enforces_size_and_unit_ranges() {
        let b = Brush {
            kind: BrushKind::Soft,
            size: 999.0,
            opacity: 1.5,
            flow: -0.2,
            smoothing: 2.0,
        };
        let c = b.clamped();
        assert_eq!(c.size, MAX_SIZE);
        assert_eq!(c.opacity, 1.0);
        assert_eq!(c.flow, 0.0);
        assert_eq!(c.smoothing, 1.0);
        assert_eq!(c.kind, BrushKind::Soft);
    }

    #[test]
    fn presets_are_unique_and_cover_all_kinds() {
        let mut ids = Vec::new();
        for p in PRESETS.iter() {
            assert!(!ids.contains(&p.id), "重复 id {}", p.id);
            ids.push(p.id);
        }
        for kind in [
            BrushKind::Pen,
            BrushKind::Marker,
            BrushKind::Pencil,
            BrushKind::Soft,
            BrushKind::Airbrush,
            BrushKind::Watercolor,
            BrushKind::Neon,
            BrushKind::Eraser,
        ] {
            assert!(
                PRESETS.iter().any(|p| p.brush.kind == kind),
                "缺少 {:?}",
                kind
            );
        }
    }

    #[test]
    fn every_preset_satisfies_invariants() {
        for p in PRESETS.iter() {
            let c = p.brush.clamped();
            assert_eq!(c, p.brush, "预设 {} 未夹紧", p.id);
            assert!(c.size >= MIN_SIZE && c.size <= MAX_SIZE);
        }
    }

    #[test]
    fn tuning_differs_by_kind() {
        let pen = BrushKind::Pen.tuning();
        let soft = BrushKind::Soft.tuning();
        let air = BrushKind::Airbrush.tuning();
        assert!(pen.hardness > soft.hardness);
        assert!(air.jitter > pen.jitter);
        assert!(BrushKind::Watercolor.tuning().flow_scale < 1.0);
    }
}
