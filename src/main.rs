//! STranslate (GPUI 版)：Flutter + flutter_rust_bridge 版本的纯 Rust 重写。
//!
//! 业务逻辑（翻译引擎、Windows 取词/热键、配置）直接复用自 Flutter 版的 Rust 侧，
//! 去掉了 flutter_rust_bridge 这一层；界面用 gpui-kit（GPUI Component）重画。

mod app;
mod config;
mod selection;
mod settings_view;
mod translate;
mod tray;
mod win;

use gpui_kit::*;

/// 从 GPUI 的 `Window` 里取出原生 HWND（Windows），交给 `win::window` 的裸 Win32 调用。
///
/// 注意：`Window` 上有个同名的 inherent 方法 `window_handle()` 返回 `AnyWindowHandle`，
/// 会盖掉 `HasWindowHandle` trait 的同名方法，所以这里用 UFCS 显式点名 trait 方法。
pub fn window_hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);

            let bounds = Bounds::centered(None, size(px(460.), px(560.)), cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(380.), px(360.))),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                kind: WindowKind::Normal,
                ..Default::default()
            };

            let (handle, _view) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app::TranslateApp::new(window, cx))
            })
            .expect("打开窗口失败");

            // 托盘：图标 + 菜单 + 左键唤回。
            tray::init(handle, cx);

            cx.activate(true);
        });
}
