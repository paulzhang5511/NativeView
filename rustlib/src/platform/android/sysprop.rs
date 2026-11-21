//! 平台安全封装层：系统属性读取（仅探测用途）。

use super::sys::sysprop::PROP_VALUE_MAX;

/// 系统属性读取器（无状态，仅承载安全转换）。
#[derive(Clone, Copy, Debug, Default)]
pub struct SysProp;

impl SysProp {
    /// 读取一个系统属性。
    ///
    /// # 失败语义
    /// 属性不存在或名字含 NUL 时返回 `None`（调用方降级处理，不 panic）。
    pub fn get(&self, name: &str) -> Option<String> {
        let Ok(name_c) = std::ffi::CString::new(name) else {
            return None;
        };
        // 栈缓冲按 PROP_VALUE_MAX 预分配；NDK 保证写入以 NUL 结尾。
        // Android 侧写入需要可变借用；主机桩仅读取，故按平台门控 mut。
        #[cfg_attr(not(target_os = "android"), allow(unused_mut))]
        let mut buf = [0u8; PROP_VALUE_MAX];
        #[cfg(not(target_os = "android"))]
        let _ = &name_c;
        #[cfg(target_os = "android")]
        // SAFETY: name_c 与 buf 均为合法指针，长度由常量保证。
        let n = unsafe {
            super::sys::sysprop::__system_property_get(name_c.as_ptr(), buf.as_mut_ptr() as *mut _)
        };
        #[cfg(not(target_os = "android"))]
        let n = 0;
        if n > 0 {
            Some(
                // SAFETY: NDK 写入的是 UTF-8（系统属性由 init/框架写为 ASCII/UTF-8）。
                String::from_utf8_lossy(&buf[..n as usize]).into_owned(),
            )
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NUL 名字被拒绝，不进入 FFI。
    #[test]
    fn nul_name_rejected() {
        assert!(SysProp.get("ro.\0bad").is_none());
        assert!(SysProp.get("ro.build.version.sdk").is_some() == cfg!(target_os = "android"));
    }
}
