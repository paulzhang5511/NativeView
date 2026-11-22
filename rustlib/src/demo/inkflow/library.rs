//! 作品库屏：筛选条、2×2 作品卡、空卡、统计条。
//!
//! 卡片与卡外 meta 分两轮绘制（先全部卡片再全部文字），
//! 否则下一行卡片背景会盖掉上一行的标题。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::gfx::paint::Paint;
use crate::gfx::scene::Scene;
use crate::text::layout::TextAlign;
use crate::ui::dimen::{dp, sp};

use super::layout::Layout;
use super::theme::*;
use super::{Inkflow, PAPERS};

impl Inkflow {
    /// 作品库页：标题/副标 → 筛选条 → 2×2 作品卡 → 底部统计条。
    pub(super) fn draw_library(&mut self, c: &mut Canvas, scene: &mut Scene, l: &Layout) {
        let pad = dp(16.0);
        let text_w = l.w - pad * 2.0;
        self.text_block(
            scene,
            "我的涂鸦",
            pad,
            l.top + dp(2.0),
            sp(24.0),
            text_main(),
            TextAlign::Left,
            text_w,
        );
        self.text_block(
            scene,
            "12 张作品 · 最近编辑 3 天前",
            pad,
            l.top + dp(34.0),
            sp(11.0),
            text_faint(),
            TextAlign::Left,
            text_w,
        );

        // 右上「＋」。
        let plus = l.lib_plus;
        fill_round(c, plus, dp(24.0), violet());
        let pcx = (plus.x + plus.w / 2.0) as f32;
        let pcy = (plus.y + plus.h / 2.0) as f32;
        c.draw_rect(
            r(pcx - dp(1.5), pcy - dp(10.0), dp(3.0), dp(20.0)),
            &Paint::default().color(Color::WHITE),
        );
        c.draw_rect(
            r(pcx - dp(10.0), pcy - dp(1.5), dp(20.0), dp(3.0)),
            &Paint::default().color(Color::WHITE),
        );

        // 筛选条：选中项紫底白字。
        let filters = ["全部", "收藏", "草稿", "已导出"];
        for (i, chip) in l.lib_filters.iter().enumerate() {
            let on = i == self.filter_idx;
            fill_round(
                c,
                *chip,
                (chip.h as f32) / 2.0,
                if on {
                    violet()
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
                filters[i],
                *chip,
                sp(11.5),
                if on { Color::WHITE } else { text_dim() },
            );
        }

        // 作品卡：整块缩略图 + 下方标题/时间；第 4 张是「新建画布」空卡。
        let samples = [
            ("鸢尾花 · 01", "3 天前", "草稿", 0usize),
            ("落日速写", "1 周前", "已导出", 1),
            ("霓虹夜", "2 周前", "", 2),
        ];
        for (i, card) in l.lib_cards.iter().enumerate() {
            let card = shift_rect(*card, self.card_shift(i));
            if i == 3 {
                // 虚线感用两层细边模拟：外圈描边 + 内圈描边。
                stroke_round(
                    c,
                    card,
                    dp(16.0),
                    dp(1.0),
                    Color {
                        r: 0x33,
                        g: 0x33,
                        b: 0x3D,
                        a: 255,
                    },
                );
                let mid = r(
                    card.x as f32,
                    (card.y + card.h / 2.0) as f32 - dp(18.0),
                    card.w as f32,
                    dp(20.0),
                );
                self.text_center(scene, "新建画布", mid, sp(12.0), text_faint());
                self.text_center(
                    scene,
                    "开始下一张",
                    r(
                        mid.x as f32,
                        mid.y as f32 + dp(22.0),
                        mid.w as f32,
                        mid.h as f32,
                    ),
                    sp(12.0),
                    text_faint(),
                );
                continue;
            }
            let (_, _, badge, paper_idx) = samples[i];
            let visible = self.lib_card_visible(i);
            if !visible {
                stroke_round(
                    c,
                    card,
                    dp(16.0),
                    dp(1.0),
                    Color {
                        r: 0x2A,
                        g: 0x24,
                        b: 0x38,
                        a: 255,
                    },
                );
                self.text_center(scene, "—", card, sp(16.0), text_faint());
                continue;
            }
            fill_round(c, card, dp(16.0), PAPERS[paper_idx].1);
            // 缩略笔迹：两道同色系线条，模拟参考图的画面感。
            let ink_col = [
                Color {
                    r: 0x3E,
                    g: 0x8E,
                    b: 0x6E,
                    a: 220,
                },
                Color {
                    r: 0xE0,
                    g: 0xA4,
                    b: 0x58,
                    a: 230,
                },
                Color {
                    r: 0xFF,
                    g: 0x6B,
                    b: 0xD6,
                    a: 220,
                },
            ][i];
            let sw = card.w as f32;
            c.draw_rect(
                r(
                    card.x as f32 + dp(22.0),
                    card.y as f32 + dp(38.0),
                    sw * 0.52,
                    dp(4.0),
                ),
                &Paint::default().color(ink_col),
            );
            c.draw_rect(
                r(
                    card.x as f32 + dp(40.0),
                    card.y as f32 + dp(62.0),
                    sw * 0.42,
                    dp(2.5),
                ),
                &Paint::default().color(Color {
                    r: ink_col.r,
                    g: ink_col.g,
                    b: ink_col.b,
                    a: 150,
                }),
            );
            if !badge.is_empty() {
                let chip = r(
                    card.x as f32 + dp(10.0),
                    card.y as f32 + dp(10.0),
                    dp(56.0),
                    dp(22.0),
                );
                fill_round(
                    c,
                    chip,
                    dp(11.0),
                    Color {
                        r: 0x2A,
                        g: 0x24,
                        b: 0x3A,
                        a: 235,
                    },
                );
                self.text_center(scene, badge, chip, sp(10.0), text_main());
            }
        }

        // 第二遍单独画卡外 meta：先画完所有卡片，避免下一行卡片覆盖上一行文字。
        for i in 0..3 {
            if !self.lib_card_visible(i) {
                continue;
            }
            let card = shift_rect(l.lib_cards[i], self.card_shift(i));
            let (name, time, _, _) = samples[i];
            let meta_y = card.y as f32 + card.h as f32 + dp(8.0);
            self.text_block(
                scene,
                name,
                card.x as f32 + dp(2.0),
                meta_y,
                sp(12.5),
                text_main(),
                TextAlign::Left,
                card.w as f32 - dp(56.0),
            );
            self.text_block(
                scene,
                time,
                card.x as f32 + card.w as f32 - dp(56.0),
                meta_y + dp(2.0),
                sp(10.0),
                text_faint(),
                TextAlign::Right,
                dp(56.0),
            );
        }

        // 底部统计条：4 段数字 + 标签。
        let stats = l.lib_stats;
        fill_round(c, stats, dp(16.0), surface());
        let items = [
            ("12", "作品"),
            ("3", "草稿"),
            ("2048", "常用画布"),
            ("PNG", "默认导出"),
        ];
        let seg = stats.w as f32 / 4.0;
        for (i, (num, label)) in items.iter().enumerate() {
            let x = stats.x as f32 + seg * i as f32;
            self.text_block(
                scene,
                num,
                x,
                stats.y as f32 + dp(12.0),
                sp(16.0),
                text_main(),
                TextAlign::Center,
                seg,
            );
            self.text_block(
                scene,
                label,
                x,
                stats.y as f32 + dp(36.0),
                sp(9.5),
                text_faint(),
                TextAlign::Center,
                seg,
            );
        }
    }
    /// 作品卡是否在当前筛选下可见。
    ///
    /// 筛选不卸载节点（卡片仍是可点目标），只决定缩略图是否绘制。
    pub(super) fn lib_card_visible(&self, i: usize) -> bool {
        match self.filter_idx {
            0 => true,
            2 => i == 0,
            3 => i == 1,
            _ => false,
        }
    }
}
