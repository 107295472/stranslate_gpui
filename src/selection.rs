//! 划词：Alt+D 热键 / 全局鼠标钩子 + 取词。
//!
//! 相比 Flutter 版，这里不再用 flutter_rust_bridge 的 StreamSink，而是用
//! `async-channel` 把事件推给 GPUI 的视图（视图在 `cx.spawn` 里 `recv().await`）。
//!
//! 分工（与原版一致）：
//! - 输入监听线程（`win::input`）只负责判定"触发了"，回调里绝不阻塞；
//! - 取词（`Ctrl+C` + 读剪贴板，会阻塞几百毫秒）在本模块的工作线程里做。

use crate::win::input::{self, Trigger};
use parking_lot::Mutex;
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

/// 事件类型。
#[derive(Debug, Clone)]
pub enum SelectionKind {
    /// 左键按下（仅鼠标自动划词模式；用于收起上一次的弹窗）
    Started,
    /// 判定为一次划词（热键或鼠标）
    Completed,
    /// 取到选中文本
    Text,
    /// 取词失败（可以退化成"输入翻译"）
    Failed,
}

/// 推给界面的事件。
#[derive(Debug, Clone)]
pub struct SelectionEvent {
    pub kind: SelectionKind,
    /// 触发时的光标位置，Win32 物理像素。弹窗居中不依赖它，暂留给将来按位置弹窗用。
    #[allow(dead_code)]
    pub x: i32,
    #[allow(dead_code)]
    pub y: i32,
    /// 只有 Text 事件才有
    pub text: Option<String>,
}

static EVENTS: OnceLock<(
    async_channel::Sender<SelectionEvent>,
    async_channel::Receiver<SelectionEvent>,
)> = OnceLock::new();
static CHANNEL: OnceLock<Mutex<Option<Sender<Trigger>>>> = OnceLock::new();
static WORKER_STARTED: OnceLock<()> = OnceLock::new();

fn events_cell() -> &'static (
    async_channel::Sender<SelectionEvent>,
    async_channel::Receiver<SelectionEvent>,
) {
    EVENTS.get_or_init(async_channel::unbounded)
}

fn channel_cell() -> &'static Mutex<Option<Sender<Trigger>>> {
    CHANNEL.get_or_init(|| Mutex::new(None))
}

/// 订阅划词事件流。界面在构造时调一次，拿到 receiver 后在 `cx.spawn` 里消费。
///
/// 注意：这里**不**顺带打开监听，开关由 `set_enabled` 单独负责，
/// 这样注册热键失败之类的错误能直接报给界面，而不是被订阅流程吞掉。
pub fn events() -> async_channel::Receiver<SelectionEvent> {
    let _ = ensure_worker();
    events_cell().1.clone()
}

/// 开关划词。开启时按当前配置决定是否同时装鼠标钩子。
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    ensure_worker()?;

    let config = crate::config::get();

    if !enabled {
        input::stop();
        return Ok(());
    }

    let sender = channel_cell()
        .lock()
        .clone()
        .ok_or_else(|| "划词内部错误：事件通道未初始化".to_string())?;

    input::start(sender, config.selection_auto_mouse)
}

/// 界面展示用的快捷键文本（改热键只需改 `win::input`）。
pub fn hotkey() -> String {
    input::HOTKEY_LABEL.to_string()
}

/// 起一个常驻的取词线程。
fn ensure_worker() -> Result<(), String> {
    if WORKER_STARTED.get().is_some() {
        return Ok(());
    }

    let (sender, receiver) = channel::<Trigger>();
    *channel_cell().lock() = Some(sender);

    std::thread::Builder::new()
        .name("stranslate-selection".to_string())
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                handle(event);
            }
        })
        .map_err(|e| format!("启动划词线程失败: {e}"))?;

    let _ = WORKER_STARTED.set(());
    Ok(())
}

fn handle(event: Trigger) {
    // 热键和鼠标划词对上层是同一件事：取当前选中的文本
    let (x, y) = match event {
        Trigger::Started { x, y } => {
            emit(SelectionKind::Started, x, y, None);
            return;
        }
        Trigger::Completed { x, y } | Trigger::Hotkey { x, y } => (x, y),
    };

    emit(SelectionKind::Completed, x, y, None);

    let timeout = crate::config::get().selection_fetch_timeout_ms;

    match crate::win::clipboard::get_selected_text(timeout as u64) {
        Some(text) => emit(SelectionKind::Text, x, y, Some(text)),
        None => emit(SelectionKind::Failed, x, y, None),
    }
}

fn emit(kind: SelectionKind, x: i32, y: i32, text: Option<String>) {
    // try_send：界面还没开始消费时直接丢弃，不阻塞取词线程。
    let _ = events_cell().0.try_send(SelectionEvent { kind, x, y, text });
}
