//! 内置控件单测：FrameLayout/LinearLayout/TextView/ImageView（Task 16）。

use velloview::gfx::color::Color;
use velloview::ui::measure_spec::{MeasureSpec, Mode};
use velloview::ui::view::View;
use velloview::ui::view_group::MarginLayoutParams;
use velloview::ui::widgets::{
    Button, Ellipsize, FrameLayout, ImageView, LinearLayout, ScaleType, ScrollView, TextAlignment,
    TextView, ViewWidget,
};

const MATCH_PARENT: i32 = -1;
const WRAP_CONTENT: i32 = -2;

/// FrameLayout：子视图 gravity 居中。
#[test]
fn frame_layout_centers_single_child() {
    let mut frame = FrameLayout::new();
    let mut child = View::new();
    child.set_background_color(Some(Color::RED));
    frame.add_child(
        child,
        MarginLayoutParams::new(40, 40),
        velloview::ui::view::Gravity::CENTER,
    );

    let parent = MeasureSpec::make(Mode::Exactly, 100);
    frame.measure(parent, parent);
    frame.layout(0.0, 0.0, 100.0, 100.0);

    let c = frame.child_at(0).unwrap();
    // 100x100 容器内 40x40 子视图居中：(100-40)/2 = 30。
    assert_eq!(c.child.view().left(), 30.0);
    assert_eq!(c.child.view().top(), 30.0);
    assert_eq!(c.child.view().width(), 40.0);
    assert_eq!(c.child.view().height(), 40.0);
}

/// LinearLayout 垂直：子视图按声明高度堆叠。
#[test]
fn linear_layout_vertical_stacks_children() {
    let mut linear = LinearLayout::vertical();
    for h in [30, 40] {
        let child = View::new();
        linear.add_child(
            child,
            MarginLayoutParams::new(MATCH_PARENT, h),
            velloview::ui::view::Gravity::NO_GRAVITY,
        );
    }
    let parent = MeasureSpec::make(Mode::Exactly, 100);
    linear.measure(parent, parent);
    linear.layout(0.0, 0.0, 100.0, 100.0);

    let c0 = linear.child_at(0).unwrap();
    assert_eq!(c0.child.view().top(), 0.0);
    assert_eq!(c0.child.view().height(), 30.0);
    let c1 = linear.child_at(1).unwrap();
    assert_eq!(c1.child.view().top(), 30.0);
    assert_eq!(c1.child.view().height(), 40.0);
}

/// LinearLayout 水平：子视图横向排列。
#[test]
fn linear_layout_horizontal_arranges_children() {
    let mut linear = LinearLayout::horizontal();
    for w in [50, 25] {
        let child = View::new();
        linear.add_child(
            child,
            MarginLayoutParams::new(w, WRAP_CONTENT),
            velloview::ui::view::Gravity::NO_GRAVITY,
        );
    }
    let parent = MeasureSpec::make(Mode::Exactly, 200);
    linear.measure(parent, parent);
    linear.layout(0.0, 0.0, 200.0, 100.0);

    let c0 = linear.child_at(0).unwrap();
    assert_eq!(c0.child.view().left(), 0.0);
    assert_eq!(c0.child.view().width(), 50.0);
    let c1 = linear.child_at(1).unwrap();
    assert_eq!(c1.child.view().left(), 50.0);
    assert_eq!(c1.child.view().width(), 25.0);
}

/// LinearLayout weight：剩余空间按 weight 比例分配。
#[test]
fn linear_layout_weight_distribution() {
    let mut linear = LinearLayout::horizontal();
    let mut lp = MarginLayoutParams::new(WRAP_CONTENT, WRAP_CONTENT);
    lp.weight = 3.0;
    let a = View::new();
    let lp_a = lp;
    lp.weight = 1.0;
    let b = View::new();
    let lp_b = lp;
    linear.add_child(a, lp_a, velloview::ui::view::Gravity::NO_GRAVITY);
    linear.add_child(b, lp_b, velloview::ui::view::Gravity::NO_GRAVITY);

    let parent = MeasureSpec::make(Mode::Exactly, 100);
    linear.measure(parent, parent);
    linear.layout(0.0, 0.0, 100.0, 10.0);

    // 无固有宽度的 weight 子视图：100 按 3:1 分 = 75 / 25。
    let a = linear.child_at(0).unwrap();
    assert_eq!(a.child.view().width(), 75.0);
    let b = linear.child_at(1).unwrap();
    assert_eq!(b.child.view().width(), 25.0);
}

/// TextView：WRAP_CONTENT 测量为文本固有尺寸（>0）。
#[test]
fn text_view_wrap_measures_to_content() {
    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
    );
    let font = velloview::text::font::load_from_path(std::path::Path::new(font_path))
        .expect("fixture font must load");
    let mut tv = TextView::new();
    tv.set_context(velloview::text::layout::TextContext::new(&font));
    tv.set_text("Hello");
    tv.set_font_size(32.0);
    let am = MeasureSpec::make(Mode::AtMost, 1000);
    tv.measure(am, am);
    assert!(
        tv.view.measured_width() > 0.0,
        "wrap width must be positive"
    );
    assert!(
        tv.view.measured_height() > 0.0,
        "wrap height must be positive"
    );
}

/// TextView 对齐文本：TextAlignment 枚举可设置/读取。
#[test]
fn text_view_alignment() {
    let mut tv = TextView::new();
    assert_eq!(tv.text_alignment(), TextAlignment::Left);
    tv.set_text_alignment(TextAlignment::Center);
    assert_eq!(tv.text_alignment(), TextAlignment::Center);
}

/// ImageView FIT_CENTER：宽高比保持，适配进 bounds。
#[test]
fn image_view_fit_center_keeps_aspect() {
    let mut iv = ImageView::new();
    // 200x100 图（构造测试图像：RGBA 全白）。
    let img = velloview::gfx::image::Image::from_rgba8(200, 100, vec![255; 200 * 100 * 4]);
    iv.set_image(Some(img));
    iv.set_scale_type(ScaleType::FitCenter);

    // bounds 100x100：fit 后 100x50 居中。
    let e = MeasureSpec::make(Mode::Exactly, 100);
    iv.measure(e, e);
    iv.layout(0.0, 0.0, 100.0, 100.0);
    let dst = iv.content_bounds();
    assert_eq!(dst.w, 100.0);
    assert_eq!(dst.h, 50.0);
    assert_eq!(dst.y, 25.0, "vertical centering expected");
}

/// ImageView CENTER_CROP：短边铺满，裁剪溢出。
#[test]
fn image_view_center_crop_fills() {
    let mut iv = ImageView::new();
    let img = velloview::gfx::image::Image::from_rgba8(200, 100, vec![255; 200 * 100 * 4]);
    iv.set_image(Some(img));
    iv.set_scale_type(ScaleType::CenterCrop);

    let e = MeasureSpec::make(Mode::Exactly, 100);
    iv.measure(e, e);
    iv.layout(0.0, 0.0, 100.0, 100.0);
    let dst = iv.content_bounds();
    // crop：高铺满 100 → 宽 200 居中溢出（dst 表达可见区，源图裁剪）。
    assert_eq!(dst.w, 100.0);
    assert_eq!(dst.h, 100.0);
}

/// Button：文本设置与测量委托（V1 无状态，背景绘制走圆角路径）。
#[test]
fn button_holds_text_and_measures() {
    let mut btn = Button::new();
    btn.set_text("OK");
    assert_eq!(btn.text(), "OK");
    // 未注入排版上下文时测量为 0（与 TextView 未注入行为一致）。
    let am = MeasureSpec::make(Mode::AtMost, 200);
    btn.measure(am, am);
    assert_eq!(btn.text_view.view.measured_width(), 0.0);
}

/// TextView Ellipsize END：可用宽度不足时尾部追加省略号且适配。
#[test]
fn text_view_ellipsize_end_fits_available_width() {
    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
    );
    let font = velloview::text::font::load_from_path(std::path::Path::new(font_path))
        .expect("fixture font must load");
    let mut tv = TextView::new();
    tv.set_context(velloview::text::layout::TextContext::new(&font));
    tv.set_text("The quick brown fox jumps over the lazy dog");
    tv.set_font_size(32.0);
    tv.set_ellipsize(Ellipsize::End);
    // 长文本 + 窄容器：必然触发截断。
    let am = MeasureSpec::make(Mode::AtMost, 200);
    tv.measure(am, am);
    assert!(
        tv.view.measured_width() <= 200.0,
        "must fit available width"
    );
    // 省略后必然发生了截断（宽度远小于全文 32px 排版宽度）。
    assert!(tv.view.measured_width() > 0.0, "must render something");
}

/// TextView Ellipsize 默认 None：不截断（超宽时自然折行，宽度不超限）。
#[test]
fn text_view_ellipsize_none_wraps_instead() {
    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
    );
    let font = velloview::text::font::load_from_path(std::path::Path::new(font_path))
        .expect("fixture font must load");
    let mut tv = TextView::new();
    tv.set_context(velloview::text::layout::TextContext::new(&font));
    tv.set_text("The quick brown fox jumps over the lazy dog");
    tv.set_font_size(32.0);
    let am = MeasureSpec::make(Mode::AtMost, 80);
    tv.measure(am, am);
    // 未开启省略时折行（不拆词：长单词可溢出 max_advance，与 Android 一致）。
    assert!(
        tv.view.measured_height() > 32.0,
        "wrap grows height beyond one line"
    );
}

/// ScrollView：子内容溢出可滚动，偏移钳制到 [0, max]。
#[test]
fn scroll_view_clamps_scroll_offset() {
    let mut sv = ScrollView::new();
    let mut child = View::new();
    child.set_background_color(Some(Color::RED));
    sv.set_child(
        Box::new(ViewWidget(child)),
        MarginLayoutParams::new(MATCH_PARENT, 400),
    );

    let e = MeasureSpec::make(Mode::Exactly, 100);
    sv.measure(e, e);
    sv.layout(0.0, 0.0, 100.0, 100.0);

    // 容器 100 / 内容 400 → 最大 300。
    assert_eq!(sv.max_scroll(), 300.0);
    sv.request_scroll_to(500.0);
    assert_eq!(sv.scroll_y(), 300.0, "clamp to max");
    sv.request_scroll_to(-20.0);
    assert_eq!(sv.scroll_y(), 0.0, "clamp to min");
}

/// ScrollView 绘制：裁剪 + 平移命令序列正确。
#[test]
fn scroll_view_draw_clips_and_translates() {
    let mut sv = ScrollView::new();
    let mut child = View::new();
    child.set_background_color(Some(Color::BLUE));
    sv.set_child(
        Box::new(ViewWidget(child)),
        MarginLayoutParams::new(MATCH_PARENT, 300),
    );

    let e = MeasureSpec::make(Mode::Exactly, 100);
    sv.measure(e, e);
    sv.layout(0.0, 0.0, 100.0, 100.0);
    sv.request_scroll_to(50.0);

    use velloview::gfx::scene::Scene;
    let mut scene = Scene::new(100, 100);
    sv.draw(&mut scene);
    // 命令序列：裁剪 → 内容 FillRect（平移后 y=-50）→ 弹出裁剪。
    use velloview::gfx::scene::Command;
    let has_clip = scene
        .commands
        .iter()
        .any(|c| matches!(c, Command::ClipRect { .. }));
    assert!(has_clip, "must clip to bounds");
    let translated = scene.commands.iter().any(|c| match c {
        Command::FillRect { rect, .. } => rect.y < 0.0,
        _ => false,
    });
    assert!(translated, "content must be offset by -scroll_y");
}
