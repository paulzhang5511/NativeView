//! 新建画布屏：4 档尺寸、4 种纸色、两条默认笔刷滑杆。
//!
//! 滑杆即时改写「默认笔刷」，创建画布时作为新文档的初始参数。

use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;

use crate::gfx::scene::Scene;
use crate::text::layout::TextAlign;
use crate::ui::dimen::{dp, sp};

use super::layout::Layout;
use super::theme::*;
use super::{Inkflow, PAPERS, SIZES};

impl Inkflow {
    /// 新建画布页：返回/标题 →「尺寸」→「纸色」→ 迷你滑杆 → 主按钮。
    pub(super) fn draw_new_canvas(&mut self, c: &mut Canvas, scene: &mut Scene, l: &Layout) {
        let back = l.nc_back;
        fill_round(c, back, dp(22.0), surface_hi());
        self.text_center(scene, "‹", back, sp(20.0), text_main());
        self.text_block(
            scene,
            "新建画布",
            back.x as f32 + back.w as f32 + dp(12.0),
            l.top + dp(20.0),
            sp(20.0),
            text_main(),
            TextAlign::Left,
            l.w - dp(120.0),
        );

        // 分组标题。
        let pad = dp(16.0);
        self.text_block(
            scene,
            "尺寸",
            pad,
            l.nc_size_title_y,
            sp(11.0),
            text_dim(),
            TextAlign::Left,
            dp(120.0),
        );

        let subs = ["手机分享", "头像 / 插画", "打印 / 海报", "随手涂"];
        for (i, cell) in l.nc_sizes.iter().enumerate() {
            let selected = i == self.size_idx;
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
                stroke_round(c, *cell, dp(14.0), dp(1.5), violet());
            }
            // 左侧形状缩略：竖/方/纵向纸/窄屏，用比例暗示画布形状。
            let (sw, sh) = [(18.0, 26.0), (24.0, 24.0), (19.0, 27.0), (16.0, 24.0)][i];
            let shape = r(
                cell.x as f32 + dp(14.0),
                cell.y as f32 + (cell.h as f32 - dp(sh)) / 2.0,
                dp(sw),
                dp(sh),
            );
            stroke_round(
                c,
                shape,
                dp(4.0),
                dp(1.0),
                if selected { violet() } else { text_faint() },
            );
            let tx = shape.x as f32 + shape.w as f32 + dp(12.0);
            let tw = (cell.x as f32 + cell.w as f32 - tx - dp(10.0)).max(0.0);
            self.text_block(
                scene,
                SIZES[i].0,
                tx,
                cell.y as f32 + dp(22.0),
                sp(11.5),
                text_main(),
                TextAlign::Left,
                tw,
            );
            self.text_block(
                scene,
                subs[i],
                tx,
                cell.y as f32 + dp(44.0),
                sp(9.5),
                text_faint(),
                TextAlign::Left,
                tw,
            );
        }

        self.text_block(
            scene,
            "纸色",
            pad,
            l.nc_paper_title_y,
            sp(11.0),
            text_dim(),
            TextAlign::Left,
            dp(120.0),
        );
        for (i, cell) in l.nc_papers.iter().enumerate() {
            let selected = i == self.paper_idx;
            fill_round(c, *cell, dp(12.0), PAPERS[i].1);
            if selected {
                stroke_round(c, *cell, dp(12.0), dp(2.0), violet());
            }
            self.text_block(
                scene,
                PAPERS[i].0,
                cell.x as f32,
                cell.y as f32 + cell.h as f32 + dp(6.0),
                sp(10.0),
                if selected { text_main() } else { text_faint() },
                TextAlign::Center,
                cell.w as f32,
            );
        }

        // 迷你滑杆：标签 + 轨道 + 数值。
        let brush = self.brush();
        let rows = [
            (
                "默认笔刷",
                format!("{:.0} px", brush.size),
                (brush.size - 1.0) / 119.0,
            ),
            (
                "默认不透明度",
                format!("{:.0} %", brush.opacity * 100.0),
                brush.opacity,
            ),
        ];
        for (i, cell) in l.nc_sliders.iter().enumerate() {
            self.text_block(
                scene,
                rows[i].0,
                cell.x as f32,
                cell.y as f32 + dp(8.0),
                sp(11.0),
                text_dim(),
                TextAlign::Left,
                dp(96.0),
            );
            let ty = (cell.y as f32 + dp(16.0)) as f64;
            let tx0 = (cell.x as f32 + dp(110.0)) as f64;
            let tx1 = (cell.x as f32 + cell.w as f32 - dp(84.0)) as f64;
            fill_round(
                c,
                r(tx0 as f32, (ty - 1.5) as f32, (tx1 - tx0) as f32, dp(3.0)),
                dp(2.0),
                surface_hi(),
            );
            let t = rows[i].2.clamp(0.0, 1.0) as f64;
            fill_circle(c, tx0 + (tx1 - tx0) * t, ty, dp(7.0) as f64, violet());
            self.text_block(
                scene,
                &rows[i].1,
                cell.x as f32 + cell.w as f32 - dp(72.0),
                cell.y as f32 + dp(8.0),
                sp(10.5),
                text_main(),
                TextAlign::Right,
                dp(72.0),
            );
        }

        // 主按钮。
        let start = l.nc_start;
        fill_round(c, start, dp(26.0), violet());
        self.text_center(scene, "创建画布", start, sp(15.0), Color::WHITE);
    }
}
