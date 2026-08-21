//! Settings window — separate from the island overlay.

use super::ui::label;
use crate::theme;
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, CursorStyle, FocusHandle, FontWeight,
    KeyDownEvent, MouseButton, SharedString, Window,
};
use nook_core::settings::AppSettings;
use std::sync::atomic::{AtomicU8, Ordering};

/// Last pane the user had open. Survives closing the window so reopen does
/// not dump them back on Appearance.
static LAST_PANE: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum SettingsPane {
    Appearance = 0,
    Widgets = 1,
    Metrics = 2,
}

impl SettingsPane {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Widgets,
            2 => Self::Metrics,
            _ => Self::Appearance,
        }
    }
}

pub(super) struct SettingsView {
    pane: SettingsPane,
    url_focus: FocusHandle,
    token_focus: FocusHandle,
    query_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    query_draft: String,
    catalog: Vec<String>,
    catalog_error: Option<String>,
    catalog_loading: bool,
}

impl SettingsView {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let settings = nook_core::settings::get_app_settings();
        Self {
            pane: SettingsPane::from_u8(LAST_PANE.load(Ordering::Relaxed)),
            url_focus: cx.focus_handle(),
            token_focus: cx.focus_handle(),
            query_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            token_draft: settings.observe.metrics_token,
            query_draft: String::new(),
            catalog: Vec::new(),
            catalog_error: None,
            catalog_loading: false,
        }
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
        s.observe.prometheus_url = draft;
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
        let mut s = nook_core::settings::get_app_settings();
        tweak(&mut s);
        nook_core::settings::update_app_settings(s);
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
        div()
            .id("settings-root")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::WINDOW_BG)
            .text_color(theme::LABEL)
            .p_5()
            .gap_4()
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(px(theme::TITLE_2.size))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("openNook"),
            )
            .child(self.pane_switch(cx))
            .child(match self.pane {
                SettingsPane::Appearance => settings_group(vec![
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
                ])
                .into_any_element(),
                SettingsPane::Widgets => div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(settings_group(vec![
                        toggle_row("Show Now Playing", settings.show_media, cx, |s| {
                            s.show_media = !s.show_media;
                        })
                        .into_any_element(),
                        toggle_row("Show Calendar", settings.show_calendar, cx, |s| {
                            s.show_calendar = !s.show_calendar;
                        })
                        .into_any_element(),
                        toggle_row("Show Reminders", settings.show_reminders, cx, |s| {
                            s.show_reminders = !s.show_reminders;
                        })
                        .into_any_element(),
                        toggle_row("Show Agents", settings.show_agents, cx, |s| {
                            s.show_agents = !s.show_agents;
                        })
                        .into_any_element(),
                        toggle_row("Show Observe", settings.show_observe, cx, |s| {
                            s.show_observe = !s.show_observe;
                        })
                        .into_any_element(),
                    ]))
                    .child(
                        div()
                            .id("edit-notes")
                            .h(px(theme::HIT_MIN + 8.0))
                            .px_3()
                            .rounded(px(theme::CONTROL_RADIUS))
                            .bg(theme::GROUPED_BG)
                            .flex()
                            .items_center()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(theme::FILL_SECONDARY))
                            .active(|s| s.opacity(0.85))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|_, _, _, _| {
                                    if let Err(err) = nook_core::notes::open_notes_editor() {
                                        log::warn!("open notes: {err}");
                                    }
                                }),
                            )
                            .child(label("Edit Notes…", theme::BODY, true)),
                    )
                    .into_any_element(),
                SettingsPane::Metrics => self
                    .render_observe_settings(
                        &settings,
                        url_focused,
                        token_focused,
                        query_focused,
                        cx,
                    )
                    .into_any_element(),
            })
            .child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .text_color(theme::SECONDARY_LABEL)
                    .child(
                        "Hover the notch to expand. Settings and Quit live in the menu bar extra.",
                    ),
            )
    }
}

impl SettingsView {
    fn pane_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_1()
            .p(px(2.))
            .rounded(px(theme::CONTROL_RADIUS))
            .bg(theme::FILL)
            .child(pane_chip(
                "Appearance",
                self.pane == SettingsPane::Appearance,
                SettingsPane::Appearance,
                cx,
            ))
            .child(pane_chip(
                "Widgets",
                self.pane == SettingsPane::Widgets,
                SettingsPane::Widgets,
                cx,
            ))
            .child(pane_chip(
                "Metrics",
                self.pane == SettingsPane::Metrics,
                SettingsPane::Metrics,
                cx,
            ))
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
                let chart_caption = metric.chart.caption();
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
            "Bearer token for /admin/metrics"
        } else {
            self.token_draft.as_str()
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
                "Pins default to a line chart. Samples are kept for 24 h. Pick a range to view.",
                theme::SUBHEADLINE,
                false,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(window_chip(
                        "30 min",
                        settings.observe.window == nook_core::observe::ObserveWindow::ThirtyMinutes,
                        cx,
                        |_, _, cx| {
                            SettingsView::persist_observe(|s| {
                                nook_core::observe::set_window(
                                    &mut s.observe,
                                    nook_core::observe::ObserveWindow::ThirtyMinutes,
                                );
                            });
                            cx.notify();
                        },
                    ))
                    .child(window_chip(
                        "5 h",
                        settings.observe.window == nook_core::observe::ObserveWindow::FiveHours,
                        cx,
                        |_, _, cx| {
                            SettingsView::persist_observe(|s| {
                                nook_core::observe::set_window(
                                    &mut s.observe,
                                    nook_core::observe::ObserveWindow::FiveHours,
                                );
                            });
                            cx.notify();
                        },
                    ))
                    .child(window_chip(
                        "24 h",
                        settings.observe.window == nook_core::observe::ObserveWindow::OneDay,
                        cx,
                        |_, _, cx| {
                            SettingsView::persist_observe(|s| {
                                nook_core::observe::set_window(
                                    &mut s.observe,
                                    nook_core::observe::ObserveWindow::OneDay,
                                );
                            });
                            cx.notify();
                        },
                    )),
            )
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
                            .child(SharedString::from(token_text.to_string())),
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
                    })),
            )
            .child(label("Pinned metrics", theme::CALLOUT, true))
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

fn pane_chip(
    caption: &'static str,
    selected: bool,
    pane: SettingsPane,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    window_chip(caption, selected, cx, move |this, _, cx| {
        this.pane = pane;
        LAST_PANE.store(pane as u8, Ordering::Relaxed);
        cx.notify();
    })
}

fn settings_chip(
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    window_chip(caption, false, cx, on_click)
}

fn window_chip(
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
        .child(
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
                .child(div().size(px(20.)).rounded_full().bg(rgb(0xffffff))),
        )
}
