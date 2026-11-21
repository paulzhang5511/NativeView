//! Drawable 单测：L1 抽象 + 三实现（Task 16 切片一）。

use velloview::gfx::color::Color;
use velloview::gfx::scene::{Rect, Scene};
use velloview::ui::drawable::{ColorDrawable, GradientDrawable, GradientOrientation};

/// ColorDrawable：绘制 bounds 尺寸纯色矩形。
#[test]
fn color_drawable_fills_bounds() {
    let mut d = ColorDrawable::new(Color::RED);
    d.set_bounds(Rect {
        x: 10.0,
        y: 10.0,
        w: 20.0,
        h: 30.0,
    });
    let mut scene = Scene::new(100, 100);
    d.draw(&mut scene);
    match &scene.commands[..] {
        [velloview::gfx::scene::Command::FillRect { rect, paint }] => {
            assert_eq!(
                *rect,
                Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 20.0,
                    h: 30.0
                }
            );
            assert_eq!(paint.color, Color::RED);
        }
        _ => panic!("expected single FillRect, got {:?}", scene.commands),
    }
}

/// setAlpha 语义：0 = 全透明（不绘制），255 = 不透明。
#[test]
fn drawable_alpha_gate() {
    let mut d = ColorDrawable::new(Color::BLUE);
    d.set_bounds(Rect {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    });
    // alpha=0：完全不透明 → 无绘制命令。
    d.set_alpha(0);
    let mut scene = Scene::new(50, 50);
    d.draw(&mut scene);
    assert!(scene.is_empty(), "alpha=0 must skip draw");
    // 恢复可见。
    d.set_alpha(255);
    let mut scene = Scene::new(50, 50);
    d.draw(&mut scene);
    assert!(!scene.is_empty());
}

/// setVisible(false)：不绘制。
#[test]
fn drawable_visibility_gate() {
    let mut d = ColorDrawable::new(Color::GREEN);
    d.set_bounds(Rect {
        x: 0.0,
        y: 0.0,
        w: 5.0,
        h: 5.0,
    });
    d.set_visible(false);
    let mut scene = Scene::new(20, 20);
    d.draw(&mut scene);
    assert!(scene.is_empty());
}

/// GradientDrawable（纯色圆角子集）：圆角半径进 path。
#[test]
fn gradient_drawable_round_rect() {
    let mut d = GradientDrawable::rounded(Color::GRAY, 8.0);
    d.set_bounds(Rect {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 20.0,
    });
    let mut scene = Scene::new(50, 50);
    d.draw(&mut scene);
    // 圆角矩形以 FillPath 表达。
    assert!(
        matches!(
            &scene.commands[..],
            [velloview::gfx::scene::Command::FillPath { .. }]
        ),
        "rounded background must emit FillPath, got {:?}",
        scene.commands
    );
}

/// GradientDrawable 渐变方向：水平双色渐变。
#[test]
fn gradient_drawable_linear_gradient() {
    let mut d =
        GradientDrawable::linear(GradientOrientation::Horizontal, &[Color::RED, Color::BLUE]);
    d.set_bounds(Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 50.0,
    });
    let mut scene = Scene::new(120, 60);
    d.draw(&mut scene);
    match &scene.commands[..] {
        [velloview::gfx::scene::Command::FillPath { paint, .. }] => {
            assert!(paint.shader.is_some(), "gradient must attach a shader");
        }
        _ => panic!("expected FillPath with shader, got {:?}", scene.commands),
    }
}
