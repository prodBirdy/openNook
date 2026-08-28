//! Per-app volume mixer card.

use crate::icons::lucide_color;
use crate::island::media::app_icon_image;
use crate::island::ui::{nook_empty, nook_pane, scroll_body};
use crate::island::Island;
use crate::theme;
use gpui::{
    div, img, prelude::*, px, relative, rgb, rgba, AnyElement, Context, CursorStyle, FontWeight,
    MouseButton, MouseDownEvent, ObjectFit, SharedString,
};
use nook_core::mixer::{self, CaptureStatus, MixerApp, TCC_PREPROMPT, UNITY};

pub(crate) fn mixer_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    nook_pane("nook-mixer").child(
        div()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .flex()
            .flex_col()
            .child(mixer_header(island))
            .child(if island.mixer_prompt.is_some() {
                preprompt(cx).into_any_element()
            } else if island.mixer_apps.is_empty() {
                empty_state(island).into_any_element()
            } else {
                app_list(island, cx).into_any_element()
            }),
    )
}

fn mixer_header(island: &Island) -> impl IntoElement {
    let status = mixer::capture_status();
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .pb(px(6.))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child("Mixer"),
        )
        .child(
            div()
                .text_size(px(10.))
                .line_height(px(12.))
                .text_color(match status {
                    CaptureStatus::Denied => theme::DESTRUCTIVE,
                    CaptureStatus::Active => theme::accent(),
                    _ => theme::TERTIARY_LABEL,
                })
                .child(header_caption(island, status)),
        )
}

fn header_caption(island: &Island, status: CaptureStatus) -> SharedString {
    if !island.mixer_apps.is_empty() {
        let n = island.mixer_apps.len();
        return if n == 1 {
            "1 app".into()
        } else {
            format!("{n} apps").into()
        };
    }
    mixer::capture_status_label(status).into()
}

fn empty_state(island: &Island) -> impl IntoElement {
    let status = mixer::capture_status();
    match status {
        CaptureStatus::Denied => nook_empty("triangle-alert", "Audio recording denied"),
        CaptureStatus::Unavailable => nook_empty("music", "Needs macOS 14.4"),
        _ if island.mixer_apps.is_empty() => nook_empty("music", "No apps playing"),
        _ => nook_empty("music", "No apps playing"),
    }
}

fn preprompt(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("mixer-preprompt")
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(theme::SECONDARY_LABEL)
                .child(TCC_PREPROMPT),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(prompt_btn("mixer-cancel", "Cancel", false, cx, |this, cx| {
                    this.dismiss_mixer_prompt(cx);
                }))
                .child(prompt_btn(
                    "mixer-continue",
                    "Continue",
                    true,
                    cx,
                    |this, cx| {
                        this.accept_mixer_prompt(cx);
                    },
                )),
        )
}

fn prompt_btn(
    id: &'static str,
    title: &'static str,
    accent: bool,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(24.))
        .px(px(10.))
        .rounded(px(8.))
        .bg(if accent {
            theme::accent()
        } else {
            rgba(0xffffff18)
        })
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.88))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(title),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
}

fn app_list(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let mut col = div().flex().flex_col().gap(px(8.)).w_full();
    for app in &island.mixer_apps {
        col = col.child(app_row(app, cx));
    }
    scroll_body("nook-mixer-list", col)
}

fn app_row(app: &MixerApp, cx: &mut Context<Island>) -> impl IntoElement {
    let bundle = SharedString::from(app.bundle_id.clone());
    let mute_bundle = bundle.clone();
    let gain = app.gain;
    let muted = app.muted;
    let level = app.level;
    let t = (gain / mixer::GAIN_MAX).clamp(0.0, 1.0);
    div()
        .id(SharedString::from(format!("mixer-{}", app.bundle_id)))
        .w_full()
        .flex()
        .flex_col()
        .gap(px(3.))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(app_badge(&app.bundle_id, &app.name))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::LABEL)
                        .text_ellipsis()
                        .child(app.name.clone()),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("mixer-mute-{}", app.bundle_id)))
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::PointingHand)
                        .opacity(if muted { 0.55 } else { 0.95 })
                        .hover(|s| s.opacity(1.0))
                        .child(lucide_color(
                            if muted { "volume-x" } else { "volume-2" },
                            14.0,
                            if muted {
                                theme::TERTIARY_LABEL
                            } else {
                                theme::LABEL
                            },
                        ))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.toggle_mixer_mute(mute_bundle.as_ref(), cx);
                            }),
                        ),
                ),
        )
        .child(gain_slider(bundle, t, cx))
        .child(vu_meter(level, gain))
}

fn app_badge(bundle_id: &str, name: &str) -> AnyElement {
    if let Some(image) = app_icon_image(Some(bundle_id), Some(name)) {
        return img(image)
            .size(px(18.))
            .rounded(px(4.))
            .object_fit(ObjectFit::Fill)
            .into_any_element();
    }
    div()
        .size(px(18.))
        .rounded(px(4.))
        .bg(rgba(0xffffff14))
        .flex()
        .items_center()
        .justify_center()
        .child(lucide_color("music", 11.0, theme::SECONDARY_LABEL))
        .into_any_element()
}

fn gain_slider(
    bundle: SharedString,
    t: f32,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    const SEGMENTS: u32 = 24;
    let mut hits = div().absolute().inset_0().flex();
    for i in 0..SEGMENTS {
        let ratio = (i as f32 + 0.5) / SEGMENTS as f32;
        let gain = ratio * mixer::GAIN_MAX;
        let id = bundle.clone();
        hits = hits.child(
            div()
                .id(SharedString::from(format!("mixer-gain-{}-{i}", bundle)))
                .flex_1()
                .h_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.apply_mixer_gain(id.as_ref(), gain, cx);
                    }),
                ),
        );
    }
    div()
        .relative()
        .w_full()
        .h(px(12.))
        .flex()
        .items_center()
        .cursor(CursorStyle::PointingHand)
        .child(
            div()
                .w_full()
                .h(px(3.))
                .rounded_full()
                .bg(rgba(0xffffff26))
                .child(
                    div()
                        .h_full()
                        .w(relative(t))
                        .rounded_full()
                        .bg(if t < (UNITY / mixer::GAIN_MAX) - 0.01 {
                            theme::accent()
                        } else {
                            rgb(0xffffff)
                        }),
                ),
        )
        .child(hits)
}

fn vu_meter(level: f32, gain: f32) -> impl IntoElement {
    let show = !mixer::is_unity(gain) && level > 0.01;
    div()
        .w_full()
        .h(px(2.))
        .rounded_full()
        .bg(rgba(0xffffff14))
        .opacity(if show { 1.0 } else { 0.25 })
        .child(
            div()
                .h_full()
                .w(relative(level.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(theme::accent()),
        )
}

impl Island {
    pub(crate) fn apply_mixer_gain(&mut self, bundle_id: &str, gain: f32, cx: &mut Context<Self>) {
        if !mixer::capture_acknowledged() {
            self.mixer_prompt = Some((bundle_id.to_string(), gain));
            cx.notify();
            return;
        }
        mixer::set_gain(bundle_id, gain);
        mixer::pump();
        self.mixer_apps = mixer::snapshot();
        self.mixer_gen = mixer::generation();
        cx.notify();
    }

    pub(crate) fn toggle_mixer_mute(&mut self, bundle_id: &str, cx: &mut Context<Self>) {
        if !mixer::capture_acknowledged() {
            let current = self
                .mixer_apps
                .iter()
                .find(|app| app.bundle_id == bundle_id)
                .map(|app| app.gain)
                .unwrap_or(UNITY);
            let next = if current <= 0.001 { UNITY } else { 0.0 };
            self.mixer_prompt = Some((bundle_id.to_string(), next));
            cx.notify();
            return;
        }
        mixer::toggle_mute(bundle_id);
        mixer::pump();
        self.mixer_apps = mixer::snapshot();
        self.mixer_gen = mixer::generation();
        cx.notify();
    }

    pub(crate) fn accept_mixer_prompt(&mut self, cx: &mut Context<Self>) {
        let Some((bundle, gain)) = self.mixer_prompt.take() else {
            return;
        };
        mixer::acknowledge_capture();
        mixer::set_gain(&bundle, gain);
        mixer::pump();
        self.mixer_apps = mixer::snapshot();
        self.mixer_gen = mixer::generation();
        cx.notify();
    }

    pub(crate) fn dismiss_mixer_prompt(&mut self, cx: &mut Context<Self>) {
        self.mixer_prompt = None;
        cx.notify();
    }
}
