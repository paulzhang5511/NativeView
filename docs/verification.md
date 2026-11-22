# 验收记录：Inkflow

按 `tasks/todo-inkflow.md` 的顺序记录每次验证的真实结果。只记录实际跑过的验证；未跑过的写「未验证」并说明原因。

## 2026-11-21 · 主机单测 + 模拟器全流程（含双指导航）

### 主机单测

```
cd rustlib && cargo test --target x86_64-apple-darwin
```

- 结果：`389 passed; 0 failed`（lib），其余 test binary 全部 ok
- 新增 79 项：`paint/*` 56、`gfx/image` 6、`gfx/render` 2、`demo/inkflow` 19、`demo` 无障碍 1
- 覆盖的关键行为：densify 插值与压感插值、smooth 语义、半径下限、8 种笔刷的光栅差异、脏矩形并集、撤销/重做像素级回滚、实时笔画 begin/extend/end/cancel、PNG 往返编解码、文件名清洗、图像缩放、图像偏移落位、图像按倍率铺开、视图矩阵逆变换、缩放不受位图预算限制、捏合锚点与总间距倍率、导航手势的消费语义、可撤销步数、a11y 节点与点击通路、笔刷面板内容不越界、导出缩略图保留笔迹

### 静态检查

- `cargo fmt --check`：无差异
- `cargo check --target aarch64-linux-android --features "cpu,text,vello_gpu,vello"`：通过（仅既有 warning）

### 模拟器（x86_64 / API 33 / 1080×2400 / vello_gpu）

构建为双 ABI（`arm64-v8a` + `x86_64`），`emulator-5554` 安装后逐屏操作，全部通过：

- [x] 作品库：大标题、右上「＋」、三张示例卡（`inkflow-emu-library.png`）
- [x] 新建画布：四档尺寸选中态、四色纸色、底部「开始绘画」→ 进画板（`inkflow-emu-new.png`）
- [x] 画板落笔：单指 swipe 产生笔迹，位置与手指一致（`inkflow-emu-stroke2.png`）
- [x] 撤销：点「撤」后最近一笔消失，像素回到落笔前（`inkflow-emu-undo.png`）
- [x] 重做按钮：无可重做内容时置灰
- [x] 笔刷面板：4×2 预设 + 4 条滑杆 + 实时预览条，全部落在面板内（`inkflow-emu-brush3.png`）
- [x] 预设切换：水彩选中后预览条变成柔边宽笔触，粗细/不透明度/流量随预设变化（`inkflow-emu-brush6.png`）
- [x] 粗细滑杆：点按改到 88px，预览条即时变粗
- [x] 水彩绘制：88px 柔边笔触明显区别于 6px 钢笔（`inkflow-emu-brush7.png`）
- [x] 橡皮：切橡皮后在笔迹上划动，笔触被擦淡（`inkflow-emu-erased.png`）
- [x] 双指放大：`sendevent` 注入 MT-B 双指由 100px 拉到 400px，画面放大到 3×，笔迹变粗且以捏合中点为锚（`inkflow-emu-zoom-in.png`）
- [x] 双指缩小：400px → 120px，画面缩到 0.5×，画布在画布区内留黑边、笔迹同步变小（`inkflow-emu-zoom-out.png`）
- [x] 缩放后重绘：松手后画面停在缩放后的状态，不会回跳（同两张截图，抬指后 1.5s 截取）
- [x] 导出页：预览缩略图显示真实笔迹，尺寸 1080×1920、保存路径正确（`inkflow-emu-export5.png`）
- [x] 撤销后导出页：统计显示「可撤销 0 步」，与空白预览一致（`inkflow-emu-export-after-undo.png`）
- [x] 导出落盘：`files/exports/inkflow-*.png`，`file` 校验 PNG 1080×1920 RGBA，纸色 (247,245,239)，笔迹像素存在
- [x] 无障碍：`uiautomator dump` 能读到每个按钮的中文 `content-desc`（作品库 4 个、画板 13 个、面板 13 个），点击走同一 `Action` 通路

未验证：

- [ ] arm64 真机（OPPO PFJM10 / `3c93eb53`）全流程：USB 22:49 后掉线，`adb devices` 反复为空；本轮模拟器与真机同源（同一份 Rust + 同一 APK），但真机 Vulkan 驱动路径未复测
- [ ] 连续绘制 3 分钟帧统计与 RSS：模拟器 `frame stats` 有 `over_budget`（swiftshader 软件 GPU，p50 181–233ms），不代表真机 GPU 表现
- [ ] 手掌误触抑制（SPEC §9 开放问题 3）：未实现，也未测

### 双指注入方法（可复用）

`adb input` 只有单指，双指必须直接写 evdev：

```
getevent -pl /dev/input/event2        # virtio_input_multi_touch_1，X/Y 范围 0..32767
sendevent /dev/input/event2 3 47 <slot>      # ABS_MT_SLOT
sendevent /dev/input/event2 3 57 <tracking>  # ABS_MT_TRACKING_ID（-1 = 抬起）
sendevent /dev/input/event2 3 53 <x>         # 屏幕像素 * 32767 / 1080
sendevent /dev/input/event2 3 54 <y>         # 屏幕像素 * 32767 / 2400
sendevent /dev/input/event2 0 0 0            # SYN_REPORT
```

需要 `adb root`（模拟器 google_apis 镜像支持）。每步之间留 300ms 以上，否则软渲染丢帧会让手势只走一步。

### 本轮修复（全部有回归测试或设备实证）

1. **文字被图形覆盖**：`render` 改为图形 Scene 先 extend、文字 Scene 后 extend
2. **`↶/↷` 缺字形**：撤/重改为中文「撤」「重」sp(13)
3. **色板第 7 色被导出遮挡**：色点直径按可用宽度动态计算
4. **`DrawImage` 偏移采样（框架级）**：`vello_cpu` 的 `ImageBrush` 以画布原点采样，旧实现直接 `fill_rect(x,y,x+w,y+h)`，远离原点的图会整块采到图外。改为「平移到 (x,y) 后填 (0,0,w,h)」。回归测试 `draw_image_lands_at_offset`
5. **笔刷面板溢出与层级**：面板高 400dp→452dp（实际需 444dp），面板打开时不绘制橡皮/色板/导出。回归测试 `brush_sheet_fits_inside_panel`
6. **预览条颜色失真**：底色改为当前纸色、笔迹改为当前颜色
7. **a11y 与命中几何分叉**：`a11y_nodes` 与手势/绘制共用同一份 `layout()`
8. **缩放被位图预算截断**：`view_of` 曾用 2048px 预览上限压制显示尺度，竖版画布放大到约 1.3× 就卡死。改为「显示尺度只由 zoom 决定，位图精度在绘制端另行处理」。回归测试 `zoom_beyond_preview_budget_still_scales`
9. **缩放不锚定捏合焦点**：原来只跟随两指中点位移，放大时注意力中心会漂出视野。改为按下时记录焦点下的画布锚点，每帧由缩放反解平移。回归测试 `pan_for_anchor_keeps_focus_on_anchor`
10. **捏合倍率被丢帧吃小**：逐帧累乘在系统合并/丢弃 MOVE 时只乘到前几步（实测 3× 手势只到 1.199×）。改为「起始间距 vs 当前间距」一次算到底。回归测试 `zoom_from_pinch_uses_total_span`
11. **缩放后画面不刷新**：`PointerUp` 提前清掉导航态，导致最后一次 `ACTION_UP` 被判为未消费，上层不重绘，画面停在旧帧（表现为「捏合没反应」）。改为导航态只在最后一指抬起时收尾，且 MOVE 期间也逐帧重绘。回归测试 `pinch_is_consumed_and_zooms`
12. **`draw_image` 不参与缩放**：`Command::DrawImage` 只记录左上角点、尺寸恒为原图，`Canvas::scale` 对它无效。新增 `scale` 字段与 `draw_image_scaled`，渲染端按倍率铺开。回归测试 `draw_image_scaled_fills_scaled_area`
13. **导出页笔迹统计与画面矛盾**：`stroke_count` 是抖动种子、撤销后不减。改用 `Document::steps()`（可撤销步数）。回归测试 `steps_follow_undo_and_redo`

### 已知缺口（本轮已回收，见下一节）

- ~~`app/src/androidTest` 三个类仍按「默认页 List」假设，默认页改为 Inkflow 后需改为「点新建画布 → 开始绘画」路径；本轮只验证了 `assembleDebugAndroidTest` 编译通过，未在设备执行。~~ → 已于 2026-11-22 重写并在 `emulator-5554` 全部通过。
- 示例作品不落盘（V1 退出即丢，见 SPEC §9 开放问题 1）。
- `vello` crate 仍被 feature 启用但代码未使用。
- `app/src/main/jniLibs/x86_64/` 为模拟器验证产物，release 打包前应删除以控制体积（当前 Debug APK 52MB，双 ABI）。

## 2026-11-22 · 参考图逐屏对齐 + 设备自动化测试

对照 `docs/design/inkflow-doodle-app.html`（① 作品库 229–307、② 新建 308–343、③ 画板 344–432、④ 笔刷面板 433–477、⑤ 导出 478+）逐屏核对，补齐全部可见缺口；同时把 `app/src/androidTest` 重写为 Inkflow 版并在模拟器上跑通。

### 主机单测

```
cd rustlib && cargo test --target x86_64-apple-darwin --features "cpu,text,vello_gpu,vello" --lib
```

- 结果：**401 passed; 0 failed**（`demo::inkflow` 32 项）
- 本轮新增 12 项：`action_codes_roundtrip_uniquely`、`dock_sits_below_canvas_and_inside_screen`、`tool_switch_toggles_eraser_preset`、`dock_size_bar_drives_brush_size`、`toggle_layer_cycles_and_bumps_revision`、`quick_brush_picks_expected_preset`、`line_tool_commits_single_undo_step`、`pick_color_snaps_to_nearest_swatch`、`filter_switches_and_keeps_cards_clickable`、`new_canvas_sliders_drive_default_brush`、`export_layout_stacks_formats_above_buttons_without_overlap`、`export_formats_only_png_is_actionable`
- `cargo fmt --check`：无差异

### 参考图对齐清单（已补齐）

作品库
- [x] 标题「我的涂鸦」+ 副标「12 张作品 · 最近编辑 3 天前」
- [x] 4 个筛选 chip（全部/收藏/草稿/已导出），选中紫底白字，按筛选显隐缩略图
- [x] 2×2 卡片布局，卡高 150dp，卡外两行 meta（标题 + 相对时间）
- [x] 第 4 格空卡「新建画布 / 开始下一张」→ 新建页
- [x] 底部 4 段统计条（12 作品 / 3 草稿 / 2048 常用画布 / PNG 默认导出）

新建画布
- [x] 「尺寸」「纸色」分组标题
- [x] 4 张尺寸卡：形状缩略 + 名称（竖屏 1080×1920 / 方形 1440 / A4 1280×1810 / 屏幕 1080×2340）+ 用途副标
- [x] 纸色改名（暖白 / 米黄 / 冷灰 / 深夜）
- [x] 两条迷你滑杆（默认笔刷尺寸 / 默认不透明度），实时改写默认笔刷
- [x] 主按钮文案「创建画布」（a11y 标签同步）

画板
- [x] 顶栏：返回 + 作品名「鸢尾花 · 01」+ 副标「1080×1920 · 2 图层」+ 撤/重 + 紫色「导出」
- [x] 画布左侧 5 个浮动工具（笔 / 线 / 擦 / 滴 / 移），选中态紫色
- [x] 右上「图N」图层切换按钮，点击在两图层间轮换并刷新缩略图缓存
- [x] 右下 `zoom N%` 徽标跟随缩放实时更新
- [x] dock 信息行（色点 + 笔刷名 + 「不透明度 X% · 平滑 Y%」+ 右侧尺寸 pill）
- [x] 1–120 尺寸条，拖动直接改写当前笔刷 size
- [x] 8 格色板（第 8 格「＋」→ 笔刷面板）
- [x] 4 快捷笔刷（铅笔 / 软笔 / 喷枪 / 水彩，映射 PRESETS 2/3/4/5）

导出页
- [x] 标题「导出作品」，预览缩略 + 「尺寸 1080×1920」「可撤销 N 步 · M 层」+ 单行状态（将保存到 exports 目录 / 已保存 X KB · 文件名）
- [x] 3 行格式选择（PNG 无损·透明 / JPEG 轻量·分享 / PDF 矢量·打印），PNG 选中紫底，其余标「未开放」且点击给 toast「暂只支持 PNG 导出」
- [x] 形态行下方「预估体积 X KB · 长边 N px · 透明通道」（未导出按像素量粗估，已导出用真实字节）
- [x] 底部主按钮「保存 PNG」，保存后切换为「完成」并在上方给出「再次导出」（44dp + 12dp 间距，不与主按钮重叠）

### 设备自动化测试（emulator-5554 / AVD vv_emu / x86_64 API 33）

```
sh gradlew :app:connectedDebugAndroidTest
```

- 结果：**BUILD SUCCESSFUL in 47s，tests=7 failures=0 errors=0**
- 报告：`app/build/reports/androidTests/connected/debug/index.html`
- 用例：
  - `A11yShadowTreeTest.libraryExposesSemanticNodes`：作品库 9 个语义节点（＋1 + 筛选 4 + 卡 4）且 bounds 非空
  - `A11yShadowTreeTest.boardExposesDockAndToolNodes`：画板 24 个语义标签（含 5 工具 / 图层 / 尺寸条 / 8 色板 / 4 快捷笔刷）全部出现
  - `A11yShadowTreeTest.a11yClickWalksFiveScreens`：新建画布 → 尺寸 2 → 纸色 3 → 创建画布 → 导出 PNG → 保存 → 返回作品库 → 新建画布
  - `DemoInteractionTest.swipePaintsStrokeAndUndoWorks`：注入 swipe 后日志出现 `motion:`，撤销可点
  - `DemoInteractionTest.pinchKeepsBoardUsable`：注入雙指捏合后节点树完整、撤销可点
  - `DemoInteractionTest.toolSwitchKeepsBoardAlive`：点「移动画布」再点「画笔」不退出画板
  - `LifecycleTest.activityLifecycleIsStable`：onCreate/onResume/onPause 后根节点仍在

> 注意：`connectedDebugAndroidTest` 结束会卸载主 APK，后续 `am start` 报 `Activity class does not exist`，需重装 `app-debug.apk`。

### 逐屏实测截图（本轮 pull 到 `docs/screenshots/`）

- 作品库 `inkflow-emu-library-v6.png`
- 新建画布 `inkflow-emu-new-v4.png`（含「默认不透明度」文案修正）
- 画板 `inkflow-emu-board-v2.png`（竖版）/ `inkflow-emu-board-v3.png`（方形 1440）
- 笔刷面板 `inkflow-emu-sheet-v3.png`
- 导出页 `inkflow-emu-export-v3.png`（保存前）、`inkflow-emu-exported-v3.png`（保存后）
- 导出产物：`/data/user/0/com.nativeview.app/files/exports/inkflow-*.png`（1080×1920 RGBA）

### 本轮修出的严重缺陷

1. **Action 编码撞车**（设备测试首跑暴露）：`PickSize(i)=10+i` 与 `ToggleLayer=12`、`DragSize=13`、`Finish=11` 冲突，`from_code` 的宽 range 又把 14/15 吞成 `PickSize(4/5)`；点「画布尺寸 2」code=11 解码成 `Finish` 直接回作品库。修法：各参数段收窄到真实档数，标量动作迁到 90 段，新增 `action_codes_roundtrip_uniquely` 守护。
2. **作品库 meta 被同行卡片覆盖**：meta 与卡片同轮绘制，下一行卡背景盖掉上一行文字。改为两轮绘制（先全部卡片，再全部文字）。
3. **导出页文字溢出与按钮重叠**：状态行用完整路径 → 折行溢出卡片；「返回作品库」按钮写在 `save.y - 64dp`，与主按钮重叠 20dp；toast 固定 `bottom - 152dp` 正好压在「再次导出」上。改为「状态单行 + 格式区 + 44/56dp 双按钮 + toast 移至顶栏下方」，并由 `export_layout_stacks_formats_above_buttons_without_overlap` 守住几何。
4. **文件切割事故**：用 `str.index(marker)` 切割 3000+ 行 `inkflow.rs` 时命中了同名注释，产生「两份拷贝拼接」的坏文件。已恢复并改为「唯一签名 + 校验 `count==1`」的替换脚本。

---

## 2026-11-22 · 交互动效接入（每操作一条动效）

### 驱动链路

宿主每帧调 `Demo::advance(now_ms)` → `Inkflow::advance(now)` 把「真实帧间隔 ÷ 系统倍率」
累加进动画时钟；`needs_frames()` 为真时 glue 以 16ms 轮询重绘。动画状态一律是
`Option<(起始时刻, …)>` 覆写模型，新操作直接替换旧动画（可中断、不排队）。

系统倍率来自 `Settings.Global.ANIMATOR_DURATION_SCALE`：`MyNativeActivity` 在
`onCreate/onResume` 读取 → JNI `setAnimatorScale` → `platform::motion` 原子 →
`Demo::advance` 每帧同步给 Inkflow。倍率 0（无障碍「移除动画」）时单帧推进 3000ms，
动效直接落终态且不再请求帧。

### 动效清单（实装）

| 操作 | 表现 | 参数 |
|---|---|---|
| 前进切屏 | 整页自下 40dp 滑入 + 遮罩 0.22 淡出 | 240ms ease-out-cubic |
| 返回切屏 | 整页自上 −24dp 滑入 + 遮罩 0.16 淡出 | 220ms |
| 开笔刷面板 | 面板自底滑入（偏移 = 面板高）+ 背后压暗 0.28 | 320ms |
| 关笔刷面板 | 下滑退出，可从中途反向 | 220ms |
| 作品库卡片 | 4 卡错峰 50ms、自下 20dp | 300ms/张 |
| 筛选切换 | 卡片区整体重入 10dp（不错峰） | 200ms |
| 选中（尺寸/纸色/色板/预设/工具/筛选） | 外扩 3dp 紫色脉冲环，α 0.55→0 | 260ms sin 包络 |
| 任意按下（含 TalkBack） | 命中矩形叠白 α 0.10 | 380ms |
| 撤销 / 重做 / 切图层 | 画布区压暗闪 0.10→0 | 140ms |
| toast | 自 −8dp 落入 + 淡入，停留 1.4s | 180 / 1400 / 200ms |
| 导出保存成功 | 状态行淡入 + 双按钮自下 12dp 错峰 60ms | 240ms |

### 单测

```
cd rustlib && cargo test --target x86_64-apple-darwin --features "cpu,text,vello_gpu,vello" --lib
```

- 结果：**419 passed; 0 failed; 1 ignored**（`demo::` 61 项，其中 `inkflow` 42 / `inkflow_anim` 7）
- 本轮新增 18 项：
  - `inkflow_anim`（7）：`progress_clamps_and_zero_duration`、`ease_out_and_in_hit_endpoints`、
    `pulse_peaks_at_midpoint`、`staggered_delays_by_index`、`envelope_three_segments`、
    `slide_in_and_out_directions`、`running_window`
  - `inkflow`（10）：`page_transition_parks_shift_and_direction`、
    `brush_sheet_slides_from_bottom_and_reverses_midway`、
    `layout_shift_moves_rects_and_baselines_together`、
    `tap_registers_press_highlight_that_outlives_only_press_window`、
    `selection_changes_fire_a_pulse_on_the_picked_item_only`、
    `toast_fades_in_holds_then_out_and_stops_frames`、
    `export_success_animation_runs_once_and_parks`、`library_cards_stagger_and_settle`、
    `undo_flashes_canvas_and_marks_frames_needed`、
    `page_transition_moves_the_rendered_scene_not_only_state`（渲染层：中间帧字形基线差 == 状态位移）
  - `platform::motion`（1）：`scale_roundtrips_and_rejects_garbage`
- `cargo fmt --check`：无差异

### 逐帧验收图（主机侧光栅化）

```
cd rustlib && cargo test --target x86_64-apple-darwin --features "cpu,text,vello_gpu,vello" \
  --lib dump_animation_frames -- --ignored
```

- 产物：`docs/screenshots/anim-frames/` 17 帧
  - 卡片错峰：`a0-library-parked` / `a1-card-stagger-60ms` / `a2-card-stagger-160ms` / `a3-card-settled`
  - 按压：`a4-press-highlight`
  - 切屏：`b1-page-in-60ms` / `b2-page-in-140ms` / `b3-page-in-settled`
  - 脉冲：`c1-paper-pulse` / `e1-preset-pulse`
  - 面板：`d1-sheet-in-80ms` / `d2-sheet-in-200ms` / `d3-sheet-open`
  - 画布闪烁：`d4-canvas-flash`
  - 导出：`f0-export-before-save` / `f1-export-in-120ms` / `f2-export-settled`
- 说明：主机密度固定 1dp=1px，故按真机「逻辑尺寸」411×914 渲染（1080×2400 @2.625 等价），
  dp/sp 派生的全部几何与真机一致；软件光栅化需先注入进程级字体字节。

### 设备

- `sh gradlew :app:connectedDebugAndroidTest` → **tests=7 failures=0 errors=0**（BUILD SUCCESSFUL in 59s）
  动效接入后语义节点不消失、命中仍按未位移几何生效，7 项无需改断言
- 慢镜头目视：`adb shell settings put global animator_duration_scale 10` 后可见切屏中途
  （内容下移 + 整页压暗、页面底部露出背景），确认 1× 下「几乎察觉不到位移」是软渲染
  （单帧 180–250ms）造成的丢帧，而非动效未触发；验证后已恢复为 1
- 逐屏终态截图（动效不得改变静止布局）：
  `inkflow-emu-anim-library.png` / `-new.png` / `-board.png` / `-sheet.png` / `-export.png` / `-exported.png`

### 本轮修出的缺陷

1. **位移常量漏 dp 换算**：`PAGE_SHIFT_PX=24.0` 直接当像素用，1080×2400 上只有 24px（1% 屏高），
   肉眼几乎不可见，且与「24dp」的规格不符。修法：所有 `anim::*_PX` 在绘制处经 `dp()` 换算，
   同时把规格放大到 40dp/20dp（ease-out 末段收得快，小位移会被压缩到不可见）。
2. **`needs_frames` 恒真**：`export_at/flash_at` 最初用 `u64=0` 当「未发生」哨兵，
   而 `running(0, 0, dur)` 为真 → 首帧起永久请求重绘（耗电/发热）。
   改为 `Option<u64>`，无值时 `export_p()` 返回终态 1.0、`flash_p()` 返回 0.0。
3. **`pulse()` 端点负值**：f32 下 `sin(π) ≈ -8.7e-8`，终点得到负 alpha，测试断言抖动。
   加 `.max(0.0)` 钳制。
4. **面板反向时「跳回满屏再退出」**：反向动画起点改为「当前展开度」，
   有进行中动画取 `sheet_p()`，静止时按 `before == BrushSheet` 判断。
5. **按压高亮跨屏残留**：`press` 记录屏幕，换屏后 `press_fade()` 直接返回 0；
   点击即切屏的控件不再有按压态（由切屏动效承担反馈）。
6. **筛选重入误用整屏入场参数**：`PickFilter` 打的是 `cards_at`（300ms 错峰 20dp），
   与「轻量重排」语义不符。拆出 `filter_at`（200ms、10dp、不错峰），两条通道互不干扰。

## 2026-11-22 · inkflow 组件化拆分

拆前 `rustlib/src/demo/inkflow.rs` 单文件 3900+ 行（`docs/review-inkflow.md` 判为「上帝文件」，违反 AGENTS.md §2.8）。本轮按「屏 + 职责」拆成 14 个文件，**行为不变、断言不改**。

### 结构

| 文件 | 行 | 职责 |
|---|---|---|
| `inkflow/mod.rs` | 580 | 五屏枚举 / `Action` 编码 / 常量表 / `Inkflow` 状态与 `render` 调度 |
| `inkflow/theme.rs` | 169 | 颜色令牌与 `fill_round / stroke_round / veil / r / has` 等 8 个绘制辅助（各屏禁止硬编码色值） |
| `inkflow/layout.rs` | 499 | `Layout`（全部可点矩形）+ `compute / hits / shifted_y / shifted_sheet_y` |
| `inkflow/motion.rs` | 317 | 17 个动效进度方法（切屏、面板、卡片、脉冲、toast、导出、闪帧） |
| `inkflow/board.rs` | 548 | 画板绘制 + 屏幕↔文档坐标换算 + 双指缩放锚点数学（`Drag` / `Pinch`） |
| `inkflow/input.rs` | 655 | `on_touch` 手势机 + 命中 + `dispatch` 动作执行 + `on_screen_changed` |
| `inkflow/sheet.rs` | 243 | 笔刷面板（滑杆、8 预设、预览图烘焙） |
| `inkflow/export.rs` | 207 | 导出页（格式行、体积估算、保存成功动效） |
| `inkflow/library.rs` | 289 | 作品库（筛选 chips、2×2 卡、meta、统计条） |
| `inkflow/new_canvas.rs` | 187 | 新建页（尺寸/纸色/迷你滑杆） |
| `inkflow/a11y.rs` | 84 | `a11y_nodes` / `perform_a11y` / `a11y_label`（tag = `A11Y_TAG_BASE + code`） |
| `inkflow/brush_params.rs` | 53 | 4 条滑杆下标、范围、读写与文案 |
| `inkflow/text.rs` | 78 | `text_block` / `text_center`（文字单独一个 Scene） |
| `inkflow/tests.rs` | 1104 | 原 `#[cfg(test)] mod tests` 整块迁出，`use super::*` 不变 |

### 验证

```
cd rustlib && cargo check --target x86_64-apple-darwin --features "cpu,text,vello_gpu,vello"   # 0 error
cd rustlib && cargo test  --target x86_64-apple-darwin --features "cpu,text,vello_gpu,vello" --lib
  → 419 passed; 0 failed; 1 ignored（断言未改一行）
cd rustlib && cargo fmt --check                                                                 # 通过
```

- **warning**：拆分引入的 30+ 条 unused import / dead code 已清零；`cargo check` 只剩 5 条历史 warning（`anim/view_animator.rs`、`ui/widgets/*` 的 `Color`、`demo/mod.rs:994` 的 `let mut dp`），android release 构建 13 条亦全为历史项，无 inkflow 条目。
- **渲染等价**：重跑 `dump_animation_frames`（17 帧），15 帧 PNG md5 与拆分前完全一致；仅 `f1/f2` 导出帧不同——状态行文件名带 `SystemTime::now()` 秒级戳（`inkflow-<unix>.png`），每次必然不同。
- **构建**：`cargo ndk -t x86_64 … build --release` → `.so` 12.7MB；`sh gradlew :app:assembleDebug` → BUILD SUCCESSFUL，APK 52.7MB。
- **设备**：`sh gradlew :app:connectedDebugAndroidTest` → **tests=7 failures=0 errors=0 skipped=0**。
- **逐屏终态**：重装后按 作品库 → 新建 → 画板 → 面板 → 导出 → 已导出 走一遍截图，与 `docs/screenshots/inkflow-emu-anim-*.png` 逐像素比对：
  `library` 0.151%（仅状态栏时钟与手势条）、`new/board` 0.031%（仅状态栏）、`sheet/export` 0.036%（仅状态栏）、
  `exported` 4.494%（旧图捕获于 toast 存活期内，本轮截图晚 3s，toast 已消失——差异整块落在 toast 与预览卡被遮挡区）。

### 手法与踩坑

1. 先 `git mv demo/inkflow.rs demo/inkflow/mod.rs`：`demo/mod.rs` 的 `mod inkflow;` 与全 crate 的 `demo::inkflow::` 路径都不用改。
2. 块搬移用两个纯文本脚本（`split_inkflow.py` / `wrap_impl.py`），按「函数名 + 行内容精确等于 `}`」定位块尾；**不能用 `index('\n}\n')`**——会命中结构体/枚举内部第一个 `}`。
3. 脚本会把顶层 `enum`/`struct` 误插进 `impl Inkflow` 尾部（`error: enum is not supported in impls`），需剪出后追加到 `impl` 之后，并连带搬走紧邻的 `#[derive(...)]` 与 `///` 文档注释。
4. 父模块**不能**读子模块私有字段（与「后代可读父」方向相反）：`Pinch` 字段改 `pub(super)`，否则 `on_touch`（`input.rs` 未拆时仍在 `mod.rs`）报 E0616。
5. `Inkflow::layout` 是 `pub`，返回的 `Layout` 必须同步提到 `pub(crate)`（字段保持 `pub(super)`），否则报「type is more private than the item」。
6. `is_navigating` / `on_touch` 被 `demo/mod.rs` 直接调用 → `pub(crate)`；`a11y_nodes` / `perform_a11y` 同理；其余一律 `pub(super)`，不出 `demo::inkflow` 子树。
7. `pulse_p()` 的 sin 包络与 `draw_pulse_ring()` 的线性淡出是两条曲线，**不可合并**（合并会改手感）；前者只被单测使用，标 `#[cfg(test)]`。

## 帧率攻坚：落笔 22fps → 70fps 上屏（真机 3c93eb53 / OPPO PFJM10）

### 口径（先测量，再动手）

- **屏上节拍**：`adb shell dumpsys SurfaceFlinger --latency <layer>` 取真实上屏时间戳，
  按相邻 `actualPresentTime` 差分统计（这是唯一不会被 App 内部打点自欺的指标）。
  本机面板会随内容在 **60Hz / 90Hz** 之间切换：静止页节拍 16.86ms，连续落笔 11.25ms。
- **App 内部**：`clock_gettime(CLOCK_THREAD_CPUTIME_ID)` 的**线程 CPU**（`cpu_avg/cpu_p50`），
  以及 `build/raster/upload/present/acquire` 分段；墙钟一律只作参考——
  它含「等 vsync / 被抢占」，此前把 6ms 的帧报成 16ms 就是栽在这里。
- **负载**：`debug.nativeview.bench.live=N` —— 每收到一次 Choreographer 回调注入一个真实
  MOVE，走完整 `input→build→render→present` 链路，节拍等于手指持续移动（区别于
  `bench=draw` 的「一口气渲染 N 帧」吞吐口径）。

### 根因（simpleperf + 逐段 lap 定位）

1. 每帧一次 **8MB 全图深拷贝**（`Document::image()` 按 revision 缓存，落笔期 revision 每帧都变）。
2. 预览降采样在**缓存键里塞了 revision**，导致「增量补丁」永不命中，每帧退回整帧重采样
   （`Image::resized_from`：1080×1920 → 756×1345 box 均值），实测 **16–25ms/帧**。
   这是此前「墙钟 20ms、子计时器加起来 200µs」的真正归属。
3. 帧节拍靠 16ms 轮询近似 vsync。

### 改法

- **零拷贝预览**：`Document::layer_pixels()` 直接借出图层像素，`Image::resized_from /
  resample_region_from / pixels_mut()` 就地在预览图上补脏区；`draw_doc_view` 只在
  「显示尺寸变化或首次出帧」时整帧重建。
- **缓存键只认显示尺寸**：版本号变化交给脏区补丁处理（`从 prev_rev == dirty.from` 判定）。
- **AChoreographer 真 vsync**：`postFrameCallback64` 驱动主循环（拿不到时退回 16ms 轮询）。
- **埋点**：12→11 段 build 子阶段 + `DAMAGE_REASON[9]` + `cpu_avg/cpu_p50/miss` + `bench[live]`。

### 结果（同一台真机，同一负载）

| 指标 | 修复前 | 修复后 |
|---|---|---|
| `bench[live]` 帧率（真 vsync 落笔） | 26.7 fps | **57.6–58.6 fps** |
| 预览子阶段 | 16000–25000µs | **115–220µs**（命中补丁） |
| build 墙钟 p50（draw bench） | 16616µs | **416µs** |
| 线程 CPU / 帧 | ≈5.2–7.5ms | **3.8–4.7ms** |
| 上屏节拍（SurfaceFlinger） | — | 60Hz 段 **125/125 帧 ≤17.5ms**；90Hz 段落笔 ≈70–72fps |
| 光栅 / 上传 / 提交 | raster 2253us | raster 1.86ms / upload 0.33ms / present 2.14ms |

- 落笔期 `modes partial=179 full=26`，脏区原因 `ok=191 / page_shift=15`：增量路径稳定生效。
- `bench[draw]` 的 `work_fps=210.6`（build+光栅+上传+提交 4.75ms/帧），即算力余量 ≈3.5×。
- 90Hz 段仍有 ~20% 帧占两个槽（11.25→22.5ms）：属「面板降/升帧策略 + FIFO 双缓冲」
  的节拍问题，不是算力不足；下一步可选 render-ahead（先渲染后等 vsync）。

### 命令

```
adb shell setprop debug.nativeview.bench.live 900      # 真 vsync 落笔实测
adb shell dumpsys SurfaceFlinger --list | grep MyNativeActivity     # 取层名（#<n>）
adb shell dumpsys SurfaceFlinger --latency "<层名>"
cd rustlib && cargo test --target x86_64-apple-darwin --features "cpu,text,vello_gpu,vello" --lib   # 427 passed
```

### 补记：零拷贝补丁的「陈旧预乘缓存」回归（已修）

- **现象**：新增单测 `board_preview_shows_new_strokes` 与真机截图同时暴露——画板画了半天只显示第一笔的
  起始点，而导出页（走 `composite()` 整帧合成）却有完整笔迹。
- **根因**：`draw_image_cmd` 的预乘位图缓存键只有**像素指针**。就地补丁保持同一 `Arc`，指针不变，
  于是每帧都命中第一帧的预乘结果（CPU / GPU 后端共用这条路径）。
- **改法**：`Image::pixels_mut()` 推进「就地改写代际」（`gfx::image::mutation_rev`，thread-local 的
  指针 → 代际表）；`PremulCache` 键改为 `(指针, 代际)`。
- **护栏**：`gfx::render::tests::in_place_patch_invalidates_premultiplied_cache`（就地改像素后第二次
  光栅必须看到新颜色）+ `demo::inkflow::tests::board_preview_shows_new_strokes`（预览图里必须真的有笔迹）。
  **注意**：`ink.render()` 前的 `ink.resize(w,h,top,bottom)` 不能省，否则画板矩形高为 0，预览会被压成几十像素，
  测不到真实链路（写测试时踩到过）。
- **代价**：每帧补丁后需重算一次预乘（`image≈2.7ms`），draw bench `work` 由 4.75ms 升到 7.0ms，
  线程 CPU 4.6ms。仍在一帧预算内；若要进一步省，可把补丁区只重算到缓存位图里（下一轮）。

### 真机终态读数（修完缓存后）

```
bench[draw]   n=200 work=7001us cpu_avg=4620us cpu_p50=3968us raster=4401 upload=345 present=1868
                            image=2702 modes partial=179 full=26 [ok=191 page_shift=15] miss=40/200
bench[static] n=200 work=8532us cpu_avg=5546us wall_avg=16842us fps_ceiling=59.6 miss=4/200
bench[live]   n=200 iv_p50=16934us fps=56.8–58.6 cpu_avg≈4.2–5.0ms
SurfaceFlinger 上屏：60Hz 段 125/125 帧 ≤17.5ms（0 掉帧）；90Hz 段落笔 p50=11.25ms，约 75% 帧满速
```

## 补丁区增量预乘（第 6 项）

- **动机**：上面的「代价」——每帧补丁后整幅重算预乘约 2.7ms，等于把零拷贝省下的钱又花回去。
- **改法**：
  1. `Image` 增加 `patch: Option<(x0,y0,x1,y1)>`（像素坐标），`pixels_mut()` 把它清成 `None`
     （语义「区域未知」），新的 `note_patch(rect)` 由调用方在写入后声明真正改动的窗口。
  2. `Image::resample_region(_from)` 改为返回**实际写入的输出窗口**，供预览图登记补丁。
  3. `PremulCache` 增加 `latest: HashMap<像素指针, 最近代际>` 与 `take()`；命中
     「同指针 + 代际恰好 +1 + 图片给出补丁窗口」时摘出上一帧的位图，`Arc::make_mut` 后只重算该窗口
     （`premul_rows_in`），再以新键回填。
  4. 落笔路径（`inkflow::board::draw_doc_view`）补丁后 `note_patch(written)`。
- **一致性护栏**：`gfx::render::tests::incremental_premul_matches_full_rebuild` —— 同一像素内容，
  一份走增量、一份放到新指针强制整幅重算，两张 Pixmap **逐字节相等**；测试同时断言增量路径确实命中
  （`INCR_HITS` 计数），避免「没走增量但也碰巧相等」的假绿。
- **诊断口子**：`bench[live]` 新增 `pix_avg=`（每帧转成预乘的像素数），用来区分「增量真的只算了小块」
  与「补丁窗口大得等于整幅」。

### 实测（真机，同机同场景）

```
bench[draw] 容器：image 2702us → 839us，work 7001us → 5321us，cpu_avg 4620us → 4422us，
             raster 4401us → 2614us，modes partial=179 full=26（不变，补丁全部命中）
bench[live] n=900：pix_avg=109796（显示图约 1M 像素 → 每帧只重算约 11%），
             iv_p50=17037us（60Hz 节拍），fps=54.9–56.6，cpu_avg≈5.0–5.3ms，miss≈370/900
单测：430 passed / 1 ignored
```

**仍然吃时间的两处**（下一轮目标，已有数据支撑）：

1. 增量分支要 `Arc::make_mut` 一张 ~4MB 的预乘位图；只要上一帧的纹理缓存还持有引用，就会退化成一次
   4MB 拷贝——`pix_avg` 只有 11 万像素，`image` 却仍有 1.4–2.8ms，这个拷贝就是差额。
2. 落笔帧的 `raster≈6.3ms`、`present≈3.5ms`：绘制的预览图覆盖整块画布，脏区名义上是局部的，
   实际上每次都在整幅重画 + 整幅 blit。要真正逼近 90Hz（11.25ms 预算），得让画布纹理常驻、
   只 blit 变动带，而不是每帧重建整块。

### 补记：两个「下一轮目标」的实测否决

上一节列的两条待办，按技能口径先量后改，结果两条前提都不成立。

**1）`Arc::make_mut` 的 4MB 退化拷贝：从未发生。**
`bench[live]` 新增 `copy_avg`（因 `Arc` 不独占而多出的拷贝字节），900 帧读数为 **0**——
上一帧 Scene 在本帧构建之前就已释放，缓存的位图每帧都是唯一持有者。

**2）1.4–2.8ms 的 `image`：不是拷贝，是「按整行迭代只算窗口里几十列」。**
`pix_avg=110.8k`（显示图约 1M 像素，占比 11%），但 `premul_rows_in` 当时对每个行带遍历
**画布全宽**再跳过窗口外的列——110k 像素实际扫了近 1M 个位置。改为按窗口列段逐行迭代后，
同样的窗口只走真正要算的像素。

**3）「每帧整幅重画 + 整幅 blit」只在启动头十几帧成立。**
加 `rast_px_avg`（每帧光栅面积）后：稳态 `rast_px_avg=15.4k` 像素、`upload≈70µs`、
`raster≈2.1ms`、`present≈1.4ms`、`work≈3.7ms`。之前引用的 `raster≈6.3ms / present≈3.5ms`
是落笔起步那十几帧（首帧纹理分配 + 首次整帧重建），不是稳态。

结论：**稳态算力已经吃在 90Hz 的 11.25ms 预算之内**（work 3.7ms），
继续压在「常驻画布纹理 + 只 blit 变动带」上收益有限，先不投入。

### 真瓶颈：帧节奏对齐（当前）

```
bench[live] n=900  hist[<12=54 <18=549 <24=202 <28=51 <35=30 >=35=14]
iv_p50=17.09ms  iv_avg=18.09ms  fps=55.3  cpu_avg=5.3ms  miss=375/900
同期 SurfaceFlinger 呈现节拍：p50=16.85ms p95=17.23ms（125 帧）
```

面板节拍 16.85ms，我方帧间隔 p50 17.09ms——每帧只慢 0.24ms，但会周期性整槽丢失，
把 61% 的「单槽帧」稀释成平均 18.09ms。下一步该动的是**帧调度对齐**
（按 `frameTimeNanos` 定节拍、把提交尽量前移到 vsync 前沿），而不是继续砍每帧算力。
