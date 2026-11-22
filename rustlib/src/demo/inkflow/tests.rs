//! inkflow 单测：布局几何、动作编码、手势、动效曲线与 a11y 通路。
//! 全部直接驱动 `Inkflow` 的公开/内部 API，不依赖渲染后端。

use super::board::{pan_for_anchor, to_canvas_of, view_of, zoom_from_pinch};
use super::brush_params::{PARAM_FLOW, PARAM_OPACITY, PARAM_SIZE, PARAM_SMOOTHING, set_param};
use super::*;

fn layout() -> Layout {
    Layout::compute(1080.0, 120.0, 2280.0)
}

fn bare_ink() -> Inkflow {
    Inkflow::new(None, 1080.0, 2400.0, 120.0, 2280.0)
}

#[test]
fn page_transition_moves_the_rendered_scene_not_only_state() {
    use crate::gfx::scene::Command;

    // 取首屏标题的首个字形基线：它由 Layout 派生，最能证明「文字跟着整页一起动」。
    fn first_glyph_y(scene: &Scene) -> f32 {
        scene
            .commands
            .iter()
            .find_map(|c| match c {
                Command::DrawGlyphs { glyphs, .. } => glyphs.first().map(|g| g.y),
                _ => None,
            })
            .expect("每屏都应有标题文字")
    }

    // 带真实字体的实例：文字位移断言必须真的排出版来。
    let font = crate::text::font::load_from_path(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
    )))
    .expect("fixture font must load");
    let mut ink = Inkflow::new(Some(font), 1080.0, 2400.0, 120.0, 120.0);
    ink.perform(Action::CreateNew);
    ink.advance(70);
    let dy = ink.page_shift();
    assert!(dy > 0.0, "前进切屏在动画中途必须有正位移");
    let mut mid = Scene::new(1080, 2400);
    ink.render(&mut mid);

    ink.advance(900);
    let mut settled = Scene::new(1080, 2400);
    ink.render(&mut settled);

    let moved = first_glyph_y(&mid) - first_glyph_y(&settled);
    assert!(
        (moved - dy).abs() < 1.0,
        "文字必须与图形同步位移：字形基线差 {moved}，状态位移 {dy}"
    );
    assert!(!ink.needs_frames(), "动画结束必须停止请求帧");
}

/// 逐帧验收图：把动效关键帧在主机侧光栅化为 PNG，供人工比对。
///
/// # 用法
/// `cargo test --lib dump_animation_frames -- --ignored`；
/// 产物落在 `docs/screenshots/anim-frames/`，不参与常规 CI（软渲染较慢）。
#[test]
#[ignore = "生成动画逐帧验收图，耗时且无断言"]
fn dump_animation_frames() {
    use crate::gfx::image::Image;

    /// 逻辑画布尺寸（与 1080×2400 @2.625 的真机等价）。
    const W: f32 = 411.0;
    const H: f32 = 914.0;
    const INSET: f32 = 45.7;

    fn shoot(ink: &mut Inkflow, dir: &std::path::Path, name: &str) {
        let mut scene = Scene::new(W as u32, H as u32);
        ink.render(&mut scene);
        let Some(pm) = crate::gfx::render::render_scene(&scene) else {
            panic!("rasterize failed: {name}");
        };
        let img = Image::from_rgba8(
            u32::from(pm.width()),
            u32::from(pm.height()),
            pm.data_as_u8_slice().to_vec(),
        );
        let path = dir.join(format!("{name}.png"));
        std::fs::write(&path, img.encode_png().expect("png encode")).expect("write png");
    }

    /// 在当前动画时钟上推进 `ms`（`advance` 接受绝对时刻）。
    fn step(ink: &mut Inkflow, ms: u64) {
        let t = ink.clock + ms;
        let _ = ink.advance(t);
    }

    let dir = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/screenshots/anim-frames"
    ));
    std::fs::create_dir_all(dir).expect("create dir");
    let font = crate::text::font::load_from_path(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
    )))
    .expect("fixture font must load");
    // 软件光栅化需要进程级字体字节（真机由 glue 加载后注入）。
    crate::text::render::set_font_bytes(
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
        ))
        .expect("read font bytes"),
    );

    // 主机密度固定 1dp = 1px，故按真机「逻辑尺寸」渲染（411×914 dp），
    // 这样 dp/sp 派生的一切几何与真机等价（只是分辨率减半，够看动效）。
    let mut ink = Inkflow::new(Some(font), W, H, INSET, INSET);
    ink.set_animator_scale(1.0);

    // A. 作品库原地 + 卡片错峰入场（进入库时 cards_at 打点）。
    shoot(&mut ink, dir, "a0-library-parked");
    ink.cards_at = Some(ink.clock);
    step(&mut ink, 60);
    shoot(&mut ink, dir, "a1-card-stagger-60ms");
    step(&mut ink, 160);
    shoot(&mut ink, dir, "a2-card-stagger-160ms");
    step(&mut ink, 600);
    shoot(&mut ink, dir, "a3-card-settled");

    // A4. 按压反馈：按下筛选 chip 的瞬间有 10% 白色高亮（380ms 内衰减）。
    let chip = ink.layout().lib_filters[1];
    ink.press = Some((chip, ink.clock, Screen::Library));
    shoot(&mut ink, dir, "a4-press-highlight");
    ink.press = None;

    // B. 切屏：库 → 新建页（前进自下滑入 + 遮罩）。
    ink.perform(Action::CreateNew);
    step(&mut ink, 60);
    shoot(&mut ink, dir, "b1-page-in-60ms");
    step(&mut ink, 140);
    shoot(&mut ink, dir, "b2-page-in-140ms");
    step(&mut ink, 400);
    shoot(&mut ink, dir, "b3-page-in-settled");

    // C. 选中脉冲 + 按压高亮：点纸色与点筛选 chip。
    let paper = ink.layout().nc_papers[1];
    ink.perform(Action::PickPaper(1));
    step(&mut ink, 130);
    shoot(&mut ink, dir, "c1-paper-pulse");
    step(&mut ink, 2000);

    // D. 新建页 → 画板 → 笔刷面板滑入。
    ink.perform(Action::StartDrawing);
    step(&mut ink, 2000);
    ink.perform(Action::OpenBrushSheet);
    step(&mut ink, 80);
    shoot(&mut ink, dir, "d1-sheet-in-80ms");
    step(&mut ink, 200);
    shoot(&mut ink, dir, "d2-sheet-in-200ms");
    step(&mut ink, 2000);
    shoot(&mut ink, dir, "d3-sheet-open");

    // D4. 画布内容闪烁（撤销 / 重做 / 切图层）：只压画布，不动 dock 与顶栏。
    ink.flash_at = Some(ink.clock);
    shoot(&mut ink, dir, "d4-canvas-flash");
    step(&mut ink, 200);

    // E. 面板内选中预设的脉冲环。
    ink.perform(Action::PickPreset(3));
    step(&mut ink, 130);
    shoot(&mut ink, dir, "e1-preset-pulse");

    // F. 导出成功入场（状态行与双按钮滑入）。
    step(&mut ink, 2000);
    ink.perform(Action::CloseBrushSheet);
    step(&mut ink, 2000);
    ink.perform(Action::OpenExport);
    step(&mut ink, 2000);
    shoot(&mut ink, dir, "f0-export-before-save");
    // 真跑一次保存：「已保存」状态行与双按钮的入场动效由成功回调打点。
    ink.perform(Action::DoExport);
    assert!(ink.exported.is_some(), "导出入场帧需要真实保存成功");
    step(&mut ink, 120);
    shoot(&mut ink, dir, "f1-export-in-120ms");
    step(&mut ink, 2000);
    shoot(&mut ink, dir, "f2-export-settled");
    let _ = paper;
}

#[test]
fn tap_registers_press_highlight_that_outlives_only_press_window() {
    let mut ink = bare_ink();
    // 点筛选 chip（不切屏，才能观察到按压高亮）；380ms 内衰减到 0。
    let chip = ink.layout().lib_filters[1];
    let (x, y) = (
        (chip.x + chip.w / 2.0) as f32,
        (chip.y + chip.h / 2.0) as f32,
    );
    let ev = {
        use crate::ui::input::{HistoricalSample, MotionEvent, PointerCoords};
        MotionEvent {
            packed_action: 0,
            down_time: 0,
            event_time: 0,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x,
                y,
                pressure: 0.5,
            }],
            historical: Vec::<HistoricalSample>::new(),
        }
    };
    assert!(ink.on_touch(&ev));
    assert!(ink.press_fade() > 0.0, "点击必须触发按压高亮");
    assert!(ink.needs_frames());
    ink.advance(400);
    assert_eq!(ink.press_fade(), 0.0, "按压高亮必须衰减殆尽");
    // 换屏后旧高亮不得叠到新屏上。
    ink.press = Some((chip, 0, Screen::Library));
    ink.screen = Screen::Board;
    assert_eq!(ink.press_fade(), 0.0);
}

#[test]
fn selection_changes_fire_a_pulse_on_the_picked_item_only() {
    let mut ink = bare_ink();
    ink.screen = Screen::NewCanvas;
    ink.perform(Action::PickPaper(2));
    // sin 包络起点为 0（不能一上来就亮），半程达到峰值。
    assert_eq!(ink.pulse_p(Pulse::Paper, 2), 0.0);
    ink.advance(130);
    let mid = ink.pulse_p(Pulse::Paper, 2);
    assert!(mid > 0.0, "半程必须有可见脉冲");
    ink.advance(300);
    assert_eq!(ink.pulse_p(Pulse::Paper, 0), 0.0, "未选中的纸色不得脉冲");
    assert_eq!(ink.pulse_p(Pulse::Size, 2), 0.0, "不同类别不得串台");
    ink.advance(600);
    assert_eq!(ink.pulse_p(Pulse::Paper, 2), 0.0);
    assert!(!ink.needs_frames());
}

#[test]
fn toast_fades_in_holds_then_out_and_stops_frames() {
    let mut ink = bare_ink();
    ink.toast = Some("已取色 · 色板 3".into());
    ink.advance(100);
    // 内容变化即视为新提示：淡入起点为 0，停留期为 1。
    assert_eq!(ink.toast_p(), 0.0, "刚出现的 toast 从透明开始");
    ink.advance(1000);
    assert_eq!(ink.toast_p(), 1.0);
    assert!(ink.needs_frames(), "停留期仍要持续渲染");
    ink.advance(100 + 180 + 1400 + 200);
    assert_eq!(ink.toast_p(), 0.0, "淡出完成后完全透明");
    assert!(!ink.needs_frames());
}

#[test]
fn export_success_animation_runs_once_and_parks() {
    let mut ink = bare_ink();
    assert_eq!(ink.export_p(), 1.0, "未导出时按钮处于终态");
    ink.export_at = Some(500);
    ink.clock = 500;
    assert_eq!(ink.export_p(), 0.0);
    assert!(ink.needs_frames());
    let _ = ink.advance(740);
    assert_eq!(ink.export_p(), 1.0);
    let _ = ink.advance(900);
    assert!(!ink.needs_frames(), "入场结束后不再请求帧");
}

#[test]
fn library_cards_stagger_and_settle() {
    let mut ink = bare_ink();
    ink.cards_at = Some(0);
    ink.clock = 0;
    // 第 0 张立刻开始，第 3 张延后 150ms；同一时刻前者位移必须更小。
    ink.advance(120);
    let first = ink.card_shift(0);
    let last = ink.card_shift(3);
    assert!(first < last, "卡片必须错峰入场");
    ink.advance(600);
    for i in 0..4 {
        assert_eq!(ink.card_shift(i), 0.0, "入场结束必须归位");
    }
    assert!(!ink.needs_frames());
}

#[test]
fn undo_flashes_canvas_and_marks_frames_needed() {
    let mut ink = bare_ink();
    ink.screen = Screen::NewCanvas;
    ink.perform(Action::StartDrawing);
    // 落一笔：直接注入笔画，保证 undo 有东西可撤。
    let doc = ink.doc.as_mut().unwrap();
    doc.begin_live();
    doc.extend_live(&[
        StrokePoint::new(120.0, 200.0, 1.0, 0),
        StrokePoint::new(320.0, 400.0, 1.0, 40),
    ]);
    doc.end_live();
    assert!(ink.doc.as_ref().is_some_and(|d| d.steps() > 0));
    ink.perform(Action::Undo);
    assert!(ink.flash_p() > 0.0, "撤销必须触发画布闪烁");
    ink.advance(200);
    assert_eq!(ink.flash_p(), 0.0);
    // 切屏动画（进画板）比闪烁长，两条通道都结束后才允许停帧。
    ink.advance(1200);
    assert!(!ink.needs_frames());
}

#[test]
fn page_transition_parks_shift_and_direction() {
    // 前进切屏：内容自下而上，动画结束后位移必须归零，否则页面会永久错位。
    let mut ink = bare_ink();
    assert!(!ink.needs_frames(), "初始无动效，不应请求帧");
    ink.perform(Action::CreateNew);
    assert_eq!(ink.screen, Screen::NewCanvas);
    assert!(ink.page_shift() > 0.0, "前进切屏应自下而上滑入");
    assert!(ink.page_cover() > 0.0, "切换瞬间应有遮罩");
    assert!(ink.needs_frames());
    // 第一帧推进仍返回 dirty（这一帧要画出新进度）。
    assert!(ink.advance(200));
    assert!(ink.page_shift() < anim::PAGE_SHIFT_PX);
    // 推到终点后：移位与遮罩归零，宿主可以停止请求帧。
    let _ = ink.advance(900);
    assert!(!ink.needs_frames(), "动画结束后不得继续请求帧");
    assert_eq!(ink.page_shift(), 0.0);
    assert_eq!(ink.page_cover(), 0.0);
    // 返回切屏方向相反（自上而下）。
    ink.perform(Action::Back);
    assert_eq!(ink.screen, Screen::Library);
    assert!(ink.page_shift() < 0.0, "返回切屏应自上而下滑入");
}

#[test]
fn brush_sheet_slides_from_bottom_and_reverses_midway() {
    let mut ink = bare_ink();
    ink.screen = Screen::Board;
    ink.perform(Action::OpenBrushSheet);
    assert_eq!(ink.screen, Screen::BrushSheet);
    let p_open0 = ink.sheet_p();
    assert!(p_open0 < 0.05, "刚打开时面板还在屏幕外");
    ink.advance(150);
    let p_open1 = ink.sheet_p();
    assert!(p_open1 > p_open0, "打开过程中展开度单调递增");
    // 中途关闭：以当前展开度为起点退出，不能跳回满屏。
    ink.perform(Action::CloseBrushSheet);
    assert_eq!(ink.screen, Screen::Board);
    let p_close0 = ink.sheet_p();
    assert!(
        (p_close0 - p_open1).abs() < 0.02,
        "中途反向必须从当前位置继续"
    );
    ink.advance(900);
    assert!(ink.sheet_p() <= 0.001, "关闭结束后面板必须完全收起");
    assert!(!ink.needs_frames());
}

#[test]
fn layout_shift_moves_rects_and_baselines_together() {
    // 平移必须同时移动矩形与顶部基准：文字坐标都从 top / cell 派生，
    // 只动其中一方会造成「图动字不动」。
    let ink = bare_ink();
    let base = ink.layout();
    let moved = base.shifted_y(24.0);
    assert_eq!(moved.top, base.top + 24.0);
    assert_eq!(moved.lib_plus.y, base.lib_plus.y + 24.0);
    assert_eq!(moved.lib_cards[2].y, base.lib_cards[2].y + 24.0);
    assert_eq!(moved.bd_swatches[3].y, base.bd_swatches[3].y + 24.0);
    assert_eq!(moved.nc_size_title_y, base.nc_size_title_y + 24.0);
    // 横向几何不受影响。
    assert_eq!(moved.lib_plus.x, base.lib_plus.x);
    // 面板专用平移只动面板，画板控件保持原位。
    let sheet = base.shifted_sheet_y(300.0);
    assert_eq!(sheet.sh_card.y, base.sh_card.y + 300.0);
    assert_eq!(sheet.sh_presets[7].y, base.sh_presets[7].y + 300.0);
    assert_eq!(sheet.bd_canvas.y, base.bd_canvas.y);
}

#[test]
fn export_layout_stacks_formats_above_buttons_without_overlap() {
    // 参考图的导出页是「预览卡 → 格式选择 → 保存」纵向堆叠：
    // 任何两块重叠都会在真机上表现为文字压边，必须由几何断言守住。
    // 与真机一致：状态栏 120px + 导航条 120px（bare_ink 的 bottom inset 传错，会算出空内容区）。
    let ink = Inkflow::new(None, 1080.0, 2400.0, 120.0, 120.0);
    let l = ink.layout();
    assert!(
        l.ex_card.y + l.ex_card.h < l.ex_formats[0].y,
        "格式行不得压到预览卡"
    );
    for i in 1..3 {
        assert!(
            l.ex_formats[i - 1].y + l.ex_formats[i - 1].h <= l.ex_formats[i].y,
            "格式行 {i} 与前一行重叠"
        );
    }
    assert!(
        l.ex_formats[2].y + l.ex_formats[2].h < l.ex_again.y,
        "格式行不得压到「再次导出」"
    );
    assert!(
        l.ex_again.y + l.ex_again.h < l.ex_save.y,
        "「再次导出」与「完成」必须留出间距"
    );
    assert!(
        l.ex_save.y + l.ex_save.h <= l.bottom as f64,
        "主按钮不得越出内容区底边"
    );
    // 未导出：主按钮 = 保存；已导出：主按钮 = 完成，另有「再次导出」。
    let fresh = l.hits(Screen::Export, false);
    assert!(fresh.iter().any(|(_, a)| *a == Action::DoExport));
    assert!(!fresh.iter().any(|(_, a)| *a == Action::Finish));
    let done = l.hits(Screen::Export, true);
    assert!(done.iter().any(|(_, a)| *a == Action::Finish));
    assert!(done.iter().any(|(_, a)| *a == Action::DoExport));
}

#[test]
fn export_formats_only_png_is_actionable() {
    // JPEG/PDF 只是占位行：点击必须给出提示，不能悄悄什么都不发生。
    let mut ink = bare_ink();
    ink.screen = Screen::NewCanvas;
    ink.perform(Action::StartDrawing);
    ink.screen = Screen::Export;
    ink.toast = None;
    ink.perform(Action::PickFormat(1));
    assert!(ink.toast.is_some(), "点未开放格式必须给提示");
    ink.toast = None;
    ink.perform(Action::PickFormat(0));
    assert!(ink.toast.is_none(), "PNG 是默认格式，不应弹提示");
}

#[test]
fn export_screen_emits_preview_image_command() {
    // 导出页必须把缩略图作为 DrawImage 命令发出（而不是只画纸色）。
    let mut ink = bare_ink();
    ink.screen = Screen::NewCanvas;
    ink.paper_idx = 1;
    ink.perform(Action::StartDrawing);
    assert_eq!(ink.screen, Screen::Board);
    ink.screen = Screen::Export;
    let mut scene = Scene::new(1080, 2400);
    ink.render(&mut scene);
    let l = Layout::compute(1080.0, 120.0, 2280.0);
    let mut found = None;
    for c in scene.commands.iter() {
        if let crate::gfx::scene::Command::DrawImage { image, x, y, .. } = c {
            found = Some((image.width(), image.height(), *x, *y));
        }
    }
    let (iw, ih, x, y) = found.expect("export preview image command");
    assert!(iw > 1 && ih > 1, "preview image empty: {iw}x{ih}");
    // 图像必须落在预览卡片内（卡片 x 内缩 28dp）。
    assert!(
        x >= l.ex_card.x as f64 - 1.0
            && x <= (l.ex_card.x + l.ex_card.w) as f64
            && y >= l.ex_card.y as f64 - 1.0
            && y <= (l.ex_card.y + l.ex_card.h) as f64,
        "preview drawn outside card: at ({x},{y}), card {:?}",
        l.ex_card
    );
}

#[test]
fn export_preview_thumbnail_keeps_ink() {
    // 导出页预览把整图缩到约 300px 宽：细线不能被降采样抹平。
    let mut doc = Document::new(
        1080,
        1920,
        Color {
            r: 247,
            g: 245,
            b: 239,
            a: 255,
        },
        PRESETS[0].brush,
    );
    doc.begin_live();
    doc.extend_live(&[
        StrokePoint::new(200.0, 100.0, 1.0, 0),
        StrokePoint::new(600.0, 500.0, 1.0, 60),
    ]);
    doc.end_live();
    let layer = doc.image();
    let base = layer.pixels().chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(base > 500, "layer must hold the stroke, got {base}");
    let scaled = layer.resized(295, 525);
    let kept = scaled.pixels().chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(kept > 100, "thumbnail must keep ink, got {kept}");
}

#[test]
fn a11y_nodes_cover_every_screen() {
    // 每个屏幕的可点目标数量与视觉按钮一致，且标签非空。
    let mut ink = bare_ink();
    let lib = ink.a11y_nodes();
    // 新建 + 4 个筛选 chip + 4 张卡（第 4 张是空卡，同样指向新建）
    assert_eq!(lib.len(), 9);
    assert!(lib.iter().any(|n| n.1 == "新建画布"));
    for label in ["全部", "收藏", "草稿", "已导出", "示例作品 1", "示例作品 3"] {
        assert!(lib.iter().any(|n| n.1 == label), "作品库缺少 {label}");
    }

    ink.screen = Screen::Board;
    let board = ink.a11y_nodes();
    // 返回/撤/重/导出/笔刷信息/图层/尺寸条 + 5 工具 + 4 快捷笔刷 + 8 色板
    assert_eq!(board.len(), 24);
    for label in [
        "撤销",
        "导出 PNG",
        "画笔",
        "移动画布",
        "切换图层",
        "画布尺寸",
    ] {
        assert!(board.iter().any(|n| n.1 == label), "缺少 {label}");
    }
    assert!(board.iter().all(|n| n.2 >= A11Y_TAG_BASE));
}

#[test]
fn a11y_click_walks_library_to_canvas() {
    let mut ink = bare_ink();
    let plus = ink.a11y_nodes()[0].2;
    assert!(ink.perform_a11y(plus));
    assert_eq!(ink.screen, Screen::NewCanvas);

    let start = ink
        .a11y_nodes()
        .into_iter()
        .find(|n| n.1 == "创建画布")
        .expect("新建页必须有开始绘画")
        .2;
    assert!(ink.perform_a11y(start));
    assert_eq!(ink.screen, Screen::Board);
    assert!(ink.doc.is_some());
}

#[test]
fn a11y_click_rejects_unknown_tag() {
    let mut ink = bare_ink();
    assert!(!ink.perform_a11y(0));
    assert!(!ink.perform_a11y(A11Y_TAG_BASE + 99));
    assert_eq!(ink.screen, Screen::Library);
}

#[test]
fn preview_cache_key_tracks_revision() {
    // 缩略图缓存必须按文档版本号失效，不能只看像素指针。
    let mut ink = bare_ink();
    // 与 `Demo::render` 一致：先按窗口尺寸同步视口，否则画板矩形高为 0，
    // 预览会被压成几十像素的缩略图，测不到真实链路。
    ink.resize(1080.0, 2400.0, 80.0, 100.0);
    ink.screen = Screen::NewCanvas;
    ink.perform(Action::StartDrawing);
    ink.screen = Screen::Board;
    let mut scene = Scene::new(1080, 2400);
    ink.render(&mut scene);
    let rev0 = ink.doc.as_ref().map(|d| d.revision()).unwrap_or(0) as usize;
    assert_eq!(ink.preview.as_ref().expect("preview built").0, rev0);

    if let Some(doc) = ink.doc.as_mut() {
        doc.brush = PRESETS[0].brush;
        doc.begin_live();
        doc.extend_live(&[
            StrokePoint::new(100.0, 100.0, 1.0, 0),
            StrokePoint::new(300.0, 300.0, 1.0, 50),
        ]);
        doc.end_live();
    }
    let rev1 = ink.doc.as_ref().map(|d| d.revision()).unwrap_or(0) as usize;
    assert!(rev1 > rev0, "revision must advance");
    let mut scene2 = Scene::new(1080, 2400);
    ink.render(&mut scene2);
    assert_eq!(
        ink.preview.as_ref().expect("preview rebuilt").0,
        rev1,
        "preview cache must follow revision"
    );
}

#[test]
fn board_preview_shows_new_strokes() {
    // 零拷贝预览路径（直接借用图层像素 + 就地补丁）必须把新笔迹真的画进预览图：
    // 任一环节（借用错层 / 补丁落错位 / 键判断错）都会让画板"看着像没画"。
    let mut ink = bare_ink();
    // 与 `Demo::render` 一致：先按窗口尺寸同步视口，否则画板矩形高为 0，
    // 预览会被压成几十像素的缩略图，测不到真实链路。
    ink.resize(1080.0, 2400.0, 80.0, 100.0);
    ink.screen = Screen::NewCanvas;
    ink.perform(Action::StartDrawing);
    ink.screen = Screen::Board;
    let mut scene = Scene::new(1080, 2400);
    ink.render(&mut scene);

    if let Some(doc) = ink.doc.as_mut() {
        doc.brush = PRESETS[0].brush;
        doc.begin_live();
        doc.extend_live(&[
            StrokePoint::new(200.0, 200.0, 1.0, 0),
            StrokePoint::new(500.0, 600.0, 1.0, 50),
            StrokePoint::new(800.0, 1000.0, 1.0, 100),
        ]);
        doc.end_live();
    }
    let mut scene2 = Scene::new(1080, 2400);
    ink.render(&mut scene2);
    let img = &ink.preview.as_ref().expect("preview built").3;
    let painted = img.pixels().chunks(4).filter(|p| p[3] > 0).count();
    assert!(
        painted > 500,
        "preview must contain the stroke, opaque pixels={painted}"
    );
}

#[test]
fn brush_sheet_fits_inside_panel() {
    // 面板内容（标题 + 2×4 预设 + 4 条滑杆）必须全部落在卡片内，
    // 否则最后一条滑杆会掉到屏幕外、无法拖动。
    let l = Layout::compute(1080.0, 120.0, 2280.0);
    let last = l.sh_sliders[3];
    assert!(
        last.y + last.h <= l.sh_card.y + l.sh_card.h,
        "slider 4 overflows panel: {last:?} vs card {:?}",
        l.sh_card
    );
    assert!(l.sh_card.y + l.sh_card.h <= l.bottom as f64);
    assert!(l.sh_presets[7].y + l.sh_presets[7].h < l.sh_sliders[0].y);
}

#[test]
fn layout_letterboxes_canvas_area() {
    let l = layout();
    assert!(l.bd_canvas.h > 0.0);
    assert!(l.bd_canvas.y >= l.top as f64);
    assert!(l.nc_start.y + l.nc_start.h <= l.bottom as f64);
}

#[test]
fn dock_sits_below_canvas_and_inside_screen() {
    let l = layout();
    // dock 不能压住画布：否则笔迹会被工具栏遮住。
    assert!(
        l.bd_canvas.y + l.bd_canvas.h < l.bd_info.y,
        "canvas {:?} overlaps dock {:?}",
        l.bd_canvas,
        l.bd_info
    );
    // 全部 dock 控件必须在屏幕内、且在色板/快捷行内不越界。
    for cell in l.bd_tools.iter() {
        assert!(cell.y >= l.bd_canvas.y);
        assert!(cell.y + cell.h <= l.bd_canvas.y + l.bd_canvas.h);
    }
    for cell in l.bd_shortcuts.iter() {
        assert!(cell.y >= l.bd_info.y);
    }
    let last = l.bd_shortcuts[3];
    assert!(last.y + last.h < l.bottom as f64, "shortcuts overflow dock");
    // 色板 8 格互不重叠且不越界。
    assert_eq!(l.bd_swatches.len(), 8);
    for w in l.bd_swatches.windows(2) {
        assert!(w[0].x + w[0].w <= w[1].x + 0.5);
    }
    assert!(l.bd_swatches[7].x + l.bd_swatches[7].w <= l.w as f64 + 0.5);
}

#[test]
fn tool_switch_toggles_eraser_preset() {
    let mut ink = bare_ink();
    ink.perform(Action::StartDrawing);
    ink.perform(Action::PickTool(2));
    assert!(ink.is_eraser(), "橡皮工具应切到橡皮预设");
    ink.perform(Action::PickTool(0));
    assert!(!ink.is_eraser(), "画笔工具应离开橡皮预设");
}

#[test]
fn dock_size_bar_drives_brush_size() {
    let mut ink = bare_ink();
    ink.perform(Action::StartDrawing);
    let l = layout();
    ink.apply_size(&l, l.bd_sizebar.x as f32 + dp(20.0));
    assert!((ink.brush().size - 1.0).abs() < 1.0, "左端 = 1px");
    ink.apply_size(&l, (l.bd_sizebar.x + l.bd_sizebar.w) as f32 - dp(20.0));
    assert!((ink.brush().size - 120.0).abs() < 1.0, "右端 = 120px");
}

#[test]
fn toggle_layer_cycles_and_bumps_revision() {
    let mut ink = bare_ink();
    ink.perform(Action::StartDrawing);
    let rev = ink.doc.as_ref().unwrap().revision();
    let active = ink.doc.as_ref().unwrap().active;
    ink.perform(Action::ToggleLayer);
    let doc = ink.doc.as_ref().unwrap();
    assert_ne!(doc.active, active, "应切到另一层");
    assert!(doc.revision() > rev, "换层要递增 revision 让缩略图失效");
}

#[test]
fn quick_brush_picks_expected_preset() {
    let mut ink = bare_ink();
    ink.perform(Action::StartDrawing);
    for (i, idx) in [2usize, 3, 4, 5].iter().enumerate() {
        ink.perform(Action::QuickBrush(i));
        assert_eq!(ink.preset_idx, *idx, "快捷笔刷 {i} 应选 PRESETS[{idx}]");
    }
    assert_eq!(ink.tool, 0, "选笔刷后应回到画笔工具");
}

#[test]
fn line_tool_commits_single_undo_step() {
    let mut ink = bare_ink();
    ink.perform(Action::StartDrawing);
    let before = ink.doc.as_ref().unwrap().steps();
    let l = layout();
    ink.perform(Action::PickTool(1));
    ink.line_start = Some((100.0, 100.0));
    ink.commit_line((100.0, 100.0), (600.0, 500.0));
    let doc = ink.doc.as_ref().unwrap();
    assert_eq!(doc.steps(), before + 1, "一条直线 = 一步撤销");
}

#[test]
fn pick_color_snaps_to_nearest_swatch() {
    let mut ink = bare_ink();
    ink.perform(Action::StartDrawing);
    // 先在画布上落一笔当前色（色板 0），再在笔迹中点取色。
    let l = layout();
    ink.color_idx = 0;
    ink.commit_line((40.0, 40.0), (900.0, 40.0));
    ink.color_idx = 3;
    ink.perform(Action::PickTool(3));
    // 笔迹在文档坐标 y=40；按当前缩放反解出屏幕坐标再取色。
    let doc = ink.doc.as_ref().unwrap();
    let (scale, ox, oy) = view_of(doc, l.bd_canvas, 1.0, (0.0, 0.0));
    let (cx, cy) = ((ox + 200.0 * scale) as f32, (oy + 40.0 * scale) as f32);
    ink.pick_color_at(&l, cx, cy);
    assert_eq!(ink.color_idx, 0, "取色应吸附回笔迹同款色板 0");
}

#[test]
fn filter_switches_and_keeps_cards_clickable() {
    let mut ink = bare_ink();
    for (i, label) in ["全部", "收藏", "草稿", "已导出"].iter().enumerate() {
        ink.perform(Action::PickFilter(i));
        assert_eq!(ink.filter_idx, i);
        assert_eq!(ink.a11y_label(Action::PickFilter(i)), *label);
    }
    // 无论怎么筛，卡片仍是可点目标（筛选只改显隐，不卸载节点）。
    let l = layout();
    assert_eq!(l.hits(Screen::Library, false).len(), 9);
}

#[test]
fn new_canvas_sliders_drive_default_brush() {
    let mut ink = bare_ink();
    ink.screen = Screen::NewCanvas;
    let l = layout();
    let row0 = l.nc_sliders[0];
    ink.apply_default(&l, 0, row0.x as f32 + dp(110.0));
    assert!((ink.brush().size - 1.0).abs() < 1.0, "左端 = 1px");
    ink.apply_default(&l, 0, (row0.x + row0.w) as f32 - dp(84.0));
    assert!((ink.brush().size - 120.0).abs() < 1.0, "右端 = 120px");
    ink.apply_default(
        &l,
        1,
        (l.nc_sliders[1].x + l.nc_sliders[1].w) as f32 - dp(84.0),
    );
    assert!(ink.brush().opacity > 0.95, "不透明度右端应接近 100%");
}

/// 动作编码必须全局唯一且可往返。
///
/// # 为什么值得单测
/// 无障碍点击是「标签 → code → 动作」的解码链路；一旦两组编码重叠，
/// 点击「画布尺寸 3」会被解码成「切换图层」这类无关动作，界面上表现为
/// 「点一下尺寸就退出新建页」。这类错误不会编译失败，只能靠编码表断言兜住。
#[test]
fn action_codes_roundtrip_uniquely() {
    let cases = [
        Action::CreateNew,
        Action::StartDrawing,
        Action::Back,
        Action::Undo,
        Action::Redo,
        Action::ToggleEraser,
        Action::OpenBrushSheet,
        Action::CloseBrushSheet,
        Action::OpenExport,
        Action::DoExport,
        Action::Finish,
        Action::ToggleLayer,
        Action::DragSize,
        Action::PickSize(0),
        Action::PickSize(1),
        Action::PickSize(2),
        Action::PickSize(3),
        Action::PickPaper(0),
        Action::PickPaper(1),
        Action::PickPaper(2),
        Action::PickPaper(3),
        Action::OpenSample(0),
        Action::OpenSample(1),
        Action::OpenSample(2),
        Action::PickPreset(0),
        Action::PickPreset(7),
        Action::PickColor(0),
        Action::PickColor(6),
        Action::DragSlider(0),
        Action::DragSlider(3),
        Action::QuickBrush(0),
        Action::QuickBrush(3),
        Action::PickTool(0),
        Action::PickTool(4),
        Action::PickFilter(0),
        Action::PickFilter(3),
        Action::DragDefault(0),
        Action::DragDefault(1),
    ];
    let mut seen = std::collections::BTreeMap::new();
    for a in cases {
        let c = a.code();
        if let Some(prev) = seen.insert(c, a) {
            panic!("编码冲突: {a:?} 与 {prev:?} 都用 {c}");
        }
        assert_eq!(Action::from_code(c), Some(a), "解码不一致: {a:?}");
    }
}

#[test]
fn hits_cover_each_screen() {
    let l = layout();
    assert!(l.hits(Screen::Library, false).len() >= 4);
    assert!(l.hits(Screen::NewCanvas, false).len() >= 6);
    assert!(l.hits(Screen::Board, false).len() >= 5);
    assert!(l.hits(Screen::BrushSheet, false).len() >= 13);
    assert!(l.hits(Screen::Export, false).len() >= 2);
}

#[test]
fn slider_value_roundtrips_params() {
    let mut b = Brush::preset(BrushKind::Pen);
    set_param(&mut b, PARAM_SIZE, 60.0);
    assert_eq!(b.size, 60.0);
    set_param(&mut b, PARAM_OPACITY, 40.0);
    assert!((b.opacity - 0.4).abs() < 1e-6);
    set_param(&mut b, PARAM_FLOW, 30.0);
    assert!((b.flow - 0.3).abs() < 1e-6);
    set_param(&mut b, PARAM_SMOOTHING, 70.0);
    assert!((b.smoothing - 0.7).abs() < 1e-6);
}

#[test]
fn start_drawing_creates_doc_with_selected_paper() {
    let mut ink = Inkflow::new(None, 1080.0, 2400.0, 120.0, 120.0);
    ink.size_idx = 1;
    ink.paper_idx = 3;
    ink.perform(Action::StartDrawing);
    assert_eq!(ink.screen, Screen::Board);
    let doc = ink.doc.as_ref().unwrap();
    assert_eq!(doc.width, 1440);
    assert_eq!(doc.paper, PAPERS[3].1);
}

#[test]
fn back_from_board_returns_to_library() {
    let mut ink = Inkflow::new(None, 1080.0, 2400.0, 120.0, 120.0);
    ink.perform(Action::StartDrawing);
    ink.perform(Action::Back);
    assert_eq!(ink.screen, Screen::Library);
}

#[test]
fn view_zoom_scales_and_centers() {
    let d = Document::new(
        1000,
        1000,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Brush::preset(BrushKind::Pen),
    );
    let canvas = r(0.0, 0.0, 1000.0, 800.0);
    let (s1, ox1, oy1) = view_of(&d, canvas, 1.0, (0.0, 0.0));
    let (s2, ox2, oy2) = view_of(&d, canvas, 2.0, (0.0, 0.0));
    assert!((s2 / s1 - 2.0).abs() < 0.01, "2× zoom 应放大一倍");
    // 放大后画面变宽 → 原点左移。
    assert!(ox2 < ox1);
    assert!(oy2 < oy1);
}

#[test]
fn pinch_is_consumed_and_zooms() {
    use crate::ui::input::{HistoricalSample, MotionEvent, PointerCoords};
    let pin = |x: f32, y: f32| PointerCoords {
        pointer_id: 1,
        x,
        y,
        pressure: 0.6,
    };
    let mut ink = bare_ink();
    ink.screen = Screen::NewCanvas;
    ink.perform(Action::StartDrawing);
    ink.screen = Screen::Board;
    // bare_ink 的 insets 是退化值（画布区高度 0），这里换成正常机身尺寸。
    ink.resize(1080.0, 2400.0, 120.0, 120.0);
    // 用真实画布区中心构造双指坐标：避免硬编码在 letterbox 之外。
    let l = ink.layout();
    let cx = (l.bd_canvas.x + l.bd_canvas.w * 0.5) as f32;
    let cy = (l.bd_canvas.y + l.bd_canvas.h * 0.5) as f32;

    // 第一指按下：起笔
    let down = MotionEvent {
        packed_action: 0,
        down_time: 0,
        event_time: 0,
        pointers: vec![PointerCoords {
            pointer_id: 0,
            x: cx - 50.0,
            y: cy,
            pressure: 0.6,
        }],
        historical: Vec::<HistoricalSample>::new(),
    };
    assert!(ink.on_touch(&down));

    // 第二指按下：放弃笔迹并进入导航
    let second = MotionEvent {
        packed_action: 5 | (1 << 8), // ACTION_POINTER_DOWN, index 1
        down_time: 0,
        event_time: 20,
        pointers: vec![
            PointerCoords {
                pointer_id: 0,
                x: cx - 50.0,
                y: cy,
                pressure: 0.6,
            },
            pin(cx + 50.0, cy),
        ],
        historical: Vec::<HistoricalSample>::new(),
    };
    assert!(ink.on_touch(&second));
    assert!(ink.is_navigating(), "双指按下后应进入导航");

    // 两指向外拉开：间距 100 → 200，zoom 应翻倍
    let move_out = MotionEvent {
        packed_action: 2, // ACTION_MOVE
        down_time: 0,
        event_time: 60,
        pointers: vec![
            PointerCoords {
                pointer_id: 0,
                x: cx - 100.0,
                y: cy,
                pressure: 0.6,
            },
            pin(cx + 100.0, cy),
        ],
        historical: Vec::<HistoricalSample>::new(),
    };
    assert!(ink.on_touch(&move_out));
    assert!(
        (ink.zoom - 2.0).abs() < 0.01,
        "间距翻倍应放大一倍，实际 {}",
        ink.zoom
    );
    assert!(
        !ink.doc.as_ref().map(|d| d.is_live()).unwrap_or(true),
        "导航开始必须放弃笔迹"
    );

    // 第一指抬起（另一指还按着）：导航态必须保留，否则最后的 UP 会被当成未消费
    let pointer_up = MotionEvent {
        packed_action: 6 | (1 << 8), // ACTION_POINTER_UP, index 1
        down_time: 0,
        event_time: 100,
        pointers: vec![
            PointerCoords {
                pointer_id: 0,
                x: cx - 100.0,
                y: cy,
                pressure: 0.6,
            },
            pin(cx + 100.0, cy),
        ],
        historical: Vec::<HistoricalSample>::new(),
    };
    ink.on_touch(&pointer_up);
    assert!(ink.is_navigating(), "中途 POINTER_UP 不得清掉导航态");

    // 抬指：必须返回「已消费」，上层才会重绘
    let up = MotionEvent {
        packed_action: 1, // ACTION_UP
        down_time: 0,
        event_time: 120,
        pointers: vec![PointerCoords {
            pointer_id: 0,
            x: cx - 100.0,
            y: cy,
            pressure: 0.6,
        }],
        historical: Vec::<HistoricalSample>::new(),
    };
    assert!(ink.on_touch(&up), "导航结束的 UP 必须算作已消费");
    assert!(!ink.is_navigating(), "抬指后退出导航");
}

#[test]
fn zoom_from_pinch_uses_total_span() {
    // 丢帧场景：只有按下与最后一帧，倍率必须仍按总间距变化算。
    assert!((zoom_from_pinch(1.0, 200.0, 600.0) - 3.0).abs() < 1e-4);
    assert!((zoom_from_pinch(1.0, 200.0, 280.0) - 1.4).abs() < 1e-4);
    assert!((zoom_from_pinch(1.0, 200.0, 100.0) - 0.5).abs() < 1e-4);
    // 间距退化时保持原缩放，不产生 NaN / 0。
    assert!((zoom_from_pinch(1.2, 0.5, 500.0) - 1.2).abs() < 1e-4);
    // 上限 3、下限 0.5。
    assert!((zoom_from_pinch(2.5, 100.0, 400.0) - 3.0).abs() < 1e-4);
}

#[test]
fn pan_for_anchor_keeps_focus_on_anchor() {
    // 围绕捏合焦点缩放：锚点必须始终停在手指中点下，不向画布中心漂移。
    let d = Document::new(
        1080,
        1920,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Brush::preset(BrushKind::Pen),
    );
    let canvas = r(42.0, 300.0, 996.0, 1452.0);
    let anchor = (300.0f32, 600.0f32);
    let focus = (420.0f32, 760.0f32);
    for zoom in [0.6f32, 1.0, 1.8, 3.0] {
        let pan = pan_for_anchor(d.width, d.height, canvas, zoom, anchor, focus);
        let (scale, ox, oy) = view_of(&d, canvas, zoom, pan);
        let sx = ox + anchor.0 as f64 * scale;
        let sy = oy + anchor.1 as f64 * scale;
        assert!(
            (sx - focus.0 as f64).abs() < 0.5 && (sy - focus.1 as f64).abs() < 0.5,
            "zoom {zoom}: 锚点落在 ({sx:.1},{sy:.1})，应停在 {:?}",
            focus
        );
    }
}

#[test]
fn zoom_beyond_preview_budget_still_scales() {
    // 竖版画布 1080×1920、画布区 996×1452：基准 scale≈0.756，
    // 2048px 位图预算只到 1.067，若用它限制缩放，放大 1.4 倍就卡死。
    let d = Document::new(
        1080,
        1920,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Brush::preset(BrushKind::Pen),
    );
    let canvas = r(42.0, 300.0, 996.0, 1452.0);
    let (s1, _, _) = view_of(&d, canvas, 1.0, (0.0, 0.0));
    let (s3, _, _) = view_of(&d, canvas, 3.0, (0.0, 0.0));
    assert!(
        (s3 / s1 - 3.0).abs() < 0.01,
        "zoom 3× 必须真的放大三倍：s1={s1:.3} s3={s3:.3}"
    );
    assert!(
        s3 > 2048.0 / d.height as f64,
        "显示尺度不得被 2048px 位图预算截断"
    );
    // 放大后画布中心仍映射到画布区中心，落笔不会偏。
    let mid = (
        (canvas.x + canvas.w * 0.5) as f32,
        (canvas.y + canvas.h * 0.5) as f32,
    );
    let (cx, cy) = to_canvas_of(&d, canvas, 3.0, (0.0, 0.0), mid.0, mid.1).expect("中心点在画布内");
    assert!(
        (cx - 540.0).abs() < 1.0 && (cy - 960.0).abs() < 1.0,
        "放大后中心映射偏移：({cx:.1},{cy:.1})"
    );
}

#[test]
fn to_canvas_roundtrips_at_any_zoom() {
    let d = Document::new(
        800,
        600,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Brush::preset(BrushKind::Pen),
    );
    let canvas = r(0.0, 0.0, 1080.0, 900.0);
    for zoom in [0.5f32, 1.0, 2.0] {
        let (scale, ox, oy) = view_of(&d, canvas, zoom, (0.0, 0.0));
        let expect_x = (ox + (d.width as f64 * 0.5) * scale) as f32;
        let expect_y = (oy + (d.height as f64 * 0.5) * scale) as f32;
        assert_eq!(
            to_canvas_of(&d, canvas, zoom, (0.0, 0.0), expect_x, expect_y),
            Some((d.width as f32 * 0.5, d.height as f32 * 0.5)),
            "zoom {zoom} 下中心点映射必须可逆"
        );
    }
}

#[test]
fn to_canvas_rejects_outside_points() {
    let d = Document::new(
        400,
        400,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        Brush::preset(BrushKind::Pen),
    );
    let canvas = r(0.0, 0.0, 800.0, 800.0);
    assert!(to_canvas_of(&d, canvas, 1.0, (0.0, 0.0), -50.0, 0.0).is_none());
    assert!(to_canvas_of(&d, canvas, 1.0, (0.0, 0.0), 400.0, 400.0).is_some());
}

#[test]
fn export_without_doc_sets_error() {
    let mut ink = Inkflow::new(None, 1080.0, 2400.0, 120.0, 120.0);
    ink.perform(Action::DoExport);
    assert!(ink.export_error.is_some());
}
