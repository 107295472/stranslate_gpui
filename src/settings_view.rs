//! 设置面板：默认引擎、两边的请求模式、Google 服务地址、划词、超时、字体。
//!
//! 对照 Flutter 版 `lib/src/settings_page.dart`。这里不单开窗口，而是作为一个
//! 子视图挂在主窗口里（主视图根据 `settings` 是否存在决定显示哪一块）。
//!
//! 交互：底部「保存 / 取消」两个按钮，分别 emit [`SettingsEvent::Save`] /
//! [`SettingsEvent::Cancel`]，由主视图订阅处理。

use gpui_kit::component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
    switch::Switch,
    v_flex, ActiveTheme, IndexPath,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::config::{
    AppConfig, GOOGLE_MODE_CUSTOM, GOOGLE_MODE_DIRECT, MS_MODE_EDGE, MS_MODE_SIGNATURE,
};

type StrSelect = Entity<SelectState<SearchableVec<SharedString>>>;

/// 一组下拉选项：显示名 + 背后的配置值。
struct Options {
    /// 和下拉里的条目顺序一致
    labels: Vec<SharedString>,
    /// 与 `labels` 一一对应
    values: Vec<String>,
}

impl Options {
    fn new(pairs: &[(&str, &str)]) -> Self {
        Self {
            labels: pairs.iter().map(|(_, label)| (*label).into()).collect(),
            values: pairs.iter().map(|(value, _)| (*value).to_string()).collect(),
        }
    }

    /// 当前配置值在选项里的下标。
    fn index_of(&self, value: &str) -> Option<usize> {
        self.values.iter().position(|v| v == value)
    }

    /// 把下拉选中的显示名映射回配置值。
    fn value_for(&self, label: &SharedString) -> Option<String> {
        self.labels
            .iter()
            .position(|l| l == label)
            .map(|i| self.values[i].clone())
    }
}

/// 设置面板对外抛出的事件。
pub enum SettingsEvent {
    /// 用户点了保存，带回改好的配置。
    Save(AppConfig),
    /// 用户点了取消。
    Cancel,
}

pub struct Settings {
    config: AppConfig,
    hotkey: String,

    engine_opts: Options,
    ms_opts: Options,
    google_opts: Options,
    font_opts: Options,
    size_opts: Options,
    weight_opts: Options,

    engine: StrSelect,
    microsoft_mode: StrSelect,
    google_mode: StrSelect,
    font_family: StrSelect,
    font_size: StrSelect,
    font_weight: StrSelect,

    google_api_url: Entity<InputState>,
    fetch_timeout: Entity<InputState>,
    http_timeout: Entity<InputState>,

    selection_enabled: bool,
    selection_auto_mouse: bool,
}

impl EventEmitter<SettingsEvent> for Settings {}

impl Settings {
    pub fn new(
        config: AppConfig,
        fonts: Vec<String>,
        hotkey: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let engine_opts = Options::new(&[
            ("microsoft", "微软翻译（内置）"),
            ("google", "Google 翻译（内置）"),
        ]);
        let ms_opts = Options::new(&[
            (MS_MODE_EDGE, "Edge 免鉴权（推荐）"),
            (MS_MODE_SIGNATURE, "Signature 自签名"),
        ]);
        let google_opts = Options::new(&[
            (GOOGLE_MODE_DIRECT, "直连 translate.google.com"),
            (GOOGLE_MODE_CUSTOM, "自定义服务地址"),
        ]);

        // 字体：第一项「默认字体」映射到空串（跟随系统）。
        let mut font_pairs: Vec<(&str, &str)> = vec![("", "默认字体")];
        font_pairs.extend(fonts.iter().map(|f| (f.as_str(), f.as_str())));
        let font_opts = Options::new(&font_pairs);

        let size_opts = Options::new(&[
            ("12", "12"),
            ("13", "13"),
            ("14", "14"),
            ("15", "15"),
            ("16", "16"),
            ("18", "18"),
            ("20", "20"),
            ("24", "24"),
        ]);
        let weight_opts = Options::new(&[
            ("300", "细 (300)"),
            ("400", "常规 (400)"),
            ("500", "中等 (500)"),
            ("600", "半粗 (600)"),
            ("700", "粗 (700)"),
        ]);

        let engine = Self::make_select(&engine_opts, &config.engine, window, cx);
        let microsoft_mode = Self::make_select(&ms_opts, &config.microsoft_mode, window, cx);
        let google_mode = Self::make_select(&google_opts, &config.google_mode, window, cx);
        let font_family = Self::make_select(&font_opts, &config.font_family, window, cx);
        let font_size =
            Self::make_select(&size_opts, &config.font_size.to_string(), window, cx);
        let font_weight =
            Self::make_select(&weight_opts, &config.font_weight.to_string(), window, cx);

        let google_api_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("自定义 Google 翻译服务地址（留空用直连）")
                .default_value(config.google_api_url.clone())
        });
        let fetch_timeout = cx.new(|cx| {
            InputState::new(window, cx).default_value(config.selection_fetch_timeout_ms.to_string())
        });
        let http_timeout = cx.new(|cx| {
            InputState::new(window, cx).default_value(config.http_timeout_secs.to_string())
        });

        let selection_enabled = config.selection_enabled;
        let selection_auto_mouse = config.selection_auto_mouse;

        Self {
            config,
            hotkey,
            engine_opts,
            ms_opts,
            google_opts,
            font_opts,
            size_opts,
            weight_opts,
            engine,
            microsoft_mode,
            google_mode,
            font_family,
            font_size,
            font_weight,
            google_api_url,
            fetch_timeout,
            http_timeout,
            selection_enabled,
            selection_auto_mouse,
        }
    }

    fn make_select(
        opts: &Options,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> StrSelect {
        let idx = opts.index_of(current);
        let labels = opts.labels.clone();
        cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(labels),
                idx.map(IndexPath::new),
                window,
                cx,
            )
        })
    }

    /// 从各个控件收集出一份新配置。
    fn collect(&self, cx: &Context<Self>) -> AppConfig {
        let mut cfg = self.config.clone();

        if let Some(v) = self
            .engine
            .read(cx)
            .selected_value()
            .and_then(|l| self.engine_opts.value_for(l))
        {
            cfg.engine = v;
        }
        if let Some(v) = self
            .microsoft_mode
            .read(cx)
            .selected_value()
            .and_then(|l| self.ms_opts.value_for(l))
        {
            cfg.microsoft_mode = v;
        }
        if let Some(v) = self
            .google_mode
            .read(cx)
            .selected_value()
            .and_then(|l| self.google_opts.value_for(l))
        {
            cfg.google_mode = v;
        }
        if let Some(v) = self
            .font_family
            .read(cx)
            .selected_value()
            .and_then(|l| self.font_opts.value_for(l))
        {
            cfg.font_family = v;
        }
        if let Some(v) = self
            .font_size
            .read(cx)
            .selected_value()
            .and_then(|l| self.size_opts.value_for(l))
            .and_then(|s| s.parse::<u32>().ok())
        {
            cfg.font_size = v;
        }
        if let Some(v) = self
            .font_weight
            .read(cx)
            .selected_value()
            .and_then(|l| self.weight_opts.value_for(l))
            .and_then(|s| s.parse::<u32>().ok())
        {
            cfg.font_weight = v;
        }

        cfg.google_api_url = self.google_api_url.read(cx).value().trim().to_string();

        if let Ok(v) = self.fetch_timeout.read(cx).value().trim().parse::<u32>() {
            cfg.selection_fetch_timeout_ms = v.max(1);
        }
        if let Ok(v) = self.http_timeout.read(cx).value().trim().parse::<u32>() {
            cfg.http_timeout_secs = v.max(1);
        }

        cfg.selection_enabled = self.selection_enabled;
        cfg.selection_auto_mouse = self.selection_auto_mouse;

        cfg
    }

    fn row(
        &self,
        label: impl Into<SharedString>,
        control: impl IntoElement,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .py_1p5()
            .gap_3()
            .items_center()
            .child(
                div()
                    .w(px(120.))
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.into()),
            )
            .child(div().flex_1().child(control))
    }
}

impl Render for Settings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // 标题栏
            .child(
                h_flex()
                    .w_full()
                    .h(px(40.))
                    .px_3()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().flex_1().font_weight(FontWeight(600.)).child("设置"))
                    .child(
                        Button::new("cancel")
                            .label("取消")
                            .ghost()
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SettingsEvent::Cancel);
                            })),
                    )
                    .child(
                        Button::new("save")
                            .label("保存")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let cfg = this.collect(cx);
                                cx.emit(SettingsEvent::Save(cfg));
                            })),
                    ),
            )
            // 表单
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .w_full()
                    .px_3()
                    .py_2()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .child(self.row(
                                "默认引擎",
                                Select::new(&self.engine).id("engine").w_full(),
                                cx,
                            ))
                            .child(self.row(
                                "微软请求模式",
                                Select::new(&self.microsoft_mode).id("ms-mode").w_full(),
                                cx,
                            ))
                            .child(self.row(
                                "Google 请求模式",
                                Select::new(&self.google_mode).id("google-mode").w_full(),
                                cx,
                            ))
                            .child(self.row(
                                "Google 服务地址",
                                Input::new(&self.google_api_url),
                                cx,
                            ))
                            .child(self.row(
                                "字体",
                                Select::new(&self.font_family).id("font").w_full(),
                                cx,
                            ))
                            .child(self.row(
                                "字号",
                                Select::new(&self.font_size).id("font-size").w_full(),
                                cx,
                            ))
                            .child(self.row(
                                "字重",
                                Select::new(&self.font_weight).id("font-weight").w_full(),
                                cx,
                            ))
                            .child(self.row(
                                "取词超时 (ms)",
                                Input::new(&self.fetch_timeout),
                                cx,
                            ))
                            .child(self.row(
                                "HTTP 超时 (s)",
                                Input::new(&self.http_timeout),
                                cx,
                            ))
                            .child(
                                h_flex().w_full().py_1p5().gap_3().items_center().child(
                                    Switch::new("selection-enabled")
                                        .checked(self.selection_enabled)
                                        .label(format!("启用划词（{}）", self.hotkey))
                                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                            this.selection_enabled = *checked;
                                            cx.notify();
                                        })),
                                ),
                            )
                            .child(
                                h_flex().w_full().py_1p5().gap_3().items_center().child(
                                    Switch::new("selection-auto-mouse")
                                        .checked(self.selection_auto_mouse)
                                        .label("鼠标自动划词（拖选/双击后弹窗）")
                                        .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                            this.selection_auto_mouse = *checked;
                                            cx.notify();
                                        })),
                                ),
                            ),
                    ),
            )
    }
}
