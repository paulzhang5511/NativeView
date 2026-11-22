//! Inkflow 涂鸦 App 的页面层：五步流程（作品库 → 新建 → 画板 → 笔刷面板 → 导出）。
//!
//! # 为什么直接往 Scene 里画
//! 原型要求精确的深色卡片、圆角描边与 letterbox 画布，现有 widget 体系不含
//! 阴影/渐变细节；而画板手势必须拿原始 `MotionEvent` 坐标。这里沿用
//! `demo::draw_p2_overlay` 的做法：布局是纯函数（[`Layout`]），绘制与命中
//! 测试共用同一份矩形，保证「看到的位置」就是「点到的位置」。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::gfx::image::Image;
use crate::gfx::paint::Paint;
use crate::gfx::scene::{Rect, Scene};
use crate::paint::{Brush, BrushKind, Document, ExportInfo, PRESETS, StrokePoint};
use crate::text::font::FontData;
use crate::text::layout::TextContext;
use crate::ui::dimen::{dp, sp};

mod a11y;
mod board;
mod brush_params;
mod export;
mod input;
mod layout;
mod library;
mod motion;
mod new_canvas;
mod sheet;
mod text;
mod theme;

use self::board::{Drag, Pinch};
use self::layout::Layout;
use self::theme::*;

use super::inkflow_anim as anim;

/// 当前页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// 作品库。
    Library,
    /// 新建画布（尺寸/纸色）。
    NewCanvas,
    /// 画板。
    Board,
    /// 笔刷面板（底部半屏）。
    BrushSheet,
    /// 导出。
    Export,
}

/// 屏在导航栈里的层级（用于判定切屏方向：变大 = 前进，变小 = 返回）。
fn screen_rank(screen: Screen) -> u8 {
    match screen {
        Screen::Library => 0,
        Screen::NewCanvas => 1,
        Screen::Board => 2,
        // 笔刷面板是画板上的浮层，层级与画板相同（不走整页位移）。
        Screen::BrushSheet => 2,
        Screen::Export => 3,
    }
}

/// 动效里的选中脉冲类别（哪一组控件刚被改变）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pulse {
    /// 新建页尺寸卡。
    Size,
    /// 新建页纸色。
    Paper,
    /// 画板 / 面板色板。
    Color,
    /// 笔刷预设。
    Preset,
    /// 画板工具。
    Tool,
    /// 作品库筛选。
    Filter,
}

/// 画布尺寸档位：名称 + 宽高（像素）。
pub const SIZES: [(&str, u32, u32); 4] = [
    ("竖屏 1080×1920", 1080, 1920),
    ("方形 1440", 1440, 1440),
    ("A4 1280×1810", 1280, 1810),
    ("屏幕 1080×2340", 1080, 2340),
];

/// 导出格式行：名称 + 副标（目前只有 PNG 真正可产出）。
pub const FORMATS: [(&str, &str); 3] = [
    ("PNG 无损 · 透明", "支持透明通道"),
    ("JPEG 轻量 · 分享", "暂未实现"),
    ("PDF 矢量 · 打印", "暂未实现"),
];

/// 纸色档位。
pub const PAPERS: [(&str, Color); 4] = [
    (
        "暖白",
        Color {
            r: 0xF7,
            g: 0xF5,
            b: 0xEF,
            a: 255,
        },
    ),
    (
        "米黄",
        Color {
            r: 0xF3,
            g: 0xE6,
            b: 0xC8,
            a: 255,
        },
    ),
    (
        "冷灰",
        Color {
            r: 0xE6,
            g: 0xE9,
            b: 0xEE,
            a: 255,
        },
    ),
    (
        "深夜",
        Color {
            r: 0x1B,
            g: 0x1B,
            b: 0x22,
            a: 255,
        },
    ),
];

/// 色板（7 色）。
pub const SWATCHES: [Color; 7] = [
    Color {
        r: 0x2B,
        g: 0x2B,
        b: 0x33,
        a: 255,
    },
    Color {
        r: 0x7C,
        g: 0x5C,
        b: 0xFF,
        a: 255,
    },
    Color {
        r: 0xFF,
        g: 0x6F,
        b: 0xA5,
        a: 255,
    },
    Color {
        r: 0xFF,
        g: 0x9E,
        b: 0x4A,
        a: 255,
    },
    Color {
        r: 0x3D,
        g: 0xDC,
        b: 0x97,
        a: 255,
    },
    Color {
        r: 0x4A,
        g: 0xA8,
        b: 0xFF,
        a: 255,
    },
    Color {
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
        a: 255,
    },
];

/// 按钮/滑杆的语义动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 打开新建页。
    CreateNew,
    /// 选择画布尺寸档位。
    PickSize(usize),
    /// 选择纸色档位。
    PickPaper(usize),
    /// 用当前尺寸/纸色开始绘画。
    StartDrawing,
    /// 打开示例作品（默认尺寸）。
    OpenSample(usize),
    /// 返回上一页。
    Back,
    /// 撤销一笔。
    Undo,
    /// 重做一笔。
    Redo,
    /// 切换橡皮。
    ToggleEraser,
    /// 打开笔刷面板。
    OpenBrushSheet,
    /// 关闭笔刷面板。
    CloseBrushSheet,
    /// 选择笔刷预设。
    PickPreset(usize),
    /// 选择颜色。
    PickColor(usize),
    /// 打开导出页。
    OpenExport,
    /// 执行导出。
    DoExport,
    /// 导出完成后返回作品库。
    Finish,
    /// 滑杆拖动（参数下标）。
    DragSlider(usize),
    /// 画板工具栏切换（0 笔 / 1 直线 / 2 橡皮 / 3 取色 / 4 平移）。
    PickTool(usize),
    /// 切换活动图层。
    ToggleLayer,
    /// 画板内联快捷笔刷（铅笔/软笔/喷枪/水彩）。
    QuickBrush(usize),
    /// 画板内尺寸滑杆拖动。
    DragSize,
    /// 作品库筛选（全部/收藏/草稿/已导出）。
    PickFilter(usize),
    /// 新建页迷你滑杆拖动（默认笔刷 / 默认不透明度）。
    DragDefault(usize),
    /// 导出格式选择（PNG / JPEG / PDF）。
    PickFormat(usize),
}

/// 无障碍动作标签基址：Demo 全局动作标签占用 `<1000`，Inkflow 用 `2000+code`。
pub const A11Y_TAG_BASE: i32 = 2000;

/// 系统关闭动画（倍率 0）时单帧推进的动画时间：足够跨过所有单通道时长。
const STEP_WHEN_REMOVED: u64 = 3000;

impl Action {
    /// 动作编码（稳定整数，用于无障碍动作标签透传）。
    pub fn code(self) -> i32 {
        match self {
            Action::CreateNew => 0,
            Action::PickSize(i) => 10 + i as i32,
            Action::PickPaper(i) => 20 + i as i32,
            Action::StartDrawing => 1,
            Action::OpenSample(i) => 30 + i as i32,
            Action::Back => 2,
            Action::Undo => 3,
            Action::Redo => 4,
            Action::ToggleEraser => 5,
            Action::OpenBrushSheet => 6,
            Action::CloseBrushSheet => 7,
            Action::PickPreset(i) => 40 + i as i32,
            Action::PickColor(i) => 50 + i as i32,
            Action::OpenExport => 8,
            Action::DoExport => 9,
            Action::Finish => 90,
            Action::DragSlider(i) => 60 + i as i32,
            Action::PickTool(i) => 100 + i as i32,
            // 10..=13 被 PickSize 占用，标量动作放在 90 段，避免与参数段撞车。
            Action::ToggleLayer => 91,
            Action::QuickBrush(i) => 70 + i as i32,
            Action::DragSize => 92,
            Action::PickFilter(i) => 110 + i as i32,
            Action::DragDefault(i) => 93 + i as i32,
            Action::PickFormat(i) => 120 + i as i32,
        }
    }

    /// 由编码还原动作；非法编码返回 None（防止宿主传入脏值）。
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Action::CreateNew),
            1 => Some(Action::StartDrawing),
            2 => Some(Action::Back),
            3 => Some(Action::Undo),
            4 => Some(Action::Redo),
            5 => Some(Action::ToggleEraser),
            6 => Some(Action::OpenBrushSheet),
            7 => Some(Action::CloseBrushSheet),
            8 => Some(Action::OpenExport),
            9 => Some(Action::DoExport),
            90 => Some(Action::Finish),
            10..=13 => Some(Action::PickSize((code - 10) as usize)),
            20..=23 => Some(Action::PickPaper((code - 20) as usize)),
            30..=33 => Some(Action::OpenSample((code - 30) as usize)),
            40..=47 => Some(Action::PickPreset((code - 40) as usize)),
            50..=56 => Some(Action::PickColor((code - 50) as usize)),
            60..=63 => Some(Action::DragSlider((code - 60) as usize)),
            70..=73 => Some(Action::QuickBrush((code - 70) as usize)),
            100..=104 => Some(Action::PickTool((code - 100) as usize)),
            91 => Some(Action::ToggleLayer),
            92 => Some(Action::DragSize),
            93..=94 => Some(Action::DragDefault((code - 93) as usize)),
            110..=113 => Some(Action::PickFilter((code - 110) as usize)),
            120..=122 => Some(Action::PickFormat((code - 120) as usize)),
            _ => None,
        }
    }
}

/// Inkflow 全部可变状态。
pub struct Inkflow {
    /// 当前页。
    pub screen: Screen,
    text: Option<TextContext>,
    doc: Option<Document>,
    /// 作品库筛选下标（0 全部 / 1 收藏 / 2 草稿 / 3 已导出）。
    pub filter_idx: usize,
    /// 选择中的尺寸档位。
    pub size_idx: usize,
    /// 选择中的纸色档位。
    pub paper_idx: usize,
    /// 当前色板下标。
    pub color_idx: usize,
    /// 当前笔刷预设下标（`PRESETS` 下标；橡皮是独立开关）。
    pub preset_idx: usize,
    /// 滑杆改出的自定义参数；选预设时清空。
    custom_brush: Option<Brush>,
    /// 画布预览缓存：`(revision, 宽, 高, 图)`。
    preview: Option<(usize, u32, u32, Image)>,
    /// 本帧可声明的脏区（`Some` = 只有这块变了）。默认 `None`（整帧）。
    ///
    /// 只在「确定整帧其余像素不可能变」时由画板落笔路径填写：画面静止、
    /// 没有动效/面板/提示，且文档改动是刚发生的一小段笔迹。后端据此只重绘
    /// 这一小块，落笔帧的光栅成本从整屏降到笔迹附近。
    frame_damage: Option<Rect>,
    /// 上一次真正出帧时的文档 revision（校验脏提示链条是否连续）。
    rendered_doc_rev: u32,
    /// 画布视图缩放（1.0 = 整张塞满画布区）。
    pub zoom: f32,
    /// 画布视图平移（屏幕像素）。
    pub pan: (f32, f32),
    /// 双指导航状态：`(上一帧两指间距, 上一帧中点)`；None = 未进入缩放。
    pinch: Option<Pinch>,
    /// 本次手势是否用过双指导航（抬指收尾要据此算「已消费」）。
    nav_used: bool,
    /// 当前画板工具：0 笔 / 1 直线 / 2 橡皮 / 3 取色 / 4 平移。
    tool: usize,
    /// 直线工具起点（画布像素）。
    line_start: Option<(f32, f32)>,
    /// 平移工具的按下点与起始 pan。
    pan_drag: Option<(f32, f32, f32, f32)>,
    /// 笔刷面板预览缓存：参数指纹 → 预览图。
    preview_brush: Option<((u8, u32, u32, u32, u32), Image)>,
    /// 落笔点（画布像素）；抬手前一直保留。
    last_point: Option<StrokePoint>,
    /// 是否正在拖动滑杆。
    drag: Option<Drag>,
    /// 导出结果。
    pub exported: Option<ExportInfo>,
    /// 导出失败提示。
    pub export_error: Option<String>,
    /// 轻提示。
    pub toast: Option<String>,
    // ---- 动效状态（时间戳模型：None = 该动效未在进行） ----
    /// 单调动画时钟（毫秒），由宿主帧回调推进。
    clock: u64,
    /// 页面切换：`(起始时刻, 是否前进)`。
    page_anim: Option<(u64, bool)>,
    /// 笔刷面板开合：`(起始时刻, 是否打开, 起始展开度)`（可中途反向）。
    sheet_anim: Option<(u64, bool, f32)>,
    /// 按压高亮：`(命中矩形, 起始时刻, 所属屏)`（换屏后不再叠加）。
    press: Option<(Rect, u64, Screen)>,
    /// 选中脉冲：`(类别, 下标, 起始时刻)`。
    pulse: Option<(Pulse, usize, u64)>,
    /// toast 起始时刻（文本变化时刷新）。
    toast_at: u64,
    /// 上一次 toast 文本，用于探测新提示。
    toast_seen: Option<String>,
    /// 作品库卡片区入场/重入起始时刻（进入库页或切筛选时刷新）。
    cards_at: Option<u64>,
    filter_at: Option<u64>,
    /// 上一帧的真实时刻（ms）：动画时钟按「帧间隔 / 倍率」累积。
    last_now: u64,
    /// 系统动画倍率：1.0 正常；0 = 用户关闭动画（一帧直接落终态）；>1 为慢镜头。
    anim_scale: f32,
    /// 导出成功起始时刻（None = 从未导出）。
    export_at: Option<u64>,
    /// 画布内容变化时刻（撤销 / 重做 / 切图层）。
    flash_at: Option<u64>,
    width: f32,
    height: f32,
    insets_top: f32,
    insets_bottom: f32,
}

impl Inkflow {
    /// 新建（字体缺失时文字降级，图形仍可交互）。
    pub fn new(font: Option<FontData>, width: f32, height: f32, top: f32, bottom: f32) -> Self {
        let text = font.as_ref().map(|f| TextContext::new(f));
        Self {
            screen: Screen::Library,
            text,
            doc: None,
            size_idx: 0,
            paper_idx: 0,
            color_idx: 0,
            preset_idx: 0,
            custom_brush: None,
            preview: None,
            frame_damage: None,
            rendered_doc_rev: 0,
            zoom: 1.0,
            pan: (0.0, 0.0),
            pinch: None,
            nav_used: false,
            filter_idx: 0,
            tool: 0,
            line_start: None,
            pan_drag: None,
            preview_brush: None,
            last_point: None,
            drag: None,
            exported: None,
            export_error: None,
            toast: None,
            clock: 0,
            page_anim: None,
            sheet_anim: None,
            press: None,
            pulse: None,
            toast_at: 0,
            toast_seen: None,
            cards_at: None,
            filter_at: None,
            last_now: 0,
            anim_scale: 1.0,
            export_at: None,
            flash_at: None,
            width,
            height,
            insets_top: top,
            insets_bottom: bottom,
        }
    }

    /// 窗口尺寸或 insets 变化。
    pub fn resize(&mut self, width: f32, height: f32, top: f32, bottom: f32) {
        self.width = width;
        self.height = height;
        self.insets_top = top;
        self.insets_bottom = bottom;
    }

    /// 当前布局。
    pub fn layout(&self) -> Layout {
        Layout::compute(
            self.width,
            self.insets_top,
            self.height - self.insets_bottom,
        )
    }

    // ---- 动效：帧推进与进度 ----

    /// 手势被系统取消（窗口失焦/切页）：放弃当前笔迹与滑杆拖动。
    pub fn cancel_touch(&mut self) {
        if self.doc.as_ref().is_some_and(|d| d.is_live()) {
            if let Some(doc) = self.doc.as_mut() {
                doc.cancel_live();
            }
        }
        self.last_point = None;
        self.drag = None;
    }

    fn brush(&self) -> Brush {
        self.custom_brush
            .unwrap_or(PRESETS[self.preset_idx.min(PRESETS.len() - 1)].brush)
            .clamped()
    }

    fn is_eraser(&self) -> bool {
        self.brush().kind == BrushKind::Eraser
    }
}

// ---------------------------------------------------------------------------
// 几何换算（自由函数：绘制与手势共用，避免与 `&mut doc` 借用冲突）
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 绘制
// ---------------------------------------------------------------------------

impl Inkflow {
    /// 把当前页绘制到主场景（图形先进 Canvas，文字最后 emit）。
    pub fn render(&mut self, scene: &mut Scene) {
        // 动效层：整页位移用「平移后的几何」，命中测试仍用原始 layout()。
        let base = self.layout();
        let dy = self.page_shift();
        let l = if dy.abs() > 0.01 {
            base.shifted_y(dy)
        } else {
            base
        };
        // 图形与文字分两个 Scene：Canvas 命令必须整体排在文字之前，
        // 否则后追加的背景矩形会把已经 emit 的字形整片盖住。
        let mut c = Canvas::new(self.width.max(1.0) as u32, self.height.max(1.0) as u32);
        let mut labels = Scene::new(self.width.max(1.0) as u32, self.height.max(1.0) as u32);
        c.set_clear_color(bg());
        c.draw_rect(r(0.0, 0.0, l.w, self.height), &Paint::default().color(bg()));
        match self.screen {
            Screen::Library => self.draw_library(&mut c, &mut labels, &l),
            Screen::NewCanvas => self.draw_new_canvas(&mut c, &mut labels, &l),
            Screen::Board => self.draw_board(&mut c, &mut labels, &l),
            Screen::BrushSheet => {
                self.draw_board(&mut c, &mut labels, &l);
                // 压暗层在画板之上、面板之下：面板滑动时文字仍然可读。
                let scrim = anim::SHEET_SCRIM_ALPHA * self.sheet_p();
                if scrim > 0.01 {
                    veil(&mut c, l.w, self.height, 0x0D, scrim);
                }
                // 面板整体自底部滑入/滑出：位移量 = 面板高 × (1 - 展开度)。
                let dy_sheet = (1.0 - self.sheet_p()) * (l.bottom - l.sh_card.y as f32);
                let ls = if dy_sheet > 0.5 {
                    l.shifted_sheet_y(dy_sheet)
                } else {
                    l.clone()
                };
                self.draw_sheet(&mut c, &mut labels, &ls);
            }
            Screen::Export => self.draw_export(&mut c, &mut labels, &l),
        }
        // 画布内容闪烁（撤销 / 重做 / 切图层）：只压画布区域，不碰 dock 与顶栏。
        let fl = self.flash_p();
        if fl > 0.01 && self.screen == Screen::Board {
            veil_rect(&mut c, l.bd_canvas, 0x00, fl);
        }
        // 按压高亮与选中脉冲环：统一在此叠加，控件自身不必各自处理。
        self.draw_press(&mut c);
        self.draw_pulse_ring(&mut c, &l);
        if let Some(toast) = self.toast.clone() {
            // toast 固定贴顶栏下方：底栏区域在不同页面含义不同（dock / 导出按钮），
            // 放在底部会盖住真实的可点控件。
            let tp = self.toast_p();
            if tp > 0.01 {
                // 入场自上方 8dp 落入，结束后不再偏移（避免停留期抖动）。
                let dy_toast = anim::slide_in(
                    self.clock,
                    self.toast_at,
                    anim::TOAST_IN_MS,
                    dp(anim::TOAST_SHIFT_PX),
                );
                let bar = r(
                    dp(16.0),
                    l.top + dp(56.0) + dy_toast,
                    l.w - dp(32.0),
                    dp(44.0),
                );
                let a = (235.0 * tp) as u8;
                fill_round(
                    &mut c,
                    bar,
                    dp(12.0),
                    Color {
                        r: 0x33,
                        g: 0x2C,
                        b: 0x44,
                        a,
                    },
                );
                let mut fg = text_main();
                fg.a = (255.0 * tp) as u8;
                self.text_center(&mut labels, &toast, bar, sp(13.0), fg);
            }
        }
        let t_merge = std::time::Instant::now();
        let mut finished = c.finish();
        scene.commands.extend(finished.commands.drain(..));
        scene.commands.extend(labels.commands.drain(..));
        crate::platform::perf::add_build_sub_us(
            crate::platform::perf::BUILD_SUB_MERGE,
            t_merge.elapsed(),
        );
        // 脏区声明交给后端：`take` 保证「声明过一次」只作用于本帧。
        scene.damage = self.frame_damage.take();
        // 帧已出（含脏区部分重绘）：把文档版本推进到当前，并清脏提示链，
        // 让下一帧的脏区从这一刻重新累计（否则 from_rev 永远对不上）。
        if let Some(doc) = self.doc.as_ref() {
            self.rendered_doc_rev = doc.revision();
            doc.ack_dirty();
        }
        // 切屏遮罩必须最后画：它要盖住文字，否则「淡入」只对图形生效。
        let cover = self.page_cover();
        if cover > 0.01 {
            let mut veil_canvas =
                Canvas::new(self.width.max(1.0) as u32, self.height.max(1.0) as u32);
            veil(&mut veil_canvas, self.width, self.height, 0x12, cover);
            scene
                .commands
                .extend(veil_canvas.finish().commands.drain(..));
        }
    }
}

#[cfg(test)]
mod tests;
