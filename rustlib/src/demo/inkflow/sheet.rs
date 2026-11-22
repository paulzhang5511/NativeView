//! 笔刷面板屏：8 个预设、预览折线、4 条参数滑杆。
//!
//! 面板整体自底部滑入（动效在 `motion` 的 `sheet_p`），本模块只画面板本身；
//! 预览图按参数指纹缓存，拖滑杆逐帧重建。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::gfx::image::Image;
use crate::paint::{PRESETS, raster};

use crate::gfx::scene::Scene;
use crate::text::layout::TextAlign;
use crate::ui::dimen::{dp, sp};

use super::brush_params::*;
use super::layout::Layout;
use super::theme::*;
use super::{Inkflow, PAPERS, SWATCHES};

impl Inkflow {
    /// 笔刷面板（底部半屏）。
    pub(super) fn draw_sheet(&mut self, c: &mut Canvas, scene: &mut Scene, l: &Layout) {
        let card = l.sh_card;
        fill_round(
            c,
            card,
            dp(24.0),
            Color {
                r: 0x20,
                g: 0x1B,
                b: 0x2C,
                a: 250,
            },
        );
        stroke_round(
            c,
            r(card.x as f32, card.y as f32, card.w as f32, dp(1.0)),
            dp(24.0),
            dp(1.0),
            Color {
                r: 0x3A,
                g: 0x30,
                b: 0x52,
                a: 255,
            },
        );
        self.text_block(
            scene,
            "笔刷",
            card.x as f32 + dp(16.0),
            card.y as f32 + dp(28.0),
            sp(16.0),
            text_main(),
            TextAlign::Left,
            dp(120.0),
        );
        // 预览条：放在标题与「完成」之间，实时反映当前参数。
        let prev_rect = r(
            card.x as f32 + dp(96.0),
            card.y as f32 + dp(18.0),
            (card.w as f32 - dp(200.0)).max(dp(80.0)),
            dp(44.0),
        );
        stroke_round(
            c,
            prev_rect,
            dp(10.0),
            dp(1.0),
            Color {
                r: 0x3A,
                g: 0x30,
                b: 0x52,
                a: 255,
            },
        );
        let img = self.brush_preview_image(prev_rect.w as u32, prev_rect.h as u32);
        c.draw_image(&img, prev_rect.x, prev_rect.y);
        let close = l.sh_close;
        fill_round(c, close, dp(18.0), violet());
        self.text_center(scene, "完成", close, sp(12.0), Color::WHITE);

        for (i, cell) in l.sh_presets.iter().enumerate() {
            let selected = i == self.preset_idx;
            fill_round(
                c,
                *cell,
                dp(14.0),
                if selected {
                    Color {
                        r: 0x2E,
                        g: 0x24,
                        b: 0x50,
                        a: 255,
                    }
                } else {
                    surface()
                },
            );
            if selected {
                stroke_round(c, *cell, dp(14.0), dp(2.0), violet());
            }
            // 笔尖示意圆（半径反映预设粗细）。
            let dot_r = (PRESETS[i].brush.radius() as f64)
                .min((cell.h as f64) * 0.28)
                .max(4.0);
            fill_circle(
                c,
                cell.x + cell.w / 2.0,
                cell.y + cell.h as f64 * 0.38,
                dot_r,
                SWATCHES[(i + 1) % SWATCHES.len()],
            );
            self.text_center(
                scene,
                PRESETS[i].label,
                r(
                    cell.x as f32,
                    cell.y as f32 + cell.h as f32 - dp(24.0),
                    cell.w as f32,
                    dp(20.0),
                ),
                sp(10.5),
                if selected { text_main() } else { text_dim() },
            );
        }

        let brush = self.brush();
        for (i, cell) in l.sh_sliders.iter().enumerate() {
            let v = param_value(&brush, i);
            let t = (v / param_max(i)).clamp(0.0, 1.0);
            self.text_block(
                scene,
                PARAM_LABELS[i],
                cell.x as f32,
                cell.y as f32,
                sp(11.0),
                text_dim(),
                TextAlign::Left,
                dp(72.0),
            );
            let track = r(
                cell.x as f32 + dp(76.0),
                cell.y as f32 + dp(10.0),
                cell.w as f32 - dp(140.0),
                dp(6.0),
            );
            fill_round(
                c,
                track,
                dp(3.0),
                Color {
                    r: 0x3A,
                    g: 0x32,
                    b: 0x4C,
                    a: 255,
                },
            );
            let fill = r(
                track.x as f32,
                track.y as f32,
                track.w as f32 * t,
                track.h as f32,
            );
            fill_round(c, fill, dp(3.0), violet());
            let knob = r(
                track.x as f32 + track.w as f32 * t - dp(11.0),
                cell.y as f32 + dp(2.0),
                dp(22.0),
                dp(22.0),
            );
            fill_circle(
                c,
                (knob.x + knob.w / 2.0) as f64,
                (knob.y + knob.h / 2.0) as f64,
                dp(11.0) as f64,
                Color::WHITE,
            );
            self.text_block(
                scene,
                &param_text(&brush, i),
                cell.x as f32 + cell.w as f32 - dp(56.0),
                cell.y as f32 + dp(2.0),
                sp(11.0),
                text_main(),
                TextAlign::Right,
                dp(56.0),
            );
        }
    }
    /// 笔刷面板预览图：按当前参数在 surface 底色上画一段 S 形笔迹。
    ///
    /// # 缓存
    /// 参数指纹不变时复用同一张图；指纹由类型/粗细/不透明度/流量/平滑组成，
    /// 拖动滑杆会逐帧重建（尺寸仅一百多像素，代价可忽略）。
    pub(super) fn brush_preview_image(&mut self, pw: u32, ph: u32) -> Image {
        let (pw, ph) = (pw.max(1), ph.max(1));
        let brush = self.brush();
        let key = (
            brush.kind as u8,
            brush.size.round() as u32,
            (brush.opacity * 100.0).round() as u32,
            (brush.flow * 100.0).round() as u32,
            (brush.smoothing * 100.0).round() as u32,
        );
        if let Some((k, img)) = self.preview_brush.as_ref() {
            if *k == key {
                return img.clone();
            }
        }
        // 预览底色直接用当前纸色、笔迹用当前颜色：所见即所得。
        // 底色烘焙进像素，输出不透明图，避免依赖后端 alpha 混合行为。
        let base = self
            .doc
            .as_ref()
            .map(|d| d.paper)
            .unwrap_or(PAPERS[self.paper_idx.min(PAPERS.len() - 1)].1);
        let ink = SWATCHES[self.color_idx.min(SWATCHES.len() - 1)];
        let mut buf = vec![0u8; pw as usize * ph as usize * 4];
        for px in buf.chunks_exact_mut(4) {
            px.copy_from_slice(&[base.r, base.g, base.b, 255]);
        }
        let y = ph as f32 * 0.5;
        let amp = ph as f32 * 0.22;
        let pts = [
            (pw as f32 * 0.10, y, 1.0),
            (pw as f32 * 0.34, y - amp, 1.0),
            (pw as f32 * 0.62, y + amp, 1.0),
            (pw as f32 * 0.90, y, 1.0),
        ];
        raster::stroke(
            &mut buf,
            pw,
            ph,
            &pts,
            &brush,
            [ink.r, ink.g, ink.b, 255],
            7,
        );
        let img = Image::from_rgba8(pw, ph, buf);
        self.preview_brush = Some((key, img.clone()));
        img
    }
}
