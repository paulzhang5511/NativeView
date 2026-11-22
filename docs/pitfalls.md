# 踩坑记录（性能排查手册）

本文件记录 NativeView 从「真机落笔 22fps」到「60Hz 节拍满速、稳态 3.7ms/帧」过程中
**真实踩过的坑**：现象、当时的误判、最终的根因、以及下次怎么快速判定。

用途：下次再遇到「帧率不对 / 画面不对 / 埋点矛盾」时，先来这里对号，别从零复现。
配套读数与优化过程见 `docs/verification.md`。

---

## 0. 索引

| # | 现象 | 真实根因 | 一句话判定 |
|---|---|---|---|
| 1 | 子阶段加起来几十 µs，整帧墙钟却 4.7–15ms | 墙钟含「等 vsync / 被抢占」 | 用线程 CPU 时间或 profiler 交叉验证 |
| 2 | 每帧凭空多出 12MB 内存流量 | `Document::image()` 深拷贝 8MB + `pixels().to_vec()` 4MB | simpleperf 看 `memcpy` 调用栈 |
| 3 | 落笔只有 26fps，且「预览子计时器」只有 200µs | 缓存键含 `revision`，补丁路径永不命中 | 逐段 lap 打点，看 rebuild 段 |
| 4 | 画板画了半天只显示第一笔的起点 | 预乘位图缓存键只有像素指针，就地补丁后指针不变 | 导出页（走整帧合成）是否正常 |
| 5 | `AChoreographer_postCallback` 符号不存在，启动即崩 | libandroid 只导出 `postFrameCallback64` | 崩溃日志里的 dlopen/符号名 |
| 6 | partial 帧恒为 0 | `press.is_some()` 写了永真表达式 | 打印 `modes partial/full` |
| 7 | 改了 `debug.*` 开关必须重启 App | `OnceLock` 把属性读结果缓存了 | 开关前后读数是否变化 |
| 8 | `adb shell input swipe` 画不出连续笔迹 | 系统只派发 2 个事件；`motionevent` 每条命令独立 downTime | 改用应用内合成事件（`bench_touch`） |
| 9 | 单测里预览图被压成 21×38，误以为渲染有 bug | 忘记先 `ink.resize(...)`，`canvas.h=0` | 断言预览尺寸而非只看颜色 |
| 10 | SurfaceFlinger 差分算出「11ms 密布」这种假直方图 | 未滤掉 `actualPresentTime == INT64_MAX` 的无效行 | 差分前先过滤 |
| 11 | `dumpsys --latency` 报「找不到层」 | 层名后缀每次重启都变；`--list` 里还有 InputSink 等干扰项 | `--list` 里挑无 handle、非 InputSink 的那行 |
| 12 | 「每帧整幅重画 + 整幅 blit」占 6.3+3.5ms | 只在落笔起步十几帧成立；稳态只有 15.4k 像素 | 加 `rast_px_avg` 分稳态/起步 |
| 13 | 以为 `Arc::make_mut` 每帧退化成 4MB 拷贝 | 测量为 0，Arc 每帧都独占 | 加 `copy_avg` 直接记账 |
| 14 | 11 万像素的预乘要 2ms，与整幅 1M 像素 2.7ms 不成比例 | 行带按**全宽**遍历再跳过窗口外列 | 对比 `pix_avg` 与耗时 |
| 15 | 面板 90Hz 但 p50 仍是 16.9ms | 面板策略按触摸/场景在 60↔90 间切换，非算力问题 | `dumpsys SurfaceFlinger --latency` 看 p50 |

---

## 1. 测量口径本身会骗人（最贵的一课）

### 1.1 墙钟 ≠ 花费

手写 `Instant` 子阶段埋点，长期出现算术不可能的局面：

```
frame build 墙钟 4700–15000us，而 preview+text+input+advance+scene 合计只有几十 us
```

试过的补救全部失败：加 gap_a/gap_b/gap_c 补齐差值、`SystemTime` 双时钟交叉验证、
8MB `memcpy` 校准换算系数。

**结论**：`Instant` 是墙钟，包含「等 vsync」和「被调度器抢占」。子计时器本身无辜，
是分母里混进了不属于计算的时间。

**正确做法**：

- 线程 CPU 时间（`CLOCK_THREAD_CPUTIME_ID`，即 `perf::thread_cpu_us` / `bench[...] cpu_avg`）——
  只统计真正在跑的时间，是判断「还有没有优化空间」的唯一可靠分母；
- 或直接上 `simpleperf` 看调用栈；
- 绝对不要用墙钟差值去给某个子函数「分摊」时间。

### 1.2 「名义局部」可能是「实际整幅」

脏区链路一旦退化成整帧重绘**不会报错**，只会慢。所以必须把「这帧到底画了多少像素」
记成数字（`rast_px_avg`、`pix_avg`），而不能靠「代码里写了 render_rect 所以它是局部的」推断。

### 1.3 取样窗口会把起步帧算进稳态

落笔起步头十几帧包含首次纹理分配、首次整帧重建：`raster≈7ms`、`upload≈1.5ms`。
把这段平均进来，就会得出「每帧要 10ms」的错误结论。看稳态要看**逐帧日志的中后段**，
或者等 `frame_slow` 只打印异常帧时再判断。

---

## 2. 渲染链路

### 2.1 每帧 12MB 隐形内存流量（`Document::image()`）

**现象**：落笔 26fps，算力看起来不该这么慢。

**定位**：`simpleperf record -e task-clock -f 1000 --call-graph fp`，report 用
`--children --sort symbol -g caller --full-callgraph`，看到 `memcpy` 挂在
`draw_doc_view → Document::image()`。

**根因**：`Document::image()` 每次 clone 8MB 全图，调用点又 `pixels().to_vec()` 再拷 4MB。

**修法**：`Document::layer_pixels()` 借用 + `Image::resized_from / resample_region_from /
pixels_mut()` 零拷贝。

### 2.2 补丁路径永不命中（缓存键含 revision）

**现象**：`docview[pipe] total=25390us rebuild=25377us key=756x1345`——每帧整帧重采样。

**根因**：缓存键 `(revision, disp_w, disp_h)` 里的 revision 每落一个点就变，`cached_ok` 永假，
补丁分支永远进不去。

**修法**：`cached_ok` 只比 `disp_w/disp_h`；是否补丁交给 `*prev_rev == dirty.from` 判断。
效果 `patch=115–217us`、`rebuild=0`，fps 37 → 58。

**教训**：凡是把「版本号」放进缓存键的地方，先问一句「这个版本号是不是每帧都变」。
会变的只能做**校验条件**，不能做命中条件。

### 2.3 预乘位图缓存只按指针做键 → 画板只有第一笔

**现象**：真机画板只显示第一笔起点；导出页（走 `composite()` 整帧合成）笔迹完整。

**排查陷阱**：新写的单测也失败（`painted=30`），但那是因为测试里 `ink.render()` 前
**没调 `ink.resize(w,h,top,bottom)`** → `Layout::compute` 得 `canvas.h=0.0` → 预览被压成
`21×38`，测的根本不是真实链路。补上 resize 后测试才有判别力。

**根因**：`draw_image_cmd` 的预乘缓存键只有 `image.pixels().as_ptr()`。就地补丁保持同一
`Arc`，指针不变 → 每帧都命中第一帧的预乘位图（CPU / GPU 后端共用此路径）。

**修法**：`Image::pixels_mut()` 推进 thread-local「就地改写代际」（指针 → 代际表），
缓存键改 `(指针, 代际)`。

**教训**：任何「按指针做键」的缓存，都要配一个「内容被就地改过」的失效机制。

### 2.4 增量预乘：算 11 万像素花 2ms

**现象**：`pix_avg=110.8k`（只占显示图 11%），`image` 却 1.4–2.8ms；而整幅 1M 像素只要 2.7ms。

**误判**：先怀疑 `Arc::make_mut` 退化成 4MB 拷贝。加 `copy_avg` 直接记账后读数为 **0**
——上一帧 Scene 在本帧构建前已释放，缓存的位图每帧都是唯一持有者。假设否掉。

**真根因**：`premul_rows_in` 对每个行带遍历**画布全宽**再跳过窗口外的列。
窗口通常只有几十列（细长笔迹），110k 像素实际扫了近 1M 个位置。

**修法**：按窗口列段逐行迭代（`&mut band[off+x0 .. off+x1]` + `chunks_exact(4)`）。

**护栏**：`gfx::render::tests::incremental_premul_matches_full_rebuild` ——
同一份像素内容，一份走增量、一份放到新指针强制整幅重算，两张 Pixmap **逐字节相等**；
同时断言增量路径确实命中（`INCR_HITS`），避免「没走增量但碰巧相等」的假绿。

### 2.5 帧调度：90Hz 还是 60Hz 由面板决定

`AChoreographer` 只在面板真实的 vsync 上回调：

- 静止页 / 入库页跑 60Hz（16.86ms 节拍）；
- 连续落笔时面板切 90Hz（11.25ms 节拍），`dumpsys SurfaceFlinger --latency` 差分为
  `11ms×94 / 22ms×17 / 34ms×1`。

所以「帧率上限」是跟面板走的，不是常数。判定面板当前节拍只能看 SurfaceFlinger，
不能看应用自己的帧间隔。

---

## 3. 工具与命令的坑

### 3.1 SurfaceFlinger 差分

- 必须**先滤掉 `actualPresentTime == INT64_MAX`（无效行）再差分**，否则会算出假的
  11ms 密布直方图。
- 层名后缀编号每次重启都变：`--list | grep MyNativeActivity`，挑**没有 handle 前缀**、
  也不是 `ActivityRecordInputSink` / `ActivityRecord{...}` 的那一行。
- 列含义：`[desiredPresent, actualPresent, frameReady]`（ns），差分用 `actualPresent`。

### 3.2 simpleperf

- 需要**带符号**的 `.so`：`cargo ndk -t arm64-v8a build --release --features ...`（不带 `-o`），
  再手动 `cp` 到 `app/src/main/jniLibs/arm64-v8a/`。
- 常规发布用 `-o ./jniLibs` 的版本即可，两者别混。
- report 需要 `--children --sort symbol -g caller --full-callgraph` 才能看到调用栈归属；
  `--symdir / --symfs` 解析 APK 内的 `.so` 不可靠，别浪费时间。

### 3.3 输入事件

- `adb shell input swipe` 只会派发 2 个事件，做不了落笔基准。
- `adb shell input motionevent DOWN/MOVE/UP` 序列**画不出连续笔迹**：每条命令独立
  `downTime`，应用会视为彼此分离的手势。
- 要连续笔迹基线就用应用内合成事件（`bench_touch` + `debug.nativeview.bench.live=N`）。

### 3.4 属性开关

- `debug.*` 属性用 `property_get` 每帧现读（亚微秒级），**不要缓存到 `OnceLock`**，
  否则改了开关必须重启 App——调试时极难受。
- 特别注意：逐帧读属性的**探针**留在热路径会污染读数，定位完必须删干净。

### 3.5 测试前必须把环境摆正

`ink.render()` 之前不调 `ink.resize(...)`，`canvas.h` 会是 0，预览被压成几十像素，
任何「颜色对不对」的断言都会得到误导性结论。写 inkflow 渲染测试先确认尺寸。

---

## 4. 排查方法论（下次照这个顺序走）

1. **先画口径，再谈优化**：每帧的 `work / cpu / 各子阶段 / 光栅面积 / 预乘像素数` 都要有数字。
   没有数字的优化一律不做。
2. **墙钟只用来发现异常，不用来做归因**；归因看线程 CPU 时间或 profiler。
3. **假设要能证否**：给它配一个计数器（`copy_avg`、`rast_px_avg`、`pix_avg`），
   一次实测就能否掉，别靠推理。
4. **区分场景**：落笔 / 静止 / 导出 / 缩略图四条链路的成本结构完全不同，
   同一读数混在一起会得出错误结论。
5. **区分起步与稳态**：起步十几帧天然更贵，别把它平均进稳态。
6. **缓存键要自问三件事**：这个键每帧都变吗？内容会被就地改吗？同尺寸重建会误命中吗？
7. **改一处、复测一处**（性能技能要求）：每次改动都跑同一套 bench + 同一台上屏差分。
8. **加护栏测试**：修完显示/缓存类 bug，必须有能复现原 bug 的单测，否则下次重构会重犯。

---

## 5. 关键参数与开关（当前版本）

| 开关 | 作用 |
|---|---|
| `debug.nativeview.bench=N` | 连续渲染 N 帧，输出 work/cpu/sub/modes 汇总 |
| `debug.nativeview.bench.warm=N` | 预热帧数（代码里 `.max(2)`） |
| `debug.nativeview.bench.scenario=draw` | 先导航到画板再跑基准；缺省为静止页 |
| `debug.nativeview.bench.live=N` | 真 vsync 落笔实测（连续合成 MOVE），输出 fps/iv/pix/copy/thr/rast/hist |
| `debug.nativeview.frame_log=1` | 每帧一行完整拆分（会淹没 logcat，短时用） |
| `debug.nativeview.frame_slow=N` | 只打印 ≥N ms 的帧（含完整相位），定位偶发掉拍用 |
| `debug.nativeview.backend` / `.current` | 后端选择 |

`bench[live]` 汇总字段含义：

- `iv_p50/iv_p95/iv_max`：帧间隔（µs）；
- `fps = 1e6 / iv_avg`；
- `cpu_avg/cpu_p50`：线程 CPU 时间；
- `miss`：`iv > 17.5ms` 的帧数；
- `pix_avg`：每帧转成预乘的像素数（判断增量是否真的只算小块）；
- `copy_avg`：`Arc` 不独占导致的退化拷贝字节（正常应为 0）；
- `thr_avg`：每帧 spawn 的辅助线程数（小任务多线程会反噬）；
- `rast_px_avg`：每帧光栅面积（判断局部重绘是否真的局部）；
- `hist[..]`：帧间隔直方图（<12 / <18 / <24 / <28 / <35 / ≥35 ms）。

---

## 6. 尚未解决的坑（留待后续）

- **帧节奏对齐**：面板 16.85ms 一节，我方 `iv_p50=17.09ms`，每帧只差 0.24ms 却周期性
  整槽丢失（`hist[<18=549 / <24=202]`），把 61% 单槽帧稀释成平均 18.09ms。方向：
  按 `frameTimeNanos` 定节拍、提交前移到 vsync 前沿。
- **仓库文档策略**：`.gitignore` 整体忽略 `*.md`（只放行 `README.md` 与
  `docs/verification.md`）。若要再放行新文档，需要显式加否定行，否则 `git add` 会直接被拒。
- **AGENTS.md 真机型号不一致**：文档写 `10AG6A2DEF000JT`，实际设备是 `3c93eb53`（OPPO PFJM10），
  尚未修订。
