//! MeasureSpec 单测：Android 位打包一致性（Task 14，A5）。
//!
//! # 基准
//! Android `MeasureSpec`：32 bit int，高 2 bit 模式 + 低 30 bit 尺寸。
//! 模式常量：UNSPECENDED=0<<30，EXACTLY=1<<30，AT_MOST=2<<30。
//! 常量：MATCH_PARENT=-1，WRAP_CONTENT=-2（尺寸字段可为负哨兵值，
//! 打包后 size 字段读出仍为该负数——与 Android 一致）。

use velloview::ui::measure_spec::{MeasureSpec, Mode};

/// make→get 往返：模式与尺寸无损。
#[test]
fn roundtrip_mode_and_size() {
    let specs = [
        (Mode::Unspecified, 0u32),
        (Mode::Unspecified, 100),
        (Mode::Exactly, 0),
        (Mode::Exactly, 1080),
        (Mode::AtMost, 777),
    ];
    for (mode, size) in specs {
        let spec = MeasureSpec::make(mode, size);
        assert_eq!(spec.mode(), mode, "mode must roundtrip for size {size}");
        assert_eq!(spec.size(), size, "size must roundtrip for mode {mode:?}");
    }
}

/// 位布局必须与 Android 完全一致：raw = (mode << 30) | (size & 0x3FFFFFFF)。
#[test]
fn bit_packing_matches_android() {
    // Android: MeasureSpec.makeMeasureSpec(1080, EXACTLY) == 0x40000000 | 1080
    let spec = MeasureSpec::make(Mode::Exactly, 1080);
    assert_eq!(spec.raw(), 0x4000_0000u32 | 1080);

    let spec = MeasureSpec::make(Mode::AtMost, 777);
    assert_eq!(spec.raw(), 0x8000_0000u32 | 777);

    let spec = MeasureSpec::make(Mode::Unspecified, 5);
    assert_eq!(spec.raw(), 5u32);

    // 从 raw 解析。
    let parsed = MeasureSpec::from_raw(0x4000_0000 | 500);
    assert_eq!(parsed.mode(), Mode::Exactly);
    assert_eq!(parsed.size(), 500);
}

/// MATCH_PARENT(-1)/WRAP_CONTENT(-2) 哨兵尺寸：打包不破坏模式位，
/// size 读出为 Android `getSize` 同款掩码值（低 30 bit）。
#[test]
fn negative_sentinels_pack_without_mode_corruption() {
    let spec = MeasureSpec::make(Mode::Exactly, (-1i32) as u32);
    assert_eq!(spec.mode(), Mode::Exactly);
    assert_eq!(spec.size(), (-1i32 as u32) & 0x3FFF_FFFF);

    let spec = MeasureSpec::make(Mode::AtMost, (-2i32) as u32);
    assert_eq!(spec.mode(), Mode::AtMost);
    assert_eq!(spec.size(), (-2i32 as u32) & 0x3FFF_FFFF);
}

/// 常量数值与 Android 一致。
#[test]
fn constants_match_android() {
    assert_eq!(MeasureSpec::MATCH_PARENT, -1i32 as u32);
    assert_eq!(MeasureSpec::WRAP_CONTENT, -2i32 as u32);
}

/// 30 bit 尺寸上限：0x3FFFFFFF 打包后不溢出到模式位。
#[test]
fn max_size_roundtrip() {
    let spec = MeasureSpec::make(Mode::Exactly, 0x3FFF_FFFF);
    assert_eq!(spec.size(), 0x3FFF_FFFF);
    assert_eq!(spec.mode(), Mode::Exactly);
}
