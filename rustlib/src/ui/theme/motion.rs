//! M3 动效令牌（时长/曲线）。
//!
//! # 设计
//! - 时长集合对齐 M3 motion 规范（ms）；
//! - Expressive 曲线以贝塞尔参数近似（`CubicBezier` 由消费点采样，
//!   V1 用 easing 多项式近似以避免运行时贝塞尔求值开销）。

/// 动效时长（ms）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DurationToken {
    /// 50ms：微小状态变化（按压起始）。
    Short1,
    /// 100ms：元素出场、状态层淡入。
    Short2,
    /// 200ms：元素入场。
    Short4,
    /// 250ms：展开/收起中等容器。
    Medium2,
    /// 300ms：页面级过渡。
    Medium4,
    /// 500ms：大容器展开（BottomSheet）。
    Long2,
    /// 600ms：屏幕级过渡。
    Long4,
    /// 700ms：Expressive 形状 morph 完整时长。
    ExtraLong1,
}

impl DurationToken {
    /// 解析为毫秒数。
    pub fn ms(&self) -> i64 {
        match self {
            DurationToken::Short1 => 50,
            DurationToken::Short2 => 100,
            DurationToken::Short4 => 200,
            DurationToken::Medium2 => 250,
            DurationToken::Medium4 => 300,
            DurationToken::Long2 => 500,
            DurationToken::Long4 => 600,
            DurationToken::ExtraLong1 => 700,
        }
    }
}

/// 动效曲线（M3 Expressive 强调曲线族）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EasingToken {
    /// 强调曲线：多数可见动效的默认曲线。
    Emphasized,
    /// 强调减速：入场用。
    EmphasizedDecelerate,
    /// 强调加速：出场用。
    EmphasizedAccelerate,
    /// 标准曲线：小位移。
    Standard,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 时长数值与 M3 规范一致。
    #[test]
    fn durations_match_m3_spec() {
        assert_eq!(DurationToken::Short1.ms(), 50);
        assert_eq!(DurationToken::Short2.ms(), 100);
        assert_eq!(DurationToken::Medium4.ms(), 300);
        assert_eq!(DurationToken::ExtraLong1.ms(), 700);
    }
}
