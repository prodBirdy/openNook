//! Settings window — macOS split view: sidebar + grouped inset lists.

use super::ui::label;
use crate::icons::lucide_color;
use crate::theme;
use gpui::{
    canvas, div, img, prelude::*, px, rgb, rgba, AnyElement, Bounds, Context, CursorStyle, Entity,
    FocusHandle, FontWeight, Image, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, Rgba, SharedString, Subscription, Window,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use nook_core::settings::{AppSettings, IslandSwatch, WidgetModule, ISLAND_SWATCHES};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Default settings window. Sidebar + grouped pane, landscape so the widget
/// list and island preview sit side by side with the navigation.
pub(super) const SETTINGS_SIZE: (f32, f32) = (780.0, 560.0);
pub(super) const SETTINGS_MIN: (f32, f32) = (680.0, 480.0);

const SIDEBAR_W: f32 = 180.0;
/// Room for traffic lights on a transparent titlebar.
const TITLEBAR_INSET: f32 = 52.0;
const GROUP_PAD: f32 = 12.0;
const ROW_H: f32 = 36.0;

/// Last surface the user had open. Survives closing the window.
static LAST_CATEGORY: AtomicU8 = AtomicU8::new(SettingsCategory::Widgets as u8);
static LAST_MODULE: AtomicU8 = AtomicU8::new(WidgetModule::Calendar as u8);

fn token_text(token: &str, revealed: bool) -> String {
    if revealed {
        token.to_string()
    } else {
        "••••••••".into()
    }
}

fn hairline() -> Rgba {
    rgba(0xffffff14)
}

fn desktop_wallpaper_image() -> Option<std::sync::Arc<Image>> {
    static WALLPAPER: std::sync::OnceLock<Option<std::sync::Arc<Image>>> =
        std::sync::OnceLock::new();
    WALLPAPER
        .get_or_init(|| {
            crate::platform::desktop_wallpaper_png()
                .map(|png| std::sync::Arc::new(Image::from_bytes(gpui::ImageFormat::Png, png)))
        })
        .clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SettingsCategory {
    General = 0,
    Widgets = 1,
}

impl SettingsCategory {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Widgets,
            _ => Self::General,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Widgets => "Widgets",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => "settings",
            Self::Widgets => "layout-grid",
        }
    }
}

trait WidgetModuleExt {
    fn name(self) -> &'static str;
    fn icon(self) -> &'static str;
    fn subtitle(self, settings: &AppSettings) -> SharedString;
    fn enabled(self, settings: &AppSettings) -> bool;
    fn set_enabled(self, settings: &mut AppSettings);
    fn preview_label(self) -> &'static str;
}

#[derive(Clone, Copy)]
struct WidgetDrag(WidgetModule);

impl gpui::Render for WidgetDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(220.))
            .h(px(ROW_H))
            .px(px(10.))
            .rounded(px(8.))
            .bg(theme::GROUPED_BG)
            .shadow_md()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(lucide_color("grip-vertical", 14.0, theme::TERTIARY_LABEL))
            .child(lucide_color(self.0.icon(), 14.0, theme::LABEL))
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::LABEL)
                    .child(self.0.name()),
            )
    }
}

impl WidgetModuleExt for WidgetModule {
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
            Self::Mirror => "Mirror",
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
            Self::Mirror => "webcam",
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
            Self::Mirror => "Camera".into(),
        }
    }

    fn enabled(self, settings: &AppSettings) -> bool {
        settings.is_enabled(self)
    }

    fn set_enabled(self, settings: &mut AppSettings) {
        settings.toggle_enabled(self);
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
            Self::Mirror => "Mirror",
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

fn create_width_slider(
    module: WidgetModule,
    settings: &AppSettings,
    cx: &mut Context<SettingsView>,
) -> (Entity<SliderState>, Subscription) {
    let min = module.min_cells();
    let max = settings.max_cells_for(module).max(min);
    let value = settings.cells_for(module).clamp(min, max);
    let slider = cx.new(|_| {
        SliderState::new()
            .min(min as f32)
            .max(max as f32)
            .step(1.0)
            .default_value(value as f32)
    });
    let subscription = cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
        let SliderEvent::Change(value) = event;
        nook_core::settings::tweak_app_settings(|settings| {
            settings.set_cells(module, value.start().round() as u8)
        });
        cx.notify();
    });
    (slider, subscription)
}

pub(super) struct SettingsView {
    category: SettingsCategory,
    module: WidgetModule,
    url_focus: FocusHandle,
    token_focus: FocusHandle,
    query_focus: FocusHandle,
    shell_focus: FocusHandle,
    timeout_focus: FocusHandle,
    url_draft: String,
    token_draft: String,
    token_revealed: bool,
    query_draft: String,
    shell_draft: String,
    timeout_draft: String,
    catalog: Vec<String>,
    catalog_error: Option<String>,
    catalog_loading: bool,
    width_slider: Entity<SliderState>,
    width_slider_config: (WidgetModule, u8, u8),
    _width_slider_subscription: Subscription,
    placement_drag: bool,
    placement_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
}

impl SettingsView {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        let settings = nook_core::settings::get_app_settings();
        let module = WidgetModule::from_u8(LAST_MODULE.load(Ordering::Relaxed));
        let min = module.min_cells();
        let max = settings.max_cells_for(module).max(min);
        let (width_slider, width_slider_subscription) = create_width_slider(module, &settings, cx);
        Self {
            category: SettingsCategory::from_u8(LAST_CATEGORY.load(Ordering::Relaxed)),
            module,
            url_focus: cx.focus_handle(),
            token_focus: cx.focus_handle(),
            query_focus: cx.focus_handle(),
            shell_focus: cx.focus_handle(),
            timeout_focus: cx.focus_handle(),
            url_draft: settings.observe.prometheus_url,
            token_draft: settings.observe.metrics_token,
            token_revealed: false,
            query_draft: String::new(),
            shell_draft: settings.terminal_shell.clone(),
            timeout_draft: settings.terminal_timeout_secs.to_string(),
            catalog: Vec::new(),
            catalog_error: None,
            catalog_loading: false,
            width_slider,
            width_slider_config: (module, min, max),
            _width_slider_subscription: width_slider_subscription,
            placement_drag: false,
            placement_bounds: Rc::new(RefCell::new(None)),
        }
    }

    fn apply_placement(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(bounds) = *self.placement_bounds.borrow() else {
            return;
        };
        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        if width < 8.0 || height < 8.0 {
            return;
        }
        const PILL_W: f32 = 52.0;
        const PILL_H: f32 = 14.0;
        let left = (x - origin_x - PILL_W * 0.5).clamp(0.0, (width - PILL_W).max(0.0));
        let top = (y - origin_y - PILL_H * 0.5).clamp(0.0, (height - PILL_H).max(0.0));
        nook_core::settings::tweak_app_settings(|s| {
            s.set_island_origin(left, top, width, height, PILL_W);
        });
        cx.notify();
    }

    fn persist_nav(&self) {
        LAST_CATEGORY.store(self.category as u8, Ordering::Relaxed);
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

        div()
            .id("settings-root")
            .size_full()
            .flex()
            .bg(theme::SETTINGS_GLASS)
            .text_color(theme::LABEL)
            .when(self.placement_drag, |d| {
                d.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    this.apply_placement(event.position.x.into(), event.position.y.into(), cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        this.placement_drag = false;
                        cx.notify();
                    }),
                )
            })
            .child(self.sidebar(cx))
            .child(div().w(px(1.)).h_full().bg(hairline()))
            .child(if self.category == SettingsCategory::Widgets {
                self.render_widgets(&settings, url_focused, token_focused, query_focused, cx)
                    .into_any_element()
            } else {
                self.render_general(&settings, window, cx).into_any_element()
            })
    }
}

impl SettingsView {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-sidebar")
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme::SETTINGS_WELL)
            .pt(px(TITLEBAR_INSET))
            .px(px(10.))
            .pb(px(16.))
            .gap(px(2.))
            .child(self.sidebar_item(SettingsCategory::General, cx))
            .child(self.sidebar_item(SettingsCategory::Widgets, cx))
    }

    fn sidebar_item(&self, category: SettingsCategory, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.category == category;
        let icon_color = if selected {
            theme::accent()
        } else {
            theme::SECONDARY_LABEL
        };
        div()
            .id(SharedString::from(format!("sidebar-{}", category.title())))
            .h(px(28.))
            .px(px(8.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(selected, |d| d.bg(theme::FILL_SECONDARY))
            .hover(|s| if selected { s } else { s.bg(theme::FILL) })
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.category = category;
                    this.persist_nav();
                    cx.notify();
                }),
            )
            .child(lucide_color(category.icon(), 15.0, icon_color))
            .child(
                div()
                    .text_size(px(theme::BODY.size))
                    .line_height(px(theme::BODY.leading))
                    .font_weight(if selected {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if selected {
                        theme::LABEL
                    } else {
                        theme::SECONDARY_LABEL
                    })
                    .child(category.title()),
            )
    }

    fn pane(title: &'static str, body: impl IntoElement) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("pane-{title}")))
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .pt(px(TITLEBAR_INSET))
            .child(
                div()
                    .px(px(20.))
                    .pb(px(12.))
                    .flex_shrink_0()
                    .text_size(px(theme::TITLE_2.size))
                    .line_height(px(theme::TITLE_2.leading))
                    .font_weight(theme::TITLE_2.emphasized)
                    .text_color(theme::LABEL)
                    .child(title),
            )
            .child(
                div()
                    .id(SharedString::from(format!("pane-body-{title}")))
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(20.))
                    .pb(px(24.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(body),
            )
    }

    fn render_general(
        &self,
        settings: &AppSettings,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Self::pane(
            "General",
            div()
                .id("general-pane")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Appearance",
                    settings_group(vec![
                        toggle_row("Liquid Glass island", settings.liquid_glass_mode, cx, |s| {
                            s.liquid_glass_mode = !s.liquid_glass_mode
                        })
                        .into_any_element(),
                        self.color_row(settings, cx).into_any_element(),
                    ]),
                    Some("A custom color replaces the default black island."),
                ))
                .child(section(
                    "Position",
                    settings_group(vec![
                        self.placement_canvas(settings, cx).into_any_element(),
                        self.alignment_row(settings, cx).into_any_element(),
                        self.reset_row(cx).into_any_element(),
                    ]),
                    Some("Drag the island on the preview. Option-drag it on the display to place it."),
                ))
                .child(section(
                    "Behavior",
                    settings_group(vec![
                        toggle_row(
                            "Show island without a notch",
                            settings.non_notch_mode,
                            cx,
                            |s| {
                                s.non_notch_mode = !s.non_notch_mode;
                            },
                        )
                        .into_any_element(),
                        toggle_row(
                            "Hide when an app fills the display",
                            settings.hide_when_maximized,
                            cx,
                            |s| {
                                s.hide_when_maximized = !s.hide_when_maximized;
                            },
                        )
                        .into_any_element(),
                    ]),
                    Some("Hover the island to expand. Settings and Quit are in the menu bar extra."),
                ))
                .child(section(
                    "Termi-Notch",
                    settings_group(vec![
                        toggle_row(
                            "Enable one-shot shell",
                            settings.terminal_enabled,
                            cx,
                            |s| s.terminal_enabled = !s.terminal_enabled,
                        )
                        .into_any_element(),
                        field_row(
                            "term-shell",
                            "Shell",
                            if self.shell_draft.is_empty() {
                                "$SHELL"
                            } else {
                                self.shell_draft.as_str()
                            },
                            self.shell_draft.is_empty(),
                            self.shell_focus.is_focused(window),
                            &self.shell_focus,
                            cx,
                            |this, event, cx| {
                                if SettingsView::apply_key(&mut this.shell_draft, event, cx) {
                                    nook_core::settings::tweak_app_settings(|s| {
                                        s.terminal_shell = this.shell_draft.trim().to_string();
                                    });
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        field_row(
                            "term-timeout",
                            "Timeout",
                            &self.timeout_draft,
                            false,
                            self.timeout_focus.is_focused(window),
                            &self.timeout_focus,
                            cx,
                            |this, event, cx| {
                                if SettingsView::apply_key(&mut this.timeout_draft, event, cx) {
                                    this.timeout_draft
                                        .retain(|ch| ch.is_ascii_digit());
                                    if let Ok(secs) = this.timeout_draft.parse::<u32>() {
                                        nook_core::settings::tweak_app_settings(|s| {
                                            s.terminal_timeout_secs = secs.clamp(1, 600);
                                        });
                                    }
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        toggle_row(
                            "Remember command history",
                            settings.terminal_history,
                            cx,
                            |s| s.terminal_history = !s.terminal_history,
                        )
                        .into_any_element(),
                    ]),
                    Some("Typed in the island only. opennook://, the CLI, and Finder Services never run commands. Default off."),
                )),
        )
    }

    fn color_row(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let mut swatches = div().flex().items_center().gap(px(6.));
        for swatch in ISLAND_SWATCHES {
            swatches = swatches.child(color_swatch(swatch, settings.island_color, cx));
        }
        settings_row("island-color")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(label("Island color", theme::BODY, true))
                    .child(label(
                        settings.island_swatch_name(),
                        theme::SUBHEADLINE,
                        false,
                    )),
            )
            .child(swatches)
    }

    fn placement_canvas(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        const PILL_W: f32 = 52.0;
        const PILL_H: f32 = 14.0;
        const FALLBACK_W: f32 = 640.0;
        const FALLBACK_H: f32 = 128.0;
        let (cw, ch) = self
            .placement_bounds
            .borrow()
            .map(|b| {
                let w: f32 = b.size.width.into();
                let h: f32 = b.size.height.into();
                (w.max(1.0), h.max(1.0))
            })
            .unwrap_or((FALLBACK_W, FALLBACK_H));
        let (pill_x, pill_y) = settings.island_origin(cw, ch, PILL_W, PILL_H);
        let fill = theme::island_fill(settings.island_color);
        let bounds_cell = self.placement_bounds.clone();

        div()
            .id("placement-canvas")
            .w_full()
            .h(px(FALLBACK_H))
            .overflow_hidden()
            .relative()
            .cursor(if self.placement_drag {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .child(div().absolute().inset_0().bg(rgb(0x1b1b1f)))
            .child(
                canvas(
                    move |bounds, _, _| {
                        *bounds_cell.borrow_mut() = Some(bounds);
                        bounds
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .absolute()
                    .left(px(pill_x))
                    .top(px(pill_y))
                    .w(px(PILL_W))
                    .h(px(PILL_H))
                    .rounded_full()
                    .bg(fill),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.placement_drag = true;
                    this.apply_placement(event.position.x.into(), event.position.y.into(), cx);
                }),
            )
    }

    fn alignment_row(&self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let x = settings.island_x;
        let left = (x - 0.0).abs() < 0.02;
        let center = (x - 0.5).abs() < 0.02;
        let right = (x - 1.0).abs() < 0.02;
        settings_row("island-align")
            .child(label("Alignment", theme::BODY, true))
            .child(
                segmented_group()
                    .child(segment("Left", left, cx, |_, _, cx| {
                        nook_core::settings::tweak_app_settings(|s| s.island_x = 0.0);
                        cx.notify();
                    }))
                    .child(segment("Center", center, cx, |_, _, cx| {
                        nook_core::settings::tweak_app_settings(|s| s.island_x = 0.5);
                        cx.notify();
                    }))
                    .child(segment("Right", right, cx, |_, _, cx| {
                        nook_core::settings::tweak_app_settings(|s| s.island_x = 1.0);
                        cx.notify();
                    })),
            )
    }

    fn reset_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        settings_row("island-reset")
            .child(label("Restore default position", theme::BODY, true))
            .child(push_button("island-reset-btn", "Reset", cx, |_, _, cx| {
                nook_core::settings::tweak_app_settings(|s| s.reset_island_position());
                cx.notify();
            }))
    }

    fn render_widgets(
        &mut self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let remaining = settings.remaining_cells();
        let total = AppSettings::TOTAL_CELLS;
        let preview_footer = if remaining == 0 {
            format!("All {total} cells are in use.")
        } else if remaining == 1 {
            "1 cell remaining.".to_string()
        } else {
            format!("{remaining} of {total} cells remaining.")
        };

        let mut list = Vec::new();
        for module in settings.ordered_widgets() {
            list.push(self.widget_row(module, settings, cx).into_any_element());
        }

        Self::pane(
            "Widgets",
            div()
                .id("custom-widgets")
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(section(
                    "Preview",
                    settings_group(vec![self.island_preview(settings).into_any_element()]),
                    Some(preview_footer),
                ))
                .child(section(
                    "Widgets",
                    settings_group(list),
                    Some("Drag to change the order on the island."),
                ))
                .child(self.module_section(
                    settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    cx,
                )),
        )
    }

    fn widget_row(
        &self,
        module: WidgetModule,
        settings: &AppSettings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.module == module;
        let on = module.enabled(settings);
        let cells = settings.cells_for(module);
        let caption = if module.occupies_nook_cells() {
            if cells == 1 {
                "1 cell".to_string()
            } else {
                format!("{cells} cells")
            }
        } else {
            "Tray tab".to_string()
        };
        div()
            .id(SharedString::from(format!("mod-{}", module.name())))
            .px(px(GROUP_PAD))
            .min_h(px(ROW_H + 4.0))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(selected, |d| d.bg(theme::FILL))
            .hover(|s| {
                if selected {
                    s
                } else {
                    s.bg(theme::FILL_TERTIARY)
                }
            })
            .drag_over::<WidgetDrag>(move |style, drag, _, _| {
                if drag.0 == module {
                    style
                } else {
                    style.bg(theme::FILL_SECONDARY)
                }
            })
            .can_drop(move |value, _, _| {
                value
                    .downcast_ref::<WidgetDrag>()
                    .is_some_and(|drag| drag.0 != module)
            })
            .on_drop(cx.listener(move |_, drag: &WidgetDrag, _, cx| {
                nook_core::settings::tweak_app_settings(|settings| {
                    settings.move_widget_to(drag.0, module)
                });
                cx.notify();
            }))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.module = module;
                    this.persist_nav();
                    cx.notify();
                }),
            )
            .on_drag(WidgetDrag(module), |drag, _, _, cx| cx.new(|_| *drag))
            .child(lucide_color("grip-vertical", 14.0, theme::TERTIARY_LABEL))
            .child(lucide_color(module.icon(), 15.0, theme::LABEL))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(label(module.name(), theme::BODY, true))
                    .child(label(caption, theme::SUBHEADLINE, false)),
            )
            .child(module_toggle(on, module, cx))
    }

    fn island_preview(&self, settings: &AppSettings) -> impl IntoElement {
        let enabled: Vec<_> = settings
            .ordered_widgets()
            .into_iter()
            .filter(|module| module.occupies_nook_cells() && module.enabled(settings))
            .collect();
        let used = enabled
            .iter()
            .map(|module| settings.cells_for(*module) as f32)
            .sum::<f32>()
            .max(1.0);
        let inner = 480.0_f32;
        let wallpaper = desktop_wallpaper_image();
        let mut chips = div().flex().items_center().gap(px(6.)).px(px(8.));
        if enabled.is_empty() {
            chips = chips.child(
                div()
                    .text_size(px(theme::SUBHEADLINE.size))
                    .text_color(theme::SECONDARY_LABEL)
                    .child("No widgets enabled"),
            );
        } else {
            for module in enabled {
                let cells = settings.cells_for(module) as f32;
                let width = ((cells / used) * inner).max(56.0);
                chips = chips.child(self.preview_chip(module, width, module == self.module));
            }
        }

        div()
            .id("island-preview")
            .w_full()
            .h(px(132.))
            .overflow_hidden()
            .relative()
            .bg(theme::SETTINGS_GLASS)
            .when_some(wallpaper, |preview, wallpaper| {
                preview.child(
                    img(wallpaper)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .h(px(64.))
                            .px(px(8.))
                            .rounded(px(20.))
                            .bg(rgb(0x000000))
                            .flex()
                            .items_center()
                            .child(chips),
                    ),
            )
    }

    fn preview_chip(&self, module: WidgetModule, width: f32, selected: bool) -> impl IntoElement {
        div()
            .h(px(48.))
            .w(px(width))
            .rounded(px(12.))
            .bg(if selected {
                rgb(0x2a2a2a)
            } else {
                rgb(0x1a1a1a)
            })
            .when(selected, |d| d.border_1().border_color(rgba(0xffffff33)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .child(lucide_color(module.icon(), 14.0, theme::LABEL))
            .child(
                div()
                    .text_size(px(theme::FOOTNOTE.size))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::LABEL)
                    .child(module.preview_label()),
            )
    }

    fn width_slider(&mut self, settings: &AppSettings, cx: &mut Context<Self>) -> impl IntoElement {
        let module = self.module;
        let value = settings.cells_for(module);
        let min = module.min_cells();
        let max = settings.max_cells_for(module).max(min);
        let config = (module, min, max);
        if self.width_slider_config != config {
            let (slider, subscription) = create_width_slider(module, settings, cx);
            self.width_slider = slider;
            self._width_slider_subscription = subscription;
            self.width_slider_config = config;
        }
        let enabled = module.occupies_nook_cells();

        settings_row("width-row")
            .opacity(if enabled { 1.0 } else { 0.45 })
            .child(label("Width", theme::BODY, true))
            .child(
                div()
                    .id("width-slider")
                    .flex_1()
                    .h(px(theme::HIT_MIN))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        Slider::new(&self.width_slider)
                            .disabled(!enabled)
                            .bg(theme::accent())
                            .text_color(rgb(0xffffff)),
                    ),
            )
            .child(
                div().w(px(28.)).flex().justify_end().child(
                    div()
                        .text_size(px(theme::BODY.size))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::SECONDARY_LABEL)
                        .child(value.to_string()),
                ),
            )
    }

    fn module_section(
        &mut self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self.module.name();
        let enabled = self.module.enabled(settings);
        let mut rows = vec![self.width_slider(settings, cx).into_any_element()];
        match self.module {
            WidgetModule::Calendar => {
                rows.push(
                    action_row(
                        "calendar-app",
                        "Calendar app",
                        "Open Calendar",
                        cx,
                        |_, _, _| {
                            crate::platform::open_calendar();
                        },
                    )
                    .into_any_element(),
                );
            }
            WidgetModule::Notes => {
                rows.push(
                    action_row("notes-edit", "Notes", "Edit Notes…", cx, |_, _, _| {
                        if let Err(err) = nook_core::notes::open_notes_editor() {
                            log::warn!("open notes: {err}");
                        }
                    })
                    .into_any_element(),
                );
            }
            WidgetModule::Observe => {
                rows.push(
                    action_row(
                        "observe-browse",
                        "Metric names",
                        "Browse Metrics",
                        cx,
                        |this, _, cx| {
                            this.browse_metrics(cx);
                        },
                    )
                    .into_any_element(),
                );
            }
            _ => {}
        }

        div()
            .id("module-controls")
            .flex()
            .flex_col()
            .gap(px(16.))
            .opacity(if enabled { 1.0 } else { 0.55 })
            .child(section(
                name,
                settings_group(rows),
                Some(module_blurb(self.module)),
            ))
            .when(self.module == WidgetModule::Observe, |d| {
                d.child(self.render_observe_settings(
                    settings,
                    url_focused,
                    token_focused,
                    query_focused,
                    cx,
                ))
            })
    }

    fn render_observe_settings(
        &self,
        settings: &AppSettings,
        url_focused: bool,
        token_focused: bool,
        query_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut pinned_rows = Vec::new();
        if settings.observe.metrics.is_empty() {
            pinned_rows.push(
                settings_row("pin-empty")
                    .child(label("No pinned metrics", theme::BODY, false))
                    .into_any_element(),
            );
        } else {
            for metric in &settings.observe.metrics {
                let query = metric.query.clone();
                let chart_query = metric.query.clone();
                let alert_query = metric.query.clone();
                let chart_caption = metric.chart.caption();
                let alert_caption = metric.alert_caption();
                pinned_rows.push(
                    div()
                        .id(SharedString::from(format!("pin-{}", metric.query)))
                        .px(px(GROUP_PAD))
                        .py(px(6.))
                        .min_h(px(ROW_H))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(1.))
                                .child(label(metric.label.clone(), theme::BODY, true))
                                .child(label(metric.query.clone(), theme::SUBHEADLINE, false)),
                        )
                        .child(push_button(
                            SharedString::from(format!("chart-{}", metric.query)),
                            chart_caption,
                            cx,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                SettingsView::persist_observe(|s| {
                                    nook_core::observe::cycle_metric_chart(
                                        &mut s.observe,
                                        &chart_query,
                                    );
                                });
                                cx.notify();
                            },
                        ))
                        .child(push_button(
                            SharedString::from(format!("alert-{}", metric.query)),
                            alert_caption,
                            cx,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                SettingsView::persist_observe(|s| {
                                    nook_core::observe::cycle_metric_alert(
                                        &mut s.observe,
                                        &alert_query,
                                    );
                                });
                                cx.notify();
                            },
                        ))
                        .child(push_button(
                            SharedString::from(format!("unpin-{}", metric.query)),
                            "Remove",
                            cx,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                SettingsView::persist_observe(|s| {
                                    nook_core::observe::unpin_metric(&mut s.observe, &query);
                                });
                                cx.notify();
                            },
                        ))
                        .into_any_element(),
                );
            }
        }

        let mut catalog_rows = Vec::new();
        if self.catalog_loading {
            catalog_rows.push(
                settings_row("cat-loading")
                    .child(label("Loading metric names…", theme::BODY, false))
                    .into_any_element(),
            );
        } else if let Some(err) = &self.catalog_error {
            catalog_rows.push(
                settings_row("cat-error")
                    .child(caption_text(err.clone()))
                    .into_any_element(),
            );
        }
        for name in self.catalog.iter().take(12) {
            let query = name.clone();
            let label_text = name.clone();
            catalog_rows.push(
                div()
                    .id(SharedString::from(format!("cat-{name}")))
                    .px(px(GROUP_PAD))
                    .min_h(px(ROW_H))
                    .flex()
                    .items_center()
                    .hover(|s| s.bg(theme::FILL_TERTIARY))
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
                    .child(label(name.clone(), theme::BODY, true))
                    .into_any_element(),
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

        let mut range = segmented_group();
        for option in nook_core::observe::ObserveRange::all() {
            let selected = settings.observe.range == option;
            range = range.child(segment(option.label(), selected, cx, move |_, _, cx| {
                SettingsView::persist_observe(|s| {
                    nook_core::observe::set_range(&mut s.observe, option);
                });
                cx.notify();
            }));
        }

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(section(
                "Source",
                settings_group(vec![
                    field_row(
                        "prom-url",
                        "URL",
                        url_text,
                        url_placeholder,
                        url_focused,
                        &self.url_focus,
                        cx,
                        |this, event, cx| {
                            let persist = SettingsView::apply_key(&mut this.url_draft, event, cx)
                                || event.keystroke.key == "enter";
                            if persist {
                                this.persist_url();
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                    field_row(
                        "prom-token",
                        "Token",
                        &token_text,
                        token_placeholder,
                        token_focused,
                        &self.token_focus,
                        cx,
                        |this, event, cx| {
                            let persist = SettingsView::apply_key(&mut this.token_draft, event, cx)
                                || event.keystroke.key == "enter";
                            if persist {
                                this.persist_token();
                                cx.notify();
                            }
                        },
                    )
                    .into_any_element(),
                    settings_row("observe-actions")
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(push_button("paste-url", "Paste URL", cx, |this, _, cx| {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        this.url_draft = text.trim().to_string();
                                        this.persist_url();
                                        cx.notify();
                                    }
                                }))
                                .child(push_button(
                                    "toggle-token",
                                    if self.token_revealed {
                                        "Hide Token"
                                    } else {
                                        "Show Token"
                                    },
                                    cx,
                                    |this, _, cx| {
                                        this.token_revealed = !this.token_revealed;
                                        cx.notify();
                                    },
                                )),
                        )
                        .into_any_element(),
                ]),
                Some("Defaults to the warmUP API. Prometheus still works if you point at a Prom host."),
            ))
            .child(section(
                "Lookback",
                settings_group(vec![settings_row("observe-range")
                    .child(label("Range", theme::BODY, true))
                    .child(range)
                    .into_any_element()]),
                Some("Pins default to a line chart. Prometheus uses query_range; warmUP keeps 24 h of samples."),
            ))
            .child(section(
                "Pinned Metrics",
                settings_group(pinned_rows),
                Some("The compact island shows Observe only while a threshold you set is firing."),
            ))
            .child(section(
                "Pin Query",
                settings_group(vec![
                    field_row(
                        "prom-query",
                        "Query",
                        query_text,
                        query_placeholder,
                        query_focused,
                        &self.query_focus,
                        cx,
                        |this, event, cx| {
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
                        },
                    )
                    .into_any_element(),
                    settings_row("pin-query-action")
                        .child(div().flex_1())
                        .child(push_button("pin-query", "Pin Query", cx, |this, _, cx| {
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
                        .into_any_element(),
                ]),
                None::<SharedString>,
            ))
            .when(!catalog_rows.is_empty(), |d| {
                d.child(section(
                    "Catalog",
                    settings_group(catalog_rows),
                    Some("Choose a name to pin it."),
                ))
            })
    }
}

fn module_blurb(module: WidgetModule) -> SharedString {
    match module {
        WidgetModule::Calendar => {
            "Week strip is today ± 3 days. EventKit includes every calendar the system allows — there is no per-calendar filter yet.".into()
        }
        WidgetModule::Notes => "Scratchpad on the island. Edit here or in the expanded card.".into(),
        WidgetModule::Observe => {
            "Pinned metrics on the compact island and the expanded card.".into()
        }
        WidgetModule::Music => {
            "Now Playing from MediaRemote on macOS, with an AppleScript fallback.".into()
        }
        WidgetModule::Files => "Drop zone and tray live on the Tray tab of the expanded island.".into(),
        WidgetModule::Timers => "Countdown presets and a compact ring while a timer is running.".into(),
        WidgetModule::Reminders => "Incomplete reminders from EventKit, same store as Calendar.".into(),
        WidgetModule::Speed => "Cloudflare (then OVH) download probe. Runs from the island card.".into(),
        WidgetModule::Agents => {
            "Working coding-agent sessions on the compact face and expanded card.".into()
        }
        WidgetModule::Mirror => "A live camera preview that opens when you click the Mirror card.".into(),
    }
}

fn section(
    header: impl Into<SharedString>,
    group: impl IntoElement,
    footer: Option<impl Into<SharedString>>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(section_header(header))
        .child(group)
        .when_some(footer, |d, text| {
            d.child(div().px(px(4.)).child(caption_text(text)))
        })
}

fn section_header(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px(px(4.))
        .text_size(px(theme::BODY.size))
        .line_height(px(theme::BODY.leading))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme::SECONDARY_LABEL)
        .child(text.into())
}

fn caption_text(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(16.))
        .text_color(theme::SECONDARY_LABEL)
        .child(text.into())
}

fn settings_group(rows: Vec<AnyElement>) -> impl IntoElement {
    let mut group = div()
        .flex()
        .flex_col()
        .rounded(px(theme::INNER_RADIUS))
        .bg(theme::GROUPED_BG)
        .overflow_hidden();
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            group = group.child(div().h(px(1.)).ml(px(GROUP_PAD)).bg(hairline()));
        }
        group = group.child(row);
    }
    group
}

fn settings_row(id: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(GROUP_PAD))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .min_h(px(ROW_H))
}

fn action_row(
    id: &'static str,
    title: &'static str,
    caption: &'static str,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    settings_row(id)
        .child(label(title, theme::BODY, true))
        .child(push_button(
            SharedString::from(format!("{id}-btn")),
            caption,
            cx,
            on_click,
        ))
}

fn toggle_row(
    label_text: &'static str,
    on: bool,
    cx: &mut Context<SettingsView>,
    tweak: impl Fn(&mut AppSettings) + 'static,
) -> impl IntoElement {
    settings_row(label_text)
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
        .min_h(px(theme::HIT_MIN))
        .flex()
        .items_center()
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

fn color_swatch(
    swatch: IslandSwatch,
    selected: Option<u32>,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let on = swatch.rgb == selected;
    let fill = theme::island_fill(swatch.rgb);
    let name = swatch.name;
    div()
        .id(SharedString::from(format!("swatch-{name}")))
        .size(px(theme::HIT_MIN))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                cx.stop_propagation();
                nook_core::settings::tweak_app_settings(|s| s.island_color = swatch.rgb);
                cx.notify();
            }),
        )
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(fill)
                .when(on, |d| d.border_2().border_color(theme::LABEL))
                .when(!on, |d| d.border_1().border_color(rgba(0xffffff33))),
        )
}

fn toggle_knob(on: bool) -> impl IntoElement {
    div()
        .w(px(38.))
        .h(px(22.))
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
        .child(div().size(px(18.)).rounded_full().bg(rgb(0xffffff)))
}

fn segmented_group() -> gpui::Div {
    div()
        .h(px(theme::HIT_MIN))
        .p(px(2.))
        .rounded(px(6.))
        .bg(theme::FILL)
        .flex()
        .items_center()
}

fn segment(
    caption: &'static str,
    selected: bool,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("seg-{caption}")))
        .h(px(24.))
        .px(px(10.))
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .when(selected, |d| d.bg(theme::FILL_SECONDARY))
        .hover(|s| {
            if selected {
                s
            } else {
                s.bg(theme::FILL_TERTIARY)
            }
        })
        .active(|s| s.opacity(0.85))
        .cursor(CursorStyle::PointingHand)
        .child(
            div()
                .text_size(px(theme::CALLOUT.size))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    theme::LABEL
                } else {
                    theme::SECONDARY_LABEL
                })
                .child(caption),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
}

fn push_button(
    id: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    cx: &mut Context<SettingsView>,
    on_click: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let caption = caption.into();
    div()
        .id(id.into())
        .h(px(theme::HIT_MIN))
        .px(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .bg(theme::FILL)
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

fn field_row(
    id: &'static str,
    title: &'static str,
    value: &str,
    placeholder: bool,
    focused: bool,
    focus: &FocusHandle,
    cx: &mut Context<SettingsView>,
    on_key: impl Fn(&mut SettingsView, &KeyDownEvent, &mut Context<SettingsView>) + 'static,
) -> impl IntoElement {
    let focus = focus.clone();
    settings_row(id)
        .child(label(title, theme::BODY, true).w(px(56.)))
        .child(
            div()
                .id(SharedString::from(format!("{id}-field")))
                .track_focus(&focus)
                .flex_1()
                .min_w(px(0.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(5.))
                .bg(theme::FILL)
                .when(focused, |d| d.border_1().border_color(theme::accent()))
                .flex()
                .items_center()
                .cursor(CursorStyle::IBeam)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&focus);
                        cx.notify();
                    }),
                )
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                    on_key(this, event, cx);
                }))
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(if placeholder {
                            theme::TERTIARY_LABEL
                        } else {
                            theme::LABEL
                        })
                        .text_size(px(theme::BODY.size))
                        .child(SharedString::from(value.to_string())),
                ),
        )
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
    fn settings_window_is_landscape() {
        let (w, h) = SETTINGS_SIZE;
        assert_eq!((w, h), (780.0, 560.0));
        assert!(w > h, "default size stays landscape");
        let (min_w, min_h) = SETTINGS_MIN;
        assert!(min_w > min_h, "min size stays landscape");
        assert!(min_w >= 680.0 && min_h >= 480.0);
        assert!(w > SIDEBAR_W + 400.0, "pane has room beside the sidebar");
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
                "Mirror",
            ]
        );
        assert!(!names
            .iter()
            .any(|n| n.contains("Shortcuts") || n.contains("Tencent") || n.contains("License")));
    }

    #[test]
    fn nav_enums_round_trip() {
        assert_eq!(SettingsCategory::from_u8(1), SettingsCategory::Widgets);
        assert_eq!(SettingsCategory::from_u8(0), SettingsCategory::General);
        assert_eq!(SettingsCategory::from_u8(99), SettingsCategory::General);
        assert_eq!(WidgetModule::from_u8(0), WidgetModule::Calendar);
        assert_eq!(WidgetModule::from_u8(99), WidgetModule::Calendar);
    }

    #[test]
    fn remaining_cells_saturate_at_zero() {
        let settings = AppSettings::default();
        assert_eq!(AppSettings::TOTAL_CELLS, 11);
        assert!(settings.used_cells() >= AppSettings::TOTAL_CELLS);
        assert_eq!(settings.remaining_cells(), 0);
    }
}
