//! Voice-memo Nook pane: dated list and a ringed record/stop control.

use crate::island::ui::{label, nook_empty, nook_pane, scroll_body, timer_text};
use crate::island::{CompactMode, Island};
use crate::theme;
use chrono::{Local, TimeZone, Utc};
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgba, Context, CursorStyle,
    FontWeight, MouseButton, MouseDownEvent, SharedString, Window,
};
use nook_core::recorder::{self, RecordingItem};

const RING: f32 = 48.0;
const DOT: f32 = 30.0;
const STOP: f32 = 16.0;
const STOP_RADIUS: f32 = 5.0;

pub(crate) fn recorder_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let recording = island.recording;
    let elapsed = island.recording_elapsed_secs();
    let clock = recorder::format_duration_ms(elapsed as i64 * 1000);
    let hint = island
        .recorder_error
        .clone()
        .or_else(recorder::permission_hint);
    let transcript = island.live_transcript.trim();

    let list = if island.recordings.is_empty() && !recording {
        nook_empty("mic", hint.unwrap_or_else(|| "Tap to record".into())).into_any_element()
    } else {
        let mut rows = div().flex().flex_col().w_full().pb(px(28.));
        if recording {
            rows = rows.child(live_row(&clock, transcript));
        }
        for item in &island.recordings {
            rows = rows.child(recording_row(
                item,
                island.playing_recording == Some(item.id),
                cx,
            ));
        }
        div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .child(scroll_body("rec-list", rows))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(28.))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(rgba(0x00000000), 0.0),
                        linear_color_stop(rgba(0x000000CC), 1.0),
                    )),
            )
            .into_any_element()
    };

    nook_pane("nook-recorder").w_full().child(list).child(
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(6.))
            .when(recording, |d| {
                d.child(
                    timer_text(clock.clone(), theme::BODY)
                        .text_size(px(theme::CALLOUT.size))
                        .pb(px(6.)),
                )
            })
            .child(record_btn(recording, cx)),
    )
}

fn live_row(clock: &str, transcript: &str) -> impl IntoElement {
    let subtitle = if transcript.is_empty() {
        "Recording".to_string()
    } else {
        transcript.chars().take(48).collect()
    };
    div()
        .id("rec-live")
        .w_full()
        .flex()
        .items_center()
        .py(px(8.))
        .border_b_1()
        .border_color(rgba(0xFFFFFF14))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(theme::TITLE_3.size))
                        .line_height(px(theme::TITLE_3.leading))
                        .font_weight(theme::TITLE_3.emphasized)
                        .text_color(theme::LABEL)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(clock.to_string())),
                )
                .child(
                    label(subtitle, theme::SUBHEADLINE, false)
                        .w_full()
                        .min_w(px(0.)),
                ),
        )
}

fn recording_row(
    item: &RecordingItem,
    playing: bool,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let id = item.id;
    let (title, date) = memo_stamp(item.created_at);
    let dur = recorder::format_duration_ms(item.duration_ms);
    div()
        .id(SharedString::from(format!("rec-{id}")))
        .w_full()
        .flex()
        .items_center()
        .gap(px(10.))
        .py(px(8.))
        .border_b_1()
        .border_color(rgba(0xFFFFFF14))
        .rounded(px(6.))
        .when(playing, |d| d.bg(theme::FILL_TERTIARY))
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.toggle_playback(id, window, cx);
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(theme::TITLE_3.size))
                        .line_height(px(theme::TITLE_3.leading))
                        .font_weight(theme::TITLE_3.emphasized)
                        .text_color(theme::LABEL)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(title)),
                )
                .child(label(date, theme::SUBHEADLINE, false)),
        )
        .child(
            timer_text(dur, theme::CALLOUT)
                .text_color(theme::SECONDARY_LABEL)
                .font_weight(FontWeight::NORMAL)
                .flex_shrink_0(),
        )
        .child(
            div()
                .id(SharedString::from(format!("rec-del-{id}")))
                .size(px(theme::HIT_MIN))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .hover(|s| s.bg(theme::FILL))
                .active(|s| s.opacity(0.8))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.delete_recording(id, cx);
                    }),
                )
                .child(crate::icons::lucide_color("x", 12.0, theme::TERTIARY_LABEL)),
        )
}

fn record_btn(recording: bool, cx: &mut Context<Island>) -> impl IntoElement {
    let inner = if recording {
        div()
            .size(px(STOP))
            .rounded(px(STOP_RADIUS))
            .bg(theme::DESTRUCTIVE)
    } else {
        div().size(px(DOT)).rounded_full().bg(theme::DESTRUCTIVE)
    };
    div()
        .id("rec-toggle")
        .size(px(RING))
        .rounded_full()
        .border_2()
        .border_color(rgba(0xffffff66))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.8))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if this.recording {
                    this.stop_recording(cx);
                } else {
                    this.begin_recording(cx);
                }
            }),
        )
        .child(inner)
}

pub(crate) fn memo_stamp(created_at: i64) -> (String, String) {
    if created_at <= 0 {
        return ("Recording".into(), String::new());
    }
    let Some(utc) = Utc.timestamp_opt(created_at, 0).single() else {
        return ("Recording".into(), String::new());
    };
    let local = utc.with_timezone(&Local);
    let title = local.format("%a %H:%M").to_string();
    let date = local.format("%e %b %Y").to_string();
    (title, date.trim().to_string())
}

impl Island {
    pub(crate) fn recording_elapsed_secs(&self) -> u32 {
        self.recording_started
            .map(|t| t.elapsed().as_secs() as u32)
            .unwrap_or(0)
    }

    pub(crate) fn begin_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording {
            return;
        }
        self.recorder_error = None;
        self.live_transcript.clear();
        let transcribe = self.settings.recorder_transcribe;
        self.preferred = Some(CompactMode::Recording);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = nook_core::runtime()
                .spawn(recorder::start(transcribe))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.recording = recorder::is_live();
                        this.recording_started = recorder::snapshot().started;
                        this.preferred = Some(CompactMode::Recording);
                    }
                    Err(err) => {
                        this.recorder_error = Some(err);
                        this.recording = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        match recorder::stop() {
            Ok(item) => {
                if let Some(item) = item {
                    self.recordings.insert(0, item);
                } else {
                    self.recordings = recorder::list();
                }
            }
            Err(err) => self.recorder_error = Some(err),
        }
        self.recording = false;
        self.recording_started = None;
        self.recorder_level = 0.0;
        if self.preferred == Some(CompactMode::Recording) {
            self.preferred = None;
        }
        cx.notify();
    }

    pub(crate) fn toggle_playback(
        &mut self,
        id: i64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.playing_recording == Some(id) {
            recorder::stop_playback();
            self.playing_recording = None;
        } else if let Err(err) = recorder::play(id) {
            self.recorder_error = Some(err);
            self.playing_recording = None;
        } else {
            self.playing_recording = Some(id);
        }
        cx.notify();
    }

    pub(crate) fn delete_recording(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Err(err) = recorder::delete(id) {
            self.recorder_error = Some(err);
        } else {
            self.recordings.retain(|r| r.id != id);
            if self.playing_recording == Some(id) {
                self.playing_recording = None;
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::memo_stamp;

    #[test]
    fn memo_stamp_falls_back_when_date_is_invalid() {
        let (title, date) = memo_stamp(-1);
        assert_eq!(title, "Recording");
        assert!(date.is_empty());
        let (title, date) = memo_stamp(0);
        assert_eq!(title, "Recording");
        assert!(date.is_empty());
    }

    #[test]
    fn memo_stamp_splits_weekday_time_and_date() {
        let (title, date) = memo_stamp(1_720_540_800);
        assert!(title.contains(':'), "{title}");
        assert!(date.chars().any(|c| c.is_ascii_digit()), "{date}");
        assert!(
            date.contains("2024") || date.contains("2025") || date.contains("2023"),
            "{date}"
        );
    }
}
