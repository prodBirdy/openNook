//! Voice-memo Nook pane: dated list and a ringed record/stop control.
//! Compact face: live waveform, `00:06` clock, ringed stop.

use crate::island::ui::{
    label, nook_empty, nook_pane, open_privacy_pane, scroll_body, text_btn, timer_text,
};
use crate::island::{CompactMode, Island};
use crate::theme;
use chrono::{Local, TimeZone, Utc};
use gpui::{
    div, linear_color_stop, linear_gradient, prelude::*, px, rgba, AnyElement, Context,
    CursorStyle, FontWeight, MouseButton, MouseDownEvent, SharedString, Window,
};
use nook_core::recorder::{self, RecordingItem};
use std::cell::Cell;
use std::time::{Duration, Instant};

thread_local! {
    static PENDING_DELETE: Cell<Option<i64>> = const { Cell::new(None) };
}

const RING: f32 = 48.0;
const DOT: f32 = 30.0;
const STOP: f32 = 16.0;
const STOP_RADIUS: f32 = 5.0;

const WAVE_BARS: usize = 10;
const WAVE_DOTS: usize = 6;
const WAVE_H: f32 = 18.0;
const WAVE_BAR_W: f32 = 3.0;
const WAVE_GAP: f32 = 1.8;
const WAVE_INTERVAL: Duration = Duration::from_millis(50);

const COMPACT_STOP_RING: f32 = 26.0;
const COMPACT_STOP_INNER: f32 = 10.0;
const COMPACT_STOP_RADIUS: f32 = 3.0;

/// Extra width on the compact recording face so the waveform and stop
/// control fit beside the camera housing.
pub(crate) const COMPACT_EXTRA: f32 = 176.0;
pub(crate) const COMPACT_HOVER_EXTRA: f32 = 192.0;

pub(crate) fn recorder_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let recording = island.recording;
    let elapsed = island.recording_elapsed_secs();
    let clock = recorder::format_duration_ms(elapsed as i64 * 1000);
    let hint = island.recorder_error.clone();
    let transcript = island.live_transcript.trim();
    let mic = mic_caption(recording, island.recordings.is_empty());

    let mic_denied = hint
        .as_deref()
        .is_some_and(|h| h.to_lowercase().contains("denied"));
    let list = if island.recordings.is_empty() && !recording {
        if mic_denied {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(nook_empty(
                    "mic-off",
                    hint.unwrap_or_else(|| "Microphone access is off".into()),
                ))
                .child(text_btn("Open Privacy Settings", cx, |_, _, _| {
                    open_privacy_pane("Privacy_Microphone");
                }))
                .into_any_element()
        } else {
            nook_empty("mic", hint.unwrap_or_else(|| "Tap to record".into())).into_any_element()
        }
    } else {
        let mut rows = div().flex().flex_col().w_full().flex_shrink_0().pb(px(28.));
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
            .flex()
            .flex_col()
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

    nook_pane("nook-recorder")
        .w_full()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                if PENDING_DELETE.get().is_some() {
                    PENDING_DELETE.set(None);
                    cx.notify();
                }
            }),
        )
        .child(list)
        .child(
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
                .when_some(mic, |d, (text, err)| {
                    d.child(
                        label(text, theme::FOOTNOTE, false)
                            .text_color(if err {
                                theme::DESTRUCTIVE
                            } else {
                                theme::SECONDARY_LABEL
                            })
                            .pb(px(6.)),
                    )
                    .when(err, |d| {
                        d.child(div().pb(px(6.)).child(text_btn(
                            "Open Privacy Settings",
                            cx,
                            |_, _, _| {
                                open_privacy_pane("Privacy_Microphone");
                            },
                        )))
                    })
                })
                .child(record_btn(recording, cx)),
        )
}

fn mic_caption(recording: bool, empty: bool) -> Option<(String, bool)> {
    let hint = recorder::permission_hint()?;
    let denied = hint.to_lowercase().contains("denied");
    if denied {
        // Empty list already shows the privacy CTA; skip the footer duplicate.
        if empty && !recording {
            return None;
        }
        return Some((hint, true));
    }
    if !recording && empty {
        return Some((
            "Recording uses the microphone. macOS will ask for access.".into(),
            false,
        ));
    }
    None
}

fn live_row(clock: &str, transcript: &str) -> impl IntoElement {
    let subtitle = if transcript.is_empty() {
        "Recording".to_string()
    } else {
        transcript.to_string()
    };
    div()
        .id("rec-live")
        .w_full()
        .flex()
        .flex_shrink_0()
        .items_center()
        .py(px(8.))
        .border_b_1()
        .border_color(theme::FILL_TERTIARY)
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
    let pending = PENDING_DELETE.get() == Some(id);
    div()
        .id(SharedString::from(format!("rec-{id}")))
        .w_full()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(10.))
        .py(px(8.))
        .min_h(px(theme::HIT_MIN))
        .border_b_1()
        .border_color(theme::FILL_TERTIARY)
        .when(playing, |d| d.bg(theme::FILL_TERTIARY))
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .active(|s| s.bg(theme::FILL_SECONDARY))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if PENDING_DELETE.get().is_some() {
                    PENDING_DELETE.set(None);
                    cx.notify();
                }
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
        .child(if pending {
            div()
                .id(SharedString::from(format!("rec-del-{id}")))
                .h(px(theme::HIT_MIN))
                .px_3()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::CONTROL_RADIUS))
                .bg(theme::FILL)
                .hover(|s| s.bg(theme::FILL_SECONDARY))
                .active(|s| s.opacity(0.85))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        PENDING_DELETE.set(None);
                        this.delete_recording(id, cx);
                    }),
                )
                .child(label("Delete", theme::CALLOUT, true).text_color(theme::DESTRUCTIVE))
                .into_any_element()
        } else {
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
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        PENDING_DELETE.set(Some(id));
                        cx.notify();
                    }),
                )
                .child(crate::icons::lucide_color("x", 12.0, theme::TERTIARY_LABEL))
                .into_any_element()
        })
}

pub(crate) fn compact_left(island: &Island) -> AnyElement {
    rec_waveform(island.recorder_wave.iter().copied()).into_any_element()
}

pub(crate) fn compact_right(island: &Island, cx: &mut Context<Island>) -> AnyElement {
    let clock = format_recording_clock(island.recording_elapsed_secs());
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            timer_text(clock, theme::BODY)
                .text_color(theme::DESTRUCTIVE)
                .min_w(px(42.))
                .text_right(),
        )
        .child(compact_stop_btn(cx))
        .into_any_element()
}

fn rec_waveform(levels: impl IntoIterator<Item = f32>) -> impl IntoElement {
    let samples: Vec<f32> = {
        let mut v: Vec<f32> = levels.into_iter().take(WAVE_BARS).collect();
        v.resize(WAVE_BARS, 0.0);
        v
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(WAVE_GAP))
        .h(px(WAVE_H))
        .flex_shrink_0();
    for (i, rms) in samples.iter().enumerate() {
        let scale = wave_bar_height(*rms);
        let fade = 1.0 - (i as f32 / WAVE_BARS as f32) * 0.28;
        row = row.child(
            div()
                .id(SharedString::from(format!("rec-bar-{i}")))
                .w(px(WAVE_BAR_W))
                .h(px((WAVE_H * scale).max(2.0)))
                .rounded_full()
                .bg(theme::DESTRUCTIVE)
                .opacity(fade),
        );
    }
    for i in 0..WAVE_DOTS {
        let t = i as f32 / (WAVE_DOTS.saturating_sub(1).max(1) as f32);
        let size = (2.6 - t * 0.7).max(1.6);
        row = row.child(
            div()
                .id(SharedString::from(format!("rec-dot-{i}")))
                .size(px(size))
                .rounded_full()
                .bg(theme::SECONDARY_LABEL)
                .opacity(0.85 - t * 0.45),
        );
    }
    row
}

/// Speech RMS is typically 0.01–0.2; boost so quiet talk still reads as bars.
pub(crate) fn wave_bar_height(rms: f32) -> f32 {
    let boosted = (rms.max(0.0) * 8.0).min(1.0);
    if boosted < 0.04 {
        0.14
    } else {
        boosted.powf(0.55).clamp(0.22, 1.0)
    }
}

/// Compact recording clock: `00:06`, then `1:00:06` past an hour.
pub(crate) fn format_recording_clock(seconds: u32) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn compact_stop_btn(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("rec-stop")
        .size(px(COMPACT_STOP_RING))
        .rounded_full()
        .border_2()
        .border_color(theme::LABEL)
        .flex_shrink_0()
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
                }
            }),
        )
        .child(
            div()
                .size(px(COMPACT_STOP_INNER))
                .rounded(px(COMPACT_STOP_RADIUS))
                .bg(theme::DESTRUCTIVE),
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
        .border_color(theme::SECONDARY_LABEL)
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
        self.clear_recorder_wave();
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
                        this.clear_recorder_wave();
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
        self.clear_recorder_wave();
        if self.preferred == Some(CompactMode::Recording) {
            self.preferred = None;
        }
        cx.notify();
    }

    pub(crate) fn sample_recorder_wave(&mut self, level: f32, now: Instant) -> bool {
        if now.duration_since(self.recorder_wave_at) < WAVE_INTERVAL {
            return false;
        }
        self.recorder_wave_at = now;
        self.recorder_wave.push_front(level);
        while self.recorder_wave.len() > WAVE_BARS {
            self.recorder_wave.pop_back();
        }
        true
    }

    pub(crate) fn clear_recorder_wave(&mut self) {
        self.recorder_wave.clear();
        self.recorder_level = 0.0;
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
    use super::{format_recording_clock, memo_stamp, wave_bar_height};

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

    #[test]
    fn recording_clock_is_zero_padded_mm_ss() {
        assert_eq!(format_recording_clock(0), "00:00");
        assert_eq!(format_recording_clock(6), "00:06");
        assert_eq!(format_recording_clock(65), "01:05");
        assert_eq!(format_recording_clock(3600), "1:00:00");
        assert_eq!(format_recording_clock(3661), "1:01:01");
    }

    #[test]
    fn wave_height_boosts_quiet_speech_and_floors_silence() {
        assert!(wave_bar_height(0.0) > 0.0);
        assert!(wave_bar_height(0.0) < wave_bar_height(0.08));
        assert!(wave_bar_height(0.08) < wave_bar_height(0.2));
        assert_eq!(wave_bar_height(1.0), 1.0);
    }
}
