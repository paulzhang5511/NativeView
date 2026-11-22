//! 单帧布局：绘制与命中测试共用的唯一几何来源。
//!
//! # 不变量
//! - `Layout::compute` 是纯函数：同样的 (宽, 上/下 inset) 必得同一份几何；
//! - 动效层通过 `shifted_y` / `shifted_sheet_y` 生成「平移副本」，
//!   命中测试永远用未平移的那一份，因此动画不会改变可点区域。

use crate::gfx::scene::Rect;
use crate::ui::dimen::dp;

use super::theme::r;
use super::{Action, SWATCHES, Screen};

/// 按窗口尺寸与 insets 计算布局（纯函数，窗口变化才需要重算）。
/// 单帧布局：所有可点区域，绘制与命中测试共用。
///
/// Clone 用于动效层：在原始几何上整体平移得到「动画中的几何」，
/// 命中测试始终使用未平移的那一份。
#[derive(Clone)]
pub(crate) struct Layout {
    pub(super) w: f32,
    pub(super) top: f32,
    pub(super) bottom: f32,
    pub(super) lib_plus: Rect,
    pub(super) lib_cards: [Rect; 4],
    /// 作品库筛选条（全部/收藏/草稿/已导出）。
    pub(super) lib_filters: [Rect; 4],
    /// 作品库底部统计条。
    pub(super) lib_stats: Rect,
    pub(super) nc_back: Rect,
    pub(super) nc_sizes: [Rect; 4],
    pub(super) nc_papers: [Rect; 4],
    pub(super) nc_start: Rect,
    /// 新建页两条迷你滑杆（默认笔刷 / 默认不透明度）。
    pub(super) nc_sliders: [Rect; 2],
    /// 新建页「尺寸」分组标题的基线。
    pub(super) nc_size_title_y: f32,
    /// 新建页「纸色」分组标题的基线。
    pub(super) nc_paper_title_y: f32,
    pub(super) bd_back: Rect,
    pub(super) bd_title: Rect,
    pub(super) bd_undo: Rect,
    pub(super) bd_redo: Rect,
    pub(super) bd_brush: Rect,
    pub(super) bd_canvas: Rect,
    pub(super) bd_export: Rect,
    /// 画布内左侧工具栏（笔 / 直线 / 橡皮 / 取色 / 平移）。
    pub(super) bd_tools: [Rect; 5],
    /// 画布右上「图层」按钮。
    pub(super) bd_layers: Rect,
    /// 画布右下角缩放比例徽标。
    pub(super) bd_zoom: Rect,
    /// 底部 dock 的笔刷信息行（点按打开笔刷面板）。
    pub(super) bd_info: Rect,
    /// dock 内的尺寸胶囊（点按打开笔刷面板）。
    pub(super) bd_sizepill: Rect,
    /// dock 内的尺寸滑杆（1–120）。
    pub(super) bd_sizebar: Rect,
    /// 色板：7 色 + 末尾「＋」。
    pub(super) bd_swatches: [Rect; 8],
    /// dock 底部的 4 个快捷笔刷（铅笔 / 软笔 / 喷枪 / 水彩）。
    pub(super) bd_shortcuts: [Rect; 4],
    /// 兼容旧字段：橡皮按钮位置（= bd_tools[2]）。
    pub(super) bd_eraser: Rect,
    pub(super) sh_card: Rect,
    pub(super) sh_presets: [Rect; 8],
    pub(super) sh_sliders: [Rect; 4],
    pub(super) sh_close: Rect,
    pub(super) ex_back: Rect,
    pub(super) ex_card: Rect,
    /// 导出页格式选择行（PNG / JPEG / PDF）。
    pub(super) ex_formats: [Rect; 3],
    /// 已保存后出现在主按钮上方的「再次导出」。
    pub(super) ex_again: Rect,
    pub(super) ex_save: Rect,
}

impl Layout {
    pub(super) fn compute(w: f32, top: f32, bottom: f32) -> Self {
        let pad = dp(16.0);
        let content_w = w - pad * 2.0;
        // 顶部返回/工具按钮统一 44dp。
        let btn = dp(44.0);
        let toolbar_y = top + dp(8.0);

        // 作品库：标题 + 副标 → 筛选条 → 2×2 卡片（第 4 张是「新建」空卡）→ 统计条。
        let plus = r(w - pad - dp(48.0), top + dp(6.0), dp(48.0), dp(48.0));
        let filter_y = top + dp(50.0);
        let filter_h = dp(32.0);
        let chip_w = dp(52.0);
        let chip_gap = dp(8.0);
        let lib_filters: [Rect; 4] = std::array::from_fn(|i| {
            r(
                pad + i as f32 * (chip_w + chip_gap),
                filter_y,
                if i == 3 { dp(64.0) } else { chip_w },
                filter_h,
            )
        });
        let cards_y = filter_y + filter_h + dp(16.0);
        let card_gap = dp(14.0);
        let card_w = (content_w - card_gap) / 2.0;
        // 卡片只放缩略图，标题/时间在卡外下方（与参考图一致）。
        let card_h = dp(150.0);
        let meta_h = dp(34.0);
        let stride = card_h + meta_h + dp(8.0);
        let cards: [Rect; 4] = std::array::from_fn(|i| {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            r(
                pad + col * (card_w + card_gap),
                cards_y + row * stride,
                card_w,
                card_h,
            )
        });
        let lib_stats = r(pad, cards_y + 2.0 * stride + dp(6.0), content_w, dp(64.0));

        // 新建页：返回键 + 标题 →「尺寸」2×2 →「纸色」一排 → 两条迷你滑杆 → 主按钮。
        let nc_back = r(pad, toolbar_y, btn, btn);
        let nc_size_title_y = top + dp(58.0);
        let grid_w = (content_w - dp(12.0)) / 2.0;
        let grid_h = dp(92.0);
        let grid_y = top + dp(72.0);
        let nc_sizes = std::array::from_fn(|i| {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            r(
                pad + col * (grid_w + dp(12.0)),
                grid_y + row * (grid_h + dp(12.0)),
                grid_w,
                grid_h,
            )
        });
        let nc_paper_title_y = grid_y + 2.0 * (grid_h + dp(12.0)) + dp(8.0);
        let papers_y = nc_paper_title_y + dp(22.0);
        let paper_w = (content_w - dp(3.0) * dp(8.0)) / 4.0;
        let nc_papers = std::array::from_fn(|i| {
            r(
                pad + i as f32 * (paper_w + dp(8.0)),
                papers_y,
                paper_w,
                dp(56.0),
            )
        });
        let sliders_y = papers_y + dp(56.0) + dp(18.0);
        let nc_sliders: [Rect; 2] =
            std::array::from_fn(|i| r(pad, sliders_y + i as f32 * dp(40.0), content_w, dp(34.0)));
        let nc_start = r(pad, bottom - dp(80.0), content_w, dp(56.0));

        // 画板：顶部（返回 / 标题 / 撤销 / 重做 / 导出），左右浮层工具，
        // 底部 dock（笔刷信息 + 尺寸滑杆 + 色板 + 快捷笔刷）。
        let bd_back = r(pad, toolbar_y + dp(2.0), dp(40.0), btn - dp(4.0));
        let export_w = dp(56.0);
        let bd_export = r(
            w - pad - export_w,
            toolbar_y + dp(4.0),
            export_w,
            btn - dp(8.0),
        );
        let bd_redo = r(
            bd_export.x as f32 - dp(8.0) - dp(40.0),
            toolbar_y + dp(2.0),
            dp(40.0),
            btn - dp(4.0),
        );
        let bd_undo = r(
            bd_redo.x as f32 - dp(8.0) - dp(40.0),
            toolbar_y + dp(2.0),
            dp(40.0),
            btn - dp(4.0),
        );
        let bd_title = r(
            pad + dp(48.0),
            toolbar_y,
            (bd_undo.x as f32 - pad - dp(56.0)).max(0.0),
            btn,
        );

        // 底部 dock 固定 200dp：信息行 44 + 尺寸条 36 + 色板 40 + 快捷笔刷 56 + 边距。
        let dock_h = dp(200.0);
        let dock_top = bottom - dock_h;
        let canvas_top = top + dp(64.0);
        let canvas_bottom = dock_top - dp(8.0);
        let canvas = r(
            pad,
            canvas_top,
            content_w,
            (canvas_bottom - canvas_top).max(0.0),
        );

        // 左侧竖排工具（笔 / 直线 / 橡皮 / 取色 / 平移），贴在画布左内侧。
        let tool_x = pad + dp(8.0);
        let tool_y0 = canvas_top + dp(12.0);
        let bd_tools: [Rect; 5] = std::array::from_fn(|i| {
            r(
                tool_x,
                tool_y0 + i as f32 * (dp(40.0) + dp(8.0)),
                dp(40.0),
                dp(40.0),
            )
        });
        // 兼容旧字段：橡皮按钮就是工具栏第 3 个。
        let bd_eraser = bd_tools[2];
        let bd_layers = r(
            pad + content_w - dp(8.0) - dp(44.0),
            canvas_top + dp(12.0),
            dp(44.0),
            dp(40.0),
        );
        let bd_zoom = r(
            (canvas.x + canvas.w - (dp(8.0) + dp(84.0)) as f64) as f32,
            (canvas.y + canvas.h - (dp(8.0) + dp(26.0)) as f64) as f32,
            dp(84.0),
            dp(26.0),
        );

        let bd_info = r(pad, dock_top, content_w, dp(44.0));
        let bd_sizepill = r(w - pad - dp(64.0), dock_top + dp(8.0), dp(64.0), dp(28.0));
        // 旧入口字段：笔刷面板由尺寸胶囊打开。
        let bd_brush = bd_sizepill;
        let bd_sizebar = r(pad, dock_top + dp(50.0), content_w, dp(34.0));

        // 色板 8 格（7 色 + 末尾「＋」），间距按可用宽度动态计算。
        let swatch_d = ((content_w - dp(28.0)) / 8.0).clamp(dp(22.0), dp(34.0));
        let swatch_gap = ((content_w - swatch_d * 8.0) / 7.0).max(dp(3.0));
        let swatch_y = dock_top + dp(88.0);
        let bd_swatches: [Rect; 8] = std::array::from_fn(|i| {
            r(
                pad + (i as f32) * (swatch_d + swatch_gap),
                swatch_y,
                swatch_d,
                swatch_d,
            )
        });
        let short_gap = dp(8.0);
        let short_w = (content_w - short_gap * 3.0) / 4.0;
        let short_y = dock_top + dp(130.0);
        let bd_shortcuts: [Rect; 4] = std::array::from_fn(|i| {
            r(
                pad + i as f32 * (short_w + short_gap),
                short_y,
                short_w,
                dp(48.0),
            )
        });

        // 笔刷面板：底部半屏，预设 4×2、滑杆 4 行、完成按钮。
        // 高度按内容反推：标题 76 + 预设两行 160 + 间距 16 + 滑杆 4×44 + 底部留白,
        // 取 452dp 保证最后一条滑杆不越过卡片底边（否则会掉到屏幕外无法拖动）。
        let sh_h = dp(452.0);
        let sh_card = r(pad * 0.5, bottom - sh_h, w - pad, sh_h);
        let preset_y = bottom - sh_h + dp(76.0);
        let preset_w = (content_w - pad - dp(24.0)) / 4.0;
        let preset_h = dp(76.0);
        let sh_presets = std::array::from_fn(|i| {
            let col = (i % 4) as f32;
            let row = (i / 4) as f32;
            r(
                pad + col * (preset_w + dp(8.0)),
                preset_y + row * (preset_h + dp(8.0)),
                preset_w,
                preset_h,
            )
        });
        let slider_y = preset_y + 2.0 * (preset_h + dp(8.0)) + dp(16.0);
        let sh_sliders =
            std::array::from_fn(|i| r(pad, slider_y + (i as f32) * dp(44.0), content_w, dp(36.0)));
        let sh_close = r(
            w - pad - dp(76.0),
            bottom - sh_h + dp(16.0),
            dp(76.0),
            dp(36.0),
        );

        // 导出页：返回、预览卡、格式选择、保存按钮。
        let ex_back = r(pad, toolbar_y, btn, btn);
        let ex_card = r(pad, top + dp(88.0), content_w, dp(292.0));
        let ex_formats: [Rect; 3] = std::array::from_fn(|i| {
            r(
                pad,
                top + dp(392.0) + i as f32 * dp(56.0),
                content_w,
                dp(48.0),
            )
        });
        let ex_again = r(pad, bottom - dp(148.0), content_w, dp(44.0));
        let ex_save = r(pad, bottom - dp(80.0), content_w, dp(56.0));

        Self {
            w,
            top,
            bottom,
            lib_plus: plus,
            lib_cards: cards,
            lib_filters,
            lib_stats,
            nc_back,
            nc_sizes,
            nc_papers,
            nc_start,
            nc_sliders,
            nc_size_title_y,
            nc_paper_title_y,
            bd_back,
            bd_title,
            bd_undo,
            bd_redo,
            bd_brush,
            bd_canvas: canvas,
            bd_export,
            bd_tools,
            bd_layers,
            bd_zoom,
            bd_info,
            bd_sizepill,
            bd_sizebar,
            bd_eraser,
            bd_swatches,
            bd_shortcuts,
            sh_card,
            sh_presets,
            sh_sliders,
            sh_close,
            ex_back,
            ex_card,
            ex_formats,
            ex_again,
            ex_save,
        }
    }

    /// 当前页面上的可点区域（按优先级排序：后加入者优先命中）。
    pub(super) fn hits(&self, screen: Screen, exported: bool) -> Vec<(Rect, Action)> {
        let mut v: Vec<(Rect, Action)> = Vec::new();
        match screen {
            Screen::Library => {
                v.push((self.lib_plus, Action::CreateNew));
                for (i, chip) in self.lib_filters.iter().enumerate() {
                    v.push((*chip, Action::PickFilter(i)));
                }
                for (i, card) in self.lib_cards.iter().enumerate() {
                    // 第 4 张是「新建画布」空卡。
                    v.push((
                        *card,
                        if i < 3 {
                            Action::OpenSample(i)
                        } else {
                            Action::CreateNew
                        },
                    ));
                }
            }
            Screen::NewCanvas => {
                v.push((self.nc_back, Action::Back));
                for (i, cell) in self.nc_sizes.iter().enumerate() {
                    v.push((*cell, Action::PickSize(i)));
                }
                for (i, cell) in self.nc_papers.iter().enumerate() {
                    v.push((*cell, Action::PickPaper(i)));
                }
                v.push((self.nc_start, Action::StartDrawing));
                for (i, cell) in self.nc_sliders.iter().enumerate() {
                    v.push((*cell, Action::DragDefault(i)));
                }
            }
            Screen::Board => {
                v.push((self.bd_back, Action::Back));
                v.push((self.bd_undo, Action::Undo));
                v.push((self.bd_redo, Action::Redo));
                v.push((self.bd_info, Action::OpenBrushSheet));
                v.push((self.bd_export, Action::OpenExport));
                v.push((self.bd_layers, Action::ToggleLayer));
                v.push((self.bd_sizebar, Action::DragSize));
                for (i, cell) in self.bd_tools.iter().enumerate() {
                    v.push((*cell, Action::PickTool(i)));
                }
                for (i, cell) in self.bd_shortcuts.iter().enumerate() {
                    v.push((*cell, Action::QuickBrush(i)));
                }
                // 第 8 格是「＋」，打开笔刷面板。
                for (i, cell) in self.bd_swatches.iter().enumerate() {
                    v.push((
                        *cell,
                        if i < SWATCHES.len() {
                            Action::PickColor(i)
                        } else {
                            Action::OpenBrushSheet
                        },
                    ));
                }
            }
            Screen::BrushSheet => {
                v.push((self.sh_close, Action::CloseBrushSheet));
                for (i, cell) in self.sh_presets.iter().enumerate() {
                    v.push((*cell, Action::PickPreset(i)));
                }
                for (i, cell) in self.sh_sliders.iter().enumerate() {
                    v.push((*cell, Action::DragSlider(i)));
                }
            }
            Screen::Export => {
                v.push((self.ex_back, Action::Back));
                for (i, cell) in self.ex_formats.iter().enumerate() {
                    v.push((*cell, Action::PickFormat(i)));
                }
                // 已保存后主按钮变成「完成」，上方另有「再次导出」。
                if exported {
                    v.push((self.ex_again, Action::DoExport));
                    v.push((self.ex_save, Action::Finish));
                } else {
                    v.push((self.ex_save, Action::DoExport));
                }
            }
        }
        v
    }

    /// 整页纵向平移（切屏动效）；返回新几何，原几何保持不变。
    ///
    /// 只平移矩形与 `top/bottom`：绘制层所有坐标都从这份 `Layout` 派生，
    /// 因此文字会与图形一起移动，不会出现「图动字不动」。
    pub(super) fn shifted_y(&self, dy: f32) -> Self {
        let mut l = self.clone();
        l.shift_y(dy);
        l
    }

    /// 仅平移笔刷面板的几何（面板滑入/滑出）。
    pub(super) fn shifted_sheet_y(&self, dy: f32) -> Self {
        let mut l = self.clone();
        if dy != 0.0 {
            l.sh_card.y += dy as f64;
            l.sh_close.y += dy as f64;
            for cell in l.sh_presets.iter_mut() {
                cell.y += dy as f64;
            }
            for cell in l.sh_sliders.iter_mut() {
                cell.y += dy as f64;
            }
        }
        l
    }

    /// 全字段纵向平移。
    pub(super) fn shift_y(&mut self, dy: f32) {
        if dy == 0.0 {
            return;
        }
        self.top += dy;
        self.bottom += dy;
        self.nc_size_title_y += dy;
        self.nc_paper_title_y += dy;
        macro_rules! ones {
            ($($f:ident),+ $(,)?) => { $( self.$f.y += dy as f64; )+ };
        }
        macro_rules! many {
            ($($f:ident),+ $(,)?) => { $( for cell in self.$f.iter_mut() { cell.y += dy as f64; } )+ };
        }
        ones!(
            lib_plus,
            lib_stats,
            nc_back,
            nc_start,
            bd_back,
            bd_title,
            bd_undo,
            bd_redo,
            bd_brush,
            bd_canvas,
            bd_export,
            bd_layers,
            bd_zoom,
            bd_info,
            bd_sizepill,
            bd_sizebar,
            bd_eraser,
            sh_card,
            sh_close,
            ex_back,
            ex_card,
            ex_again,
            ex_save,
        );
        many!(
            lib_filters,
            lib_cards,
            nc_sizes,
            nc_papers,
            nc_sliders,
            bd_tools,
            bd_swatches,
            bd_shortcuts,
            sh_presets,
            sh_sliders,
            ex_formats,
        );
    }
}
