//! `sys/system_properties.h` 的手写绑定（NDK 27.3.13750724）。
//!
//! V1 仅用于设备能力探测（如 `ro.product.*`），不做任何写入。

/// 系统属性值的最大长度（含结尾 NUL），头文件 `#define PROP_VALUE_MAX 92`。
pub const PROP_VALUE_MAX: usize = 92;

#[cfg(target_os = "android")]
unsafe extern "C" {
    /// 读取系统属性；返回值为拷贝到 `__value` 的字节数（不含 NUL），
    /// 属性不存在时返回 0 且 `__value` 内容为空字符串。
    pub fn __system_property_get(
        __name: *const core::ffi::c_char,
        __value: *mut core::ffi::c_char,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 长度上限与 NDK 头文件对照：不足 92 会截断 `ro.build.description` 类长属性。
    #[test]
    fn prop_value_max_matches_ndk() {
        assert_eq!(PROP_VALUE_MAX, 92);
    }
}
