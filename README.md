# NativeView

纯原生的 Android 视图框架：以 `NativeActivity` 为唯一入口，用 Rust 重建一套与 Android 框架同名同义的 `View` / `ViewGroup` 体系，绘制交给 Vello 生态，业务侧不需要写一行 Java/Kotlin UI 代码。

```text
Kotlin（约 600 行）            Rust（约 15k 行）
MyNativeActivity  ──JNI──▶  ui / gfx / text / anim / backends
ImeBridge / A11yBridge        platform/android（手写 FFI）
```

## 特性

- **三后端可切换**：`vello`（GPU 全管线）、`vello_gpu`（wgpu 预处理 + 软件光栅）、`cpu`（`vello_cpu` SIMD 多核）。按 SoC 探测选择，失败静默降级，也可用系统属性强制指定。
- **完整文字栈**：parley 排版 + skrifa 解析 + glifo 字形，原生支持 CJK 与字体 mmap。
- **语义对齐 Android**：`View` 的 measure/layout/draw、`padding` / `margin` / `GONE` / `INVISIBLE`、`ScrollView` 惯性、`Button` 按压态、`EditText` 光标与选区。
- **真实输入与 IME**：`AInputQueue` 泵取 + 手势注册表（唯一 view id）、双指缩放、软键盘文本回推与 `CursorAnchorInfo`。
- **无障碍影子树**：原生节点经 `AccessibilityNodeProvider` 暴露给 TalkBack，支持 `ACTION_CLICK` 等动作回传。
- **硬约束**：跨 JNI 只传 `Surface` / `fd` / 基本类型，图像原始帧全程留在 Rust 侧。

## 内置控件

| 控件 | 能力 |
|---|---|
| `TextView` | 自动换行、行高、颜色、字号、省略 |
| `EditText` | 焦点、光标绘制、IME 文本同步、`setText` 动作 |
| `Button` | 点击、按压态（背景压暗 18%）、`isChecked` 汇报 |
| `ImageView` | 位图绘制、缩放、旋转 |
| `ScrollView` | 拖拽滚动、指数摩擦惯性、拦截优先分发 |
| `LinearLayout` | 水平/垂直、`padding` / `margin`、`GONE` 不占位 |

## Demo 三页

- **List** — 200 行列表，验证长列表滚动与复用。
- **Draw** — 自绘路径 + 动画 + 图片 + 双指缩放。
- **Text** — 多字号文本 + 输入框 + 软键盘回推。

页签由原生树自举装配，点击切换走 `request_blur` + 重绘。

## 仓库结构

```text
app/                    Android 壳层（Activity、JNI 桥、资源、instrumentation 测试）
rustlib/                Rust 框架（crate 名 velloview）
  src/platform/         平台层：Android 手写 FFI + 主机测试桩
  src/ui/               View 体系、输入分发、IME、无障碍影子树、控件
  src/gfx/              Canvas、Paint、Path、Scene、Render
  src/text/             字体、度量、排版
  src/backends/         cpu / vello_gpu / vello 三后端与选择器
  src/anim/             动画驱动（Choreographer）
  src/demo/             三页 Demo 与交互逻辑
  assets/               字体与示例图片（`include_bytes!` 编入 .so）
gradle/                 版本目录与 wrapper
```

## 构建

前置：JDK 17+、Android SDK（`compileSdk 37`）、NDK `27.3.13750724`、`cargo-ndk`。

```bash
# 1. SDK 路径（不要提交）
echo "sdk.dir=$HOME/Library/Android/sdk" > local.properties
```

**主机单测（桌面端，不需要 NDK）**

```bash
cd rustlib
cargo test --target x86_64-apple-darwin   # Intel Mac 必须显式指定 target
```

**构建 Android 库**

```bash
cd rustlib
cargo ndk -t arm64-v8a -t x86_64 -o ./jniLibs build --release \
  --features "cpu,text,vello_gpu,vello"
cp jniLibs/arm64-v8a/libvelloview.so ../app/src/main/jniLibs/arm64-v8a/
cp jniLibs/x86_64/libvelloview.so   ../app/src/main/jniLibs/x86_64/
```

**打包 APK**

```bash
sh gradlew :app:assembleDebug
# app/build/outputs/apk/debug/app-debug.apk
```

## 运行

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.nativeview.app/.MyNativeActivity
```

模拟器的 GPU 路径走 SwiftShader，单帧可达秒级，验收时强制 CPU 后端：

```bash
adb shell setprop debug.nativeview.backend cpu
```

## 测试

| 层 | 命令 | 规模 |
|---|---|---|
| Rust 单测 | `cargo test --target x86_64-apple-darwin` | 259 项 |
| Android instrumentation | `sh gradlew :app:connectedDebugAndroidTest` | 5 通过 / 1 跳过 |

设备侧用例以 **Rust info 日志**为判据（`list scrolled to …`、`p2 scale …`），不依赖 SurfaceView 截图：

- 无障碍影子树暴露页签、`ACTION_CLICK` 切页
- 单指滚动真的改变 `ScrollView` 偏移
- 双指捏合真的改变 P2 缩放系数（多指序列必须以单指 `ACTION_DOWN` 起手，且所有事件统一 `toolType=FINGER`，否则 InputDispatcher 会静默丢弃）
- 二次 `onDestroy` 不崩溃

`imeTextReachesNativeState` 因模拟器无真实软键盘焦点（`input text` 建立不了 `InputConnection`）保持 `@Ignore`；文本回推由 `ImeManager` 单测覆盖。

## 已知限制

- 仅交付 `arm64-v8a` 真机；`x86_64` 只为模拟器。
- 输入框获得焦点后销毁 Activity，Android 13 会对同一 `InputQueue` 二次 `dispose`（Scudo abort），修复需把 IME/无障碍桥宿主迁到独立窗口，排在 V2。
- 视图状态不做框架级保存：进程重建/旋转不恢复，Demo 仅进程内保留。
- 无 XML/JSON 布局解析，页面由 Rust 代码构建。

## 开发约定

- 所有 Rust 代码只在 `rustlib/` 内；禁止在其他目录散落 `.rs`。
- 注释用简体中文，运行时日志用英文（Android 侧经 Timber，Rust 侧 `log` + `android_logger`）。
- 用户可见文案一律进 `strings.xml`。
- `.so` 为构建产物，不提交。
