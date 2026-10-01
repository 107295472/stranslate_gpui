//! 系统托盘：图标 + 右键菜单（显示主窗口 / 退出），左键单击唤回窗口。
//!
//! gpui-kit 自身没有托盘能力，这里用 `tray-icon`。关键约束：
//! - 托盘要创建在带消息泵的线程上。GPUI 的主线程本来就在跑 Win32 消息循环，
//!   `DispatchMessage` 会把消息派发给托盘的隐藏窗口，所以在主线程建托盘即可。
//! - `tray-icon` 的 `TrayIcon` 在 Windows 上是 `!Send`。我们把它搬进一个
//!   前台（主线程）常驻任务里持有，顺便在同一个任务里轮询事件通道，
//!   既保证它不被 Drop（Drop 了图标就没了），又不跨线程。

use std::time::Duration;

use gpui_kit::{AnyWindowHandle, App};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// 建立托盘并起一个轮询任务。`handle` 用来在点击时把窗口唤回前台。
pub fn init(handle: AnyWindowHandle, cx: &mut App) {
    let menu = Menu::new();
    let show_item = MenuItem::new("显示主窗口", true, None);
    let quit_item = MenuItem::new("退出", true, None);
    if menu.append_items(&[&show_item, &quit_item]).is_err() {
        return;
    }
    let show_id = show_item.id().clone();
    let quit_id = quit_item.id().clone();

    let tray = TrayIconBuilder::new()
        .with_tooltip("STranslate")
        .with_menu(Box::new(menu))
        .with_icon(default_icon())
        .build();
    let tray = match tray {
        Ok(t) => t,
        Err(_) => return,
    };

    cx.spawn(async move |cx| {
        // 持有 tray，别让它在任务里被 Drop；任务跑在主线程，!Send 也没问题。
        let _tray = tray;
        let menu_rx = MenuEvent::receiver();
        let tray_rx = TrayIconEvent::receiver();

        loop {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;

            let mut should_show = false;
            let mut should_quit = false;

            while let Ok(ev) = menu_rx.try_recv() {
                if ev.id == show_id {
                    should_show = true;
                } else if ev.id == quit_id {
                    should_quit = true;
                }
            }
            while let Ok(ev) = tray_rx.try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = ev
                {
                    should_show = true;
                }
            }

            if should_quit {
                cx.update(|cx| cx.quit());
                break;
            }
            if should_show {
                let _ = handle.update(cx, |_view, window, _app| {
                    if let Some(hwnd) = crate::window_hwnd(window) {
                        crate::win::window::show(hwnd);
                    }
                    window.activate_window();
                });
            }
        }
    })
    .detach();
}

/// 托盘图标：用项目里的 `imgs/favicon.png`，编译期打进二进制，运行时解码成 RGBA。
/// 解码失败则回退到一个纯色小方块，保证托盘总能起来。
fn default_icon() -> tray_icon::Icon {
    const PNG: &[u8] = include_bytes!("../imgs/favicon.png");
    if let Ok(img) = image::load_from_memory(PNG) {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        if let Ok(icon) = tray_icon::Icon::from_rgba(rgba.into_raw(), w, h) {
            return icon;
        }
    }
    // 回退：蓝色实心块（STranslate 主色调）
    const SIZE: u32 = 16;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.extend_from_slice(&[0x2F, 0x6F, 0xED, 0xFF]);
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE).expect("托盘图标生成失败")
}
