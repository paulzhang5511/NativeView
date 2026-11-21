//! 构建脚本：平台相关的库链接。
//!
//! NDK 符号（`ANativeActivity_*` / `ANativeWindow_*` / `AInputQueue_*` /
//! `ALooper_*` / `AAssetManager_*` / `__system_property_*`）由系统镜像的
//! `libandroid.so` 提供，必须在 Android 链接期声明依赖，否则 `dlopen` 时
//! 报 `cannot locate symbol`（PRD §12.R14 同源的链接期变体）。
//! 主机（Windows）测试构建无此库，按目标平台门控。
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("android") {
        println!("cargo:rustc-link-lib=dylib=android");
        println!("cargo:rustc-link-lib=dylib=log");
    }
}
