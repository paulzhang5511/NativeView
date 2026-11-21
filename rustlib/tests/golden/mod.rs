//! 黄金图测试框架：主机 cpu 渲染 → PNG 落盘 → 与基准逐像素比对。
//!
//! # 确定性要求
//! - 固定尺寸、固定字体（sha256 校验）、无随机数、无时间依赖；
//! - 基准缺失时**明确报错**（不生成基准后静默通过）；
//! - 容差：单通道差 ≤ 2 且差异像素数 ≤ 总像素 0.1%。

use velloview::gfx::render::render_scene;
use velloview::gfx::scene::Scene;

/// 差异像素判定容差：单通道最大差。
const CHANNEL_TOLERANCE: u8 = 2;
/// 差异像素占比上限（超过判失败）。
const MAX_DIFF_RATIO: f64 = 0.001;

/// 渲染场景为 RGBA8888 缓冲。
pub fn render(scene: &Scene) -> Vec<u8> {
    let pixmap = render_scene(scene).expect("golden render failed");
    pixmap.data_as_u8_slice().to_vec()
}

/// 比对结果。
#[derive(Debug, PartialEq)]
pub struct Diff {
    /// 超容差的像素数。
    pub diff_pixels: usize,
    /// 总像素数。
    pub total_pixels: usize,
    /// 最大单通道差。
    pub max_channel_diff: u8,
}

impl Diff {
    /// 是否通过（差异像素占比 ≤ MAX_DIFF_RATIO）。
    pub fn is_pass(&self) -> bool {
        let ratio = self.diff_pixels as f64 / self.total_pixels as f64;
        ratio <= MAX_DIFF_RATIO
    }
}

/// 逐像素比对两个 RGBA8888 缓冲。
///
/// # 失败语义
/// 尺寸不一致直接返回 `None`（调用方报错）。
pub fn compare(actual: &[u8], expected: &[u8]) -> Option<Diff> {
    if actual.len() != expected.len() || actual.len() % 4 != 0 {
        return None;
    }
    let mut diff = Diff {
        diff_pixels: 0,
        total_pixels: actual.len() / 4,
        max_channel_diff: 0,
    };
    for (a, e) in actual.chunks_exact(4).zip(expected.chunks_exact(4)) {
        let mut pixel_diff: u8 = 0;
        for (ac, ec) in a.iter().zip(e.iter()) {
            pixel_diff = pixel_diff.max(ac.abs_diff(*ec));
        }
        diff.max_channel_diff = diff.max_channel_diff.max(pixel_diff);
        if pixel_diff > CHANNEL_TOLERANCE {
            diff.diff_pixels += 1;
        }
    }
    Some(diff)
}

/// 运行一次黄金图用例：渲染 → 与基准 PNG 比对。
///
/// # 基准策略
/// - 基准存在：比对，超容差 panic（测试失败）；
/// - 基准缺失：panic 并提示"先落基准"（禁止静默生成后通过）。
///
/// # 环境变量
/// `UPDATE_GOLDEN=1` 时写入基准 PNG（基准维护用，非 CI 路径）。
pub fn check(scene: &Scene, name: &str) {
    let actual = render(scene);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/baselines");
    std::fs::create_dir_all(dir).unwrap();
    let path = format!("{dir}/{name}.png");

    // 编码 actual 为 PNG（image crate，无损）。
    let (w, h) = (scene.width, scene.height);
    let png_bytes = encode_png(&actual, w, h);

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(&path, &png_bytes).unwrap();
        panic!("golden baseline written for '{name}', re-run without UPDATE_GOLDEN to verify");
    }

    match std::fs::read(&path) {
        Ok(baseline_bytes) => {
            let expected = decode_png(&baseline_bytes, w, h);
            let diff = compare(&actual, &expected)
                .unwrap_or_else(|| panic!("golden '{name}': baseline size mismatch"));
            assert!(
                diff.is_pass(),
                "golden '{name}' failed: {}/{} pixels differ, max channel diff {}",
                diff.diff_pixels,
                diff.total_pixels,
                diff.max_channel_diff
            );
        }
        Err(_) => {
            panic!("golden baseline missing for '{name}': run with UPDATE_GOLDEN=1 to create it");
        }
    }
}

/// PNG 编码（RGBA8888 无损）。
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec()).unwrap();
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

/// PNG 解码为紧凑 RGBA8888。
fn decode_png(bytes: &[u8], width: u32, height: u32) -> Vec<u8> {
    let img = image::load_from_memory(bytes).unwrap().to_rgba8();
    assert_eq!(img.width(), width, "baseline width mismatch");
    assert_eq!(img.height(), height, "baseline height mismatch");
    img.into_raw()
}

/// 容差判定逻辑单测：完全一致通过。
#[test]
fn compare_identical_passes() {
    let pixels = vec![255, 0, 0, 255, 0, 255, 0, 255];
    let diff = compare(&pixels, &pixels).unwrap();
    assert!(diff.is_pass());
    assert_eq!(diff.diff_pixels, 0);
}

/// 容差判定逻辑单测：小偏差（≤容差）不算差异像素。
#[test]
fn compare_small_noise_within_tolerance() {
    let a = vec![100, 100, 100, 255];
    let b = vec![102, 98, 101, 255];
    let diff = compare(&a, &b).unwrap();
    assert!(diff.is_pass());
    assert_eq!(diff.diff_pixels, 0);
    assert_eq!(diff.max_channel_diff, 2);
}

/// 容差判定逻辑单测：超容差样例失败。
#[test]
fn compare_large_diff_fails() {
    // 2 像素，1 个差异像素 = 50% > 0.1%。
    let a = vec![0, 0, 0, 255, 0, 0, 0, 255];
    let b = vec![255, 255, 255, 255, 0, 0, 0, 255];
    let diff = compare(&a, &b).unwrap();
    assert!(!diff.is_pass());
    assert_eq!(diff.diff_pixels, 1);
    assert_eq!(diff.total_pixels, 2);
}

/// 尺寸不一致返回 None。
#[test]
fn compare_size_mismatch_returns_none() {
    assert!(compare(&[0; 4], &[0; 8]).is_none());
}
