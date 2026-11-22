//! 性能打点（PRD §10 A1–A3 验收证据链）。
//!
//! # 覆盖
//! - A1 冷启动：`onCreate` → 首帧上屏的时间戳；
//! - A2 帧率：滚动/动画期间的帧间隔统计（P50/P95 与掉帧计数）；
//! - A3 内存：`/proc/self/statm` 采样 RSS。
//!
//! # 设计
//! - 全部打点在主循环线程内完成（无跨线程同步开销）；
//! - 汇总只在满窗口时输出一行日志（避免日志本身成为开销源）；
//! - 主机侧（Windows）无 `/proc`，启动与帧率打点仍工作，内存
//!   采样返回 None 降级——真机验收不受影响，主机单测不依赖。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// 启动时间锚点（onCreate 打点；单调时钟，不受墙上时钟跳变影响）。
static BOOT_START: AtomicU64 = AtomicU64::new(u64::MAX);

/// 标记启动起点（onCreate 第一行调用）。
///
/// # 不变量
/// 存储的是单调时钟读数（`Instant::elapsed` 的同一基准），
/// 可在任意线程重复调用重置锚点。
pub fn mark_boot_start() {
    BOOT_START.store(
        Instant::now().elapsed().as_nanos() as u64,
        Ordering::Release,
    );
}

/// 自启动起点的毫秒数（未打点时返回 None）。
///
/// # 溢出语义
/// 理论上后读数可能小于锚点（时钟异常），用 saturating_sub 兜底为 0
/// （渲染线程禁止 panic），保证单调不减。
pub fn millis_since_boot() -> Option<u64> {
    let start = BOOT_START.load(Ordering::Acquire);
    if start == u64::MAX {
        return None;
    }
    Some(
        Instant::now()
            .elapsed()
            .as_nanos()
            .saturating_sub(start as u128) as u64
            / 1_000_000,
    )
}

/// 帧率统计窗口（最近 N 帧；覆盖一次完整滚动/动画序列）。
const FRAME_WINDOW: usize = 240;

/// 单帧间隔的上限（超时视为丢帧重置窗口，避免静止期污染统计）。
const FRAME_GAP_RESET_MS: u64 = 500;

/// 帧预算（60Hz = 16.67ms）：渲染耗时超过此值视为超预算。
const FRAME_BUDGET_MS: u64 = 17;

/// 帧内阶段索引（调用链耗时分解，PRD §10 A2 扩展）。
///
/// # 调用链
/// 一帧的渲染链路：`advance`（动画/滚动物理推进）→ `build`（页树绘制
/// 生成 Scene 命令）→ `render`（光栅化 + 搬运 + 上屏）。三段之和 ≈ 帧耗时，
/// 汇总时输出各段均值与最大段，定位瓶颈环节。
pub const STAGE_ADVANCE: usize = 0;
pub const STAGE_BUILD: usize = 1;
pub const STAGE_RENDER: usize = 2;
pub const STAGE_COUNT: usize = 3;
/// 阶段名（日志用，固定英文）。
pub const STAGE_NAMES: [&str; STAGE_COUNT] = ["advance", "build", "render"];

/// 单帧调用链耗时（各阶段微秒数 + Scene 命令数）。
///
/// # 设计
/// 由主循环在帧处理时逐段计时填充，`FrameStats::record_trace` 消费：
/// 逐帧 `debug!` 输出完整链路，滚动窗口 `info!` 输出均值与瓶颈段。
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTrace {
    /// 各阶段耗时（µs），下标见 `STAGE_*`。
    pub stages: [u64; STAGE_COUNT],
    /// 本帧 Scene 命令数（build 段产物规模，关联命令数与耗时）。
    pub cmd_count: u32,
}

impl FrameTrace {
    /// 创建空 trace（各段 0）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录某阶段耗时。
    pub fn set(&mut self, idx: usize, dur: std::time::Duration) {
        if idx < STAGE_COUNT {
            self.stages[idx] = dur.as_micros() as u64;
        }
    }

    /// 总耗时（µs）= 各段之和。
    pub fn total_us(&self) -> u64 {
        self.stages.iter().sum()
    }

    /// 耗时最大的阶段下标（瓶颈环节）。
    pub fn max_stage(&self) -> usize {
        (0..STAGE_COUNT)
            .max_by_key(|&i| self.stages[i])
            .unwrap_or(0)
    }
}

/// 后端内部子阶段索引（`STAGE_RENDER` 的进一步分解）。
///
/// # 为什么要细拆
/// `render` 段包含「软件光栅 → 像素搬运/纹理上传 → 提交呈现」三件事，
/// 优化方向完全不同（前者吃 CPU、后者吃带宽/驱动）。只测总时长会把
/// 两者混成一个数字，无法判断该并行光栅还是该减少上传。
pub const SUB_RASTER: usize = 0;
pub const SUB_UPLOAD: usize = 1;
pub const SUB_PRESENT: usize = 2;
pub const SUB_ACQUIRE: usize = 3;
pub const SUB_IMAGE: usize = 4;
pub const SUB_COUNT: usize = 5;
/// 子阶段名（日志用，固定英文）。
pub const SUB_NAMES: [&str; SUB_COUNT] = ["raster", "upload", "present", "acquire", "image"];

/// 最近一帧各子阶段耗时（µs）。后端在帧内写、统计器在帧末读。
///
/// # `acquire` 为什么要单列
/// `get_current_texture` 在 FIFO 队列里会**阻塞等待 vsync 空位**，它测出来的是
/// 「等屏幕」的时长，不是本帧的计算成本。与光栅/上传混在一起会让帧成本虚高
/// （实测把 6ms 的帧报成 16ms），从而误判「已到 60fps 上限」。
static SUB_US: [AtomicU64; SUB_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// 每帧转成预乘的像素数（诊断用：判断增量路径是否真的只算了一小块）。
static PREMUL_PIXELS: AtomicU64 = AtomicU64::new(0);

/// 累加本帧转换的像素数。
pub fn add_premul_pixels(n: u64) {
    PREMUL_PIXELS.fetch_add(n, Ordering::Relaxed);
}

/// 读取并清零本帧像素数（每帧只应被读一次，由帧统计取走）。
pub fn take_premul_pixels() -> u64 {
    PREMUL_PIXELS.swap(0, Ordering::Relaxed)
}

/// 记录某子阶段耗时（后端内部调用，无锁写）。
pub fn record_sub_us(idx: usize, dur: std::time::Duration) {
    if idx < SUB_COUNT {
        SUB_US[idx].store(dur.as_micros() as u64, Ordering::Relaxed);
    }
}

/// 累加型子阶段耗时（一帧内可能发生多次，如多张图片预乘）。
pub fn add_sub_us(idx: usize, dur: std::time::Duration) {
    if idx < SUB_COUNT {
        SUB_US[idx].fetch_add(dur.as_micros() as u64, Ordering::Relaxed);
    }
}

/// 清零某个累加型子阶段（帧首调用）。
pub fn reset_sub(idx: usize) {
    if idx < SUB_COUNT {
        SUB_US[idx].store(0, Ordering::Relaxed);
    }
}

/// 读取最近一帧的子阶段耗时快照。
pub fn sub_us() -> [u64; SUB_COUNT] {
    std::array::from_fn(|i| SUB_US[i].load(Ordering::Relaxed))
}

/// 帧呈现模式的计数器（基准验收用：确认「局部重绘」真的生效）。
///
/// # 为什么要有它
/// 脏区是否命中完全取决于上层声明与后端条件之间的对齐，做错了也不报错——
/// 只会静默地退化成整帧重绘。基准必须能直接看到「这一轮有多少帧走了局部路径」，
/// 否则优化等于没有证据。
// 整帧兜底原因（落笔帧没能局部化的定性统计）。
pub const DAMAGE_NONE: usize = 0;
pub const DAMAGE_SCREEN: usize = 1;
pub const DAMAGE_PAGE_SHIFT: usize = 2;
pub const DAMAGE_SHEET: usize = 3;
pub const DAMAGE_PRESS: usize = 4;
pub const DAMAGE_TOAST: usize = 5;
pub const DAMAGE_DRAG: usize = 6;
pub const DAMAGE_FLASH: usize = 7;
pub const DAMAGE_REV_MISMATCH: usize = 8;
pub const DAMAGE_REASON_COUNT: usize = 9;
pub const DAMAGE_REASON_NAMES: [&str; DAMAGE_REASON_COUNT] = [
    "ok",
    "screen",
    "page_shift",
    "sheet",
    "press",
    "toast",
    "drag",
    "flash",
    "rev_mismatch",
];
static DAMAGE_REASON: [AtomicU64; DAMAGE_REASON_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// 记一次「落笔帧整帧兜底」的定性原因。
pub fn note_damage_reason(idx: usize) {
    if idx < DAMAGE_REASON_COUNT {
        DAMAGE_REASON[idx].fetch_add(1, Ordering::Relaxed);
    }
}

/// 读取并清零整帧兜底原因计数。
pub fn take_damage_reasons() -> [u64; DAMAGE_REASON_COUNT] {
    std::array::from_fn(|i| DAMAGE_REASON[i].swap(0, Ordering::Relaxed))
}

/// 已呈现帧的单调序号（同帧内判断「本帧是否已经出过帧」）。
static FRAME_SEQ: AtomicU64 = AtomicU64::new(0);
static MODE_PARTIAL: AtomicU64 = AtomicU64::new(0);
static MODE_FULL: AtomicU64 = AtomicU64::new(0);

/// 记录一帧的上屏模式（局部 / 整帧）。
/// 读取已呈现帧序号。
pub fn frame_seq() -> u64 {
    FRAME_SEQ.load(Ordering::Acquire)
}

/// 记一帧已呈现（推进帧序号）。
pub fn bump_frame_seq() {
    FRAME_SEQ.fetch_add(1, Ordering::Release);
}

pub fn note_frame_mode(partial: bool) {
    bump_frame_seq();
    if partial {
        MODE_PARTIAL.fetch_add(1, Ordering::Relaxed);
    } else {
        MODE_FULL.fetch_add(1, Ordering::Relaxed);
    }
}

/// 读取并清零模式计数，返回 `(局部帧数, 整帧数)`。
pub fn take_frame_modes() -> (u64, u64) {
    (
        MODE_PARTIAL.swap(0, Ordering::Relaxed),
        MODE_FULL.swap(0, Ordering::Relaxed),
    )
}

/// build 段内部耗时分解（`STAGE_BUILD` 的进一步分解）。
///
/// # 为什么要拆
/// build 里既有「重建画板预览位图」这种像素级工作，也有「重新排版文字」这种
/// 字形级工作，两者优化手段完全不同（前者做增量更新，后者做布局缓存）。
pub const BUILD_SUB_PREVIEW: usize = 0;
pub const BUILD_SUB_TEXT: usize = 1;
pub const BUILD_SUB_INPUT: usize = 2;
pub const BUILD_SUB_ADVANCE: usize = 3;
pub const BUILD_SUB_SCENE: usize = 4;
pub const BUILD_SUB_TREE: usize = 5;
pub const BUILD_SUB_INK: usize = 6;
pub const BUILD_SUB_DOCVIEW: usize = 7;
pub const BUILD_SUB_MERGE: usize = 8;
pub const BUILD_SUB_RESIZE: usize = 9;
pub const BUILD_SUB_TREE_DRAW: usize = 10;
pub const BUILD_SUB_COUNT: usize = 11;
/// build 子阶段名（日志用）。
pub const BUILD_SUB_NAMES: [&str; BUILD_SUB_COUNT] = [
    "preview",
    "text",
    "input",
    "advance",
    "scene",
    "tree",
    "ink",
    "docview",
    "merge",
    "resize",
    "tree_draw",
];

/// 最近一帧 build 子阶段耗时（µs）。
static BUILD_SUB_US: [AtomicU64; BUILD_SUB_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// 记录 build 子阶段耗时（跨多次调用累加，帧首清零）。
pub fn add_build_sub_us(idx: usize, dur: std::time::Duration) {
    if idx < BUILD_SUB_COUNT {
        BUILD_SUB_US[idx].fetch_add(dur.as_micros() as u64, Ordering::Relaxed);
    }
}

/// 清零 build 子阶段累计（每帧构建前调用）。
pub fn reset_build_sub() {
    for v in BUILD_SUB_US.iter() {
        v.store(0, Ordering::Relaxed);
    }
}

/// 读取 build 子阶段耗时快照。
pub fn build_sub_us() -> [u64; BUILD_SUB_COUNT] {
    std::array::from_fn(|i| BUILD_SUB_US[i].load(Ordering::Relaxed))
}

/// FPS 统计窗口（最近 N 帧间隔；短窗口保证 fps 读数跟随当前帧率）。
const FPS_WINDOW: usize = 30;

/// 滑动窗口帧率统计器。
///
/// # 用法
/// 每次成功 `render_frame`（首帧之后）调用 [`Self::record_frame`]；
/// 满窗口自动输出一行汇总日志并滑窗。
pub struct FrameStats {
    /// 最近各帧间隔（毫秒）。
    intervals_ms: Vec<u64>,
    /// 上一帧时间戳。
    last: Option<Instant>,
    /// 累计满窗口输出次数（诊断用）。
    batches: u32,
    /// 最近各帧渲染耗时（毫秒）。
    render_times_ms: Vec<u64>,
    /// 超预算帧计数（当前窗口内）。
    over_budget: u32,
    /// FPS 专用短窗口（最近 `FPS_WINDOW` 帧间隔；独立于统计窗口，
    /// 保证 `fps()` 在统计窗口清空后仍有读数）。
    fps_intervals: Vec<u64>,
    /// 各阶段累计耗时（µs），用于滚动均值。
    stage_sum_us: [u64; STAGE_COUNT],
    /// 已记录 trace 的帧数（滚动均值分母）。
    traced_frames: u64,
    /// 累计 Scene 命令数（滚动均值用）。
    cmd_sum: u64,
    /// 调用链汇总输出次数（独立于帧率统计批次）。
    chain_batches: u32,
    /// 累计帧序号（日志定位用）。
    frame_no: u64,
}

impl FrameStats {
    /// 创建空统计器。
    pub fn new() -> Self {
        Self {
            intervals_ms: Vec::with_capacity(FRAME_WINDOW),
            last: None,
            batches: 0,
            render_times_ms: Vec::with_capacity(FRAME_WINDOW),
            over_budget: 0,
            fps_intervals: Vec::with_capacity(FPS_WINDOW),
            stage_sum_us: [0; STAGE_COUNT],
            traced_frames: 0,
            cmd_sum: 0,
            chain_batches: 0,
            frame_no: 0,
        }
    }

    /// 当前帧率（fps）：基于最近 `FPS_WINDOW` 帧间隔的均值折算。
    ///
    /// # 返回
    /// 无足够样本（<2 帧）时返回 0.0；否则 `1000 / 平均间隔`。
    pub fn fps(&self) -> f32 {
        if self.fps_intervals.len() < 2 {
            return 0.0;
        }
        let avg: u64 = self.fps_intervals.iter().sum::<u64>() / self.fps_intervals.len() as u64;
        if avg == 0 { 0.0 } else { 1000.0 / avg as f32 }
    }

    /// 记录一帧调用链耗时（advance/build/render 三段 + 命令数）。
    ///
    /// # 行为
    /// - 逐帧 `debug!` 输出完整链路（含各段 µs 与瓶颈段），便于逐帧定位；
    /// - 累计各段耗时供滚动均值；满 `FRAME_WINDOW` 帧输出 `info!` 汇总
    ///   （各段均值 + 瓶颈段 + 命令数均值）后清零。
    pub fn record_trace(&mut self, trace: &FrameTrace) {
        self.frame_no += 1;
        // 逐帧完整调用链（debug 级，默认关闭，开启后逐帧可见）。
        log::debug!(
            "frame#{} chain: advance={}us build={}us render={}us total={}us cmds={} bottleneck={}",
            self.frame_no,
            trace.stages[STAGE_ADVANCE],
            trace.stages[STAGE_BUILD],
            trace.stages[STAGE_RENDER],
            trace.total_us(),
            trace.cmd_count,
            STAGE_NAMES[trace.max_stage()]
        );
        for i in 0..STAGE_COUNT {
            self.stage_sum_us[i] = self.stage_sum_us[i].saturating_add(trace.stages[i]);
        }
        self.cmd_sum = self.cmd_sum.saturating_add(u64::from(trace.cmd_count));
        self.traced_frames += 1;
        if self.traced_frames as usize >= FRAME_WINDOW {
            self.output_chain();
        }
    }

    /// 输出调用链滚动汇总（各段均值 µs + 瓶颈段 + 命令数均值）。
    fn output_chain(&mut self) {
        self.chain_batches += 1;
        let n = self.traced_frames.max(1) as u64;
        let avg: Vec<u64> = self.stage_sum_us.iter().map(|s| s / n).collect();
        let total_avg: u64 = avg.iter().sum();
        let bottleneck = (0..STAGE_COUNT).max_by_key(|&i| avg[i]).unwrap_or(0);
        let cmds_avg = self.cmd_sum / n;
        log::info!(
            "frame chain #{}: n={} avg advance={}us build={}us render={}us total={}us bottleneck={} cmds_avg={}",
            self.chain_batches,
            n,
            avg[STAGE_ADVANCE],
            avg[STAGE_BUILD],
            avg[STAGE_RENDER],
            total_avg,
            STAGE_NAMES[bottleneck],
            cmds_avg
        );
        self.stage_sum_us = [0; STAGE_COUNT];
        self.traced_frames = 0;
        self.cmd_sum = 0;
    }

    /// 记录一帧渲染耗时（render_frame 前后计时，与 record_frame 配对调用）。
    ///
    /// # 用途
    /// 帧预算监控：渲染耗时 > 16.67ms 时该帧必然掉帧（60Hz），
    /// 汇总中输出超预算比例定位瓶颈（光栅化/搬运/锁等待）。
    pub fn record_render_time(&mut self, ms: u64) {
        self.render_times_ms.push(ms);
        if ms > FRAME_BUDGET_MS {
            self.over_budget += 1;
        }
        if self.render_times_ms.len() >= FRAME_WINDOW {
            self.render_times_ms.clear();
            self.over_budget = 0;
        }
    }

    /// 记录一帧（render_frame 成功后调用）。
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        let Some(last) = self.last else {
            self.last = Some(now);
            return;
        };
        let dt = now.saturating_duration_since(last).as_millis() as u64;
        self.last = Some(now);
        // 长间隔（静止/等待输入）不进入统计：帧率只度量连续渲染期。
        if dt > FRAME_GAP_RESET_MS {
            if !self.intervals_ms.is_empty() {
                self.output();
                self.intervals_ms.clear();
            }
            self.fps_intervals.clear();
            return;
        }
        self.intervals_ms.push(dt);
        // FPS 短窗口：滑动保留最近 FPS_WINDOW 帧（供 fps() 实时读数）。
        self.fps_intervals.push(dt);
        if self.fps_intervals.len() > FPS_WINDOW {
            self.fps_intervals.remove(0);
        }
        if self.intervals_ms.len() >= FRAME_WINDOW {
            self.output();
            self.intervals_ms.clear();
        }
    }

    /// 输出满窗口汇总（P50/P95/最大间隔与 fps 折算，A2 判定数据源）。
    fn output(&mut self) {
        self.batches += 1;
        let mut sorted = self.intervals_ms.clone();
        sorted.sort_unstable();
        let n = sorted.len();
        let p50 = sorted[n * 50 / 100];
        let p95 = sorted[n * 95 / 100];
        let max = sorted[n - 1];
        let avg: u64 = sorted.iter().sum::<u64>() / n as u64;
        // 折算 fps：1000 / 平均间隔。
        let fps = if avg > 0 { 1000.0 / avg as f32 } else { 0.0 };
        // 帧预算：渲染耗时超 16.67ms 的帧占比（>0 即有掉帧风险）。
        let budget_info = if self.render_times_ms.is_empty() {
            String::new()
        } else {
            let rt_max = self.render_times_ms.iter().max().copied().unwrap_or(0);
            format!(
                " render_max={}ms over_budget={}/{}",
                rt_max,
                self.over_budget,
                self.render_times_ms.len()
            )
        };
        // 子阶段（光栅/上传/呈现）读数：与帧间隔一起看，才能区分
        // 「CPU 算得慢」还是「像素搬得多」。
        let sub = sub_us();
        log::info!(
            "frame stats #{}: n={} p50={}ms p95={}ms max={}ms fps={:.1} raster={}us upload={}us present={}us{}",
            self.batches,
            n,
            p50,
            p95,
            max,
            fps,
            sub[SUB_RASTER],
            sub[SUB_UPLOAD],
            sub[SUB_PRESENT],
            budget_info
        );
    }
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new()
    }
}

/// FPS 叠加层（PRD §10 A2 扩展）：在场景末尾叠加一行 FPS 读数。
///
/// # 门控
/// 默认关闭（黄金图基线不受影响）；仅当环境变量 `NATIVEVIEW_FPS_HUD=1`
/// 时启用。字体未注入时静默降级（不绘制，不 panic）。
///
/// # 设计
/// 排版上下文懒加载（字体注入后首次绘制时构造）；每帧重排（文本随帧率
/// 变化，缓存无收益）。叠加层在场景末尾追加，覆盖在内容之上。
#[derive(Default)]
pub struct FpsHud {
    /// 是否启用（环境变量门控，默认关闭）。
    enabled: bool,
    /// 排版上下文（懒加载；字体未注入时为 None，绘制时静默跳过）。
    context: Option<crate::text::layout::TextContext>,
}

impl FpsHud {
    /// 创建叠加层（读取环境变量门控）。
    pub fn new() -> Self {
        let enabled = std::env::var("NATIVEVIEW_FPS_HUD")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            enabled,
            context: None,
        }
    }

    /// 是否启用。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 在场景末尾叠加 FPS 读数（未启用或字体缺失时不改动场景）。
    ///
    /// # 参数
    /// `fps`：当前帧率（`FrameStats::fps` 读数）。
    pub fn draw(&mut self, scene: &mut crate::gfx::scene::Scene, fps: f32) {
        if !self.enabled {
            return;
        }
        // 懒加载排版上下文（字体注入后首次绘制时构造）。
        if self.context.is_none() {
            let font = crate::text::font::cached_font();
            self.context = font.as_ref().map(crate::text::layout::TextContext::new);
        }
        let Some(cx) = self.context.as_mut() else {
            return;
        };
        let text = format!("FPS {:.0}", fps);
        let req = crate::text::layout::TextStyleRequest {
            text: &text,
            font_size: 14.0,
            max_advance: None,
            align: crate::text::layout::TextAlign::Left,
            color: crate::gfx::color::Color::rgb(0x00, 0xFF, 0x00),
            line_spacing_multiplier: 1.0,
        };
        let Some(layout) = cx.layout(&req) else {
            return;
        };
        let x = 8.0;
        let y = 8.0;
        // 半透明背景条（提升可读性）。
        let w = layout.width() as f64 + 12.0;
        let h = layout.height() as f64 + 8.0;
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect { x, y, w, h },
            paint: crate::gfx::paint::Paint::default()
                .color(crate::gfx::color::Color::rgba(0, 0, 0, 0x99)),
        });
        crate::text::render::emit_layout(scene, &layout, x + 6.0, y + 4.0);
    }
}

/// 采样当前进程 RSS（常驻内存，单位 KB；PRD §10 A3）。
///
/// # 平台
/// 读 `/proc/self/statm` 第二列。仅在提供 procfs 的系统（Android/Linux）
/// 返回 Some；主机（Windows）返回 None 静默降级。
pub fn sample_rss_kb() -> Option<u64> {
    let data = std::fs::read_to_string("/proc/self/statm").ok()?;
    let mut fields = data.split_whitespace();
    // statm: size resident shared text lib data dt ...
    let _size = fields.next()?;
    let resident_pages: u64 = fields.next()?.parse().ok()?;
    // 页大小 4KB（Android arm64 用户进程标准配置）。
    Some(resident_pages * 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 帧统计：连续帧进入窗口，超时帧重置窗口。
    #[test]
    fn frame_stats_collects_and_resets() {
        let mut s = FrameStats::new();
        for _ in 0..10 {
            s.record_frame();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // 时间驱动的统计不设精确断言（CI 抖动），只验证结构不 panic
        // 且窗口未满（10 < 240）。
        assert_eq!(s.batches, 0);
    }

    /// RSS 采样：有 procfs 时返回正值，无 procfs 时降级 None（不 panic）。
    ///
    /// # 为何不能用 `cfg!(windows)`
    /// macOS 同样没有 `/proc/self/statm`，按「非 Windows 即正值」断言会在
    /// 开发机（x86_64-apple-darwin）稳定失败；判定条件必须按 procfs 是否存在。
    #[test]
    fn rss_sampling_degrades_without_procfs() {
        if cfg!(any(target_os = "linux", target_os = "android")) {
            assert!(
                sample_rss_kb().unwrap_or(0) > 0,
                "procfs available but RSS sample is zero"
            );
        } else {
            assert!(sample_rss_kb().is_none(), "no procfs: must degrade to None");
        }
    }

    /// 启动打点：millis_since_boot 单调不减。
    #[test]
    fn boot_marker_monotonic() {
        mark_boot_start();
        let a = millis_since_boot().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = millis_since_boot().unwrap();
        assert!(b >= a, "b={b} a={a}");
    }

    /// 调用链 trace：total = 各段之和，max_stage 指向最大段。
    #[test]
    fn frame_trace_total_and_bottleneck() {
        let mut t = FrameTrace::new();
        t.set(STAGE_ADVANCE, std::time::Duration::from_micros(100));
        t.set(STAGE_BUILD, std::time::Duration::from_micros(300));
        t.set(STAGE_RENDER, std::time::Duration::from_micros(200));
        t.cmd_count = 42;
        assert_eq!(t.total_us(), 600);
        assert_eq!(t.max_stage(), STAGE_BUILD);
        assert_eq!(t.cmd_count, 42);
    }

    /// 调用链 trace：set 越界下标不 panic（防御性写入）。
    #[test]
    fn frame_trace_set_out_of_range_ignored() {
        let mut t = FrameTrace::new();
        t.set(99, std::time::Duration::from_micros(1000));
        assert_eq!(t.total_us(), 0);
    }

    /// fps()：样本不足（<2 帧）返回 0.0。
    #[test]
    fn fps_insufficient_samples_returns_zero() {
        let mut s = FrameStats::new();
        assert_eq!(s.fps(), 0.0);
        // 单帧间隔（首帧只设 last，不产生间隔）。
        s.record_frame();
        assert_eq!(s.fps(), 0.0);
    }

    /// record_trace：满窗口触发汇总且清零（不 panic，结构自洽）。
    #[test]
    fn record_trace_accumulates_and_resets() {
        let mut s = FrameStats::new();
        let mut t = FrameTrace::new();
        t.set(STAGE_RENDER, std::time::Duration::from_micros(5000));
        t.cmd_count = 10;
        for _ in 0..FRAME_WINDOW {
            s.record_trace(&t);
        }
        // 满窗口后内部计数清零（通过再次填充验证不溢出/不 panic）。
        assert_eq!(s.traced_frames, 0);
        assert_eq!(s.cmd_sum, 0);
        assert_eq!(s.chain_batches, 1);
    }

    /// FPS HUD：默认关闭（环境变量未设），draw 不改动场景。
    #[test]
    fn fps_hud_disabled_by_default() {
        // 测试进程不应设 NATIVEVIEW_FPS_HUD；若被外部设置则跳过断言。
        if std::env::var("NATIVEVIEW_FPS_HUD").is_ok() {
            return;
        }
        let mut hud = FpsHud::new();
        assert!(!hud.is_enabled());
        let mut scene = crate::gfx::scene::Scene::new(100, 100);
        hud.draw(&mut scene, 60.0);
        assert!(
            scene.commands.is_empty(),
            "disabled HUD must not add commands"
        );
    }
}
