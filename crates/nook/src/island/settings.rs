//! Settings window — Nook chrome: titlebar, icon toolbar, pills, module list.

use super::ui::label;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgb, rgba, AnyElement, Context, CursorStyle, FocusHandle, FontWeight,
    KeyDownEvent, MouseButton, SharedString, Window,
};
use nook_core::settings::AppSettings;
use std::sync::atomic::{AtomicU8, Ordering};

/// Default settings window. 3:2 so the two-column Custom Widgets page
/// is neither a tall stretch (860×640) nor a cramped portrait (420×620).
pub(super) const SETTINGS_SIZE: (f32, f32) = (900.0, 600.0);
pub(super) const SETTINGS_MIN: (f32, f32) = (780.0, 520.0);

/// Last surface the user had open. Survives closing the window.
static LAST_CATEGORY: AtomicU8 = AtomicU8::new(SettingsCategory::Nook as u8);
static LAST_NOOK_TAB: AtomicU8 = AtomicU8::new(NookTab::CustomWidgets as u8);
static LAST_MODULE: AtomicU8 = AtomicU8::new(WidgetModule::Calendar as u8);

fn token_text(token: &str, revealed: bool) -> String {
    if revealed {
        token.to_string()
    } else {
        "••••••••".into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SettingsCategory {
    General = 0,
    Nook = 1,
}

impl SettingsCategory {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Nook,
            _ => Self::General,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum NookTab {
    General = 0,
    CustomWidgets = 1,
}

impl NookTab {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::CustomWidgets,
            _ => Self::General,
        }
    }
}

/// Built-in island modules. No competitor widgets (Mirror, Shortcuts, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum WidgetModule {
    Calendar = 0,
    Music = 1,
    Files = 2,
    Notes = 3,
    Observe = 4,
    Timers = 5,
    Reminders = 6,
    Speed = 7,
    Agents = 8,
}

impl WidgetModule {
    const ALL: [Self; 9] = [
        Self::Calendar,
        Self::Music,
        Self::Files,
        Self::Notes,
        Self::Observe,
        Self::Timers,
        Self::Reminders,
        Self::Speed,
        Self::Agents,
    ];

    fn from_u8(v: u8) -> Self {
        Self::ALL
            .into_iter()
            .find(|m| *m as u8 == v)
            .unwrap_or(Self::Calendar)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Calendar => "Calendar",
            Self::Music => "Music",
            Self::Files => "Files",
            Self::Notes => "Notes",
            Self::Observe => "Observe",
            Self::Timers => "Timers",
            Self::Reminders => "Reminders",
            Self::Speed => "Speed Test",
            Self::Agents => "Agents",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::Music => "music",
            Self::Files => "files",
            Self::Notes => "notebook",
            Self::Observe => "activity",
            Self::Timers => "clock",
            Self::Reminders => "list-checks",
            Self::Speed => "gauge",
            Self::Agents => "bot",
        }
    }

    fn subtitle(self, settings: &AppSettings) -> SharedString {
        match self {
            Self::Calendar => "7 days".into(),
            Self::Music => "Now Playing".into(),
            Self::Files => "Tray tab".into(),
            Self::Notes => "Scratchpad".into(),
            Self::Observe => observe_subtitle(settings.observe.metrics.len()),
            Self::Timers => "Countdown".into(),
            Self::Reminders => "EventKit".into(),
            Self::Speed => "Cloudflare".into(),
            Self::Agents => "Sessions".into(),
        }
    }

    fn enabled(self, settings: &AppSettings) -> bool {
        match self {
            Self::Calendar => settings.show_calendar,
            Self::Music => settings.show_media,
            Self::Files => settings.show_files,
            Self::Notes => settings.show_notes,
            Self::Observe => settings.show_observe,
            Self::Timers => settings.show_timers,
            Self::Reminders => settings.show_reminders,
            Self::Speed => settings.show_speed,
            Self::Agents => settings.show_agents,
        }
    }

    fn set_enabled(self, settings: &mut AppSettings) {
        match self {
            Self::Calendar => settings.show_calendar = !settings.show_calendar,
            Self::Music => settings.show_media = !settings.show_media,
            Self::Files => settings.show_files = !settings.show_files,
            Self::Notes => settings.show_notes = !settings.show_notes,
            Self::Observe => settings.show_observe = !settings.show_observe,
            Self::Timers => settings.show_timers = !settings.show_timers,
            Self::Reminders => settings.show_reminders = !settings.show_reminders,
            Self::Speed => settings.show_speed = !settings.show_speed,
            Self::Agents => settings.show_agents = !settings.show_agents,
        }
    }

    fn preview_label(self) -> &'static str {
        match self {
            Self::Calendar => "Calendar",
            Self::Music => "Music",
            Self::Files => "Files",
            Self::Notes => "Notes",
            Self::Observe => "Observe",
            Self::Timers => "Timers",
            Self::Reminders => "Reminders",
            Self::Speed => "Speed",
            Self::Agents => "Agents",
        }
    }
}

fn observe_subtitle(pinned: usize) -> SharedString {
    match pinned {
        0 => "Prometheus".into(),
        1 => "1 metric".into(),
        n => format!("{n} metrics").into(),
    }
}

pub(super) struct SettingsView {
    category: SettingsCategory,
    nook_tab: NookTab,
    module: WidgetModule,
    url_focus: FocusHandle,
    token_focus: FocusHandle,
    query_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    token_revealed: bool,
    query_draft: String,
    catalog: Vec<String>,
    catalog_error: Option<String>,
    catalog_loading: bool,
}

impl SettingsView {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let settings = nook_core::settings::get_app_settings();
        Self {
            category: SettingsCategory::from_u8(LAST_CATEGORY.load(Ordering::Relaxed)),
            nook_tab: NookTab::from_u8(LAST_NOOK_TAB.load(Ordering::Relaxed)),
            module: WidgetModule::from_u8(LAST_MODULE.load(Ordering::Relaxed)),
            url_focus: cx.focus_handle(),
            token_focus: cx.focus_handle(),
            query_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            token_draft: settings.observe.metrics_token,
            token_revealed: false,
            query_draft: String::new(),
            catalog: Vec::new(),
            catalog_error: None,
            catalog_loading: false,
        }
    }

    fn persist_nav(&self) {
        LAST_CATEGORY.store(self.category as u8, Ordering::Relaxed);
        LAST_NOOK_TAB.store(self.nook_tab as u8, Ordering::Relaxed);
        LAST_MODULE.store(self.module as u8, Ordering::Relaxed);
    }

    fn persist_url(&self) {
        let draft = self.url_draft.trim().to_string();
        if !draft.is_empty() && nook_core::observe::normalize_base_url(&draft).is_err() {
            return;
        }
        let mut s = nook_core::settings::get_app_settings();
        if s.observe.prometheus_url == draft {
            return;
        }
        nook_core::observe::set_metrics_url(&mut s.observe, draft);
        nook_core::settings::update_app_settings(s);
    }

    fn persist_token(&self) {
        let draft = self.token_draft.trim().to_string();
        let mut s = nook_core::settings::get_app_settings();
        if s.observe.metrics_token == draft {
            return;
        }
        s.observe.metrics_token = draft;
        nook_core::settings::update_app_settings(s);
    }

    fn persist_observe(tweak: impl FnOnce(&mut AppSettings)) {
        nook_core::settings::tweak_app_settings(tweak);
    }

    fn apply_key(draft: &mut String, event: &KeyDownEvent, cx: &Context<Self>) -> bool {
        let ks = &event.keystroke;
        if ks.modifiers.secondary() && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                *draft = text.trim().to_string();
                return true;
            }
            return false;
        }
        if ks.modifiers.platform || ks.modifiers.control {
            return false;
        }
        match ks.key.as_str() {
            "backspace" => {
                draft.pop();
                true
            }
            _ => {
                if let Some(ch) = &ks.key_char {
                    if !ch.chars().any(|c| c.is_control()) {
                        draft.push_str(ch);
                        return true;
                    }
                }
                false
            }
        }
    }

    fn browse_metrics(&mut self, cx: &mut Context<Self>) {
        if self.catalog_loading {
            return;
        }
        self.persist_url();
        self.catalog_loading = true;
        self.catalog_error = None;
        cx.notify();
        let config = nook_core::settings::get_app_settings().observe;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    nook_core::runtime().block_on(nook_core::observe::list_metric_names(&config))
                })
                .await;
            this.update(cx, |this, cx| {
                this.catalog_loading = false;
                match result {
                    Ok(names) => {
                        this.catalog = names;
                        this.catalog_error = None;
                    }
                    Err(err) => this.catalog_error = Some(err),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl gpui::Render for SettingsView {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = nook_core::settings::get_app_settings();
        let url_focused = self.url_focus.is_focused(window);
        let token_focused = self.token_focus.is_focused(window);
        let query_focused = self.query_focus.is_focused(window);
        let show_widgets = self.category == SettingsCategory::Nook
            && self.nook_tab == NookTab::CustomWidgets;

        div()
            .id("settings-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::SETTINGS_GLASS)
            .text_color(theme::LABEL)
            .pt(px(36.))
            .px(px(24.))
            .pb(px(20.))
            .gap(px(12.))
            .child(self.category_toolbar(cx))
            .child(self.pill_row(cx))
            .child(if show_widgets {
                self.render_custom_widgets(
                    &settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    cx,
                )
                .into_any_element()
            } else {
                self.render_general(&settings, cx).into_any_element()
            })
    }
}

impl SettingsView {
    fn category_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .justify_center()
            .gap(px(32.))
            .child(self.toolbar_item(
                "settings",
                "General",
                self.category == SettingsCategory::General,
                SettingsCategory::General,
                cx,
            ))
            .child(self.toolbar_item(
                "map-pin",
                "Nook",
                self.category == SettingsCategory::Nook,
                SettingsCategory::Nook,
                cx,
            ))
    }

    fn toolbar_item(
        &self,
        icon: &'static str,
        caption: &'static str,
        selected: bool,
        category: SettingsCategory,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let color = if selected {
            theme::accent()
        } else {
            theme::SECONDARY_LABEL
        };
        div()
            .id(SharedString::from(format!("toolbar-{caption}")))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.category = category;
                    if category == SettingsCategory::Nook {
                        this.nook_tab = NookTab::CustomWidgets;
                    }
                    this.persist_nav();
                    cx.notify();
                }),
            )
            .child(lucide_color(icon, 22.0, color))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(color)
                    .child(caption),
            )
    }

    fn pill_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().w_full().flex().justify_center().gap(px(8.));
        match self.category {
            SettingsCategory::General => {
                row = row.child(self.pill("General", true, NookTab::General, false, cx));
            }
            SettingsCategory::Nook => {
                row = row
                    .child(self.pill(
                        "General",
                        self.nook_tab == NookTab::General,
                        NookTab::General,
                        true,
                        cx,
                    ))
                    .child(self.pill(
                        "Custom Widgets",
                        self.nook_tab == NookTab::CustomWidgets,
                        NookTab::CustomWidgets,
                        true,
                        cx,
                    ));
            }
        }
        row
    }

    fn pill(
        &self,
        caption: &'static str,
        selected: bool,
        tab: NookTab,
        switch_tab: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("pill-{caption}")))
            .h(px(28.))
            .px(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(if selected {
                rgba(0xffffff22)
            } else {
                rgba(0xffffff0D)
            })
            .hover(|s| s.bg(rgba(0xffffff2A)))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if switch_tab {
                        this.nook_tab = tab;
                        this.persist_nav();
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if selected {
                        theme::LABEL
                    } else {
                        theme::SECONDARY_LABEL
                    })
                    .child(caption),
            )
    }

    fn render_general(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("general-pane")
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .child(settings_group(vec![
                toggle_row("Translucent island", settings.liquid_glass_mode, cx, |s| {
                    s.liquid_glass_mode = !s.liquid_glass_mode
                })
                .into_any_element(),
                toggle_row(
                    "Show island without a notch",
                    settings.non_notch_mode,
                    cx,
                    |s| {
                        s.non_notch_mode = !s.non_notch_mode;
                    },
                )
                .into_any_element(),
            ]))
            .child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .text_color(theme::SECONDARY_LABEL)
                    .child(
                        "Hover the notch to expand. Settings and Quit live in the menu bar extra.",
                    ),
            )
    }

    fn render_custom_widgets(
        &self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .flex()
            .gap(px(16.))
            .child(self.module_list(settings, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(self.island_preview(settings))
                    .child(
                        div()
                            .id("module-controls")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .child(self.module_controls(
                                settings,
                                url_focused,
                                token_focused,
                                query_focused,
                                cx,
                            )),
                    ),
            )
    }

    fn module_list(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .id("module-list")
            .w(px(232.))
            .flex_shrink_0()
            .h_full()
            .rounded(px(14.))
            .bg(theme::SETTINGS_WELL)
            .p(px(6.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .overflow_y_scroll();
        for module in WidgetModule::ALL {
            list = list.child(self.module_row(module, settings, cx));
        }
        list
    }

    fn module_row(
        &self,
        module: WidgetModule,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.module == module;
        let on = module.enabled(settings);
        let label_color = if selected {
            theme::LABEL
        } else {
            theme::LABEL
        };
        let sub_color = if selected {
            rgba(0xffffffCC)
        } else {
            theme::SECONDARY_LABEL
        };
        div()
            .id(SharedString::from(format!("mod-{}", module.name())))
            .w_full()
            .h(px(44.))
            .px(px(8.))
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(if selected {
                theme::accent()
            } else {
                rgba(0x00000000)
            })
            .hover(|s| {
                if selected {
                    s
                } else {
                    s.bg(rgba(0xffffff10))
                }
            })
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.module = module;
                    this.persist_nav();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .size(px(28.))
                    .rounded(px(7.))
                    .bg(if selected {
                        rgba(0xffffff28)
                    } else {
                        theme::FILL
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(lucide_color(module.icon(), 16.0, label_color)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(label_color)
                            .child(module.name()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(sub_color)
                            .child(module.subtitle(settings)),
                    ),
            )
            .child(module_toggle(on, module, cx))
    }

    fn island_preview(&self, settings: &AppSettings) -> impl IntoElement {
        let mut chips = div().flex().items_center().gap(px(14.));
        let mut shown = 0;
        for module in WidgetModule::ALL {
            if !module.enabled(settings) {
                continue;
            }
            if shown >= 4 {
                break;
            }
            shown += 1;
            let emphasize = module == self.module;
            chips = chips.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .child(lucide_color(
                        module.icon(),
                        16.0,
                        if emphasize {
                            theme::LABEL
                        } else {
                            theme::SECONDARY_LABEL
                        },
                    ))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(if emphasize {
                                theme::LABEL
                            } else {
                                theme::SECONDARY_LABEL
                            })
                            .child(module.preview_label()),
                    ),
            );
        }
        if shown == 0 {
            chips = chips.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::SECONDARY_LABEL)
                    .child("No widgets enabled"),
            );
        }

        div()
            .id("island-preview")
            .w_full()
            .h(px(128.))
            .rounded(px(14.))
            .overflow_hidden()
            .relative()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgb(0x1b2a1c)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(-40.))
                    .top(px(-50.))
                    .size(px(240.))
                    .rounded_full()
                    .bg(rgba(0x3f6a3e55)),
            )
            .child(
                div()
                    .absolute()
                    .right(px(-30.))
                    .bottom(px(-60.))
                    .size(px(220.))
                    .rounded_full()
                    .bg(rgba(0x1a3d2a66)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .h(px(52.))
                            .px(px(22.))
                            .rounded_full()
                            .bg(rgb(0x000000))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(chips),
                    ),
            )
    }

    fn module_controls(
        &self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.module {
            WidgetModule::Observe => self
                .render_observe_settings(
                    settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    cx,
                )
                .into_any_element(),
            WidgetModule::Notes => notes_controls(cx).into_any_element(),
            WidgetModule::Calendar => calendar_controls(cx).into_any_element(),
            other => module_blurb(other).into_any_element(),
        }
    }

    fn render_observe_settings(
        &self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut pinned = div().flex().flex_col().gap_1();
        if settings.observe.metrics.is_empty() {
            pinned = pinned.child(label("No pinned metrics", theme::SUBHEADLINE, false));
        } else {
            for metric in &settings.observe.metrics {
                let query = metric.query.clone();
                let chart_query = metric.query.clone();
                let alert_query = metric.query.clone();
                let chart_caption = metric.chart.caption();
                let alert_caption = metric.alert_caption();
                pinned = pinned.child(
                    div()
                        .id(SharedString::from(format!("pin-{}", metric.query)))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded(px(theme::CONTROL_RADIUS))
                        .bg(theme::FILL)
                        .child(
                            label(
                                format!("{}  {}", metric.label, metric.query),
                                theme::SUBHEADLINE,
                                true,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("chart-{}", metric.query)))
                                .h(px(theme::HIT_MIN))
                                .px_2()
                                .flex()
                                .items_center()
                                .cursor(CursorStyle::PointingHand)
                                .hover(|s| s.opacity(0.8))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        SettingsView::persist_observe(|s| {
                                            nook_core::observe::cycle_metric_chart(
                                                &mut s.observe,
                                                &chart_query,
                                            );
                                        });
                                        cx.notify();
                                    }),
                                )
                                .child(label(chart_caption, theme::SUBHEADLINE, true)),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("alert-{}", metric.query)))
                                .h(px(theme::HIT_MIN))
                                .px_2()
                                .flex()
                                .items_center()
                                .cursor(CursorStyle::PointingHand)
                                .hover(|s| s.opacity(0.8))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        SettingsView::persist_observe(|s| {
                                            nook_core::observe::cycle_metric_alert(
                                                &mut s.observe,
                                                &alert_query,
                                            );
                                        });
                                        cx.notify();
                                    }),
                                )
                                .child(label(alert_caption, theme::SUBHEADLINE, true)),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("unpin-{}", metric.query)))
                                .h(px(theme::HIT_MIN))
                                .px_2()
                                .flex()
                                .items_center()
                                .cursor(CursorStyle::PointingHand)
                                .hover(|s| s.opacity(0.8))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |_, _, _, cx| {
                                        cx.stop_propagation();
                                        SettingsView::persist_observe(|s| {
                                            nook_core::observe::unpin_metric(
                                                &mut s.observe,
                                                &query,
                                            );
                                        });
                                        cx.notify();
                                    }),
                                )
                                .child(label("Remove", theme::SUBHEADLINE, false)),
                        ),
                );
            }
        }

        let mut catalog = div().flex().flex_col().gap_1();
        if self.catalog_loading {
            catalog = catalog.child(label("Loading metric names…", theme::SUBHEADLINE, false));
        } else if let Some(err) = &self.catalog_error {
            catalog = catalog.child(label(err.clone(), theme::SUBHEADLINE, false).w_full());
        }
        for name in self.catalog.iter().take(12) {
            let query = name.clone();
            let label_text = name.clone();
            catalog = catalog.child(
                div()
                    .id(SharedString::from(format!("cat-{name}")))
                    .px_2()
                    .py_1()
                    .min_h(px(theme::HIT_MIN))
                    .flex()
                    .items_center()
                    .rounded(px(theme::CONTROL_RADIUS))
                    .bg(theme::FILL)
                    .hover(|s| s.bg(theme::FILL_SECONDARY))
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            let query = query.clone();
                            let label_text = label_text.clone();
                            SettingsView::persist_observe(|s| {
                                let _ = nook_core::observe::pin_metric(
                                    &mut s.observe,
                                    &label_text,
                                    &query,
                                );
                            });
                            cx.notify();
                        }),
                    )
                    .child(label(name.clone(), theme::SUBHEADLINE, true)),
            );
        }

        let url_placeholder = self.url_draft.is_empty();
        let url_text = if url_placeholder {
            nook_core::observe::DEFAULT_OBSERVE_URL
        } else {
            self.url_draft.as_str()
        };
        let token_placeholder = self.token_draft.is_empty();
        let token_text = if token_placeholder {
            "Bearer token for /admin/metrics".to_string()
        } else {
            token_text(&self.token_draft, self.token_revealed)
        };
        let query_placeholder = self.query_draft.is_empty();
        let query_text = if query_placeholder {
            "total_requests, 5xx, or PromQL"
        } else {
            self.query_draft.as_str()
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(theme::INNER_RADIUS))
            .bg(theme::GROUPED_BG)
            .p_3()
            .child(label("Metrics", theme::CALLOUT, true))
            .child(label(
                "Defaults to the warmUP API. Prometheus still works if you point at a Prom host.",
                theme::SUBHEADLINE,
                false,
            ))
            .child(label(
                "Pins default to a line chart. Prometheus uses query_range; warmUP keeps 24 h of samples. Pick a lookback.",
                theme::SUBHEADLINE,
                false,
            ))
            .child({
                let mut chips = div().flex().gap_2();
                for option in nook_core::observe::ObserveRange::all() {
                    let selected = settings.observe.range == option;
                    chips = chips.child(chip(
                        option.label(),
                        selected,
                        cx,
                        move |_, _, cx| {
                            SettingsView::persist_observe(|s| {
                                nook_core::observe::set_range(&mut s.observe, option);
                            });
                            cx.notify();
                        },
                    ));
                }
                chips
            })
            .child(
                div()
                    .id("prom-url")
                    .track_focus(&self.url_focus)
                    .px_2()
                    .py_2()
                    .min_h(px(theme::HIT_MIN))
                    .rounded(px(theme::CONTROL_RADIUS))
                    .bg(theme::FILL)
                    .when(url_focused, |d| d.border_1().border_color(theme::accent()))
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.url_focus);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let persist = SettingsView::apply_key(&mut this.url_draft, event, cx)
                            || event.keystroke.key == "enter";
                        if persist {
                            this.persist_url();
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_color(if url_placeholder {
                                theme::TERTIARY_LABEL
                            } else {
                                theme::LABEL
                            })
                            .text_size(px(theme::BODY.size))
                            .child(SharedString::from(url_text.to_string())),
                    ),
            )
            .child(
                div()
                    .id("prom-token")
                    .track_focus(&self.token_focus)
                    .px_2()
                    .py_2()
                    .min_h(px(theme::HIT_MIN))
                    .rounded(px(theme::CONTROL_RADIUS))
                    .bg(theme::FILL)
                    .when(token_focused, |d| {
                        d.border_1().border_color(theme::accent())
                    })
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.token_focus);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let persist = SettingsView::apply_key(&mut this.token_draft, event, cx)
                            || event.keystroke.key == "enter";
                        if persist {
                            this.persist_token();
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_color(if token_placeholder {
                                theme::TERTIARY_LABEL
                            } else {
                                theme::LABEL
                            })
                            .text_size(px(theme::BODY.size))
                            .child(SharedString::from(token_text)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(settings_chip("Paste URL", cx, |this, _, cx| {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            this.url_draft = text.trim().to_string();
                            this.persist_url();
                            cx.notify();
                        }
                    }))
                    .child(settings_chip("Browse names", cx, |this, _, cx| {
                        this.browse_metrics(cx);
                    }))
                    .child(settings_chip(
                        if self.token_revealed {
                            "Hide token"
                        } else {
                            "Show token"
                        },
                        cx,
                        |this, _, cx| {
                            this.token_revealed = !this.token_revealed;
                            cx.notify();
                        },
                    )),
            )
            .child(label("Pinned metrics", theme::CALLOUT, true))
            .child(label(
                "The compact island shows Observe only while a threshold you set is firing.",
                theme::SUBHEADLINE,
                false,
            ))
            .child(pinned)
            .child(
                div()
                    .id("prom-query")
                    .track_focus(&self.query_focus)
                    .px_2()
                    .py_2()
                    .min_h(px(theme::HIT_MIN))
                    .rounded(px(theme::CONTROL_RADIUS))
                    .bg(theme::FILL)
                    .when(query_focused, |d| {
                        d.border_1().border_color(theme::accent())
                    })
                    .cursor(CursorStyle::IBeam)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.query_focus);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" {
                            let query = this.query_draft.trim().to_string();
                            if !query.is_empty() {
                                SettingsView::persist_observe(|s| {
                                    let _ = nook_core::observe::pin_metric(
                                        &mut s.observe,
                                        &query,
                                        &query,
                                    );
                                });
                                this.query_draft.clear();
                                cx.notify();
                            }
                        } else if SettingsView::apply_key(&mut this.query_draft, event, cx) {
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .text_color(if query_placeholder {
                                theme::TERTIARY_LABEL
                            } else {
                                theme::LABEL
                            })
                            .text_size(px(theme::BODY.size))
                            .child(SharedString::from(query_text.to_string())),
                    ),
            )
            .child(settings_chip("Pin query", cx, |this, _, cx| {
                let query = this.query_draft.trim().to_string();
                if query.is_empty() {
                    return;
                }
                SettingsView::persist_observe(|s| {
                    let _ = nook_core::observe::pin_metric(&mut s.observe, &query, &query);
                });
                this.query_draft.clear();
                cx.notify();
            }))
            .child(catalog)
    }
}

fn calendar_controls(cx: &mut Context<SettingsView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(px(theme::INNER_RADIUS))
        .bg(theme::GROUPED_BG)
        .p_3()
        .child(label("Calendar", theme::CALLOUT, true))
        .child(label(
            "Week strip is today ± 3 days (7 cells). EventKit includes every calendar the system allows — there is no per-calendar filter yet.",
            theme::SUBHEADLINE,
            false,
        ))
        .child(settings_chip("Open Calendar", cx, |_, _, _| {
            crate::platform::open_calendar();
        }))
}

fn notes_controls(cx: &mut Context<SettingsView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(px(theme::INNER_RADIUS))
        .bg(theme::GROUPED_BG)
        .p_3()
        .child(label("Notes", theme::CALLOUT, true))
        .child(label(
            "Scratchpad on the island. Edit here or in the expanded card.",
            theme::SUBHEADLINE,
            false,
        ))
        .child(settings_chip("Edit Notes…", cx, |_, _, _| {
            if let Err(err) = nook_core::notes::open_notes_editor() {
                log::warn!("open notes: {err}");
            }
        }))
}

fn module_blurb(module: WidgetModule) -> impl IntoElement {
    let copy = match module {
        WidgetModule::Music => {
            "Now Playing from MediaRemote on macOS, with an AppleScript fallback."
        }
        WidgetModule::Files => "Drop zone and tray live on the Tray tab of the expanded island.",
        WidgetModule::Timers => "Countdown presets and a compact ring while a timer is running.",
        WidgetModule::Reminders => "Incomplete reminders from EventKit, same store as Calendar.",
        WidgetModule::Speed => "Cloudflare (then OVH) download probe. Runs from the island card.",
        WidgetModule::Agents => "Working coding-agent sessions on the compact face and expanded card.",
        WidgetModule::Calendar | WidgetModule::Notes | WidgetModule::Observe => "",
    };
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded(px(theme::INNER_RADIUS))
        .bg(theme::GROUPED_BG)
        .p_3()
        .child(label(module.name(), theme::CALLOUT, true))
        .child(label(copy, theme::SUBHEADLINE, false))
}

fn settings_chip(
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    chip(caption, false, cx, on_click)
}

/// One selectable pill used for actions and option chips.
fn chip(
    caption: impl Into<SharedString>,
    selected: bool,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(caption.clone())
        .h(px(theme::HIT_MIN))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::CONTROL_RADIUS))
        .bg(if selected {
            theme::accent()
        } else {
            theme::FILL
        })
        .when(selected, |d| d.border_1().border_color(theme::accent()))
        .hover(|s| s.bg(theme::FILL_SECONDARY))
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(label(caption, theme::CALLOUT, true))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
}

fn settings_group(rows: Vec<AnyElement>) -> impl IntoElement {
    let mut group = div()
        .flex()
        .flex_col()
        .rounded(px(theme::INNER_RADIUS))
        .bg(theme::GROUPED_BG)
        .px_3();
    for row in rows {
        group = group.child(row);
    }
    group
}

fn toggle_row(
    label_text: &'static str,
    on: bool,
    cx: &mut Context<SettingsView>,
    tweak: impl Fn(&mut AppSettings) + 'static,
) -> impl IntoElement {
    div()
        .id(label_text)
        .flex()
        .items_center()
        .justify_between()
        .h(px(36.))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                let mut s = nook_core::settings::get_app_settings();
                tweak(&mut s);
                nook_core::settings::update_app_settings(s);
                cx.notify();
            }),
        )
        .child(label(label_text, theme::BODY, true))
        .child(toggle_knob(on))
}

fn module_toggle(
    on: bool,
    module: WidgetModule,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("tog-{}", module.name())))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                let mut s = nook_core::settings::get_app_settings();
                module.set_enabled(&mut s);
                nook_core::settings::update_app_settings(s);
                cx.notify();
            }),
        )
        .child(toggle_knob(on))
}

fn toggle_knob(on: bool) -> impl IntoElement {
    div()
        .w(px(40.))
        .h(px(24.))
        .rounded_full()
        .bg(if on {
            theme::accent()
        } else {
            theme::FILL_SECONDARY
        })
        .flex()
        .items_center()
        .when(on, |d| d.justify_end())
        .px(px(2.))
        .child(div().size(px(20.)).rounded_full().bg(rgb(0xffffff)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_token_is_masked_by_default() {
        assert_eq!(token_text("secret-value", false), "••••••••");
        assert_eq!(token_text("secret-value", true), "secret-value");
    }

    #[test]
    fn settings_window_is_landscape_three_by_two() {
        let (w, h) = SETTINGS_SIZE;
        assert_eq!((w, h), (900.0, 600.0));
        assert!((w / h - 1.5).abs() < 0.01, "default size should be 3:2");
        let (min_w, min_h) = SETTINGS_MIN;
        assert!(min_w > min_h, "min size stays landscape");
        assert!(min_w >= 780.0 && min_h >= 520.0);
    }

    #[test]
    fn calendar_subtitle_uses_the_week_strip_count() {
        let settings = AppSettings::default();
        assert_eq!(
            WidgetModule::Calendar.subtitle(&settings).as_ref(),
            "7 days"
        );
    }

    #[test]
    fn observe_subtitle_counts_pinned_metrics() {
        assert_eq!(observe_subtitle(0).as_ref(), "Prometheus");
        assert_eq!(observe_subtitle(1).as_ref(), "1 metric");
        assert_eq!(observe_subtitle(5).as_ref(), "5 metrics");
    }

    #[test]
    fn module_list_is_our_widgets_only() {
        let names: Vec<_> = WidgetModule::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(
            names,
            [
                "Calendar",
                "Music",
                "Files",
                "Notes",
                "Observe",
                "Timers",
                "Reminders",
                "Speed Test",
                "Agents",
            ]
        );
        assert!(!names.iter().any(|n| n.contains("Mirror")
            || n.contains("Shortcuts")
            || n.contains("Tencent")
            || n.contains("License")));
    }

    #[test]
    fn nav_enums_round_trip() {
        assert_eq!(SettingsCategory::from_u8(1), SettingsCategory::Nook);
        assert_eq!(NookTab::from_u8(1), NookTab::CustomWidgets);
        assert_eq!(WidgetModule::from_u8(0), WidgetModule::Calendar);
        assert_eq!(WidgetModule::from_u8(99), WidgetModule::Calendar);
    }
}
