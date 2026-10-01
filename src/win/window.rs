//! 窗口的 Win32 收尾：GPUI 没暴露这些运行期能力，只能靠裸 HWND 直接调。
//!
//! - [`apply_toolwindow`]：挂 `WS_EX_TOOLWINDOW`，让窗口不进任务栏、不进 Alt+Tab；
//! - [`set_topmost`]：置顶开关（工具栏的「钉住」）；
//! - [`hide`] / [`show`]：收进托盘 / 从托盘唤回（GPUI 只有 minimize，没有真正的 hide）。
//!
//! HWND 由上层用 `raw-window-handle` 从 GPUI 的 `Window` 取出来，这里只认 `isize`，
//! 不依赖 gpui，保持这一层“纯 Win32”。

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_NOTOPMOST,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

/// 挂上 `WS_EX_TOOLWINDOW`：窗口从任务栏和 Alt+Tab 里消失（托盘程序的常规做法）。
///
/// 关键：窗口已经显示后再改 ex-style，任务栏按钮不会自动消失，必须「隐藏→改样式→再显示」
/// 这一轮才会让任务栏重新评估。同时清掉 `WS_EX_APPWINDOW`（它会强制上任务栏，优先级高于
/// `WS_EX_TOOLWINDOW`）。
#[cfg(windows)]
pub fn apply_toolwindow(hwnd: isize) {
    unsafe {
        let h = hwnd as _;
        ShowWindow(h, SW_HIDE);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let ex = (ex & !(WS_EX_APPWINDOW as isize)) | WS_EX_TOOLWINDOW as isize;
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex);
        ShowWindow(h, SW_SHOW);
    }
}

/// 置顶开关。开启后窗口压在其它窗口之上，关闭则回到普通层级。
#[cfg(windows)]
pub fn set_topmost(hwnd: isize, on: bool) {
    unsafe {
        let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        SetWindowPos(
            hwnd as _,
            after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// 收进托盘（真正隐藏，不是最小化）。
#[cfg(windows)]
pub fn hide(hwnd: isize) {
    unsafe {
        ShowWindow(hwnd as _, SW_HIDE);
    }
}

/// 从托盘唤回。
#[cfg(windows)]
pub fn show(hwnd: isize) {
    unsafe {
        ShowWindow(hwnd as _, SW_SHOW);
    }
}

// 非 Windows 下给空实现，保证跨平台编译（业务上本项目只跑 Windows）。
#[cfg(not(windows))]
pub fn apply_toolwindow(_hwnd: isize) {}
#[cfg(not(windows))]
pub fn set_topmost(_hwnd: isize, _on: bool) {}
#[cfg(not(windows))]
pub fn hide(_hwnd: isize) {}
#[cfg(not(windows))]
pub fn show(_hwnd: isize) {}
