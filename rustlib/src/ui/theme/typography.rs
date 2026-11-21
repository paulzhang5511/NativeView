//! M3 字体比例（Typography Scale）。
//!
//! # 设计
//! - 15 档（display/headline/title/body/label × large/medium/small）；
//! - 每档只携带字号与行高（px 换算后的语义值，sp 缩放在消费点发生）；
//! - 字重 V1 仅记录字段（单字体文件），不参与渲染差异。

/// 单档字阶令牌：字号（sp）与行高（sp，按 M3 规范取整）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeToken {
    /// 字号（sp）。
    pub font_size: f32,
    /// 行高（sp）。
    pub line_height: f32,
}

/// 15 档字阶集合（数值对齐 M3 规范）。
///
/// 消费点：`TextView::set_text_style` 按档位设置字号与行距
/// （行距 = 行高 − 字号）。
#[derive(Debug, Clone, Copy)]
pub struct Typography {
    pub display_large: TypeToken,
    pub display_medium: TypeToken,
    pub display_small: TypeToken,
    pub headline_large: TypeToken,
    pub headline_medium: TypeToken,
    pub headline_small: TypeToken,
    pub title_large: TypeToken,
    pub title_medium: TypeToken,
    pub title_small: TypeToken,
    pub body_large: TypeToken,
    pub body_medium: TypeToken,
    pub body_small: TypeToken,
    pub label_large: TypeToken,
    pub label_medium: TypeToken,
    pub label_small: TypeToken,
}

/// M3 基线字阶。
pub const SCALE: Typography = Typography {
    display_large: TypeToken {
        font_size: 57.0,
        line_height: 64.0,
    },
    display_medium: TypeToken {
        font_size: 45.0,
        line_height: 52.0,
    },
    display_small: TypeToken {
        font_size: 36.0,
        line_height: 44.0,
    },
    headline_large: TypeToken {
        font_size: 32.0,
        line_height: 40.0,
    },
    headline_medium: TypeToken {
        font_size: 28.0,
        line_height: 36.0,
    },
    headline_small: TypeToken {
        font_size: 24.0,
        line_height: 32.0,
    },
    title_large: TypeToken {
        font_size: 22.0,
        line_height: 28.0,
    },
    title_medium: TypeToken {
        font_size: 16.0,
        line_height: 24.0,
    },
    title_small: TypeToken {
        font_size: 14.0,
        line_height: 20.0,
    },
    body_large: TypeToken {
        font_size: 16.0,
        line_height: 24.0,
    },
    body_medium: TypeToken {
        font_size: 14.0,
        line_height: 20.0,
    },
    body_small: TypeToken {
        font_size: 12.0,
        line_height: 16.0,
    },
    label_large: TypeToken {
        font_size: 14.0,
        line_height: 20.0,
    },
    label_medium: TypeToken {
        font_size: 12.0,
        line_height: 16.0,
    },
    label_small: TypeToken {
        font_size: 11.0,
        line_height: 16.0,
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    /// 字阶数值与 M3 规范一致。
    #[test]
    fn scale_matches_m3_spec() {
        assert_eq!(SCALE.display_large.font_size, 57.0);
        assert_eq!(SCALE.headline_medium.line_height, 36.0);
        assert_eq!(SCALE.body_medium.font_size, 14.0);
        assert_eq!(SCALE.label_small.font_size, 11.0);
    }

    /// 行高必须不小于字号（否则多行文本会重叠）。
    #[test]
    fn line_height_ge_font_size() {
        let t = SCALE;
        let all = [
            t.display_large,
            t.display_medium,
            t.display_small,
            t.headline_large,
            t.headline_medium,
            t.headline_small,
            t.title_large,
            t.title_medium,
            t.title_small,
            t.body_large,
            t.body_medium,
            t.body_small,
            t.label_large,
            t.label_medium,
            t.label_small,
        ];
        for tk in all {
            assert!(tk.line_height >= tk.font_size, "{tk:?}");
        }
    }
}
