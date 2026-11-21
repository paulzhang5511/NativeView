//! View 基类单测：L1 属性默认值（表驱动）+ measure 不变量（Task 14）。

use velloview::gfx::scene::Scene;
use velloview::ui::measure_spec::{MeasureSpec, Mode};
use velloview::ui::view::{Gravity, View, Visibility};

/// 属性默认值表（与 Android View 对齐）。
#[test]
fn view_defaults_match_android() {
    let v = View::new();
    assert_eq!(v.visibility(), Visibility::Visible);
    assert!(v.is_enabled());
    assert!(!v.is_clickable());
    assert_eq!(v.alpha(), 1.0);
    assert_eq!(v.translation_x(), 0.0);
    assert_eq!(v.translation_y(), 0.0);
    assert_eq!(v.scale_x(), 1.0);
    assert_eq!(v.scale_y(), 1.0);
    assert_eq!(v.rotation(), 0.0);
    assert_eq!(v.padding_left(), 0.0);
    assert_eq!(v.padding_right(), 0.0);
    assert_eq!(v.padding_top(), 0.0);
    assert_eq!(v.padding_bottom(), 0.0);
    assert_eq!(v.left(), 0.0);
    assert_eq!(v.top(), 0.0);
    assert_eq!(v.width(), 0.0);
    assert_eq!(v.height(), 0.0);
    assert_eq!(v.gravity(), Gravity::NO_GRAVITY);
}

/// setter/getter 往返。
#[test]
fn view_setters_roundtrip() {
    let mut v = View::new();
    v.set_visibility(Visibility::Gone);
    assert_eq!(v.visibility(), Visibility::Gone);
    v.set_enabled(false);
    assert!(!v.is_enabled());
    v.set_clickable(true);
    assert!(v.is_clickable());
    v.set_alpha(0.5);
    assert_eq!(v.alpha(), 0.5);
    v.set_translation_x(10.0);
    assert_eq!(v.translation_x(), 10.0);
    v.set_scale_y(2.0);
    assert_eq!(v.scale_y(), 2.0);
    v.set_rotation(90.0);
    assert_eq!(v.rotation(), 90.0);
    v.set_padding(1.0, 2.0, 3.0, 4.0);
    assert_eq!(v.padding_left(), 1.0);
    assert_eq!(v.padding_top(), 2.0);
    assert_eq!(v.padding_right(), 3.0);
    assert_eq!(v.padding_bottom(), 4.0);
    v.set_gravity(Gravity::CENTER);
    assert_eq!(v.gravity(), Gravity::CENTER);
}

/// layout 后几何 getter 派生正确（right = left + width 等）。
#[test]
fn layout_derives_geometry() {
    let mut v = View::new();
    let e = MeasureSpec::make(Mode::Exactly, 100);
    v.measure(e, e);
    v.layout(10.0, 20.0, 110.0, 120.0);
    assert_eq!(v.left(), 10.0);
    assert_eq!(v.top(), 20.0);
    assert_eq!(v.right(), 110.0);
    assert_eq!(v.bottom(), 120.0);
    assert_eq!(v.width(), 100.0);
    assert_eq!(v.height(), 100.0);
}

/// measure 不变量：spec 未变且未 forceLayout 时重复 measure 不回调 onMeasure。
#[test]
fn measure_short_circuits_on_same_spec() {
    let e = MeasureSpec::make(Mode::Exactly, 100);
    let mut v = View::new();
    v.measure(e, e);
    let first = v.on_measure_count();
    // 同一 spec 再 measure：不得重复回调。
    v.measure(e, e);
    assert_eq!(v.on_measure_count(), first, "same spec must not re-measure");
    // forceLayout 后：必须重新回调。
    v.force_layout();
    v.measure(e, e);
    assert_eq!(
        v.on_measure_count(),
        first + 1,
        "forceLayout must force re-measure"
    );
    // spec 变化后：必须重新回调。
    let e2 = MeasureSpec::make(Mode::Exactly, 200);
    v.measure(e2, e2);
    assert_eq!(
        v.on_measure_count(),
        first + 2,
        "changed spec must re-measure"
    );
}

/// EXACTLY 强制覆盖：onMeasure 默认实现按 spec 尺寸设定测量宽高。
#[test]
fn exactly_spec_sets_measured_size() {
    let mut v = View::new();
    let w = MeasureSpec::make(Mode::Exactly, 480);
    let h = MeasureSpec::make(Mode::Exactly, 800);
    v.measure(w, h);
    assert_eq!(v.measured_width(), 480.0);
    assert_eq!(v.measured_height(), 800.0);
}

/// AT_MOST：默认实现（无内容）测量为 0（Android View 默认无内容语义）。
#[test]
fn at_most_defaults_to_zero() {
    let mut v = View::new();
    let am = MeasureSpec::make(Mode::AtMost, 300);
    v.measure(am, am);
    assert_eq!(v.measured_width(), 0.0);
    assert_eq!(v.measured_height(), 0.0);
}

/// requestLayout 置脏布局标志。
#[test]
fn request_layout_marks_dirty() {
    let mut v = View::new();
    assert!(!v.is_layout_requested());
    v.request_layout();
    assert!(v.is_layout_requested());
    let e = MeasureSpec::make(Mode::Exactly, 50);
    v.measure(e, e);
    v.layout(0.0, 0.0, 50.0, 50.0);
    assert!(!v.is_layout_requested(), "layout pass clears the flag");
}

/// UNSPECIFIED（无内容）：默认 0。
#[test]
fn unspecified_defaults_to_zero() {
    let mut v = View::new();
    let u = MeasureSpec::make(Mode::Unspecified, 0);
    v.measure(u, u);
    assert_eq!(v.measured_width(), 0.0);
    assert_eq!(v.measured_height(), 0.0);
}

/// draw 调用 on_draw 并派生 Canvas（背景色可绘制）。
#[test]
fn draw_produces_scene() {
    let mut v = View::new();
    v.set_background_color(Some(velloview::gfx::color::Color::RED));
    let e = MeasureSpec::make(Mode::Exactly, 100);
    v.measure(e, e);
    v.layout(0.0, 0.0, 100.0, 100.0);
    let mut scene = Scene::new(200, 200);
    v.draw(&mut scene);
    assert!(
        !scene.is_empty(),
        "background must emit at least one command"
    );
}
