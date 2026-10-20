//! 紧凑翻译窗：工具栏 / 输入框 / 语言行 / 服务行 / 译文。
//!
//! 对照 Flutter 版 `lib/src/app.dart`，用 GPUI Component 重画；业务调用直接走
//! 复用过来的 `translate` / `config` / `selection` 模块。

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    v_flex, ActiveTheme, Disableable, IconName, IndexPath,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::config::{self, AppConfig};
use crate::selection::{self, SelectionEvent, SelectionKind};
use crate::settings_view::{Settings, SettingsEvent};
use crate::translate::lang::{self, LangInfo};

type LangSelect = Entity<SelectState<SearchableVec<SharedString>>>;

pub struct TranslateApp {
    config: AppConfig,
    langs: Vec<LangInfo>,

    input: Entity<TextareaState>,
    /// 译文用只读 Textarea 显示（可选中 / 复制 / 滚动）
    output: Entity<TextareaState>,
    from_select: LangSelect,
    to_select: LangSelect,

    result: String,
    error: Option<String>,
    busy: bool,

    /// 划词启动失败的原因（比如热键被占用），非空时窗口顶部挂条横幅
    selection_error: Option<String>,
    hotkey: String,

    /// 划词取到的文本，等下一帧 render 时塞进输入框并翻译（set_value 需要 Window）
    apply_source: Option<String>,

    /// 待写入译文框的文本，等下一帧 render 时 set_value（同样需要 Window）
    pending_output: Option<String>,

    settings: Option<Entity<Settings>>,

    /// 置顶（钉住）状态，对应工具栏的钉子按钮
    pinned: bool,
    /// 原生 HWND，首帧 render 时取出；用于置顶 / 收托盘 / 去任务栏
    hwnd: Option<isize>,
    /// 首帧是否已做完窗口收尾（挂 WS_EX_TOOLWINDOW）
    window_ready: bool,

    /// 失焦自动隐藏的豁免截止时间：程序主动唤出窗口后短暂屏蔽，防激活抖动误隐藏
    autohide_guard_until: Option<std::time::Instant>,

    _subs: Vec<Subscription>,
}

impl TranslateApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = config::get();
        let langs = lang::all();
        let hotkey = selection::hotkey();

        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(format!("输入文字，或选中后按 {hotkey} 翻译"))
        });
        let output = cx.new(|cx| TextareaState::new(window, cx).placeholder("译文"));

        let names: Vec<SharedString> = langs.iter().map(|l| l.name.clone().into()).collect();
        let from_idx = langs.iter().position(|l| l.id == config.from);
        let to_idx = langs.iter().position(|l| l.id == config.to);

        let from_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names.clone()),
                from_idx.map(IndexPath::new),
                window,
                cx,
            )
        });
        let to_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names.clone()),
                to_idx.map(IndexPath::new),
                window,
                cx,
            )
        });

        let mut subs = Vec::new();
        subs.push(
            cx.subscribe(&from_select, |this, _, ev: &SelectEvent<SearchableVec<SharedString>>, cx| {
                if let SelectEvent::Confirm(Some(name)) = ev {
                    this.on_lang_changed(true, name.clone(), cx);
                }
            }),
        );
        subs.push(
            cx.subscribe(&to_select, |this, _, ev: &SelectEvent<SearchableVec<SharedString>>, cx| {
                if let SelectEvent::Confirm(Some(name)) = ev {
                    this.on_lang_changed(false, name.clone(), cx);
                }
            }),
        );

        // 划词事件流：在后台任务里消费，收到文本就排进 apply_source。
        let rx = selection::events();
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = rx.recv().await {
                if this.update(cx, |this, cx| this.on_selection_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        // 按配置打开/关闭划词；失败（比如热键被占用）挂横幅。
        let selection_error = selection::set_enabled(config.selection_enabled).err();

        Self {
            config,
            langs,
            input,
            output,
            from_select,
            to_select,
            result: String::new(),
            error: None,
            busy: false,
            selection_error,
            hotkey,
            apply_source: None,
            pending_output: None,
            settings: None,
            pinned: false,
            hwnd: None,
            window_ready: false,
            autohide_guard_until: None,
            _subs: subs,
        }
    }

    // ---------------------------------------------------------------- 划词

    fn on_selection_event(&mut self, event: SelectionEvent, cx: &mut Context<Self>) {
        if let SelectionKind::Text = event.kind {
            if let Some(text) = event.text {
                if !text.trim().is_empty() {
                    // 窗口可能已被失焦隐藏；隐藏状态下 GPUI 不重绘，render 里的
                    // apply_source 永远跑不到，所以这里先用裸 HWND 直接唤出来，
                    // 让 GPUI 恢复渲染，剩下的（填词 / 置前 / 翻译）交给 render。
                    if let Some(hwnd) = self.hwnd {
                        crate::win::window::show(hwnd);
                    }
                    // 刚唤出的瞬间可能有一次 WM_ACTIVATE 抖动，别让它马上又被隐藏。
                    self.autohide_guard_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(600));
                    self.apply_source = Some(text);
                    cx.notify();
                }
            }
        }
    }

    // ---------------------------------------------------------------- 翻译

    fn translate_current(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        self.run_translation(text, cx);
    }

    fn run_translation(&mut self, text: String, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();

        let cfg = self.config.clone();
        cx.spawn(async move |this, cx| {
            let (tx, rx) = async_channel::bounded(1);
            // reqwest 是阻塞的，丢到独立线程里跑，别占着执行器。
            std::thread::spawn(move || {
                let _ = tx.send_blocking(crate::translate::translate(&cfg, &text));
            });
            let outcome = rx.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match outcome {
                    Ok(Ok(translated)) => {
                        this.result = translated.clone();
                        this.pending_output = Some(translated);
                        this.error = None;
                    }
                    Ok(Err(e)) => this.error = Some(e),
                    Err(_) => this.error = Some("翻译任务中断".to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------- 配置

    fn save(&mut self, cfg: AppConfig, cx: &mut Context<Self>) {
        match config::set(cfg.clone()) {
            Ok(()) => {
                self.config = cfg;
                // 划词开关 / 鼠标自动划词可能变了，重新应用一次
                self.selection_error = selection::set_enabled(self.config.selection_enabled).err();
            }
            Err(e) => self.error = Some(format!("保存配置失败: {e}")),
        }
        cx.notify();
    }

    fn on_lang_changed(&mut self, is_from: bool, name: SharedString, cx: &mut Context<Self>) {
        let Some(id) = self.langs.iter().find(|l| l.name == name.as_ref()).map(|l| l.id.clone())
        else {
            return;
        };
        let mut cfg = self.config.clone();
        if is_from {
            if cfg.from == id {
                return;
            }
            cfg.from = id;
        } else {
            if cfg.to == id {
                return;
            }
            cfg.to = id;
        }
        self.save(cfg, cx);
        self.translate_current(cx);
    }

    fn swap_languages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // auto 换到目标位没意义
        if self.config.from == "auto" {
            return;
        }
        let mut cfg = self.config.clone();
        std::mem::swap(&mut cfg.from, &mut cfg.to);
        self.save(cfg.clone(), cx);

        // 把两个下拉的选中项也同步过去
        let from_idx = self.langs.iter().position(|l| l.id == cfg.from);
        let to_idx = self.langs.iter().position(|l| l.id == cfg.to);
        if let Some(i) = from_idx {
            self.from_select
                .update(cx, |s, cx| s.set_selected_index(Some(IndexPath::new(i)), window, cx));
        }
        if let Some(i) = to_idx {
            self.to_select
                .update(cx, |s, cx| s.set_selected_index(Some(IndexPath::new(i)), window, cx));
        }
        self.translate_current(cx);
    }

    fn toggle_selection(&mut self, cx: &mut Context<Self>) {
        let mut cfg = self.config.clone();
        cfg.selection_enabled = !cfg.selection_enabled;
        self.save(cfg, cx);
    }

    // ---------------------------------------------------------------- 窗口

    /// 置顶（钉住）开关。
    fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        self.pinned = !self.pinned;
        if let Some(hwnd) = self.hwnd {
            crate::win::window::set_topmost(hwnd, self.pinned);
        }
        cx.notify();
    }

    // ---------------------------------------------------------------- 设置

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fonts = crate::win::fonts::list();
        let config = self.config.clone();
        let hotkey = self.hotkey.clone();
        let settings = cx.new(|cx| Settings::new(config, fonts, hotkey, window, cx));

        let sub = cx.subscribe(&settings, |this, _, ev: &SettingsEvent, cx| match ev {
            SettingsEvent::Save(cfg) => {
                this.save(cfg.clone(), cx);
                this.settings = None;
                cx.notify();
            }
            SettingsEvent::Cancel => {
                this.settings = None;
                cx.notify();
            }
        });
        self._subs.push(sub);
        self.settings = Some(settings);
        cx.notify();
    }

    // ---------------------------------------------------------------- 界面

    fn apply_font<E: Styled>(&self, el: E) -> E {
        // 行高按字号比例给（Textarea 内部的 Input 会把 line_height 定成 1.25rem 绝对值，
        // 字号调大后必须一起覆盖，否则行高不跟着长）。
        let mut el = el
            .text_size(px(self.config.font_size as f32))
            .line_height(relative(1.25))
            .font_weight(FontWeight(self.config.font_weight as f32));
        if !self.config.font_family.is_empty() {
            el = el.font_family(self.config.font_family.clone());
        }
        el
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let selection_on = self.config.selection_enabled;
        let has_result = !self.result.is_empty();
        let pinned = self.pinned;

        h_flex()
            .w_full()
            .h(px(40.))
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("selection")
                    .label(if selection_on { "划词：开" } else { "划词：关" })
                    .when(selection_on, |b| b.primary())
                    .when(!selection_on, |b| b.outline())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_selection(cx))),
            )
            // 系统标题栏已隐藏，仅把中间空白区作为拖拽区移动窗口；
            // 不能把整条工具栏设为拖拽区，否则按钮点击会被当成拖标题栏而失效。
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                Button::new("pin")
                    .icon(IconName::Star)
                    .when(pinned, |b| b.primary())
                    .when(!pinned, |b| b.ghost())
                    .tooltip(if pinned { "取消置顶" } else { "置顶窗口" })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pin(cx))),
            )
            .child(
                Button::new("copy")
                    .icon(IconName::Copy)
                    .ghost()
                    .disabled(!has_result)
                    .tooltip("复制译文")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.result.clone()));
                    })),
            )
            .child(
                Button::new("settings")
                    .icon(IconName::Ellipsis)
                    .ghost()
                    .tooltip("设置")
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
    }

    fn render_main(&self, cx: &mut Context<Self>) -> AnyElement {
        let engine_label = if self.config.engine == "google" {
            "Google 翻译（内置）"
        } else {
            "微软翻译（内置）"
        };

        v_flex()
            .size_full()
            .child(self.render_toolbar(cx))
            .when_some(self.selection_error.clone(), |this, err| {
                this.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .bg(cx.theme().danger.opacity(0.12))
                        .text_color(cx.theme().danger)
                        .text_sm()
                        .child(err),
                )
            })
            // 输入框
            .child(
                div().p_2().child(
                    self.apply_font(
                        div()
                            .h(px(160.0))
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_2()
                            // 字号必须直接设在 Textarea 上：Input 内部用 input_text_size()
                            // 给输入区写了固定 text_sm()，外层容器的继承字号会被它覆盖。
                            .child(
                                self.apply_font(Textarea::new(&self.input))
                                    .h_full()
                                    .appearance(false),
                            ),
                    ),
                ),
            )
            // 语言行
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .child(div().flex_1().child(Select::new(&self.from_select).id("from").w_full()))
                    .child(
                        Button::new("swap")
                            .icon(IconName::Replace)
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.swap_languages(window, cx)
                            })),
                    )
                    .child(div().flex_1().child(Select::new(&self.to_select).id("to").w_full())),
            )
            // 服务行
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .child(div().flex_1().text_sm().text_color(cx.theme().muted_foreground).child(engine_label)),
            )
            // 译文
            .child(
                div().flex_1().p_2().child(
                    self.apply_font(
                        div()
                            .id("output")
                            .size_full()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_2()
                            .child(self.render_output(cx)),
                    ),
                ),
            )
            .into_any_element()
    }

    fn render_output(&self, cx: &Context<Self>) -> AnyElement {
        if self.busy {
            return div()
                .text_color(cx.theme().muted_foreground)
                .child("翻译中…")
                .into_any_element();
        }
        if let Some(err) = &self.error {
            return div()
                .text_color(cx.theme().danger)
                .child(err.clone())
                .into_any_element();
        }
        self.apply_font(Textarea::new(&self.output))
            .h_full()
            .readonly(true)
            .appearance(false)
            .into_any_element()
    }
}

impl Render for TranslateApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 首帧收尾：取 HWND，挂 WS_EX_TOOLWINDOW（不进任务栏 / Alt+Tab）。
        if !self.window_ready {
            self.hwnd = crate::window_hwnd(window);
            if let Some(hwnd) = self.hwnd {
                crate::win::window::apply_toolwindow(hwnd);
            }
            // 点标题栏的关闭（X）不退程序，而是收进托盘；退出只走托盘菜单。
            window.on_window_should_close(cx, |window, _cx| {
                if let Some(hwnd) = crate::window_hwnd(window) {
                    crate::win::window::hide(hwnd);
                }
                false
            });
            // 失去焦点（点到别的窗口）就收进托盘；钉住时豁免，方便常驻对照。
            let sub = cx.observe_window_activation(window, |this, window, _cx| {
                if window.is_window_active() {
                    return;
                }
                if this.pinned {
                    return;
                }
                // 刚被程序主动唤出时，忽略这次失活抖动。
                if let Some(until) = this.autohide_guard_until {
                    if std::time::Instant::now() < until {
                        return;
                    }
                }
                if let Some(hwnd) = this.hwnd {
                    crate::win::window::hide(hwnd);
                }
            });
            self._subs.push(sub);
            self.window_ready = true;
        }

        // 划词取到的文本：塞进输入框、把窗口叫到前台、翻译。
        if let Some(text) = self.apply_source.take() {
            self.input
                .update(cx, |s, cx| s.set_value(text.clone(), window, cx));
            if let Some(hwnd) = self.hwnd {
                crate::win::window::show(hwnd);
            }
            window.activate_window();
            self.run_translation(text, cx);
        }

        // 新译文：写进只读的译文框。
        if let Some(text) = self.pending_output.take() {
            self.output
                .update(cx, |s, cx| s.set_value(text, window, cx));
        }

        let body = if let Some(settings) = &self.settings {
            settings.clone().into_any_element()
        } else {
            self.render_main(cx)
        };

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(body)
    }
}
