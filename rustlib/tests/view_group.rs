//! ViewGroup 单测：getChildMeasureSpec 9 种组合表驱动（Task 15，A5）。
//!
//! # 基准
//! Android `ViewGroup.getChildMeasureSpec(spec, padding, dimension)`：
//! - EXACTLY：子 LP 为负 → EXACTLY(size-padding)；否则 EXACTLY(具体值)
//! - AT_MOST：子 LP 为负 → AT_MOST(size-padding)；否则 EXACTLY(具体值)
//! - UNSPECIFIED：子 LP 为负 → UNSPECIFIED(0)；否则 **EXACTLY(具体值)**
//!   （AOSP 源码：`resultSize = childDimension; resultMode = MeasureSpec.EXACTLY`）
//! （负 = MATCH_PARENT(-1) / WRAP_CONTENT(-2)）。

use velloview::ui::measure_spec::{MeasureSpec, Mode};
use velloview::ui::view_group::{MarginLayoutParams, get_child_measure_spec};

/// 子 LP 尺寸。
const MATCH_PARENT: i32 = -1;
const WRAP_CONTENT: i32 = -2;

/// (父模式, 子 dimension, padding, 期望模式, 期望尺寸)
///
/// Android `getChildMeasureSpec` 真实表：
/// - EXACTLY 父 + 哨兵 → EXACTLY(size-padding)；+ 具体尺寸 → EXACTLY(具体值)
/// - AT_MOST 父 + 哨兵 → AT_MOST(size-padding)；+ 具体尺寸 → EXACTLY(具体值)
/// - UNSPECIFIED 父 + 哨兵 → UNSPECIFIED(0)；+ 具体尺寸 → EXACTLY(具体值)
fn table() -> [(Mode, i32, u32, Mode, u32); 9] {
    [
        // 父 EXACTLY(100)
        (Mode::Exactly, MATCH_PARENT, 10, Mode::Exactly, 90),
        (Mode::Exactly, WRAP_CONTENT, 10, Mode::Exactly, 90),
        (Mode::Exactly, 50, 10, Mode::Exactly, 50),
        // 父 AT_MOST(100)
        (Mode::AtMost, MATCH_PARENT, 10, Mode::AtMost, 90),
        (Mode::AtMost, WRAP_CONTENT, 10, Mode::AtMost, 90),
        (Mode::AtMost, 50, 10, Mode::Exactly, 50),
        // 父 UNSPECIFIED(0)
        (Mode::Unspecified, MATCH_PARENT, 10, Mode::Unspecified, 0),
        (Mode::Unspecified, WRAP_CONTENT, 10, Mode::Unspecified, 0),
        // 具体尺寸：EXACTLY（若降级为 UNSPECIFIED，控件会忽略固定高度）
        (Mode::Unspecified, 50, 10, Mode::Exactly, 50),
    ]
}

/// Android `getChildMeasureSpec` 9 组合逐项对照。
#[test]
fn get_child_measure_spec_matches_android_table() {
    for (parent_mode, child_dim, padding, want_mode, want_size) in table() {
        let parent = MeasureSpec::make(parent_mode, 100);
        let spec = get_child_measure_spec(parent, padding, child_dim);
        assert_eq!(
            spec.mode(),
            want_mode,
            "mode for parent {parent_mode:?} child {child_dim} padding {padding}"
        );
        assert_eq!(
            spec.size(),
            want_size,
            "size for parent {parent_mode:?} child {child_dim} padding {padding}"
        );
    }
}

/// measureChildWithMargins：可用尺寸 = 父尺寸 - padding - margin。
#[test]
fn measure_child_with_margins_subtracts_both() {
    // 父 EXACTLY(100)，容器 padding 10，子 margin 5 → 可用 100-10-5=85，
    // 子 LP 30 → EXACTLY(30)；LP WRAP → EXACTLY(85)（Android：EXACTLY 父 + 哨兵 = EXACTLY 剩余）。
    let parent = MeasureSpec::make(Mode::Exactly, 100);
    let mut lp = MarginLayoutParams::default();
    lp.width = 30;
    lp.left_margin = 5.0;
    lp.right_margin = 0.0;
    let spec = velloview::ui::view_group::measure_child_with_margins_spec(
        parent,
        10,
        lp.horizontal_margins() as u32,
        &lp,
    );
    assert_eq!(spec.mode(), Mode::Exactly);
    assert_eq!(spec.size(), 30);

    lp.width = WRAP_CONTENT;
    let spec = velloview::ui::view_group::measure_child_with_margins_spec(
        parent,
        10,
        lp.horizontal_margins() as u32,
        &lp,
    );
    assert_eq!(spec.mode(), Mode::Exactly);
    assert_eq!(spec.size(), 85);
}
