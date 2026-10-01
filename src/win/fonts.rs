//! 枚举系统已安装的字体族，供设置页的字体下拉使用。
//!
//! 走 GDI 的 `EnumFontFamiliesExW`：拿屏幕 DC，`lfCharSet = DEFAULT_CHARSET`、
//! 字体名为空，会按字体族回调一遍。名字以 `@` 开头的是竖排变体，跳过。

use std::collections::BTreeSet;

use windows_sys::Win32::Foundation::LPARAM;
use windows_sys::Win32::Graphics::Gdi::{
    EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW,
};

/// 回调：把字体族名塞进 `lparam` 指向的集合，返回 1 表示继续枚举。
unsafe extern "system" fn collect(
    lf: *const LOGFONTW,
    _tm: *const TEXTMETRICW,
    _font_type: u32,
    lparam: LPARAM,
) -> i32 {
    if !lf.is_null() {
        let face = &(*lf).lfFaceName;
        let end = face.iter().position(|&c| c == 0).unwrap_or(face.len());
        let name = String::from_utf16_lossy(&face[..end]);
        if !name.is_empty() && !name.starts_with('@') {
            let set = &mut *(lparam as *mut BTreeSet<String>);
            set.insert(name);
        }
    }
    1
}

/// 系统字体族列表（去重、按名排序）。枚举失败时返回空表，由调用方回退到默认。
pub fn list() -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();

    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() {
            return Vec::new();
        }

        let lf = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        EnumFontFamiliesExW(hdc, &lf, Some(collect), &mut names as *mut _ as LPARAM, 0);

        ReleaseDC(std::ptr::null_mut(), hdc);
    }

    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_installed_fonts() {
        let fonts = list();
        assert!(!fonts.is_empty(), "枚举不到任何系统字体");
        // 竖排变体（@ 开头）不该出现在列表里
        assert!(fonts.iter().all(|f| !f.starts_with('@')));
        // 去重 + 排序
        let mut sorted = fonts.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(fonts, sorted);
    }
}
