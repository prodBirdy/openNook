//! Voice-memo Nook pane: record/stop, live transcript, recordings list.

use crate::icons::lucide_color;
use crate::island::ui::{nook_display, nook_empty, nook_icon_btn, nook_pane, nook_row, scroll_body};
use crate::island::{CompactMode, Island};
use crate::theme;
use gpui::{
    div, prelude::*, px, relative, rgba, Context, CursorStyle, FontWeight, MouseButton,
    MouseDownEvent, SharedString, Window,
};
use nook_core::recorder::{self, RecordingItem};

pub(crate) fn recorder_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let recording = island.recording;
    let elapsed = island.recording_elapsed_secs();
    let clock = recorder::format_duration_ms(elapsed as i64 * 1000);
    let transcript = island.live_transcript.trim();
    let hint = island
        .recorder_error
        .clone()
        .or_else(recorder::permission_hint);

    nook_pane("nook-recorder")
        .w_full()
        .child(
            div()
                .flex()
                .items_end()
                .justify_between()
                .gap(px(8.))
                .flex_shrink_0()
                .child(nook_display(if recording { clock } else { island.recordings.len().to_string() }))
                .child(record_btn(recording, cx)),
        )
        .child(level_meter(if recording { island.recorder_level } else { 0.0 }))
        .when(recording && !transcript.is_empty(), |d| {
            d.child(
                div()
                    .w_full()
                    .min_h(px(0.))
                    .flex_1()
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(theme::SECONDARY_LABEL)
                    .child(SharedString::from(transcript.to_string())),
            )
        })
        .when(!recording && island.recordings.is_empty(), |d| {
            d.child(nook_empty(
                "mic",
                hint.unwrap_or_else(|| "Tap to record".into()),
            ))
        })
        .when(!recording && !island.recordings.is_empty(), |d| {
            d.child(scroll_body(
                "rec-list",
                recordings_list(&island.recordings, island.playing_recording, cx),
            ))
        })
}

fn record_btn(recording: bool, cx: &mut Context<Island>) -> impl IntoElement {
    nook_icon_btn(
        if recording { "pause" } else { "mic" },
        "rec-toggle",
        cx,
        |this, _, _, cx| {
            if this.recording {
                this.stop_recording(cx);
            } else {
                this.begin_recording(cx);
            }
        },
    )
}

fn level_meter(level: f32) -> impl IntoElement {
    let t = level.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(3.))
        .flex_shrink_0()
        .rounded_full()
        .overflow_hidden()
        .bg(rgba(0xffffff26))
        .child(
            div()
                .h_full()
                .w(relative(t))
                .rounded_full()
                .bg(if t > 0.02 {
                    theme::DESTRUCTIVE
                } else {
                    rgba(0xffffff33)
                }),
        )
}

fn recordings_list(
    items: &[RecordingItem],
    playing: Option<i64>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut list = div().flex().flex_col();
    for item in items.iter().take(8) {
        list = list.child(recording_row(item, playing == Some(item.id), cx));
    }
    list
}

fn recording_row(item: &RecordingItem, playing: bool, cx: &mut Context<Island>) -> impl IntoElement {
    let id = item.id;
    let title = if item.transcript.trim().is_empty() {
        recorder::format_duration_ms(item.duration_ms)
    } else {
        item.transcript.chars().take(42).collect::<String>()
    };
    let dur = recorder::format_duration_ms(item.duration_ms);
    nook_row(SharedString::from(format!("rec-{id}")))
        .gap(px(6.))
        .child(
            div()
                .id(SharedString::from(format!("rec-play-{id}")))
                .size(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.toggle_playback(id, window, cx);
                    }),
                )
                .child(lucide_color(
                    if playing { "pause" } else { "play" },
                    14.0,
                    theme::LABEL,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::LABEL)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::TERTIARY_LABEL)
                        .child(dur),
                ),
        )
        .child(
            div()
                .id(SharedString::from(format!("rec-del-{id}")))
                .size(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.delete_recording(id, cx);
                    }),
                )
                .child(lucide_color("trash-2", 13.0, theme::TERTIARY_LABEL)),
        )
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

    pub(crate) fn toggle_playback(&mut self, id: i64, _window: &mut Window, cx: &mut Context<Self>) {
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
