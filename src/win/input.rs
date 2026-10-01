//! 全局输入监听：Alt+D 热键（主触发）+ 可选的鼠标钩子自动划词。
//!
//! 两种触发方式共用同一个带消息泵的线程：
//! - 热键用 `RegisterHotKey(hwnd = NULL)` 注册到线程，`WM_HOTKEY` 直接进本线程的消息队列；
//! - 鼠标钩子（`WH_MOUSE_LL`）只在 `auto_mouse` 打开时才装。
//!
//! 线程回调只做判定和投递，取词（会阻塞几百毫秒）交给上层的工作线程。

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetDoubleClickTime, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_NOREPEAT, VK_D,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorInfo, GetCursorPos, GetMessageW, GetSystemMetrics,
    LoadCursorW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, CURSORINFO, IDC_IBEAM, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, SM_CXDOUBLECLK,
    SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG, WH_MOUSE_LL, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_QUIT,
};
// 句柄类型在新版 windows-sys 里是裸指针
use std::ptr::null_mut;

/// 注册热键用的 id，进程内唯一即可。
const HOTKEY_ID: i32 = 0x5A17;

/// 界面上展示的快捷键文本（改热键时这里和上面的常量要一起改）。
pub const HOTKEY_LABEL: &str = "Alt+D";

/// 全局输入线程上报的触发事件。
#[derive(Debug, Clone, Copy)]
pub enum Trigger {
    /// 左键按下（仅 auto_mouse 模式；用于收起上一次的弹窗）
    Started { x: i32, y: i32 },
    /// 鼠标判定出一次选词（仅 auto_mouse 模式）
    Completed { x: i32, y: i32 },
    /// 按下了 Alt+D。
    ///
    /// 坐标是按下那一刻的光标位置（Win32 物理像素）。弹窗居中由上层决定，
    /// 不依赖这两个值。
    Hotkey { x: i32, y: i32 },
}

// ------------------------------------------------------------------ 全局状态

static GESTURE: OnceLock<Mutex<Gesture>> = OnceLock::new();
static SENDER: OnceLock<Mutex<Option<Sender<Trigger>>>> = OnceLock::new();
static INPUT_THREAD: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
/// 当前生效的模式，用来判断 `start` 是否需要重启线程。
static AUTO_MOUSE: AtomicBool = AtomicBool::new(false);
static IBEAM_CURSOR: AtomicIsize = AtomicIsize::new(0);

fn sender_cell() -> &'static Mutex<Option<Sender<Trigger>>> {
    SENDER.get_or_init(|| Mutex::new(None))
}

fn thread_cell() -> &'static Mutex<Option<JoinHandle<()>>> {
    INPUT_THREAD.get_or_init(|| Mutex::new(None))
}

fn gesture_cell() -> &'static Mutex<Gesture> {
    GESTURE.get_or_init(|| Mutex::new(Gesture::default()))
}

// ------------------------------------------------------------------ 对外接口

/// 启动（或在模式变化时重启）全局输入监听。
pub fn start(sender: Sender<Trigger>, auto_mouse: bool) -> Result<(), String> {
    *sender_cell().lock() = Some(sender);

    if RUNNING.load(Ordering::SeqCst) {
        if AUTO_MOUSE.load(Ordering::SeqCst) == auto_mouse {
            return Ok(());
        }
        // 模式变了，得重启才能改钩子的装/卸
        stop();
    }

    AUTO_MOUSE.store(auto_mouse, Ordering::SeqCst);

    let (ready_tx, ready_rx) = channel::<Result<(), String>>();

    let handle = thread::Builder::new()
        .name("stranslate-input".to_string())
        .spawn(move || input_thread(ready_tx, auto_mouse))
        .map_err(|e| format!("启动输入监听线程失败: {e}"))?;

    *thread_cell().lock() = Some(handle);

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            stop();
            Err(e)
        }
        Err(_) => {
            stop();
            Err("启动全局输入监听超时".to_string())
        }
    }
}

/// 停掉监听并等待线程退出。
pub fn stop() {
    let thread_id = THREAD_ID.load(Ordering::SeqCst);
    if thread_id != 0 {
        unsafe {
            PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
        }
    }

    if let Some(handle) = thread_cell().lock().take() {
        let _ = handle.join();
    }

    RUNNING.store(false, Ordering::SeqCst);
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

// ------------------------------------------------------------------ 监听线程

fn input_thread(ready: Sender<Result<(), String>>, auto_mouse: bool) {
    unsafe {
        THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
    }

    run(ready, auto_mouse);

    THREAD_ID.store(0, Ordering::SeqCst);
    RUNNING.store(false, Ordering::SeqCst);
}

fn run(ready: Sender<Result<(), String>>, auto_mouse: bool) {
    unsafe {
        // 先取一次消息，确保本线程拥有消息队列，否则 WM_HOTKEY / PostThreadMessage 都收不到
        let mut message: MSG = std::mem::zeroed();
        PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);

        // 热键是主触发，注册不上就直接报错（通常是被别的程序占了）
        if RegisterHotKey(null_mut(), HOTKEY_ID, MOD_ALT | MOD_NOREPEAT, VK_D as u32) == 0 {
            let _ = ready.send(Err(format!(
                "注册快捷键 {HOTKEY_LABEL} 失败，可能已被其他程序占用（错误码 {}）",
                windows_sys::Win32::Foundation::GetLastError()
            )));
            return;
        }

        // 鼠标自动划词是可选项，装了才需要手势阈值和 I 型光标
        let mut hook = null_mut();
        if auto_mouse {
            *gesture_cell().lock() = Gesture {
                horizontal_threshold: GetSystemMetrics(SM_CXDRAG).max(1),
                vertical_threshold: GetSystemMetrics(SM_CYDRAG).max(1),
                double_click_width: GetSystemMetrics(SM_CXDOUBLECLK).max(1),
                double_click_height: GetSystemMetrics(SM_CYDOUBLECLK).max(1),
                double_click_time: GetDoubleClickTime(),
                ..Gesture::default()
            };

            let ibeam = LoadCursorW(null_mut(), IDC_IBEAM);
            IBEAM_CURSOR.store(ibeam as isize, Ordering::SeqCst);

            let module = GetModuleHandleW(std::ptr::null());
            hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), module, 0);

            if hook.is_null() {
                UnregisterHotKey(null_mut(), HOTKEY_ID);
                let _ = ready.send(Err(format!(
                    "安装全局鼠标钩子失败（错误码 {}）",
                    windows_sys::Win32::Foundation::GetLastError()
                )));
                return;
            }
        }

        RUNNING.store(true, Ordering::SeqCst);
        let _ = ready.send(Ok(()));

        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message == WM_HOTKEY {
                let (x, y) = cursor_position();
                emit(Trigger::Hotkey { x, y });
                continue;
            }

            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        if !hook.is_null() {
            UnhookWindowsHookEx(hook);
        }
        UnregisterHotKey(null_mut(), HOTKEY_ID);
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, wparam, lparam);
    }

    let data = &*(lparam as *const MSLLHOOKSTRUCT);
    let (x, y) = (data.pt.x, data.pt.y);
    let message = wparam as u32;
    let is_ibeam = is_ibeam_cursor();

    {
        let mut gesture = gesture_cell().lock();

        match message {
            WM_LBUTTONDOWN => {
                gesture.on_left_down(x, y, data.time, is_ibeam);
                drop(gesture);
                emit(Trigger::Started { x, y });
            }
            WM_MOUSEMOVE => {
                if gesture.is_tracking() {
                    gesture.on_mouse_move(x, y, is_ibeam);
                }
            }
            WM_LBUTTONUP if gesture.try_complete(x, y, is_ibeam) => {
                drop(gesture);
                emit(Trigger::Completed { x, y });
            }
            _ => {}
        }
    }

    CallNextHookEx(null_mut(), code, wparam, lparam)
}

fn emit(event: Trigger) {
    if let Some(sender) = sender_cell().lock().as_ref() {
        let _ = sender.send(event);
    }
}

unsafe fn cursor_position() -> (i32, i32) {
    let mut point: POINT = std::mem::zeroed();
    if GetCursorPos(&mut point) == 0 {
        (0, 0)
    } else {
        (point.x, point.y)
    }
}

/// 当前光标是不是 I 型（文本选择光标）——这是"用户正在选文本"的判据。
unsafe fn is_ibeam_cursor() -> bool {
    let mut info: CURSORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;

    if GetCursorInfo(&mut info) == 0 {
        return false;
    }

    info.hCursor as isize == IBEAM_CURSOR.load(Ordering::SeqCst)
}

// ------------------------------------------------------------------ 手势状态机

#[derive(Default)]
struct Gesture {
    horizontal_threshold: i32,
    vertical_threshold: i32,
    double_click_width: i32,
    double_click_height: i32,
    double_click_time: u32,

    start_x: i32,
    start_y: i32,
    start_time: u32,

    first_click_x: i32,
    first_click_y: i32,
    first_click_time: u32,

    left_button_down: bool,
    dragging: bool,
    seen_ibeam: bool,
    pending_click: bool,
    potential_second_click: bool,
}

impl Gesture {
    fn is_tracking(&self) -> bool {
        self.left_button_down
    }

    fn on_left_down(&mut self, x: i32, y: i32, time: u32, is_ibeam: bool) {
        self.start_x = x;
        self.start_y = y;
        self.start_time = time;
        self.left_button_down = true;
        self.dragging = false;
        self.seen_ibeam = is_ibeam;
        self.potential_second_click = self.within_double_click(x, y, time);
    }

    fn on_mouse_move(&mut self, x: i32, y: i32, is_ibeam: bool) {
        if !self.left_button_down {
            return;
        }

        self.seen_ibeam = self.seen_ibeam || is_ibeam;

        if !self.dragging {
            self.dragging = (x - self.start_x).abs() >= self.horizontal_threshold
                || (y - self.start_y).abs() >= self.vertical_threshold;
        }
    }

    /// 返回 true 表示这次是一次选词（拖选或双击选词）。
    fn try_complete(&mut self, _x: i32, _y: i32, is_ibeam: bool) -> bool {
        if !self.left_button_down {
            return false;
        }

        self.seen_ibeam = self.seen_ibeam || is_ibeam;

        if self.dragging {
            let is_text_drag = self.seen_ibeam;
            self.reset();
            return is_text_drag;
        }

        if self.potential_second_click {
            let is_text_double_click = self.seen_ibeam;
            self.reset();
            return is_text_double_click;
        }

        self.store_pending_click();
        false
    }

    fn store_pending_click(&mut self) {
        self.left_button_down = false;
        self.dragging = false;
        self.potential_second_click = false;
        self.pending_click = self.seen_ibeam;
        self.seen_ibeam = false;

        if self.pending_click {
            self.first_click_x = self.start_x;
            self.first_click_y = self.start_y;
            self.first_click_time = self.start_time;
        }
    }

    fn within_double_click(&self, x: i32, y: i32, time: u32) -> bool {
        if !self.pending_click || time.wrapping_sub(self.first_click_time) > self.double_click_time
        {
            return false;
        }

        (x - self.first_click_x).abs() * 2 <= self.double_click_width
            && (y - self.first_click_y).abs() * 2 <= self.double_click_height
    }

    fn reset(&mut self) {
        self.left_button_down = false;
        self.dragging = false;
        self.seen_ibeam = false;
        self.pending_click = false;
        self.potential_second_click = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// 注册 Alt+D，然后合成一次 Alt+D，验证事件真的能从消息泵里出来。
    ///
    /// 需要一个带窗口站的交互式桌面会话，所以默认忽略：
    ///   cargo test -- --ignored --nocapture hotkey
    #[test]
    #[ignore = "需要交互式桌面会话"]
    fn hotkey_fires_on_synthesized_alt_d() {
        let (sender, receiver) = channel();
        start(sender, false).expect("注册 Alt+D 失败");

        unsafe { synthesize_alt_d() };

        let event = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("3 秒内没收到事件");

        assert!(
            matches!(event, Trigger::Hotkey { .. }),
            "收到的不是 Hotkey: {event:?}"
        );

        stop();
    }

    /// 合成一次 Alt+D。快捷键已被本进程注册，系统会吞掉这次按键，
    /// 不会漏给前台窗口。
    unsafe fn synthesize_alt_d() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_D, VK_MENU,
        };

        fn key(vk: u16, key_up: bool) -> INPUT {
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

        let inputs = [
            key(VK_MENU, false),
            key(VK_D, false),
            key(VK_D, true),
            key(VK_MENU, true),
        ];

        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}
