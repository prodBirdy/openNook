//! File-processing job list + format / target-size pickers.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane};
use crate::island::{CompactMode, Island};
use crate::theme;
use gpui::{
    div, prelude::*, px, relative, rgba, Context, CursorStyle, FontWeight, MouseButton,
    SharedString,
};
use nook_core::files::{self, FileCapabilities};
use nook_core::process::{self, JobKind, JobParams, JobSnapshot, JobStatus};
use nook_core::settings::PdfPreset;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TARGET_SIZES: [(&str, u64); 3] = [
    ("8 MB", 8 * 1024 * 1024),
    ("25 MB", 25 * 1024 * 1024),
    ("50 MB", 50 * 1024 * 1024),
];

pub(crate) fn process_card(island: &Island, cx: &mut Context<Island>) -> impl IntoElement {
    let jobs = &island.process_jobs;
    let focus = island
        .process_focus
        .as_deref()
        .and_then(|p| island.files.iter().find(|f| f.path == p));
    let caps = focus
        .map(|f| files::item_capabilities(f, island.settings.file_actions.ffmpeg_enabled()))
        .unwrap_or_default();

    let mut pane = nook_pane("nook-process").w_full().gap(px(6.));
    if jobs.is_empty() && focus.is_none() {
        return pane.child(nook_empty("files", "Drop a file, then Convert"));
    }
    if let Some(file) = focus {
        pane = pane.child(picker_row(file.path.clone(), &file.mime_type, caps, cx));
    }
    if jobs.is_empty() {
        return pane;
    }
    let mut list = div().flex().flex_col().w_full().gap(px(4.));
    for job in jobs.iter().rev().take(6) {
        list = list.child(job_row(job, cx));
    }
    pane.child(list)
}

fn picker_row(
    path: String,
    mime: &str,
    caps: FileCapabilities,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut row = div().flex().flex_col().gap(px(4.)).w_full();
    if caps.convert {
        let formats: &[&str] = if mime.starts_with("image") {
            &["png", "jpeg", "webp"]
        } else if mime.starts_with("audio") {
            &["m4a", "wav"]
        } else {
            &["mp4", "mov"]
        };
        row = row.child(chip_row(
            "fmt",
            formats,
            cx,
            path.clone(),
            |_p, fmt| JobParams::Convert {
                format: fmt.to_string(),
            },
        ));
    }
    if caps.target_size {
        let mut sizes = div().flex().gap(px(4.));
        for (label, bytes) in TARGET_SIZES {
            let p = path.clone();
            sizes = sizes.child(chip(label, cx, move |this, cx| {
                this.begin_process_job(
                    p.clone(),
                    JobParams::TargetSize { bytes },
                    cx,
                );
            }));
        }
        row = row.child(sizes);
    }
    if caps.compress_pdf {
        row = row.child(chip_row(
            "pdf",
            &["screen", "print", "raster"],
            cx,
            path.clone(),
            |_, preset| JobParams::CompressPdf {
                preset: match preset {
                    "print" => PdfPreset::Print,
                    "raster" => PdfPreset::Raster,
                    _ => PdfPreset::Screen,
                },
            },
        ));
    }
    row
}

fn chip_row(
    id: &'static str,
    labels: &[&'static str],
    cx: &mut Context<Island>,
    path: String,
    param: impl Fn(String, &'static str) -> JobParams + 'static + Copy,
) -> impl IntoElement {
    let mut row = div().id(id).flex().gap(px(4.));
    for label in labels {
        let p = path.clone();
        let l = *label;
        row = row.child(chip(l, cx, move |this, cx| {
            this.begin_process_job(p.clone(), param(p.clone(), l), cx);
        }));
    }
    row
}

fn chip(
    caption: &'static str,
    cx: &mut Context<Island>,
    on_click: impl Fn(&mut Island, &mut Context<Island>) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("proc-chip-{caption}")))
        .h(px(22.))
        .px(px(8.))
        .rounded(px(6.))
        .bg(rgba(0xffffff1A))
        .hover(|s| s.bg(rgba(0xffffff33)))
        .cursor(CursorStyle::PointingHand)
        .flex()
        .items_center()
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::LABEL)
                .child(caption),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                on_click(this, cx);
            }),
        )
}

fn job_row(job: &JobSnapshot, cx: &mut Context<Island>) -> impl IntoElement {
    let name = job
        .input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| job.kind.label().into());
    let t = job.progress as f32 / 100.0;
    let live = job.status.is_live();
    let id = job.id;
    div()
        .id(SharedString::from(format!("job-{}", job.id)))
        .flex()
        .flex_col()
        .gap(px(3.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(label(name, theme::CALLOUT, true))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(label(status_label(job), theme::CALLOUT, false))
                        .when(live, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("job-x-{id}")))
                                    .cursor(CursorStyle::PointingHand)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |_, _, _, cx| {
                                            cx.stop_propagation();
                                            process::cancel(id);
                                            cx.notify();
                                        }),
                                    )
                                    .child(lucide_color("x", 12.0, theme::TERTIARY_LABEL)),
                            )
                        }),
                ),
        )
        .child(
            div()
                .w_full()
                .h(px(3.))
                .rounded_full()
                .overflow_hidden()
                .bg(rgba(0xffffff26))
                .child(
                    div()
                        .h_full()
                        .w(relative(t.clamp(0.0, 1.0)))
                        .rounded_full()
                        .bg(match job.status {
                            JobStatus::Failed => theme::DESTRUCTIVE,
                            JobStatus::Done => theme::accent(),
                            _ => theme::LABEL,
                        }),
                ),
        )
}

fn status_label(job: &JobSnapshot) -> String {
    match job.status {
        JobStatus::Queued => "Queued".into(),
        JobStatus::Running => format!("{}%", job.progress),
        JobStatus::Done => "Done".into(),
        JobStatus::Failed => "Failed".into(),
        JobStatus::Cancelled => "Cancelled".into(),
    }
}

pub(crate) fn compact_left(island: &Island) -> gpui::AnyElement {
    use super::timers::timer_ring;
    let progress = island
        .process_jobs
        .iter()
        .rev()
        .find(|j| j.status.is_live())
        .map(|j| j.progress as f32 / 100.0)
        .unwrap_or(1.0);
    let done = island.process_hud.is_some() && !process::any_live();
    div()
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(timer_ring(
            progress.clamp(0.0, 1.0),
            24.0,
            10.0,
            3.0,
            if done {
                theme::accent()
            } else {
                gpui::rgb(0xffffff)
            },
            rgba(0xffffff40),
        ))
        .into_any_element()
}

pub(crate) fn compact_right(island: &Island) -> gpui::AnyElement {
    if let Some((msg, _)) = island.process_hud.as_ref() {
        return label(msg.clone(), theme::BODY, true).into_any_element();
    }
    let job = island
        .process_jobs
        .iter()
        .rev()
        .find(|j| j.status.is_live());
    let text = job
        .map(|j| format!("{}%", j.progress))
        .unwrap_or_else(|| "…".into());
    label(text, theme::BODY, true).into_any_element()
}

impl Island {
    pub(crate) fn begin_process_job(
        &mut self,
        path: String,
        params: JobParams,
        cx: &mut Context<Self>,
    ) {
        if !self.settings.file_actions.enabled {
            self.show_process_hud("File actions off".into(), cx);
            return;
        }
        self.process_focus = Some(path.clone());
        self.preferred = Some(CompactMode::Process);
        let kind = params.kind();
        let snap = process::enqueue(kind, PathBuf::from(&path));
        self.process_jobs = process::snapshot_jobs();
        cx.notify();

        let settings = self.settings.file_actions.clone();
        let id = snap.id;
        cx.spawn(async move |this, cx| {
            let slot: Arc<Mutex<Option<Result<process::JobResult, String>>>> =
                Arc::new(Mutex::new(None));
            let done = Arc::new(AtomicBool::new(false));
            let slot2 = slot.clone();
            let done2 = done.clone();
            let settings2 = settings.clone();
            cx.background_executor()
                .spawn(async move {
                    let result = process::run_job(id, params, &settings2);
                    if let Ok(mut guard) = slot2.lock() {
                        *guard = Some(result);
                    }
                    done2.store(true, Ordering::SeqCst);
                })
                .detach();
            loop {
                let keep = this
                    .update(cx, |this, cx| {
                        this.process_jobs = process::snapshot_jobs();
                        cx.notify();
                        !done.load(Ordering::SeqCst)
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
            }
            this.update(cx, |this, cx| {
                this.process_jobs = process::snapshot_jobs();
                if let Ok(mut guard) = slot.lock() {
                    match guard.take() {
                        Some(Ok(result)) => {
                            if result.insert_tray {
                                if let Some(out) = result.output.as_ref() {
                                    if let Ok(item) = process::insert_output(out) {
                                        if !this.files.iter().any(|f| f.path == item.path) {
                                            this.files.push(item);
                                            let _ = files::save_file_tray(this.files.clone());
                                        }
                                    }
                                }
                            }
                            this.show_process_hud(result.hud, cx);
                        }
                        Some(Err(err)) => this.show_process_hud(err, cx),
                        None => {}
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn show_process_hud(&mut self, message: String, cx: &mut Context<Self>) {
        self.process_hud = Some((message, Instant::now()));
        self.preferred = Some(CompactMode::Process);
        nook_core::haptics::trigger(Some(nook_core::haptics::HapticConfig {
            pattern: nook_core::haptics::HapticPattern::Success,
            intensity: 0.7,
        }));
        cx.notify();
    }

    pub(crate) fn begin_kind_job(&mut self, path: String, kind: JobKind, cx: &mut Context<Self>) {
        let settings = &self.settings.file_actions;
        let mime = files::mime_from_path(&path);
        let params = match kind {
            JobKind::Convert => JobParams::Convert {
                format: if mime == "image" {
                    settings.default_image_format.clone()
                } else if mime == "audio" {
                    "m4a".into()
                } else {
                    settings.default_video_format.clone()
                },
            },
            JobKind::TargetSize => JobParams::TargetSize {
                bytes: 8 * 1024 * 1024,
            },
            JobKind::CompressPdf => JobParams::CompressPdf {
                preset: settings.pdf_preset,
            },
            JobKind::RemoveBg => JobParams::RemoveBg,
            JobKind::Ocr => JobParams::Ocr,
        };
        self.begin_process_job(path, params, cx);
    }
}

pub(crate) fn process_hud_expired(at: Instant) -> bool {
    at.elapsed() >= Duration::from_millis(2500)
}
