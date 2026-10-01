//! 剪贴板取词：模拟 Ctrl+C，等剪贴板变化，读完把用户原来的内容还回去。
//!
//! 移植自 `Helpers/ClipboardHelper.cs`。

use std::thread::sleep;
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_C,
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT,
};

/// 取当前选中的文本。
///
/// `timeout_ms` 是等待剪贴板变化的超时，原项目默认 500ms。
/// 取不到就返回 None（调用方可退化成"输入翻译"）。
pub fn get_selected_text(timeout_ms: u64) -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;

    // 先备份用户原有内容
    let backup = clipboard.get_text().ok();

    // Alt+D 触发的瞬间 Alt 还按着，这时直接发 Ctrl+C 会被系统看成 Ctrl+Alt+C：
    // 复制会失败，还会误触发别的程序注册的 Ctrl+Alt+C 快捷键。
    // 原版 STranslate 的做法是先把修饰键强制"抬起来"再发（见 release_modifiers）。
    #[cfg(windows)]
    release_modifiers();

    #[cfg(windows)]
    let sequence_before = unsafe { GetClipboardSequenceNumber() };

    #[cfg(windows)]
    send_ctrl_c();

    let timeout = Duration::from_millis(timeout_ms.max(1));
    let deadline = Instant::now() + timeout;
    let mut captured: Option<String> = None;

    while Instant::now() < deadline {
        sleep(Duration::from_millis(10));

        #[cfg(windows)]
        {
            // 序列号没动说明前台程序还没把内容写进剪贴板
            if unsafe { GetClipboardSequenceNumber() } == sequence_before {
                continue;
            }
        }

        if let Ok(text) = clipboard.get_text() {
            if !text.trim().is_empty() {
                captured = Some(text);
                break;
            }
        }
    }

    // 还原剪贴板。注意：原本为空时这里不做处理，Copied 内容会留在剪贴板里。
    if let Some(previous) = backup {
        if captured.as_deref() != Some(previous.as_str()) {
            let _ = clipboard.set_text(previous);
        }
    }

    captured
}

/// 把所有修饰键强制"抬起来"。
///
/// 移植自原版 `ClipboardHelper.SendCtrlCV()` 的开头（注释原文：「先清理可能存在的按键状态
/// ！！！很重要否则模拟复制会失败」）。Alt+D 触发的瞬间 Alt 还按着，不清掉这个状态，
/// 模拟出来的就不是 Ctrl+C 而是 Ctrl+Alt+C —— 复制失败且会撞别的程序的热键。
///
/// 原版是"主动抬键"而不是"等你松手"，所以不管你按多久都不会出问题，也不用等待。
#[cfg(windows)]
fn release_modifiers() {
    // 左右键各发一遍，跟原版一致
    let inputs = [
        key_input(VK_CONTROL, true),
        key_input(VK_LCONTROL, true),
        key_input(VK_RCONTROL, true),
        key_input(VK_MENU, true),
        key_input(VK_LMENU, true),
        key_input(VK_RMENU, true),
        key_input(VK_LWIN, true),
        key_input(VK_RWIN, true),
        key_input(VK_SHIFT, true),
        key_input(VK_LSHIFT, true),
        key_input(VK_RSHIFT, true),
    ];

    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

/// 模拟一次 Ctrl+C。
#[cfg(windows)]
fn send_ctrl_c() {
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_C, false),
        key_input(VK_C, true),
        key_input(VK_CONTROL, true),
    ];

    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

#[cfg(windows)]
fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
