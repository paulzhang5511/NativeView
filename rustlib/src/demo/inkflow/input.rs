//! 手势与动作分发：触摸事件 → 命中 → 状态变更 → 动效登记。
//!
//! 分工约定：
//! - `on_touch` 只做手势识别与几何换算，不直接改绘制结果；
//! - `dispatch` 是唯一的动作执行点，所有状态变更（含 UI 与文档）都从这里走；
//! - `perform` 是分发入口，负责在切屏前后处理 `on_screen_changed`。
//!
//! 关键约束：命中判定一律用「未位移的 layout」（`Layout::hits`），
//! 动画位移只影响绘制，绝不参与命中，否则按压中的行会移出可点区域。

use crate::paint::Document;
use crate::paint::brush::{BrushKind, PRESETS};
use crate::paint::export::export_png;
use crate::paint::stroke::StrokePoint;
use crate::ui::dimen::dp;
use crate::ui::input::{Action as TouchAction, MotionEvent};

use super::board::{
    Drag, Pinch, pan_for_anchor, pressure_of, to_canvas_of, two_finger, zoom_from_pinch,
};
use super::brush_params::*;
use super::layout::Layout;
use super::theme::has;
use super::{Action, Inkflow, PAPERS, Pulse, SIZES, SWATCHES, Screen, screen_rank};

impl Inkflow {
    /// 触摸入口：返回 true 表示需要重绘。
    ///
    /// # 手势优先级
    /// 画板内单指 = 落笔；第二根手指按下立刻放弃当前笔迹（不做缩放，
    /// V1 缩放留待下一增量），防止手掌误触画出长线。
    /// 是否正处于双指导航（捏合/平移）中。
    ///
    /// 上层据此在 MOVE 期间也重绘：缩放必须跟手，不能等抬指才跳一帧。
    pub(crate) fn is_navigating(&self) -> bool {
        self.pinch.is_some()
    }
    pub(crate) fn on_touch(&mut self, ev: &MotionEvent) -> bool {
        let l = self.layout();
        let primary = ev.primary().copied().unwrap_or_default();
        let (x, y) = (primary.x, primary.y);

        match ev.action() {
            TouchAction::Down => {
                self.toast = None;
                if self.screen == Screen::BrushSheet {
                    if let Some(i) = self.slider_at(&l, x, y) {
                        self.drag = Some(Drag::Slider(i));
                        self.apply_slider(&l, i, x);
                        return true;
                    }
                }
                // 浮层控件优先于落笔：工具条、图层、色板、尺寸条都压在画布上。
                if self.screen == Screen::NewCanvas {
                    if let Some(i) = l
                        .nc_sliders
                        .iter()
                        .position(|cell| has(*cell, x, y, dp(6.0)))
                    {
                        self.drag = Some(Drag::Default(i));
                        self.apply_default(&l, i, x);
                        return true;
                    }
                }
                if self.screen == Screen::Board && has(l.bd_sizebar, x, y, dp(6.0)) {
                    self.drag = Some(Drag::Size);
                    self.apply_size(&l, x);
                    return true;
                }
                if let Some(action) = self.hit(&l, x, y) {
                    // 按压反馈：记下命中的控件几何，绘制端统一叠一层衰减高亮。
                    if let Some((rect, _)) = l
                        .hits(self.screen, self.exported.is_some())
                        .into_iter()
                        .rev()
                        .find(|(rect, a)| *a == action && has(*rect, x, y, dp(4.0)))
                    {
                        self.press = Some((rect, self.clock, self.screen));
                    }
                    self.perform(action);
                    return true;
                }
                if self.screen == Screen::Board && has(l.bd_canvas, x, y, 0.0) {
                    // 工具语义：平移只挪视口、取色只采样、直线记录起点。
                    match self.tool {
                        4 => {
                            self.pan_drag = Some((x, y, self.pan.0, self.pan.1));
                            return true;
                        }
                        3 => {
                            self.pick_color_at(&l, x, y);
                            return true;
                        }
                        1 => {
                            self.line_start = self.doc.as_ref().and_then(|doc| {
                                to_canvas_of(doc, l.bd_canvas, self.zoom, self.pan, x, y)
                            });
                            return self.line_start.is_some();
                        }
                        _ => {}
                    }
                    let brush = self.brush();
                    let color = SWATCHES[self.color_idx.min(SWATCHES.len() - 1)];
                    let scaled = self
                        .doc
                        .as_ref()
                        .and_then(|doc| to_canvas_of(doc, l.bd_canvas, self.zoom, self.pan, x, y));
                    if let (Some((cx, cy)), Some(doc)) = (scaled, self.doc.as_mut()) {
                        doc.brush = brush;
                        doc.color = color;
                        doc.begin_live();
                        let p = StrokePoint::new(cx, cy, pressure_of(ev), 0);
                        doc.extend_live(&[p]);
                        self.last_point = Some(p);
                        return true;
                    }
                }
                false
            }
            TouchAction::PointerDown => {
                // 第二指：放弃当前笔迹并转入双指导航（缩放 + 平移）。
                if self.doc.as_ref().is_some_and(|d| d.is_live()) {
                    if let Some(doc) = self.doc.as_mut() {
                        doc.cancel_live();
                    }
                }
                self.last_point = None;
                self.nav_used = false;
                self.pinch = two_finger(ev).map(|(dist, mid)| {
                    let anchor = self.doc.as_ref().and_then(|doc| {
                        to_canvas_of(doc, l.bd_canvas, self.zoom, self.pan, mid.0, mid.1)
                    });
                    Pinch {
                        dist0: dist,
                        mid0: mid,
                        zoom0: self.zoom,
                        pan0: self.pan,
                        anchor,
                    }
                });
                if self.pinch.is_some() {
                    self.nav_used = true;
                }
                self.pinch.is_some()
            }
            TouchAction::Move => {
                match self.drag {
                    Some(Drag::Slider(i)) => {
                        self.apply_slider(&l, i, x);
                        return true;
                    }
                    Some(Drag::Size) => {
                        self.apply_size(&l, x);
                        return true;
                    }
                    Some(Drag::Default(i)) => {
                        self.apply_default(&l, i, x);
                        return true;
                    }
                    None => {}
                }
                if let Some((sx, sy, px, py)) = self.pan_drag {
                    self.pan = (px + (x - sx), py + (y - sy));
                    return true;
                }
                if self.line_start.is_some() {
                    return true;
                }
                if self.screen != Screen::Board {
                    return false;
                }
                // 多指：放弃当前笔迹，按两指间距/中点做缩放与平移。
                if ev.pointer_count() >= 2 {
                    if self.doc.as_ref().is_some_and(|d| d.is_live()) {
                        if let Some(doc) = self.doc.as_mut() {
                            doc.cancel_live();
                        }
                        self.last_point = None;
                    }
                    let Some(now) = two_finger(ev) else {
                        return false;
                    };
                    self.nav_used = true;
                    if let Some(p) = self.pinch {
                        // 缩放一次算到底；平移优先让锚点停在焦点下。
                        let zoom = zoom_from_pinch(p.zoom0, p.dist0, now.0);
                        self.zoom = zoom;
                        let (doc_w, doc_h) = self
                            .doc
                            .as_ref()
                            .map(|d| (d.width, d.height))
                            .unwrap_or((1, 1));
                        let (mut px, mut py) = match p.anchor {
                            Some(anchor) => {
                                pan_for_anchor(doc_w, doc_h, l.bd_canvas, zoom, anchor, now.1)
                            }
                            None => (p.pan0.0 + now.1.0 - p.mid0.0, p.pan0.1 + now.1.1 - p.mid0.1),
                        };
                        // 平移上限：画布最多拖出画布区 80%，避免整张画跑丢。
                        let max_pan_x = l.bd_canvas.w as f32 * 0.8;
                        let max_pan_y = l.bd_canvas.h as f32 * 0.8;
                        px = px.clamp(-max_pan_x, max_pan_x);
                        py = py.clamp(-max_pan_y, max_pan_y);
                        self.pan = (px, py);
                        log::debug!(
                            "pinch: dist0={:.0} dist={:.0} zoom={:.3} pan=({:.0},{:.0}) anchor={:?} focus=({:.0},{:.0})",
                            p.dist0,
                            now.0,
                            zoom,
                            px,
                            py,
                            p.anchor,
                            now.1.0,
                            now.1.1
                        );
                    }
                    return true;
                }
                self.pinch = None;
                let Some(prev) = self.last_point else {
                    return false;
                };
                let zoom = self.zoom;
                let pan = self.pan;
                let mut pts = Vec::with_capacity(ev.historical.len() + 1);
                {
                    let Some(doc) = self.doc.as_ref() else {
                        return false;
                    };
                    if !doc.is_live() {
                        return false;
                    }
                    for h in ev.historical.iter() {
                        if let Some(c) = h.coords.first() {
                            if let Some((hx, hy)) =
                                to_canvas_of(doc, l.bd_canvas, zoom, pan, c.x, c.y)
                            {
                                pts.push(StrokePoint::new(hx, hy, c.pressure, h.event_time));
                            }
                        }
                    }
                    if let Some((cx, cy)) = to_canvas_of(doc, l.bd_canvas, zoom, pan, x, y) {
                        pts.push(StrokePoint::new(cx, cy, pressure_of(ev), ev.event_time));
                    }
                }
                if pts.is_empty() {
                    return false;
                }
                let mut all = vec![prev];
                all.extend(pts.iter().copied());
                let dense = crate::paint::densify(&all, 2.0);
                let smoothing = self.doc.as_ref().map(|d| d.brush.smoothing).unwrap_or(0.3);
                let smoothed = crate::paint::smooth(&dense, smoothing);
                // 起点已在上一段画过，只提交新增点（含插值）。
                let tail = if smoothed.len() > 1 {
                    &smoothed[1..]
                } else {
                    &smoothed[..]
                };
                if let Some(doc) = self.doc.as_mut() {
                    doc.extend_live(tail);
                }
                self.last_point = pts.last().copied();
                true
            }
            TouchAction::Up | TouchAction::PointerUp => {
                // 曾处于双指导航：抬指必须算作「已消费」，否则上层不会重绘，
                // 画面会停在缩放前的旧帧（用户看到的是「捏合没反应」）。
                let was_navigating = self.nav_used;
                // 只有最后一指抬起才收尾：中途 POINTER_UP 若清掉导航态，
                // 随后的 ACTION_UP 会被当成「未消费」，画面就停在旧缩放帧。
                if matches!(ev.action(), TouchAction::Up) {
                    self.pinch = None;
                    self.nav_used = false;
                }
                if self.drag.take().is_some() {
                    return true;
                }
                if self.pan_drag.take().is_some() {
                    return true;
                }
                if self.line_start.is_some() {
                    if matches!(ev.action(), TouchAction::Up) {
                        if let Some(start) = self.line_start.take() {
                            if let Some(doc) = self.doc.as_ref() {
                                if let Some((ex, ey)) =
                                    to_canvas_of(doc, l.bd_canvas, self.zoom, self.pan, x, y)
                                {
                                    self.commit_line(start, (ex, ey));
                                }
                            }
                        }
                    }
                    return true;
                }
                if self.doc.as_ref().is_some_and(|d| d.is_live()) {
                    if let Some(doc) = self.doc.as_mut() {
                        doc.end_live();
                    }
                    self.last_point = None;
                    return true;
                }
                was_navigating
            }
            TouchAction::Cancel => {
                self.drag = None;
                self.pinch = None;
                self.nav_used = false;
                if self.doc.as_ref().is_some_and(|d| d.is_live()) {
                    if let Some(doc) = self.doc.as_mut() {
                        doc.cancel_live();
                    }
                }
                self.last_point = None;
                false
            }
            _ => false,
        }
    }
    /// 当前笔刷（含橡皮开关覆盖）。
    /// 命中动作；无命中返回 None。
    pub(super) fn hit(&self, l: &Layout, x: f32, y: f32) -> Option<Action> {
        l.hits(self.screen, self.exported.is_some())
            .into_iter()
            .rev()
            .find(|(rect, _)| has(*rect, x, y, dp(4.0)))
            .map(|(_, action)| action)
    }
    /// 滑杆命中。
    pub(super) fn slider_at(&self, l: &Layout, x: f32, y: f32) -> Option<usize> {
        l.sh_sliders
            .iter()
            .position(|cell| has(*cell, x, y, dp(6.0)))
    }
    /// 拖动 dock 尺寸条：把 0–1 位置映射到 1–120px，直接改写当前笔刷。
    pub(super) fn apply_size(&mut self, l: &Layout, x: f32) {
        let bar = l.bd_sizebar;
        let track_x = bar.x as f32 + dp(20.0);
        let track_w = (bar.w as f32 - dp(40.0)).max(1.0);
        let t = ((x - track_x) / track_w).clamp(0.0, 1.0);
        let mut brush = self.brush();
        brush.size =
            (1.0 + t * 119.0).clamp(crate::paint::brush::MIN_SIZE, crate::paint::brush::MAX_SIZE);
        self.custom_brush = Some(brush);
        self.toast = None;
    }
    /// 新建页迷你滑杆：0 = 默认笔刷尺寸，1 = 默认不透明度。
    pub(super) fn apply_default(&mut self, l: &Layout, i: usize, x: f32) {
        let cell = l.nc_sliders[i.min(1)];
        let track_x = cell.x as f32 + dp(110.0);
        let track_w = (cell.w as f32 - dp(190.0)).max(1.0);
        let t = ((x - track_x) / track_w).clamp(0.0, 1.0);
        let mut brush = self.brush();
        if i == 0 {
            brush.size = (1.0 + t * 119.0)
                .clamp(crate::paint::brush::MIN_SIZE, crate::paint::brush::MAX_SIZE);
        } else {
            brush.opacity = (t * 100.0).clamp(5.0, 100.0) / 100.0;
        }
        self.custom_brush = Some(brush);
        self.toast = None;
    }
    /// 取色工具：采样活动层像素，吸附到最近的调色板色。
    ///
    /// 空白处（alpha=0）只给提示，不改当前颜色，避免误把「没墨」当成黑色。
    pub(super) fn pick_color_at(&mut self, l: &Layout, x: f32, y: f32) {
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let Some((cx, cy)) = to_canvas_of(doc, l.bd_canvas, self.zoom, self.pan, x, y) else {
            return;
        };
        let img = doc.image();
        let (iw, ih) = (img.width(), img.height());
        let (ix, iy) = (cx as u32, cy as u32);
        if ix >= iw || iy >= ih {
            return;
        }
        let px = img.pixels();
        let o = ((iy * iw + ix) * 4) as usize;
        if px[o + 3] == 0 {
            self.toast = Some("这里是空白".into());
            return;
        }
        let mut best = 0usize;
        let mut best_d = f32::MAX;
        for (i, c) in SWATCHES.iter().enumerate() {
            let d = ((c.r as f32 - px[o] as f32).powi(2)
                + (c.g as f32 - px[o + 1] as f32).powi(2)
                + (c.b as f32 - px[o + 2] as f32).powi(2))
            .sqrt();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        self.color_idx = best;
        self.toast = Some(format!("已取色 · 色板 {}", best + 1));
    }
    /// 提交直线：按 2px 间隔铺点，一次压入历史（只产生一步撤销）。
    pub(super) fn commit_line(&mut self, start: (f32, f32), end: (f32, f32)) {
        let brush = self.brush();
        let color = SWATCHES[self.color_idx.min(SWATCHES.len() - 1)];
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1.0 {
            return;
        }
        let n = (len / 2.0).ceil().max(1.0) as usize;
        let pts: Vec<StrokePoint> = (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                StrokePoint::new(start.0 + dx * t, start.1 + dy * t, 1.0, i as i64 * 16)
            })
            .collect();
        if let Some(doc) = self.doc.as_mut() {
            doc.brush = brush;
            doc.color = color;
            doc.apply_stroke(&pts);
        }
        self.toast = None;
    }
    /// 拖动滑杆。
    pub(super) fn apply_slider(&mut self, l: &Layout, i: usize, x: f32) {
        let cell = l.sh_sliders[i];
        let track_x = cell.x as f32 + dp(76.0);
        let track_w = (cell.w as f32 - dp(140.0)).max(1.0);
        let t = ((x - track_x) / track_w).clamp(0.0, 1.0);
        let mut brush = self.brush();
        set_param(&mut brush, i, t * param_max(i));
        self.preset_idx = self.preset_idx.min(PRESETS.len() - 1);
        // 直接改当前预设模型下的笔刷参数（不新增预设）。
        self.custom_brush = Some(brush);
        self.toast = None;
    }
    /// 执行语义动作（对外唯一入口：手势与无障碍点击都走这里）。
    ///
    /// 切屏动效在此统一登记：内部 `dispatch` 只改状态，
    /// 方向由前后屏的导航层级推出，不要求每个分支自己记得打点。
    pub(super) fn perform(&mut self, action: Action) {
        let before = self.screen;
        self.dispatch(action);
        if self.screen != before {
            self.on_screen_changed(before);
        }
    }
    /// 切屏后的动效登记。
    pub(super) fn on_screen_changed(&mut self, before: Screen) {
        let now = self.clock;
        match (before, self.screen) {
            // 面板是浮层：只做滑入/滑出，不做整页位移。
            (_, Screen::BrushSheet) | (Screen::BrushSheet, _) => {
                // 起点取「切换前的展开度」：有进行中的动画就取当前值（可中途反向），
                // 静止时按切换前的屏判断（面板内 = 已展开，否则完全收起）。
                let from = match self.sheet_anim {
                    Some(_) => self.sheet_p(),
                    None if before == Screen::BrushSheet => 1.0,
                    None => 0.0,
                };
                let open = self.screen == Screen::BrushSheet;
                self.sheet_anim = Some((now, open, from));
            }
            _ => {
                let forward = screen_rank(self.screen) > screen_rank(before);
                self.page_anim = Some((now, forward));
                if self.screen == Screen::Library {
                    self.cards_at = Some(now);
                }
            }
        }
    }
    /// 动作的实际状态变更。
    pub(super) fn dispatch(&mut self, action: Action) {
        match action {
            Action::CreateNew => {
                self.screen = Screen::NewCanvas;
            }
            Action::PickSize(i) => {
                self.size_idx = i.min(SIZES.len() - 1);
                self.mark_pulse(Pulse::Size, self.size_idx);
            }
            Action::PickPaper(i) => {
                self.paper_idx = i.min(PAPERS.len() - 1);
                self.mark_pulse(Pulse::Paper, self.paper_idx);
            }
            Action::StartDrawing => {
                let (_, w, h) = SIZES[self.size_idx];
                let paper = PAPERS[self.paper_idx].1;
                self.doc = Some(Document::new(w, h, paper, self.brush()));
                self.preview = None;
                self.zoom = 1.0;
                self.pan = (0.0, 0.0);
                self.pinch = None;
                self.nav_used = false;
                self.screen = Screen::Board;
                self.exported = None;
                self.export_error = None;
            }
            Action::OpenSample(_) => {
                let (_, w, h) = SIZES[0];
                self.doc = Some(Document::new(w, h, PAPERS[0].1, self.brush()));
                self.preview = None;
                self.zoom = 1.0;
                self.pan = (0.0, 0.0);
                self.pinch = None;
                self.nav_used = false;
                self.screen = Screen::Board;
            }
            Action::Back => {
                self.screen = match self.screen {
                    Screen::NewCanvas => Screen::Library,
                    Screen::Board => Screen::Library,
                    Screen::BrushSheet => Screen::Board,
                    Screen::Export => Screen::Board,
                    Screen::Library => Screen::Library,
                };
            }
            Action::Undo => {
                if let Some(doc) = self.doc.as_mut() {
                    if doc.undo() {
                        // 内容变化闪烁：让「笔迹消失」有一帧视觉确认。
                        self.flash_at = Some(self.clock);
                    }
                }
            }
            Action::Redo => {
                if let Some(doc) = self.doc.as_mut() {
                    if doc.redo() {
                        self.flash_at = Some(self.clock);
                    }
                }
            }
            Action::ToggleEraser => {
                self.preset_idx = if self.is_eraser() {
                    self.preset_idx
                } else {
                    PRESETS
                        .iter()
                        .position(|p| p.brush.kind == BrushKind::Eraser)
                        .unwrap_or(7)
                };
            }
            Action::OpenBrushSheet => {
                self.screen = Screen::BrushSheet;
            }
            Action::CloseBrushSheet => {
                self.screen = Screen::Board;
            }
            Action::PickPreset(i) => {
                self.preset_idx = i.min(PRESETS.len() - 1);
                self.custom_brush = None;
                self.mark_pulse(Pulse::Preset, self.preset_idx);
            }
            Action::PickColor(i) => {
                self.color_idx = i.min(SWATCHES.len() - 1);
                self.mark_pulse(Pulse::Color, self.color_idx);
            }
            Action::OpenExport => {
                self.screen = Screen::Export;
                self.exported = None;
                self.export_error = None;
            }
            Action::PickFormat(i) => {
                // 引擎目前只产出 PNG，其余格式仅作占位，明确告知而不是假装可用。
                if i > 0 {
                    self.toast = Some("暂只支持 PNG 导出".into());
                }
            }
            Action::DoExport => {
                self.exported = None;
                self.export_error = None;
                let Some(doc) = self.doc.as_ref() else {
                    self.export_error = Some("没有可导出的画布".to_string());
                    return;
                };
                let dir = crate::platform::android::app_paths::exports_dir();
                let name = format!(
                    "inkflow-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                );
                match export_png(doc, &dir, &name) {
                    Ok(info) => {
                        self.toast = Some(format!("已导出 {:.1} KB", info.bytes as f64 / 1024.0));
                        self.exported = Some(info);
                        // 状态行与双按钮的入场动效从保存成功这一刻开始。
                        self.export_at = Some(self.clock);
                    }
                    Err(e) => {
                        self.export_error = Some(e.to_string());
                    }
                }
            }
            Action::Finish => {
                self.screen = Screen::Library;
                self.doc = None;
                self.preview = None;
            }
            Action::PickTool(i) => {
                let i = i.min(4);
                if i != self.tool {
                    self.mark_pulse(Pulse::Tool, i);
                }
                self.tool = i;
                // 工具与笔刷联动：橡皮工具切到橡皮预设，其它工具若还是橡皮则回到钢笔。
                if i == 2 {
                    if !self.is_eraser() {
                        self.preset_idx = PRESETS
                            .iter()
                            .position(|p| p.brush.kind == BrushKind::Eraser)
                            .unwrap_or(PRESETS.len() - 1);
                        self.custom_brush = None;
                    }
                } else if self.is_eraser() {
                    self.preset_idx = 0;
                    self.custom_brush = None;
                }
                self.line_start = None;
                self.pan_drag = None;
            }
            Action::ToggleLayer => {
                let switched = self.doc.as_mut().is_some_and(|doc| {
                    let n = doc.layers.len().max(1);
                    doc.set_active((doc.active + 1) % n)
                });
                if switched {
                    self.flash_at = Some(self.clock);
                }
            }
            Action::QuickBrush(i) => {
                // 快捷行：铅笔 / 软笔 / 喷枪 / 水彩（对应 PRESETS 2/3/4/5）。
                let idx = [2usize, 3, 4, 5][i.min(3)];
                self.preset_idx = idx.min(PRESETS.len() - 1);
                self.custom_brush = None;
                self.tool = 0;
            }
            Action::PickFilter(i) => {
                let i = i.min(3);
                if i != self.filter_idx {
                    self.mark_pulse(Pulse::Filter, i);
                    // 卡片区整块重入：短促上浮（FILTER_* 参数），与整屏入场区分开。
                    self.filter_at = Some(self.clock);
                }
                self.filter_idx = i;
                self.toast = Some(format!(
                    "筛选：{}",
                    ["全部", "收藏", "草稿", "已导出"][self.filter_idx]
                ));
            }
            Action::DragSlider(_) | Action::DragSize | Action::DragDefault(_) => {}
        }
    }
}
