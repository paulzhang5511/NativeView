//! 黄金图用例：P2 自定义绘制场景（PRD A7）+ 三页 Demo 静态场景（Task 20）。
//!
//! 基准生成：`UPDATE_GOLDEN=1 cargo test --test golden_scenes`；
//! 基准入库 git（确定性：固定尺寸/字体/无随机数）。

mod golden;

use velloview::demo::Demo;
use velloview::demo::Page;
use velloview::gfx::canvas::Canvas;
use velloview::gfx::color::Color;
use velloview::gfx::paint::Paint;
use velloview::gfx::path::Path;
use velloview::gfx::scene::Rect;
use velloview::gfx::shader::{Shader, TileMode};

/// 场景一：矩形 + 渐变 + 裁剪组合。
#[test]
fn golden_shapes() {
    let mut c = Canvas::new(100, 100);
    c.set_clear_color(Color::WHITE);
    // 红色矩形（左上）。
    c.draw_rect(
        Rect {
            x: 10.0,
            y: 10.0,
            w: 30.0,
            h: 20.0,
        },
        &Paint::default().color(Color::RED),
    );
    // 线性渐变矩形（右下）。
    let shader = Shader::linear(
        50.0,
        50.0,
        90.0,
        90.0,
        &[Color::BLUE, Color::GREEN],
        None,
        TileMode::Clamp,
    );
    c.draw_rect(
        Rect {
            x: 50.0,
            y: 50.0,
            w: 40.0,
            h: 40.0,
        },
        &Paint::default().shader(Some(shader)),
    );
    // 圆形路径。
    let circle = Path::circle(50.0, 25.0, 15.0);
    c.draw_path(&circle, &Paint::default().color(Color::BLACK));
    golden::check(&c.finish(), "shapes");
}

/// 场景二：裁剪语义（clip 外不绘制）。
#[test]
fn golden_clip() {
    let mut c = Canvas::new(80, 80);
    c.set_clear_color(Color::WHITE);
    c.save();
    c.clip_rect(Rect {
        x: 20.0,
        y: 20.0,
        w: 40.0,
        h: 40.0,
    });
    c.draw_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        &Paint::default().color(Color::RED),
    );
    c.restore();
    // restore 后裁剪解除：整幅蓝色矩形。
    c.draw_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 80.0,
            h: 80.0,
        },
        &Paint::default().color(Color::rgba(0, 0, 255, 128)),
    );
    golden::check(&c.finish(), "clip");
}

/// 确定性验证：同一场景连跑两次逐字节一致。
#[test]
fn golden_deterministic() {
    let build = || {
        let mut c = Canvas::new(50, 50);
        c.set_clear_color(Color::WHITE);
        let shader = Shader::radial(
            25.0,
            25.0,
            20.0,
            &[Color::RED, Color::YELLOW],
            None,
            TileMode::Clamp,
        );
        c.draw_rect(
            Rect {
                x: 5.0,
                y: 5.0,
                w: 40.0,
                h: 40.0,
            },
            &Paint::default().shader(Some(shader)),
        );
        c.finish()
    };
    let a = golden::render(&build());
    let b = golden::render(&build());
    assert_eq!(a, b, "same scene must render identically twice");
}

/// 测试字体路径（与 APK assets 同一份 NotoSerifSC）。
const FONT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
);

/// 场景三：静态页 View 树拼装（Task 16 验收——纯框架 widgets 自举：
/// 文本 + 按钮 + 滚动裁剪，不引入控件库）。
///
/// # 拼装约束
/// 容器（Frame/Linear）持有纯 `View`；完整控件（TextView/Button）的
/// 排版/圆角背景由控件自身 `draw` 输出——静态页 = 容器树（结构） +
/// 控件 `draw`（内容），与 Task 20 Demo 页装配同构。
#[test]
fn golden_static_page() {
    use velloview::text::font::load_from_path;
    use velloview::text::layout::TextContext;
    use velloview::ui::measure_spec::{MeasureSpec, Mode};
    use velloview::ui::view::Gravity;
    use velloview::ui::view_group::MarginLayoutParams;
    use velloview::ui::widgets::{Button, FrameLayout, TextView};

    // 主机渲染字体注入（一次性）。
    let font = load_from_path(std::path::Path::new(FONT_PATH)).expect("font");
    velloview::text::render::set_font_bytes(font.bytes().to_vec());

    // 根：FrameLayout（白底）。
    let mut root = FrameLayout::new();
    root.inner.view.set_background_color(Some(Color::WHITE));

    // 标题条 View（容器内纯色块，等效传统布局结构）。
    let mut title_bar = velloview::ui::view::View::new();
    title_bar.set_background_color(Some(Color::rgba(30, 144, 255, 255)));
    root.add_child(title_bar, MarginLayoutParams::new(-1, 40), Gravity::TOP);

    let e = MeasureSpec::make(Mode::Exactly, 100);
    root.measure(e, e);
    root.layout(0.0, 0.0, 100.0, 100.0);

    // 内容层：完整控件自绘（容器布局完成后定位）。
    let mut title = TextView::new();
    title.set_context(TextContext::new(&font));
    title.set_text("NativeView");
    title.set_font_size(20.0);
    title.set_text_color(Color::WHITE);
    let am = MeasureSpec::make(Mode::AtMost, 100);
    title.measure(am, am);
    title.layout(
        8.0,
        8.0,
        8.0 + title.view.measured_width(),
        8.0 + title.view.measured_height(),
    );

    let mut ok_btn = Button::new();
    ok_btn.set_text("OK");
    ok_btn.measure(am, am);
    let bw = ok_btn.text_view.view.measured_width().max(40.0);
    let bh = ok_btn.text_view.view.measured_height().max(24.0);
    ok_btn.layout(30.0, 60.0, 30.0 + bw, 60.0 + bh);

    let mut scene = velloview::gfx::scene::Scene::new(100, 100);
    root.draw(&mut scene);
    title.draw(&mut scene);
    ok_btn.draw(&mut scene);
    golden::check(&scene, "static_page");
}

/// 三页 Demo 静态黄金图（Task 20 验收）：P1 列表 / P2 绘制 / P3 文字。
///
/// # 确定性
/// 固定窗口尺寸；P2 固定相位 0.5（黄金图只覆盖静态帧——
/// 无限动画的中间态不具基准意义）；字体与 APK assets 同一份。
fn demo_golden(page: Page, name: &str) {
    use velloview::text::font::load_from_path;
    use velloview::text::render;

    let font = load_from_path(std::path::Path::new(FONT_PATH)).expect("font");
    render::set_font_bytes(font.bytes().to_vec());
    let mut d = Demo::new(360, 640, Some(font));
    d.page = page;
    if page == Page::Draw {
        d.set_anim_phase_for_test(0.5);
    }
    let scene = d.render();
    golden::check(&scene, name);
}

/// P1：滚动列表页（默认状态，无高亮）。
#[test]
fn golden_demo_list() {
    demo_golden(Page::List, "demo_list");
}

/// P2：自定义绘制页（相位 0.5 静态帧）。
#[test]
fn golden_demo_draw() {
    demo_golden(Page::Draw, "demo_draw");
}

/// P3：文字排版页。
#[test]
fn golden_demo_text() {
    demo_golden(Page::Text, "demo_text");
}
