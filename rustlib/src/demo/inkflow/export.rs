//! 导出页：预览卡、3 行格式选择、体积估算、「保存 / 完成」双按钮。
//!
//! 保存成功后的状态行与双按钮入场动效由 `export_at` 驱动（见 `motion`）。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;

use crate::gfx::scene::Scene;
use crate::text::layout::TextAlign;
use crate::ui::dimen::{dp, sp};

use crate::demo::inkflow_anim as anim;

use super::layout::Layout;
use super::theme::*;
use super::{FORMATS, Inkflow};

impl Inkflow {
    /// 导出页。
    pub(super) fn draw_export(&mut self, c: &mut Canvas, scene: &mut Scene, l: &Layout) {
        let back = l.ex_back;
        fill_round(c, back, dp(22.0), surface_hi());
        self.text_center(scene, "‹", back, sp(20.0), text_main());
        self.text_block(
            scene,
            "导出作品",
            back.x as f32 + back.w as f32 + dp(12.0),
            l.top + dp(20.0),
            sp(22.0),
            text_main(),
            TextAlign::Left,
            l.w - dp(120.0),
        );

        let card = l.ex_card;
        fill_round(c, card, dp(20.0), surface());
        // 先拷贝所需字段：绘制文本要 &mut self，不能同时持有 &doc。
        let doc_info = self
            .doc
            .as_ref()
            .map(|d| (d.width, d.height, d.steps(), d.layers.len(), d.paper));
        if let Some((dw0, dh0, steps, layers, paper)) = doc_info {
            let inner = r(
                card.x as f32 + dp(28.0),
                card.y as f32 + dp(28.0),
                card.w as f32 - dp(56.0),
                card.h as f32 - dp(132.0),
            );
            fill_round(c, inner, dp(8.0), paper);
            // 预览与画板共用同一绘制路径：纸色底 + 层内容，超出卡片裁剪。
            self.draw_doc_view(c, inner, 1.0, (0.0, 0.0), paper, false, true);
            let info_y = inner.y as f32 + inner.h as f32 + dp(14.0);
            let width_txt = format!("尺寸 {}×{}", dw0, dh0);
            let ink_txt = format!("可撤销 {} 步 · {} 层", steps, layers);
            self.text_block(
                scene,
                &width_txt,
                card.x as f32 + dp(20.0),
                info_y,
                sp(12.0),
                text_dim(),
                TextAlign::Left,
                card.w as f32 - dp(40.0),
            );
            self.text_block(
                scene,
                &ink_txt,
                card.x as f32 + dp(20.0),
                info_y + dp(22.0),
                sp(12.0),
                text_dim(),
                TextAlign::Left,
                card.w as f32 - dp(40.0),
            );
            // 状态保持单行：完整路径会让文本块折行、溢出卡片底边。
            let status = if let Some(info) = self.exported.as_ref() {
                let name = info
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("png");
                format!("已保存 {:.1} KB · {}", info.bytes as f64 / 1024.0, name)
            } else if let Some(err) = self.export_error.as_ref() {
                format!("导出失败：{}", err)
            } else {
                "将保存到 exports 目录".to_string()
            };
            // 保存成功的状态行随「导出成功」动效淡入并自下 12dp 归位。
            let ep = anim::ease_out(self.export_p());
            let status_y = info_y + dp(48.0) + dp(anim::EXPORT_SHIFT_PX) * (1.0 - ep);
            let mut faint = text_faint();
            faint.a = (faint.a as f32 * ep) as u8;
            self.text_block(
                scene,
                &status,
                card.x as f32 + dp(20.0),
                status_y,
                sp(11.0),
                faint,
                TextAlign::Left,
                card.w as f32 - dp(40.0),
            );
        }

        // 格式选择：只有 PNG 可产出，其余行明示「未开放」，点击给 toast。
        for (i, cell) in l.ex_formats.iter().enumerate() {
            let (head, sub) = FORMATS[i];
            let on = i == 0;
            if on {
                fill_round(c, *cell, dp(14.0), violet());
                stroke_round(c, *cell, dp(14.0), dp(1.5), violet_hi());
            } else {
                fill_round(c, *cell, dp(14.0), surface_hi());
            }
            let fg = if on { Color::WHITE } else { text_main() };
            let dim = if on { Color::WHITE } else { text_faint() };
            self.text_block(
                scene,
                head,
                cell.x as f32 + dp(16.0),
                cell.y as f32 + dp(7.0),
                sp(13.0),
                fg,
                TextAlign::Left,
                cell.w as f32 - dp(110.0),
            );
            self.text_block(
                scene,
                sub,
                cell.x as f32 + dp(16.0),
                cell.y as f32 + dp(27.0),
                sp(10.0),
                dim,
                TextAlign::Left,
                cell.w as f32 - dp(110.0),
            );
            self.text_block(
                scene,
                if on { "已选" } else { "未开放" },
                cell.x as f32 + cell.w as f32 - dp(88.0),
                cell.y as f32 + dp(16.0),
                sp(11.0),
                dim,
                TextAlign::Right,
                dp(72.0),
            );
        }

        // 预估体积：已导出用真实字节数，否则按像素量与 PNG 压缩比粗估。
        if let Some((dw0, dh0, _, _, _)) = self
            .doc
            .as_ref()
            .map(|d| (d.width, d.height, 0, 0, d.paper))
        {
            let kb = match self.exported.as_ref() {
                Some(info) => info.bytes as f64 / 1024.0,
                None => (dw0 * dh0) as f64 * 4.0 * 0.12 / 1024.0,
            };
            let meta = format!(
                "预估体积 {:.0} KB · 长边 {} px · 透明通道",
                kb,
                dw0.max(dh0)
            );
            let meta_y = l.ex_formats[2].y as f32 + l.ex_formats[2].h as f32 + dp(10.0);
            self.text_block(
                scene,
                &meta,
                l.w * 0.5 - dp(140.0),
                meta_y,
                sp(10.5),
                text_faint(),
                TextAlign::Left,
                dp(280.0),
            );
        }

        // 主按钮：未导出「保存 PNG」；已导出切换为「完成」并额外给出「再次导出」。
        // 保存成功后这两颗按钮自下 12dp 滑入（错峰 60ms），提示「可再次操作」。
        let ep = anim::ease_out(self.export_p());
        let dy2 = dp(anim::EXPORT_SHIFT_PX) * (1.0 - ep);
        let dy_again = dp(anim::EXPORT_SHIFT_PX)
            * (1.0
                - anim::ease_out(anim::progress(
                    self.clock,
                    self.export_at.unwrap_or(0),
                    anim::EXPORT_MS + anim::EXPORT_STAGGER_MS,
                )));
        let again = shift_rect(l.ex_again, dy_again);
        if self.exported.is_some() {
            fill_round(c, again, dp(22.0), surface_hi());
            self.text_center(scene, "再次导出", again, sp(13.0), text_main());
        }
        let save = shift_rect(l.ex_save, dy2);
        fill_round(c, save, dp(28.0), violet());
        self.text_center(
            scene,
            if self.exported.is_some() {
                "完成"
            } else {
                "保存 PNG"
            },
            save,
            sp(16.0),
            Color::WHITE,
        );
    }
}
