//! M3 形状比例（Shape Scale）。
//!
//! # 设计
//! - 7 级圆角（dp）：Expressive 强调大圆角——按钮 full 药丸、卡片 28dp；
//! - `full` 用负数哨兵值表示（消费点按控件尺寸取 min(w,h)/2），
//!   避免组件先量自身再反推圆角的循环依赖。

/// 形状令牌（dp 值；`full` 为哨兵）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeToken {
    None,
    ExtraSmall,
    Small,
    Medium,
    Large,
    ExtraLarge,
    /// 全圆（药丸/圆）：dp 值 = min(宽, 高) / 2。
    Full,
}

impl ShapeToken {
    /// 解析为具体 dp 圆角。
    ///
    /// `Full` 需要控件尺寸才能定值，由消费点传入 `min(w, h)`；
    /// 其它档位与尺寸无关，`size_hint` 被忽略。
    pub fn radius_dp(&self, size_hint: f32) -> f32 {
        match self {
            ShapeToken::None => 0.0,
            ShapeToken::ExtraSmall => 4.0,
            ShapeToken::Small => 8.0,
            ShapeToken::Medium => 12.0,
            ShapeToken::Large => 16.0,
            ShapeToken::ExtraLarge => 28.0,
            ShapeToken::Full => (size_hint / 2.0).max(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 档位数值与 M3 规范一致。
    #[test]
    fn radii_match_m3_spec() {
        assert_eq!(ShapeToken::None.radius_dp(0.0), 0.0);
        assert_eq!(ShapeToken::ExtraSmall.radius_dp(0.0), 4.0);
        assert_eq!(ShapeToken::Small.radius_dp(0.0), 8.0);
        assert_eq!(ShapeToken::Medium.radius_dp(0.0), 12.0);
        assert_eq!(ShapeToken::Large.radius_dp(0.0), 16.0);
        assert_eq!(ShapeToken::ExtraLarge.radius_dp(0.0), 28.0);
    }

    /// Full 档按尺寸取半（药丸形语义）。
    #[test]
    fn full_resolves_half_of_size() {
        assert_eq!(ShapeToken::Full.radius_dp(48.0), 24.0);
        assert_eq!(ShapeToken::Full.radius_dp(0.0), 0.0);
    }
}
