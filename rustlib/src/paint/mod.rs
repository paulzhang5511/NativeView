//! 绘画引擎：笔迹采样、笔刷光栅化、图层与撤销、PNG 导出。
//!
//! # 分层
//! - [`stroke`]：纯几何与采样（插值、平滑、压感→半径），无像素操作；
//! - [`brush`]：笔刷预设与参数，决定光栅化时的硬度/流量/抖动；
//! - [`raster`]：把一次笔迹落到 RGBA 缓冲（src-over 合成，非预乘）；
//! - [`Document`]：图层栈 + 历史栈门面，UI 层只与本类型交互。
//!
//! # 不变量
//! - 所有缓冲均为 RGBA8888、非预乘、行优先，长度必须等于 `width * height * 4`；
//! - 像素坐标原点在图层左上角，越界写入一律裁剪而非回绕；
//! - 撤销以「笔」为单位：`apply_stroke` 只在整笔结束时压栈一次；
//! - 本模块不依赖 Android，桌面端可完整单测。

pub mod brush;
pub mod export;
pub mod raster;
pub mod stroke;

pub use brush::{Brush, BrushKind, PRESETS, Preset};
pub use export::{ExportError, ExportInfo, export_png, sanitize_file_name};
pub use raster::DirtyRect;
pub use stroke::{StrokePoint, densify, radius_for, smooth};

use std::cell::{Cell, RefCell};

use crate::gfx::color::Color;
use crate::gfx::image::Image;

/// 单个图层。
///
/// # 不变量
/// `pixels.len() == width * height * 4`，由 [`Document`] 构造时保证；
/// 图层自身不持有尺寸，尺寸只在文档层维护，避免多处副本不一致。
pub struct Layer {
    /// 展示名（面板与图库角标使用）。
    pub name: String,
    /// 是否参与合成。
    pub visible: bool,
    /// 图层不透明度 [0,1]，合成时与像素 alpha 相乘。
    pub opacity: f32,
    /// RGBA8888 像素。
    pub pixels: Vec<u8>,
}

impl Layer {
    /// 新建全透明图层。
    pub fn new(name: impl Into<String>, len: usize) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            pixels: vec![0u8; len],
        }
    }

    /// 像素字节数（渲染与体积估算用）。
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    /// 是否含任何非透明像素（作品库缩略图判断“有画/空白”用）。
    pub fn is_blank(&self) -> bool {
        self.pixels.iter().skip(3).step_by(4).all(|a| *a == 0)
    }
}

/// 一步历史快照：只保存脏矩形内的前后像素，避免整层复制。
struct Snapshot {
    layer: usize,
    rect: DirtyRect,
    before: Vec<u8>,
    after: Vec<u8>,
}

/// 撤销/重做栈。
///
/// # 不变量
/// `undo.len() <= limit`；`record` 会清空 redo；同一矩形只会存在于一个栈中。
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    limit: usize,
}

impl History {
    /// 新建空栈；`limit` 至少为 1。
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// 记录一步；超出上限时丢弃最旧的一步。
    fn record(&mut self, snap: Snapshot) {
        self.redo.clear();
        self.undo.push(snap);
        while self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// 可撤销步数。
    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// 可重做步数。
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// 是否有可撤销步骤。
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// 是否有可重做步骤。
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 撤销一步：把 `before` 贴回图层。栈空返回 false。
    fn undo(&mut self, layers: &mut [Layer], w: u32) -> bool {
        let Some(snap) = self.undo.pop() else {
            return false;
        };
        if let Some(layer) = layers.get_mut(snap.layer) {
            paste_region(&mut layer.pixels, w, snap.rect, &snap.before);
            self.redo.push(snap);
            true
        } else {
            false
        }
    }

    /// 重做一步：把 `after` 贴回图层。栈空返回 false。
    fn redo(&mut self, layers: &mut [Layer], w: u32) -> bool {
        let Some(snap) = self.redo.pop() else {
            return false;
        };
        if let Some(layer) = layers.get_mut(snap.layer) {
            paste_region(&mut layer.pixels, w, snap.rect, &snap.after);
            self.undo.push(snap);
            true
        } else {
            false
        }
    }
}

/// 绘画文档：一次编辑会话的全部状态。
///
/// # 不变量
/// 每个图层的像素长度恒等于 `width * height * 4`；
/// `active < layers.len()`；`image()` 的缓存与 `revision` 严格对应。
pub struct Document {
    /// 画布宽度（像素）。
    pub width: u32,
    /// 画布高度（像素）。
    pub height: u32,
    /// 纸色（合成底色）。
    pub paper: Color,
    /// 图层栈（V1 固定两层，不提供增删 UI）。
    pub layers: Vec<Layer>,
    /// 当前绘制层下标。
    pub active: usize,
    /// 当前笔刷。
    pub brush: Brush,
    /// 当前颜色。
    pub color: Color,
    /// 撤销栈。
    pub history: History,
    /// 已落笔数（图库角标与导出页统计用）。
    pub stroke_count: u32,
    revision: Cell<u32>,
    cache: RefCell<Option<(u32, Image)>>,
    /// 实时笔画：落笔瞬间整层克隆（撤销用），抬手才压栈。
    live_before: Option<Vec<u8>>,
    /// 实时笔画累积的脏矩形。
    live_dirty: DirtyRect,
    /// 实时笔画所在层。
    live_layer: usize,
    /// 实时笔画已落笔（供 UI 判断是否处于绘制中）。
    live_active: bool,
    /// 脏提示：`(revision, 受影响矩形)`。
    ///
    /// # 用途与约束
    /// 缩略图/预览缓存按 revision 失效，但整帧重建预览（1M+ 目标像素重采样）
    /// 在每落一个点就要 5–20ms —— 60fps 的主要瓶颈。该提示让缓存持有者只
    /// 重采样受影响的那一小块，结果与整帧重建**逐像素一致**（box 均值对每个
    /// 输出像素独立）。
    ///
    /// 只记录「可局部化」的改动：撤销/重做/换层统一把 `dirty_valid` 置 false，
    /// 表示缓存必须整帧重建。`dirty_hint` 的 revision 是**起始** revision
    /// （改动发生前那帧的 revision），因此跨多个 revision（一帧内落了多个点）
    /// 也能正确并起来。
    dirty_hint: Cell<Option<(u32, DirtyRect)>>,
    /// 脏提示是否仍然可用（false = 自上次整帧重建以来有过不可局部化的改动）。
    dirty_valid: Cell<bool>,
}

impl Document {
    /// 新建文档：分配两层全透明图层。
    ///
    /// # 参数
    /// `width/height` 会夹到 `[1, 8192]`，避免误传导致巨量内存分配。
    pub fn new(width: u32, height: u32, paper: Color, brush: Brush) -> Self {
        let width = width.clamp(1, 8192);
        let height = height.clamp(1, 8192);
        let len = width as usize * height as usize * 4;
        Self {
            width,
            height,
            paper,
            layers: vec![Layer::new("笔迹", len), Layer::new("装饰", len)],
            active: 0,
            brush,
            color: Color {
                r: 0x2B,
                g: 0x2B,
                b: 0x33,
                a: 255,
            },
            history: History::new(30),
            stroke_count: 0,
            revision: Cell::new(0),
            cache: RefCell::new(None),
            live_before: None,
            live_dirty: DirtyRect::empty(),
            live_layer: 0,
            live_active: false,
            dirty_hint: Cell::new(None),
            dirty_valid: Cell::new(false),
        }
    }

    /// 当前层引用。
    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active.min(self.layers.len() - 1)]
    }

    /// 当前层像素的只读借用（紧凑 RGBA8888，行长 `width*4`）。
    ///
    /// # 为什么不让调用方走 [`Self::image`]
    /// `image()` 在版本号变化时要把整层像素拷进 `Arc`（1080×1920 ≈ 8MB，
    /// 真机一次 ~4ms）。画板每帧只需要「读这些像素做降采样」，
    /// 直接借用可把这次全图拷贝彻底省掉。
    pub fn layer_pixels(&self) -> &[u8] {
        &self.active_layer().pixels
    }

    /// 落一整笔到当前层并压入历史。
    ///
    /// # 返回
    /// 空轨迹或完全落在画布外时返回 false，且不产生历史步骤。
    pub fn apply_stroke(&mut self, points: &[StrokePoint]) -> bool {
        if points.is_empty() || self.layers.is_empty() {
            return false;
        }
        let (w, h) = (self.width, self.height);
        let brush = self.brush.clamped();
        let color = [self.color.r, self.color.g, self.color.b, self.color.a];
        let idx = self.active.min(self.layers.len() - 1);

        // 预算包围盒：半径 + 抖动余量 + 抗锯齿 2px，再与画布求交。
        // 抖动位移最大为 radius*jitter，半径最大 radius*(1+0.3jitter)，
        // 因此 pad 取 radius*(1+1.3jitter)+2 即可包住实际写入。
        let tuning = brush.kind.tuning();
        let pad = brush.radius() * (1.0 + 1.3 * tuning.jitter) + 2.0;
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for p in points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        let rect = clip_rect(min_x - pad, min_y - pad, max_x + pad, max_y + pad, w, h);
        if rect.is_empty() {
            return false;
        }

        // 先存 before（撤销用），再光栅。
        let before_full = copy_region(&self.layers[idx].pixels, w, rect);
        let pts: Vec<(f32, f32, f32)> = points.iter().map(|p| (p.x, p.y, p.pressure)).collect();
        let dirty = raster::stroke(
            &mut self.layers[idx].pixels,
            w,
            h,
            &pts,
            &brush,
            color,
            seed_for(self.stroke_count, w, h),
        );
        // 实际脏矩形裁到预算矩形内，再按它裁剪 before / after，控制内存。
        let dirty = intersect(dirty, rect);
        if dirty.is_empty() {
            return false;
        }
        let before = crop_region(&before_full, rect, dirty);
        let after = copy_region(&self.layers[idx].pixels, w, dirty);
        self.history.record(Snapshot {
            layer: idx,
            rect: dirty,
            before,
            after,
        });
        self.stroke_count = self.stroke_count.saturating_add(1);
        self.revision.set(self.revision.get() + 1);
        self.mark_dirty_hint(dirty);
        true
    }

    /// 撤销一步；无历史返回 false。
    /// 开始实时笔画：整层克隆一份 before 快照（抬手时只保留脏矩形）。
    ///
    /// 必须在单指按下时调用；重复调用返回 false。
    pub fn begin_live(&mut self) -> bool {
        if self.live_active || self.layers.is_empty() {
            return false;
        }
        let idx = self.active.min(self.layers.len() - 1);
        self.live_layer = idx;
        self.live_before = Some(self.layers[idx].pixels.clone());
        self.live_dirty = DirtyRect::empty();
        self.live_active = true;
        true
    }

    /// 追加一段实时轨迹（增量光栅，不压历史）。
    ///
    /// # 语义
    /// 未 `begin_live` 时自动开始；返回是否有像素被改动。
    pub fn extend_live(&mut self, points: &[StrokePoint]) -> bool {
        if points.is_empty() {
            return false;
        }
        if !self.live_active && !self.begin_live() {
            return false;
        }
        let (w, h) = (self.width, self.height);
        let brush = self.brush.clamped();
        let color = [self.color.r, self.color.g, self.color.b, self.color.a];
        let pts: Vec<(f32, f32, f32)> = points.iter().map(|p| (p.x, p.y, p.pressure)).collect();
        // 同一笔内种子固定，保证分多次调用的纹理连续。
        let dirty = raster::stroke(
            &mut self.layers[self.live_layer].pixels,
            w,
            h,
            &pts,
            &brush,
            color,
            seed_for(self.stroke_count, w, h),
        );
        if dirty.is_empty() {
            return false;
        }
        self.live_dirty = self.live_dirty.union(dirty);
        self.revision.set(self.revision.get() + 1);
        self.mark_dirty_hint(dirty);
        true
    }

    /// 结束实时笔画：把 before/after 按脏矩形裁出并压栈。
    ///
    /// # 返回
    /// 从未落笔（脏矩形为空）时返回 false，不产生历史步骤。
    pub fn end_live(&mut self) -> bool {
        let Some(before_full) = self.live_before.take() else {
            self.live_active = false;
            return false;
        };
        let dirty = self.live_dirty;
        self.live_active = false;
        if dirty.is_empty() {
            return false;
        }
        let (w, idx) = (self.width, self.live_layer);
        let before = crop_region(
            &before_full,
            DirtyRect {
                x0: 0,
                y0: 0,
                x1: w,
                y1: self.height,
            },
            dirty,
        );
        let after = copy_region(&self.layers[idx].pixels, w, dirty);
        self.history.record(Snapshot {
            layer: idx,
            rect: dirty,
            before,
            after,
        });
        self.stroke_count = self.stroke_count.saturating_add(1);
        self.revision.set(self.revision.get() + 1);
        self.mark_dirty_hint(dirty);
        true
    }

    /// 放弃实时笔画（双指介入/手势被系统取消）：恢复按下前的像素。
    pub fn cancel_live(&mut self) -> bool {
        let Some(before) = self.live_before.take() else {
            self.live_active = false;
            return false;
        };
        self.live_active = false;
        if let Some(layer) = self.layers.get_mut(self.live_layer) {
            if before.len() == layer.pixels.len() {
                layer.pixels = before;
                self.live_dirty = DirtyRect::empty();
                self.revision.set(self.revision.get() + 1);
                self.clear_dirty_hint();
                return true;
            }
        }
        false
    }

    /// 是否处于实时笔画中（UI 判断抬手/取消分支）。
    pub fn is_live(&self) -> bool {
        self.live_active
    }

    /// 撤销一步；无历史返回 false。
    pub fn undo(&mut self) -> bool {
        let changed = self.history.undo(&mut self.layers, self.width);
        if changed {
            self.revision.set(self.revision.get() + 1);
            self.clear_dirty_hint();
        }
        changed
    }

    /// 重做一步；无记录返回 false。
    pub fn redo(&mut self) -> bool {
        let changed = self.history.redo(&mut self.layers, self.width);
        if changed {
            self.revision.set(self.revision.get() + 1);
            self.clear_dirty_hint();
        }
        changed
    }

    /// 可直接交给 `DrawImage` 的当前层图像（带版本缓存）。
    ///
    /// 未改动时返回同一份 `Arc` 像素，多帧渲染零拷贝；改动后重建。
    /// 当前可撤销步数（撤销后会减少）：导出页的笔迹统计以它为准，
    /// `stroke_count` 只是抖动种子并包含已撤销的笔，直接用会与画面不符。
    pub fn steps(&self) -> usize {
        self.history.depth()
    }

    /// 当前像素版本号：任何落笔/撤销/重做/换层都会递增。
    ///
    /// 页面层用它做缩略图缓存键——比「像素指针」可靠：分配器会把释放的
    /// 缓冲地址再次分配出去，仅靠指针相等会误判缓存命中、显示旧图。
    pub fn revision(&self) -> u32 {
        self.revision.get()
    }

    /// 切换活动图层；越界忽略，成功时递增 revision 让缩略图缓存失效。
    pub fn set_active(&mut self, i: usize) -> bool {
        if i >= self.layers.len() || i == self.active {
            return false;
        }
        self.active = i;
        self.revision.set(self.revision.get() + 1);
        self.clear_dirty_hint();
        true
    }

    /// 记录一处可局部化的像素改动（与当前 revision 绑定）。
    ///
    /// # 合并语义
    /// 同一 revision 内的多次记录取并集；revision 已前进（说明中间有过一次
    /// 未被消费的改动）时以本次矩形覆盖——缓存方按 revision 校验，不会误用。
    fn mark_dirty_hint(&self, rect: DirtyRect) {
        if rect.is_empty() {
            return;
        }
        if !self.dirty_valid.get() {
            // 上一段链条已作废：以本条改动开启新链条（起始 = 本帧之前的 revision）。
            self.dirty_hint
                .set(Some((self.revision.get().saturating_sub(1), rect)));
            self.dirty_valid.set(true);
            return;
        }
        let merged = match self.dirty_hint.get() {
            // 同一段连续局部改动：并集，起始 revision 保持不变。
            Some((from, d)) => (from, d.union(rect)),
            None => (self.revision.get().saturating_sub(1), rect),
        };
        self.dirty_hint.set(Some(merged));
    }

    /// 作废脏提示：本次改动无法用矩形局部化（撤销/重做/换层/整层恢复）。
    fn clear_dirty_hint(&self) {
        self.dirty_hint.set(None);
        self.dirty_valid.set(false);
    }

    /// 声明「当前 revision 的画面已经出过帧」：脏提示链从这里重新累计。
    ///
    /// # 为什么必须由出帧方调用
    /// 脏提示是「自上次出帧以来」的累计量。若不清链，链条会从很久以前开始，
    /// `from_rev` 永远对不上上一帧的 revision，后端就只能整帧重绘——等于
    /// 白做增量。出帧方（demo）在每帧 regen 之后调用一次即可。
    pub fn ack_dirty(&self) {
        self.dirty_hint.set(None);
        self.dirty_valid.set(true);
    }

    /// 读取「自 `from_rev` 以来所有局部改动」的脏矩形：`(from_rev, 矩形)`。
    ///
    /// # 消费约定
    /// 持有缩略图的缓存方若缓存 revision == `from_rev`，则只需重采样该矩形
    /// 覆盖的输出像素（结果与整帧重建逐像素一致）；revision 不匹配、或返回
    /// `None`（期间有不可局部化改动）时必须整帧重建。
    ///
    /// 返回值是该脏提示的**快照**，不会因后续改动而失效——缓存方据此把
    /// 自己的 revision 推进到当前 revision 即可。
    pub fn dirty_since(&self) -> Option<(u32, (u32, u32, u32, u32))> {
        if !self.dirty_valid.get() {
            return None;
        }
        self.dirty_hint
            .get()
            .map(|(from, d)| (from, (d.x0, d.y0, d.x1, d.y1)))
    }

    pub fn image(&self) -> Image {
        let rev = self.revision.get();
        if let Some((cached_rev, img)) = self.cache.borrow().as_ref() {
            if *cached_rev == rev {
                return img.clone();
            }
        }
        let img = Image::from_rgba8(self.width, self.height, self.active_layer().pixels.clone());
        *self.cache.borrow_mut() = Some((rev, img.clone()));
        img
    }

    /// 合成全图（纸色底 + 所有可见层，含图层不透明度），导出与缩略图使用。
    pub fn composite(&self) -> Image {
        let (w, h) = (self.width, self.height);
        let mut out = vec![0u8; w as usize * h as usize * 4];
        let paper = [self.paper.r, self.paper.g, self.paper.b, self.paper.a];
        for px in out.chunks_exact_mut(4) {
            px.copy_from_slice(&paper);
        }
        for layer in &self.layers {
            if !layer.visible || layer.opacity <= 0.0 {
                continue;
            }
            let op = layer.opacity.clamp(0.0, 1.0);
            for (dst, src) in out.chunks_exact_mut(4).zip(layer.pixels.chunks_exact(4)) {
                let sa = src[3] as f32 / 255.0 * op;
                if sa <= 0.0 {
                    continue;
                }
                let da = dst[3] as f32 / 255.0;
                let out_a = sa + da * (1.0 - sa);
                if out_a <= 0.0 {
                    dst.fill(0);
                    continue;
                }
                for c in 0..3 {
                    let v = (src[c] as f32 * sa + dst[c] as f32 * da * (1.0 - sa)) / out_a;
                    dst[c] = v.round().clamp(0.0, 255.0) as u8;
                }
                dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
        Image::from_rgba8(w, h, out)
    }

    /// 是否还没有任何笔画（导出页“空白画布”提示用）。
    pub fn is_blank(&self) -> bool {
        self.stroke_count == 0
    }

    /// 层像素总字节数（内存估算）。
    pub fn byte_len(&self) -> usize {
        self.layers.iter().map(|l| l.byte_len()).sum()
    }
}

/// 把浮点矩形裁剪到画布并取整（向外取整，保证边缘不被截掉）。
fn clip_rect(x0: f32, y0: f32, x1: f32, y1: f32, w: u32, h: u32) -> DirtyRect {
    let (x0, x1) = if x0.is_finite() && x1.is_finite() {
        (x0, x1)
    } else {
        return DirtyRect::empty();
    };
    let (y0, y1) = if y0.is_finite() && y1.is_finite() {
        (y0, y1)
    } else {
        return DirtyRect::empty();
    };
    let x0 = x0.floor().max(0.0) as i64;
    let y0 = y0.floor().max(0.0) as i64;
    let x1 = x1.ceil().min(w as f32) as i64;
    let y1 = y1.ceil().min(h as f32) as i64;
    if x1 <= x0 || y1 <= y0 {
        return DirtyRect::empty();
    }
    DirtyRect {
        x0: x0 as u32,
        y0: y0 as u32,
        x1: x1 as u32,
        y1: y1 as u32,
    }
}

/// 两矩形求交；任一为空返回空。
fn intersect(a: DirtyRect, b: DirtyRect) -> DirtyRect {
    if a.is_empty() || b.is_empty() {
        return DirtyRect::empty();
    }
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    if x1 <= x0 || y1 <= y0 {
        return DirtyRect::empty();
    }
    DirtyRect { x0, y0, x1, y1 }
}

/// 拷贝矩形区域像素（行优先）。
fn copy_region(pixels: &[u8], w: u32, rect: DirtyRect) -> Vec<u8> {
    if rect.is_empty() {
        return Vec::new();
    }
    let row = (rect.x1 - rect.x0) as usize * 4;
    let mut out = Vec::with_capacity(row * (rect.y1 - rect.y0) as usize);
    for y in rect.y0..rect.y1 {
        let start = ((y as u64 * w as u64 + rect.x0 as u64) * 4) as usize;
        out.extend_from_slice(&pixels[start..start + row]);
    }
    out
}

/// 把矩形数据贴回像素缓冲。
fn paste_region(pixels: &mut [u8], w: u32, rect: DirtyRect, data: &[u8]) {
    if rect.is_empty() {
        return;
    }
    let row = (rect.x1 - rect.x0) as usize * 4;
    debug_assert_eq!(data.len(), row * (rect.y1 - rect.y0) as usize);
    for (i, y) in (rect.y0..rect.y1).enumerate() {
        let start = ((y as u64 * w as u64 + rect.x0 as u64) * 4) as usize;
        pixels[start..start + row].copy_from_slice(&data[i * row..(i + 1) * row]);
    }
}

/// 从已拷贝的 `from_rect` 数据中裁出 `to` 子矩形（`to` 必须落在 `from_rect` 内）。
fn crop_region(from: &[u8], from_rect: DirtyRect, to: DirtyRect) -> Vec<u8> {
    if to.is_empty() {
        return Vec::new();
    }
    let src_row = (from_rect.x1 - from_rect.x0) as usize * 4;
    let dst_row = (to.x1 - to.x0) as usize * 4;
    let mut out = Vec::with_capacity(dst_row * (to.y1 - to.y0) as usize);
    for y in to.y0..to.y1 {
        let dy = (y - from_rect.y0) as usize;
        let dx = (to.x0 - from_rect.x0) as usize * 4;
        let base = dy * src_row + dx;
        if base + dst_row > from.len() {
            break;
        }
        out.extend_from_slice(&from[base..base + dst_row]);
    }
    out
}

/// 由笔画序号生成稳定种子：同一序号 + 同一尺寸得到同一纹理。
fn seed_for(stroke_count: u32, w: u32, h: u32) -> u32 {
    stroke_count
        .wrapping_mul(2_654_435_761)
        .wrapping_add(w.wrapping_mul(40_503))
        .wrapping_add(h.wrapping_mul(12_289))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn black() -> Color {
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }
    }

    fn paper() -> Color {
        Color {
            r: 247,
            g: 245,
            b: 239,
            a: 255,
        }
    }

    fn doc(w: u32, h: u32) -> Document {
        let mut d = Document::new(w, h, paper(), Brush::preset(BrushKind::Pen));
        d.color = black();
        d
    }

    fn alpha_at(d: &Document, x: u32, y: u32) -> u8 {
        d.active_layer().pixels[((y * d.width + x) * 4 + 3) as usize]
    }

    fn stroke_across(d: &mut Document) -> bool {
        d.apply_stroke(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(28.0, 8.0, 1.0, 10),
        ])
    }

    #[test]
    fn new_document_has_two_empty_layers_and_paper() {
        let d = doc(32, 16);
        assert_eq!(d.layers.len(), 2);
        assert_eq!(d.paper, paper());
        assert!(d.layers.iter().all(|l| l.is_blank()));
        assert!(d.is_blank());
        assert_eq!(d.byte_len(), 32 * 16 * 4 * 2);
    }

    #[test]
    fn apply_stroke_paints_active_layer() {
        let mut d = doc(32, 16);
        assert!(stroke_across(&mut d));
        assert!(alpha_at(&d, 16, 8) > 0, "轨迹中部应被涂上");
        assert_eq!(d.stroke_count, 1);
        assert_eq!(d.history.depth(), 1);
        assert!(d.history.can_undo());
        assert!(!d.history.can_redo());
    }

    #[test]
    fn steps_follow_undo_and_redo() {
        // 导出页统计改用 steps()：撤销后必须同步减少，否则文案与画面矛盾。
        let mut d = doc(32, 16);
        assert_eq!(d.steps(), 0);
        assert!(stroke_across(&mut d));
        assert_eq!(d.steps(), 1);
        assert!(d.undo());
        assert_eq!(d.steps(), 0);
        assert!(d.redo());
        assert_eq!(d.steps(), 1);
        // stroke_count 是抖动种子，撤销后不减，两者语义不同。
        assert_eq!(d.stroke_count, 1);
    }

    #[test]
    fn apply_empty_stroke_is_noop() {
        let mut d = doc(16, 16);
        assert!(!d.apply_stroke(&[]));
        assert_eq!(d.history.depth(), 0);
        assert_eq!(d.stroke_count, 0);
    }

    #[test]
    fn stroke_fully_outside_canvas_is_noop() {
        let mut d = doc(16, 16);
        assert!(!d.apply_stroke(&[StrokePoint::new(-100.0, -100.0, 1.0, 0)]));
        assert_eq!(d.history.depth(), 0);
    }

    #[test]
    fn undo_restores_pixels_exactly() {
        let mut d = doc(32, 16);
        let before = d.active_layer().pixels.clone();
        assert!(stroke_across(&mut d));
        let after = d.active_layer().pixels.clone();
        assert_ne!(before, after);
        assert!(d.undo());
        assert_eq!(d.active_layer().pixels, before, "撤销必须逐像素恢复");
        assert!(!d.history.can_undo());
        assert!(d.history.can_redo());
    }

    #[test]
    fn redo_reapplies_pixels() {
        let mut d = doc(32, 16);
        stroke_across(&mut d);
        let inked = d.active_layer().pixels.clone();
        d.undo();
        assert!(d.redo());
        assert_eq!(d.active_layer().pixels, inked);
        assert!(!d.history.can_redo());
    }

    #[test]
    fn undo_on_empty_history_returns_false() {
        let mut d = doc(8, 8);
        assert!(!d.undo());
        assert!(!d.redo());
    }

    #[test]
    fn new_stroke_clears_redo_stack() {
        let mut d = doc(32, 16);
        stroke_across(&mut d);
        d.undo();
        assert!(d.history.can_redo());
        stroke_across(&mut d);
        assert!(!d.history.can_redo(), "新笔画必须清空重做栈");
    }

    #[test]
    fn history_limit_drops_oldest_steps() {
        let mut d = doc(64, 16);
        d.history = History::new(3);
        for i in 0..5 {
            let y = 3.0 + i as f32 * 2.0;
            d.apply_stroke(&[
                StrokePoint::new(4.0, y, 1.0, i),
                StrokePoint::new(60.0, y, 1.0, i),
            ]);
        }
        assert_eq!(d.history.depth(), 3, "上限 3 应只保留最近 3 步");
        let mut undone = 0;
        while d.undo() {
            undone += 1;
        }
        assert_eq!(undone, 3);
    }

    #[test]
    fn composite_blank_canvas_equals_paper() {
        let d = doc(4, 4);
        let img = d.composite();
        assert_eq!(img.width(), 4);
        for px in img.pixels().chunks_exact(4) {
            assert_eq!(px, [247, 245, 239, 255]);
        }
    }

    #[test]
    fn composite_puts_ink_over_paper() {
        let mut d = doc(32, 16);
        stroke_across(&mut d);
        let img = d.composite();
        let i = ((8 * d.width + 16) * 4) as usize;
        assert_eq!(
            &img.pixels()[i..i + 4],
            &[0, 0, 0, 255],
            "不透明墨迹应盖住纸色"
        );
    }

    #[test]
    fn hidden_layer_is_skipped_in_composite() {
        let mut d = doc(32, 16);
        stroke_across(&mut d);
        d.layers[0].visible = false;
        let img = d.composite();
        let i = ((8 * d.width + 16) * 4) as usize;
        assert_eq!(
            &img.pixels()[i..i + 4],
            &[247, 245, 239, 255],
            "隐藏层不应参与合成"
        );
    }

    #[test]
    fn image_cache_reuses_pixels_until_revision_changes() {
        let mut d = doc(32, 16);
        let a = d.image();
        let b = d.image();
        assert!(
            std::ptr::eq(a.pixels().as_ptr(), b.pixels().as_ptr()),
            "无改动应复用同一份像素"
        );
        stroke_across(&mut d);
        let c = d.image();
        assert!(
            !std::ptr::eq(a.pixels().as_ptr(), c.pixels().as_ptr()),
            "改动后应重建"
        );
    }

    #[test]
    fn undo_after_two_strokes_steps_back_one_at_a_time() {
        let mut d = doc(32, 32);
        d.apply_stroke(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(28.0, 8.0, 1.0, 1),
        ]);
        let after_first = d.active_layer().pixels.clone();
        d.apply_stroke(&[
            StrokePoint::new(4.0, 20.0, 1.0, 2),
            StrokePoint::new(28.0, 20.0, 1.0, 3),
        ]);
        assert!(d.undo());
        assert_eq!(d.active_layer().pixels, after_first, "只回退最后一笔");
    }

    #[test]
    fn live_stroke_paints_immediately_and_records_one_step() {
        let mut d = doc(32, 16);
        assert!(d.begin_live());
        assert!(d.is_live());
        assert!(d.extend_live(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(16.0, 8.0, 1.0, 1),
        ]));
        assert!(alpha_at(&d, 10, 8) > 0, "实时段应立即可见");
        assert_eq!(d.history.depth(), 0, "抬手前不压栈");
        assert!(d.extend_live(&[
            StrokePoint::new(16.0, 8.0, 1.0, 1),
            StrokePoint::new(28.0, 8.0, 1.0, 2),
        ]));
        assert!(d.end_live());
        assert!(!d.is_live());
        assert_eq!(d.history.depth(), 1, "整笔只压一步");
        assert_eq!(d.stroke_count, 1);
    }

    #[test]
    fn end_live_without_ink_is_noop() {
        let mut d = doc(16, 16);
        assert!(d.begin_live());
        assert!(!d.end_live());
        assert_eq!(d.history.depth(), 0);
        assert_eq!(d.stroke_count, 0);
    }

    #[test]
    fn end_live_without_begin_returns_false() {
        let mut d = doc(16, 16);
        assert!(!d.end_live());
    }

    #[test]
    fn cancel_live_restores_pixels_and_skips_history() {
        let mut d = doc(32, 16);
        let before = d.active_layer().pixels.clone();
        d.begin_live();
        d.extend_live(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(28.0, 8.0, 1.0, 1),
        ]);
        assert_ne!(d.active_layer().pixels, before);
        assert!(d.cancel_live());
        assert_eq!(d.active_layer().pixels, before, "取消必须像素级回滚");
        assert_eq!(d.history.depth(), 0);
        assert_eq!(d.stroke_count, 0);
    }

    #[test]
    fn extend_live_auto_begins_when_needed() {
        let mut d = doc(32, 16);
        assert!(d.extend_live(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(28.0, 8.0, 1.0, 1),
        ]));
        assert!(d.is_live());
        assert!(d.end_live());
    }

    #[test]
    fn live_stroke_is_undoable_as_single_step() {
        let mut d = doc(32, 16);
        let before = d.active_layer().pixels.clone();
        d.begin_live();
        d.extend_live(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(16.0, 8.0, 1.0, 1),
        ]);
        d.extend_live(&[
            StrokePoint::new(16.0, 8.0, 1.0, 1),
            StrokePoint::new(28.0, 8.0, 1.0, 2),
        ]);
        d.end_live();
        assert!(d.undo());
        assert_eq!(d.active_layer().pixels, before, "撤销整笔应回到按下前");
    }
}
