//! 画板屏：画布内容（letterbox + 缩放/平移）、左侧工具条、图层按钮、zoom 徽标。
//!
//! 同时承载「屏幕坐标 ↔ 文档像素」的换算，以及双指缩放的锚点数学：
//! 落笔坐标必须与渲染用的变换完全同源，否则缩放后笔迹会错位。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::gfx::image::Image;
use crate::gfx::paint::Paint;
use crate::gfx::scene::{Rect, Scene};
use crate::paint::Document;
use crate::paint::PRESETS;
use crate::text::layout::TextAlign;
use crate::ui::dimen::{dp, sp};
use crate::ui::input::MotionEvent;

use super::layout::Layout;
use super::theme::*;
use super::{Inkflow, PAPERS, SWATCHES, Screen};

impl Inkflow {
    /// 画板页（不含笔刷面板）。
    pub(super) fn draw_board(&mut self, c: &mut Canvas, scene: &mut Scene, l: &Layout) {
        let brush = self.brush();
        let preset = PRESETS[self.preset_idx.min(PRESETS.len() - 1)];
        // 顶栏：返回 + 作品名 + 尺寸/图层副标 + 撤/重 + 导出。
        fill_round(c, l.bd_back, dp(22.0), surface_hi());
        self.text_center(scene, "‹", l.bd_back, sp(20.0), text_main());
        let sub = self
            .doc
            .as_ref()
            .map(|d| {
                format!(
                    "{}×{} · {} 图层",
                    d.width,
                    d.height,
                    d.layers.iter().filter(|v| v.visible).count().max(1)
                )
            })
            .unwrap_or_else(|| "未命名".to_string());
        self.text_block(
            scene,
            "鸢尾花 · 01",
            l.bd_title.x as f32,
            l.top + dp(10.0),
            sp(16.0),
            text_main(),
            TextAlign::Left,
            l.bd_title.w as f32,
        );
        self.text_block(
            scene,
            &sub,
            l.bd_title.x as f32,
            l.top + dp(30.0),
            sp(10.5),
            text_faint(),
            TextAlign::Left,
            l.bd_title.w as f32,
        );

        let can_undo = self.doc.as_ref().is_some_and(|d| d.history.can_undo());
        let can_redo = self.doc.as_ref().is_some_and(|d| d.history.can_redo());
        fill_round(c, l.bd_undo, dp(20.0), surface_hi());
        fill_round(c, l.bd_redo, dp(20.0), surface_hi());
        self.text_center(
            scene,
            "撤",
            l.bd_undo,
            sp(13.0),
            if can_undo { text_main() } else { text_faint() },
        );
        self.text_center(
            scene,
            "重",
            l.bd_redo,
            sp(13.0),
            if can_redo { text_main() } else { text_faint() },
        );
        fill_round(c, l.bd_export, dp(18.0), violet());
        self.text_center(
            scene,
            "导出",
            l.bd_export,
            sp(12.5),
            Color {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF,
                a: 255,
            },
        );

        // 画布：深色底 + 纸色 letterbox + 笔迹。
        let paper = self
            .doc
            .as_ref()
            .map(|d| d.paper)
            .unwrap_or(PAPERS[self.paper_idx].1);
        fill_round(
            c,
            l.bd_canvas,
            dp(4.0),
            Color {
                r: 0x0A,
                g: 0x08,
                b: 0x10,
                a: 255,
            },
        );
        self.draw_doc_view(c, l.bd_canvas, self.zoom, self.pan, paper, true, false);

        // 笔刷面板铺满下半屏，覆盖 dock 与工具栏，先返回避免重复绘制。
        if self.screen == Screen::BrushSheet {
            return;
        }

        // 画布浮层 1：左侧竖排工具（笔/直线/橡皮/取色/平移）。
        let tool_labels = ["笔", "线", "擦", "滴", "移"];
        for (i, cell) in l.bd_tools.iter().enumerate() {
            let on = self.tool == i;
            let bg = if on {
                Color {
                    r: 0x2E,
                    g: 0x24,
                    b: 0x50,
                    a: 235,
                }
            } else {
                Color {
                    r: 0x1A,
                    g: 0x16,
                    b: 0x24,
                    a: 210,
                }
            };
            fill_round(c, *cell, dp(20.0), bg);
            self.text_center(
                scene,
                tool_labels[i],
                *cell,
                sp(12.0),
                if on { violet() } else { text_dim() },
            );
        }

        // 画布浮层 2：右上「图层」按钮（点按切到下一层）。
        let active = self.doc.as_ref().map(|d| d.active + 1).unwrap_or(1);
        let layers = self.doc.as_ref().map(|d| d.layers.len()).unwrap_or(1);
        fill_round(
            c,
            l.bd_layers,
            dp(12.0),
            Color {
                r: 0x1A,
                g: 0x16,
                b: 0x24,
                a: 215,
            },
        );
        self.text_center(
            scene,
            &format!("图{}", active),
            l.bd_layers,
            sp(11.5),
            text_main(),
        );

        // 画布浮层 3：右下缩放徽标，跟手指缩放实时变化。
        fill_round(
            c,
            l.bd_zoom,
            dp(13.0),
            Color {
                r: 0x1A,
                g: 0x16,
                b: 0x24,
                a: 200,
            },
        );
        self.text_center(
            scene,
            &format!("zoom {:.0}%", self.zoom * 100.0),
            l.bd_zoom,
            sp(10.5),
            text_dim(),
        );

        if layers == 0 {
            return;
        }

        // dock：信息行 → 尺寸条 → 色板 → 快捷笔刷。
        fill_round(
            c,
            r(
                l.bd_info.x as f32 - dp(8.0),
                l.bd_info.y as f32,
                l.bd_info.w as f32 + dp(16.0),
                l.bd_info.h as f32 + dp(132.0),
            ),
            dp(20.0),
            Color {
                r: 0x16,
                g: 0x12,
                b: 0x1E,
                a: 245,
            },
        );

        // 信息行：色点 + 笔刷名 + 参数副标 + 尺寸胶囊。
        let dot = dp(34.0) as f64;
        let dcx = (l.bd_info.x as f32 + dp(20.0)) as f64;
        let dcy = (l.bd_info.y as f32 + dp(20.0)) as f64;
        fill_circle(
            c,
            dcx,
            dcy,
            dot,
            Color {
                r: 0x1A,
                g: 0x16,
                b: 0x24,
                a: 255,
            },
        );
        fill_circle(
            c,
            dcx,
            dcy,
            dp(9.0) as f64,
            SWATCHES[self.color_idx.min(SWATCHES.len() - 1)],
        );
        let name_x = l.bd_info.x as f32 + dp(46.0);
        self.text_block(
            scene,
            preset.label,
            name_x as f32,
            l.bd_info.y as f32 + dp(4.0),
            sp(14.0),
            text_main(),
            TextAlign::Left,
            (l.bd_sizepill.x as f32 - name_x - dp(8.0)).max(0.0),
        );
        self.text_block(
            scene,
            &format!(
                "不透明度 {:.0}% · 平滑 {:.0}%",
                brush.opacity * 100.0,
                brush.smoothing * 100.0
            ),
            name_x as f32,
            l.bd_info.y as f32 + dp(24.0),
            sp(10.0),
            text_faint(),
            TextAlign::Left,
            (l.bd_sizepill.x as f32 - name_x - dp(8.0)).max(0.0),
        );
        fill_round(
            c,
            l.bd_sizepill,
            dp(14.0),
            Color {
                r: 0x2A,
                g: 0x22,
                b: 0x3C,
                a: 255,
            },
        );
        self.text_center(
            scene,
            &format!("{:.0} px", brush.size),
            l.bd_sizepill,
            sp(12.0),
            text_main(),
        );

        // 尺寸滑杆：1 ──●── 120。
        let bar = l.bd_sizebar;
        let track_y = (bar.y + bar.h / 2.0) as f64;
        let tx0 = (bar.x as f32 + dp(20.0)) as f64;
        let tx1 = (bar.x as f32 + bar.w as f32 - dp(20.0)) as f64;
        fill_round(
            c,
            r(bar.x as f32, (track_y - 1.5) as f32, bar.w as f32, dp(3.0)),
            dp(2.0),
            surface_hi(),
        );
        let t = ((brush.size - 1.0) / 119.0).clamp(0.0, 1.0) as f64;
        let kx = tx0 + (tx1 - tx0) * t;
        fill_circle(c, kx, track_y, dp(8.0) as f64, violet());
        self.text_block(
            scene,
            "1",
            bar.x as f32 + dp(2.0),
            track_y as f32 - dp(8.0),
            sp(9.5),
            text_faint(),
            TextAlign::Left,
            dp(16.0) as f32,
        );
        self.text_block(
            scene,
            "120",
            bar.x as f32 + bar.w as f32 - dp(22.0),
            track_y as f32 - dp(8.0),
            sp(9.5),
            text_faint(),
            TextAlign::Left,
            dp(20.0) as f32,
        );

        // 色板：7 色 + 末尾「＋」开笔刷面板。
        for (i, cell) in l.bd_swatches.iter().enumerate() {
            let cx = (cell.x + cell.w / 2.0) as f64;
            let cy = (cell.y + cell.h / 2.0) as f64;
            let r0 = cell.w as f64 / 2.0;
            if i < SWATCHES.len() {
                fill_circle(c, cx, cy, r0, SWATCHES[i]);
                if i == self.color_idx {
                    fill_circle(c, cx, cy, r0 + dp(3.0) as f64, violet());
                    fill_circle(c, cx, cy, r0, SWATCHES[i]);
                }
            } else {
                fill_circle(
                    c,
                    cx,
                    cy,
                    r0,
                    Color {
                        r: 0x2A,
                        g: 0x22,
                        b: 0x3C,
                        a: 255,
                    },
                );
                self.text_center(scene, "＋", *cell, sp(15.0), text_dim());
            }
        }

        // 快捷笔刷 4 个：铅笔 / 软笔 / 喷枪 / 水彩。
        for (i, cell) in l.bd_shortcuts.iter().enumerate() {
            let idx = [2usize, 3, 4, 5][i];
            let on = self.preset_idx == idx && !self.is_eraser();
            fill_round(
                c,
                *cell,
                dp(14.0),
                if on {
                    Color {
                        r: 0x2E,
                        g: 0x24,
                        b: 0x50,
                        a: 255,
                    }
                } else {
                    Color {
                        r: 0x22,
                        g: 0x1C,
                        b: 0x30,
                        a: 255,
                    }
                },
            );
            self.text_center(
                scene,
                PRESETS[idx].label,
                *cell,
                sp(11.5),
                if on { violet() } else { text_dim() },
            );
        }
    }
    /// 声明本帧脏区：只在「画板静止 + 唯一变化是刚落下的一小段笔迹」时成立。
    ///
    /// # 为什么可以只报笔迹矩形
    /// 该帧其余像素与上一帧逐位相同：没有页面/面板动效、没有按压高亮与脉冲、
    /// 没有提示条与撤销闪烁，dock 与顶栏的数值本轮未变。因此后端只重绘这一块
    /// 就是最终画面，不需要整屏校验。
    ///
    /// # 保守条件
    /// 任一条件不满足就直接放弃声明（保持 `None` = 整帧），宁可多画一帧，
    /// 也不冒「留下陈旧像素」的风险。`from_rev` 必须等于上一次出帧时的
    /// revision，才能保证这中间没有别的帧改变过画面。
    fn note_stroke_damage(
        &mut self,
        dirty: Option<(u32, (u32, u32, u32, u32))>,
        ox: f64,
        oy: f64,
        k: f64,
    ) {
        // 诊断：整帧兜底的原因计数（只看有效的「落笔帧」为何没能局部化）。
        if let Some((from_rev, _)) = dirty {
            if from_rev == self.rendered_doc_rev {
                let reason = if self.screen != Screen::Board {
                    crate::platform::perf::DAMAGE_SCREEN
                } else if self.page_shift().abs() > 0.01 {
                    crate::platform::perf::DAMAGE_PAGE_SHIFT
                } else if self.sheet_p() > 0.001 {
                    crate::platform::perf::DAMAGE_SHEET
                } else if self.press_fade() > 0.01 {
                    crate::platform::perf::DAMAGE_PRESS
                } else if self.toast_p() > 0.01 {
                    crate::platform::perf::DAMAGE_TOAST
                } else if self.drag.is_some() {
                    crate::platform::perf::DAMAGE_DRAG
                } else if self.flash_p() > 0.01 {
                    crate::platform::perf::DAMAGE_FLASH
                } else {
                    crate::platform::perf::DAMAGE_NONE
                };
                crate::platform::perf::note_damage_reason(reason);
            }
        }
        if self.screen != Screen::Board
            || self.page_shift().abs() > 0.01
            || self.sheet_p() > 0.001
            || self.press_fade() > 0.01
            || self.toast_p() > 0.01
            || self.drag.is_some()
            || self.flash_p() > 0.01
        {
            return;
        }
        let Some((from_rev, (x0, y0, x1, y1))) = dirty else {
            return;
        };
        if from_rev != self.rendered_doc_rev {
            crate::platform::perf::note_damage_reason(
                crate::platform::perf::DAMAGE_REV_MISMATCH,
            );
            return;
        }
        // 预览位图以 k 倍铺到屏幕上，再留 2px + 抗锯齿余量。
        let pad = 2.0 * k + 1.0;
        let dx0 = ox + f64::from(x0) * k - pad;
        let dy0 = oy + f64::from(y0) * k - pad;
        let dx1 = ox + f64::from(x1) * k + pad;
        let dy1 = oy + f64::from(y1) * k + pad;
        let new = r(
            dx0 as f32,
            dy0 as f32,
            (dx1 - dx0).max(1.0) as f32,
            (dy1 - dy0).max(1.0) as f32,
        );
        self.frame_damage = Some(match self.frame_damage {
            None => new,
            Some(d) => {
                let ux0 = d.x.min(new.x);
                let uy0 = d.y.min(new.y);
                let ux1 = (d.x + d.w).max(new.x + new.w);
                let uy1 = (d.y + d.h).max(new.y + new.h);
                r(ux0 as f32, uy0 as f32, (ux1 - ux0) as f32, (uy1 - uy0) as f32)
            }
        });
    }

    /// 把文档画进指定矩形：纸色底 + 当前层笔迹，超出矩形裁剪。
    ///
    /// # 缓存
    /// 按「层像素指针 + 显示尺寸」缓存缩放结果：连续绘制时层像素指针不变、
    /// 尺寸不变，因此只在落笔后重建一次；切到导出页尺寸不同会重建一次。
    pub(super) fn draw_doc_view(
        &mut self,
        c: &mut Canvas,
        rect: Rect,
        zoom: f32,
        pan: (f32, f32),
        paper: Color,
        clip: bool,
        opaque: bool,
    ) {
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        // 脏提示先取快照：后面为借用 `self` 的其他字段必须先结束对 doc 的借用。
        let dirty = doc.dirty_since();
        let (scale, ox, oy) = view_of(doc, rect, zoom, pan);
        let dw = doc.width as f64 * scale;
        let dh = doc.height as f64 * scale;
        let page = r(ox as f32, oy as f32, dw as f32, dh as f32);
        c.save();
        if clip {
            c.clip_rect(rect);
        }
        c.draw_rect(page, &Paint::default().color(paper));
        // 位图分辨率上限 2048px：超出后不再重采样，改为把位图整体放大绘制，
        // 保证放大的「尺度」不受渲染预算限制（放宽后画质略软，但手感连续）。
        let cap = 2048.0;
        let render_scale = scale
            .min(cap / doc.width as f64)
            .min(cap / doc.height as f64);
        let disp_w = (doc.width as f64 * render_scale).max(1.0) as u32;
        let disp_h = (doc.height as f64 * render_scale).max(1.0) as u32;
        let k = (scale / render_scale).max(1.0);
        let t_dv = std::time::Instant::now();
        // 版本号 + 显示尺寸做缓存键：版本号变化必然要重建，避免指针复用误判。
        let key = (doc.revision() as usize, disp_w, disp_h);
        let region = doc.dirty_since();
        // 预览图先取出：patch 期间还要借用 `doc`，不能同时持有 &mut self.preview。
        let mut prev = self.preview.take();
        // 不透明（导出/缩略）才需要合成；画板本体直接用当前层像素的只读借用，
        // 避免每帧为 `Document::image()` 的全图拷贝买单。
        let composite_needed = opaque;
        // 缓存可用只取决于「显示尺寸」：版本号每落一个点都会变，把它当命中条件
        // 会让增量补丁永不生效（每帧退回整帧重采样，实测 16–25ms/帧）。
        let cached_ok = prev
            .as_ref()
            .map(|(_, w, h, _)| *w == disp_w && *h == disp_h)
            .unwrap_or(false);
        if !cached_ok {
            // 键对不上（版本或显示尺寸变了）：丢弃旧图，走整帧重建。
            prev = None;
        } else {
            // 增量路径：脏提示起点正好等于缓存版本且尺寸未变 → 只补脏区像素。
            let can_patch = !composite_needed;
            let patched = match (&mut prev, region) {
                (Some((prev_rev, pw, ph, img)), Some((from, rect)))
                    if can_patch
                        && *pw == disp_w
                        && *ph == disp_h
                        && *prev_rev == from as usize =>
                {
                    match img.pixels_mut() {
                        Some(out) => {
                            let written = Image::resample_region_from(
                                doc.layer_pixels(),
                                doc.width,
                                doc.height,
                                disp_w,
                                disp_h,
                                out,
                                rect,
                            );
                            // 把「这次只动了哪一块」告诉下游：预乘位图缓存据此
                                // 只重算该窗口，省掉每帧整幅 4MB 的转换（约 2.7ms）。
                            if let Some(w) = written {
                                img.note_patch(w);
                            }
                            // 预览已补齐到当前版本：把键推进到本帧版本，
                            // 下一帧脏提示起点与它对齐才能继续走增量路径。
                            *prev_rev = key.0;
                            true
                        }
                        // 像素仍被上一帧 `Scene` 共享：退回整帧重建（结果一致）。
                        None => false,
                    }
                }
                _ => false,
            };
            if !patched {
                // 同为「缓存已过期」的路径，交给下面的整帧重建统一处理。
                prev = None;
            }
        }
        if prev.is_none() {
            let t_preview = std::time::Instant::now();
            let scaled = if composite_needed {
                doc.composite().resized(disp_w, disp_h)
            } else {
                Image::resized_from(
                    doc.layer_pixels(),
                    doc.width,
                    doc.height,
                    disp_w,
                    disp_h,
                )
            };
            crate::platform::perf::add_build_sub_us(
                crate::platform::perf::BUILD_SUB_PREVIEW,
                t_preview.elapsed(),
            );
            prev = Some((key.0, key.1, key.2, scaled));
        }
        self.preview = prev;
        if let Some((_, _, _, img)) = self.preview.as_ref() {
            // 位图按 k 整体铺开：不做重采样，缩放因此不受位图分辨率限制。
            c.draw_image_scaled(img, ox, oy, k);
        }
        // 落笔期的局部脏区声明（仅「纯笔迹、无其他变化」时成立）。
        self.note_stroke_damage(dirty, ox, oy, k);
        c.restore();
        crate::platform::perf::add_build_sub_us(
            crate::platform::perf::BUILD_SUB_DOCVIEW,
            t_dv.elapsed(),
        );
    }
}

/// 画板内正在拖动的控件：笔刷面板滑杆或 dock 尺寸条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Drag {
    /// 笔刷面板第 i 条滑杆（粗细/不透明度/流量/平滑）。
    Slider(usize),
    /// dock 里的画布尺寸滑杆（1–120px）。
    Size,
    /// 新建页迷你滑杆（0 默认笔刷 / 1 默认不透明度）。
    Default(usize),
}

/// 捏合缩放：以按下瞬间为基准，用两指间距的总变化量算倍率。
pub(super) fn zoom_from_pinch(zoom0: f32, dist0: f32, dist_now: f32) -> f32 {
    if dist0 <= 1.0 {
        return zoom0.clamp(0.5, 3.0);
    }
    (zoom0 * (dist_now / dist0)).clamp(0.5, 3.0)
}

/// 让画布锚点始终停在手指焦点下：由缩放反解平移量。
///
/// 这是「围绕捏合中心缩放」的手感来源；缺了它，放大时锚点会向画布中心漂移，
/// 正在看的内容会跑出视野。
pub(super) fn pan_for_anchor(
    doc_w: u32,
    doc_h: u32,
    canvas: Rect,
    zoom: f32,
    anchor: (f32, f32),
    focus: (f32, f32),
) -> (f32, f32) {
    let scale = (base_scale(doc_w, doc_h, canvas) * zoom as f64).max(0.02);
    let dw = doc_w as f64 * scale;
    let dh = doc_h as f64 * scale;
    let base_ox = canvas.x + (canvas.w - dw) / 2.0;
    let base_oy = canvas.y + (canvas.h - dh) / 2.0;
    let px = focus.0 as f64 - anchor.0 as f64 * scale - base_ox;
    let py = focus.1 as f64 - anchor.1 as f64 * scale - base_oy;
    (px as f32, py as f32)
}
/// 画布塞进画布区的基准缩放（letterbox）。`view_of` 与锚点反解共用同一基准。
pub(super) fn base_scale(doc_w: u32, doc_h: u32, canvas: Rect) -> f64 {
    (canvas.w / doc_w as f64).min(canvas.h / doc_h as f64)
}
pub(super) fn view_of(doc: &Document, canvas: Rect, zoom: f32, pan: (f32, f32)) -> (f64, f64, f64) {
    // 基准：整张画布塞进画布区（letterbox）；zoom 再乘上去。
    //
    // 这里只算「显示尺度」，不受位图上限约束：缩放必须能一路放到底，
    // 位图渲染精度的妥协放在 `draw_doc_view` 内部（超出上限后原位图放大绘制）。
    // 否则 zoom 到 1.2 左右画面就卡住不动，手感断裂且落笔映射与所见不一致。
    let base = base_scale(doc.width, doc.height, canvas);
    let scale = (base * zoom as f64).max(0.02);
    let dw = doc.width as f64 * scale;
    let dh = doc.height as f64 * scale;
    let ox = canvas.x + (canvas.w - dw) / 2.0 + pan.0 as f64;
    let oy = canvas.y + (canvas.h - dh) / 2.0 + pan.1 as f64;
    (scale, ox, oy)
}
/// 屏幕坐标 → 画布像素；落在画布外返回 None。
pub(super) fn to_canvas_of(
    doc: &Document,
    canvas: Rect,
    zoom: f32,
    pan: (f32, f32),
    x: f32,
    y: f32,
) -> Option<(f32, f32)> {
    let (scale, ox, oy) = view_of(doc, canvas, zoom, pan);
    let cx = (x as f64 - ox) / scale;
    let cy = (y as f64 - oy) / scale;
    if cx < 0.0 || cy < 0.0 || cx > doc.width as f64 || cy > doc.height as f64 {
        return None;
    }
    Some((cx as f32, cy as f32))
}
/// 两指间距与中点；少于两指返回 None。
pub(super) fn two_finger(ev: &MotionEvent) -> Option<(f32, (f32, f32))> {
    if ev.pointer_count() < 2 {
        return None;
    }
    let a = ev.pointer(0)?;
    let b = ev.pointer(1)?;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dist = (dx * dx + dy * dy).sqrt().max(1.0);
    Some((dist, ((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)))
}
/// 事件压力：系统未上报时按 1.0 处理，避免笔迹骤细。
pub(super) fn pressure_of(ev: &MotionEvent) -> f32 {
    ev.primary().map(|p| p.pressure).unwrap_or(1.0)
}

/// 捏合手势的锚点状态：按下瞬间的快照 + 焦点下的画布锚点。
///
/// 缩放按「当前间距 / 起始间距」一次算到底（`zoom_from_pinch`），
/// 不逐帧累乘——系统合并/丢弃触摸帧时，逐帧累乘会把倍率算小。
#[derive(Clone, Copy, Debug)]
pub(super) struct Pinch {
    /// 按下瞬间两指间距（像素），用于算总倍率。
    pub(super) dist0: f32,
    /// 按下瞬间两指中点，锚点失效时退化为「跟随中点平移」。
    pub(super) mid0: (f32, f32),
    /// 按下瞬间的缩放与平移，作为无锚点时的基准。
    pub(super) zoom0: f32,
    pub(super) pan0: (f32, f32),
    /// 按下瞬间中点对应的画布像素；None 表示中点落在画布外。
    pub(super) anchor: Option<(f32, f32)>,
}
