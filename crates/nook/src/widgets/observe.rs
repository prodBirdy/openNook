//! Observe Nook pane: warmUP `/admin/metrics` plus Prometheus `/api/v1/query_range`.
//!
//! The 128pt gallery card is two status rows. Clicking it opens the full-width
//! Nightwatch chart (range chips, stacked bars, legend).

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_pane, text_btn};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, deferred, div, point, prelude::*, px, relative, Bounds, Context, CursorStyle,
    MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, Rgba, SharedString,
};
use nook_core::observe::{
    ObserveRange, ObserveSnapshot, ObserveSourceKind, SamplePoint,
};
use nook_core::settings::AppSettings;
use std::cell::RefCell;
use std::rc::Rc;

/// Mockup Nook Row while Observe is expanded (`observe.html` height 240).
pub(crate) const OBSERVE_EXPANDED_BODY: f32 = 240.0;

#[derive(Clone)]
struct StatusSeries {
    group: usize,
    points: Vec<SamplePoint>,
}

#[derive(Clone, Debug, PartialEq)]
struct StackedSample {
    ts: f64,
    values: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
struct ChartBucket {
    start: f64,
    end: f64,
    values: Option<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObserveHover {
    pub bucket: usize,
}

pub(crate) fn observe_card(
    snap: &ObserveSnapshot,
    settings: &AppSettings,
    hover: Option<&ObserveHover>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let _ = hover;
    let url = settings.observe.prometheus_url.trim();
    let rows = observe_event_rows(snap);

    let mut body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .gap(px(8.))
        .justify_center();
    if url.is_empty() {
        body = body.child(
            label("No metrics URL", theme::CALLOUT, false).text_color(theme::TERTIARY_LABEL),
        );
    } else if rows.is_empty() {
        if let Some(err) = &snap.error {
            body = body.child(observe_error_row(err, cx));
        } else {
            body = body.child(
                label("No samples", theme::CALLOUT, false).text_color(theme::TERTIARY_LABEL),
            );
        }
    } else {
        for (ok, title, sub) in rows {
            body = body.child(observe_event_row(ok, title, sub));
        }
    }

    card_shell("nook-observe")
        .w_full()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(theme::FILL_TERTIARY))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.open_observe_expanded(cx);
            }),
        )
        .child(body)
}

pub(crate) fn observe_big_view(
    snap: &ObserveSnapshot,
    settings: &AppSettings,
    hover: Option<&ObserveHover>,
    cx: &mut Context<Island>,
) -> gpui::Stateful<gpui::Div> {
    // Mockup `observe-big` (nNzAv): numeral + unit + range chips, caption,
    // 30 stacked status columns, legend. No other chrome; the × only shows
    // on hover. Empty data keeps the layout with a "—" numeral.
    let range = settings.observe.range;
    let mut chips = div().flex().items_center().gap(px(6.)).flex_shrink_0();
    for option in ObserveRange::all() {
        chips = chips.child(range_chip(option, option == range, cx));
    }

    let statuses = status_series(snap);
    let featured = snap.metrics.first();
    let buckets = status_buckets(
        &stacked_samples(&statuses, range.step_seconds() as f64),
        range,
        BIG_COLUMNS,
    );
    let hovered = hover.and_then(|h| buckets.get(h.bucket));
    let headline = if let Some(bucket) = hovered {
        match bucket.values {
            Some(values) => compact_number(bucket_total(values)),
            None => "—".into(),
        }
    } else if !statuses.is_empty() {
        let latest: f64 = statuses
            .iter()
            .filter_map(|series| series.points.last())
            .map(|pt| clamp_value(pt.value))
            .sum();
        compact_number(latest)
    } else {
        featured
            .and_then(|m| m.last_value())
            .map(compact_number)
            .unwrap_or_else(|| "—".into())
    };
    let metric_label = if let Some(bucket) = hovered {
        format_bucket_range(bucket.start, bucket.end, range)
    } else if !statuses.is_empty() {
        observe_metric_caption(Some("HTTP status"), settings)
    } else {
        observe_metric_caption(featured.map(|m| m.label.as_str()), settings)
    };

    let header = div()
        .flex()
        .items_end()
        .gap(px(16.))
        .flex_shrink_0()
        .w_full()
        .child(observe_headline(headline))
        .child(observe_unit("req/s"))
        .child(div().flex_1())
        .child(chips);

    nook_pane("nook-observe-big")
        .group("nook-observe-big")
        .relative()
        .w_full()
        .bg(theme::ISLAND)
        .rounded(px(theme::ROW_RADIUS))
        .border_1()
        .border_color(theme::FILL_TERTIARY)
        .p(px(16.))
        .gap(px(12.))
        .child(header)
        .child(
            label(metric_label, theme::CALLOUT, false)
                .text_color(theme::secondary_label())
                .w_full()
                .flex_shrink_0(),
        )
        .child(status_chart(&buckets, hover, range, cx))
        .child(status_legend(status_totals(snap), range))
        .child(observe_close_btn(cx))
}

/// Columns in the big status chart (mockup: always 30).
const BIG_COLUMNS: usize = 30;
const COLUMN_GAP: f32 = 3.0;
const COLUMN_RADIUS: f32 = 3.0;
const CLOSE_INSET: f32 = 12.0;
const CLOSE_GLYPH: f32 = 16.0;
const Y_GUTTER: f32 = 28.0;
const X_AXIS_H: f32 = 14.0;
const HOVER_DIM: f32 = 0.55;
const X_TICKS: usize = 4;
const TOOLTIP_W: f32 = 168.0;
/// Connected Observe poll in `Island` is 15s; used when a series has no deltas.
const POLL_SECS: f64 = 15.0;

/// Mockup numeral format: one decimal, trailing `.0` dropped (`2.4k`, `84`).
fn compact_number(value: f64) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (value / 1e9, "G")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "k")
    } else {
        (value, "")
    };
    let text = if suffix.is_empty() && scaled.fract().abs() < 1e-9 {
        format!("{scaled:.0}")
    } else {
        format!("{scaled:.1}")
    };
    let text = text.strip_suffix(".0").map(str::to_string).unwrap_or(text);
    format!("{text}{suffix}")
}

fn clamp_value(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn bucket_total(values: [f64; 3]) -> f64 {
    values.into_iter().map(clamp_value).sum()
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Bucket stacked samples into `count` wall-clock slices of the selected range,
/// ending at the bucket that contains the newest sample. Each bucket is the
/// per-group mean of samples in `[start, end)`; empty buckets stay `None`
/// except a one-poll carry-forward after the first real sample.
fn status_buckets(
    samples: &[StackedSample],
    range: ObserveRange,
    count: usize,
) -> Vec<ChartBucket> {
    if count == 0 {
        return Vec::new();
    }
    let width = (range.seconds() as f64 / count as f64).max(1e-9);
    let newest = samples
        .iter()
        .map(|sample| sample.ts)
        .filter(|ts| ts.is_finite())
        .fold(f64::NAN, f64::max);
    let newest = if newest.is_finite() {
        newest
    } else {
        now_secs()
    };
    let last_start = (newest / width).floor() * width;
    let first_start = last_start - (count as f64 - 1.0) * width;

    let mut sums = vec![[0.0f64; 3]; count];
    let mut hits = vec![0usize; count];
    let mut last_hit_ts = vec![None::<f64>; count];
    for sample in samples {
        if !sample.ts.is_finite() {
            continue;
        }
        let idx_f = (sample.ts - first_start) / width;
        if idx_f < 0.0 {
            continue;
        }
        let idx = idx_f.floor() as i64;
        if idx < 0 || idx >= count as i64 {
            continue;
        }
        let i = idx as usize;
        for (sum, value) in sums[i].iter_mut().zip(sample.values) {
            *sum += clamp_value(value);
        }
        hits[i] += 1;
        last_hit_ts[i] = Some(last_hit_ts[i].map_or(sample.ts, |ts| ts.max(sample.ts)));
    }

    let mut buckets: Vec<ChartBucket> = (0..count)
        .map(|i| {
            let start = first_start + i as f64 * width;
            ChartBucket {
                start,
                end: start + width,
                values: (hits[i] > 0).then(|| sums[i].map(|v| clamp_value(v / hits[i] as f64))),
            }
        })
        .collect();

    let max_gap = carry_gap(samples, range);
    let mut last_real: Option<(f64, [f64; 3])> = None;
    for (bucket, hit_ts) in buckets.iter_mut().zip(last_hit_ts) {
        if let Some(values) = bucket.values {
            last_real = Some((hit_ts.unwrap_or(bucket.start), values));
        } else if let Some((ts, values)) = last_real {
            if bucket.start - ts <= max_gap {
                bucket.values = Some(values);
            }
        }
    }
    buckets
}

fn carry_gap(samples: &[StackedSample], range: ObserveRange) -> f64 {
    let step = (range.step_seconds() as f64).max(1.0);
    let mut times: Vec<f64> = samples
        .iter()
        .map(|sample| sample.ts)
        .filter(|ts| ts.is_finite())
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut deltas: Vec<f64> = times
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|d| *d > 0.0)
        .collect();
    if deltas.is_empty() {
        return step.max(POLL_SECS);
    }
    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    deltas[deltas.len() / 2].max(step)
}

fn bucket_index_at(ratio: f32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let t = ratio.clamp(0.0, 1.0);
    ((t * count as f32).floor() as usize).min(count - 1)
}

fn tooltip_flips_left(index: usize, count: usize) -> bool {
    count > 0 && (index as f32 + 0.5) / count as f32 > 0.6
}

/// 1/2/5 × 10^n ticks from 0 to a nice max ≥ `data_max`. Prefers 4 ticks, then 3.
fn nice_ticks(data_max: f64) -> (f64, Vec<f64>) {
    let data_max = if data_max.is_finite() {
        data_max.max(0.0)
    } else {
        0.0
    };
    if data_max <= 0.0 {
        return (1.0, vec![0.0, 1.0]);
    }
    for n_intervals in [3, 2] {
        let spacing = nice_num(data_max / n_intervals as f64, true);
        if !(spacing > 0.0 && spacing.is_finite()) {
            continue;
        }
        let nice_max = (data_max / spacing).ceil() * spacing;
        let mut ticks = Vec::new();
        let mut v = 0.0;
        for _ in 0..8 {
            ticks.push(v);
            if v >= nice_max - spacing * 0.25 {
                break;
            }
            v += spacing;
        }
        if (3..=4).contains(&ticks.len()) {
            return (nice_max, ticks);
        }
    }
    let nice_max = nice_num(data_max, false).max(data_max);
    (nice_max, vec![0.0, nice_max])
}

fn nice_num(x: f64, round: bool) -> f64 {
    if !x.is_finite() || x <= 0.0 {
        return 1.0;
    }
    let exp = x.log10().floor();
    let base = 10f64.powf(exp);
    let f = x / base;
    let nf = if round {
        if f < 1.5 {
            1.0
        } else if f < 3.0 {
            2.0
        } else if f < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    nf * base
}

fn status_chart(
    buckets: &[ChartBucket],
    hover: Option<&ObserveHover>,
    range: ObserveRange,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let data_max = buckets
        .iter()
        .filter_map(|bucket| bucket.values)
        .map(bucket_total)
        .fold(0.0, f64::max);
    let (nice_max, ticks) = nice_ticks(data_max);
    let hover_idx = hover.map(|h| h.bucket);
    let bounds_cell = Rc::new(RefCell::new(None::<Bounds<Pixels>>));
    let count = buckets.len();

    let mut gutter = div()
        .w(px(Y_GUTTER))
        .flex_shrink_0()
        .relative()
        .h_full();
    if nice_max > 0.0 {
        for tick in &ticks {
            let t = (tick / nice_max) as f32;
            gutter = gutter.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .right(px(4.))
                    .top(relative((1.0 - t).clamp(0.0, 1.0)))
                    .mt(px(-theme::FOOTNOTE.leading * 0.5))
                    .child(
                        label(compact_number(*tick), theme::FOOTNOTE, false)
                            .w_full()
                            .text_right()
                            .text_color(theme::tertiary_label()),
                    ),
            );
        }
    }

    let mut x_row = div()
        .flex_1()
        .min_w(px(0.))
        .relative()
        .h(px(X_AXIS_H));
    if count > 0 {
        let with_seconds = range.seconds() < 3600;
        let last = X_TICKS.saturating_sub(1).max(1);
        for i in 0..X_TICKS {
            let boundary = i * count / last;
            let t = boundary as f32 / count as f32;
            let ts = if boundary >= count {
                buckets.last().map(|b| b.end)
            } else {
                buckets.get(boundary).map(|b| b.start)
            };
            let Some(ts) = ts else {
                continue;
            };
            let text = format_chart_time(ts, with_seconds);
            let mut tick = div().absolute().top(px(0.)).child(
                label(text, theme::FOOTNOTE, false).text_color(theme::tertiary_label()),
            );
            tick = if i == X_TICKS - 1 {
                tick.right(px(0.))
            } else if i == 0 {
                tick.left(px(0.))
            } else {
                tick.left(relative(t.clamp(0.0, 1.0)))
            };
            x_row = x_row.child(tick);
        }
    }

    let painted = buckets.to_vec();
    let tooltip_bucket = hover_idx.and_then(|i| buckets.get(i).cloned());
    let mut plot = div()
        .id("observe-plot")
        .relative()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .cursor(CursorStyle::Crosshair)
        .on_mouse_move({
            let bounds_cell = bounds_cell.clone();
            cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let Some(bounds) = *bounds_cell.borrow() else {
                    return;
                };
                let width: f32 = bounds.size.width.into();
                if width < 1.0 || count == 0 {
                    return;
                }
                let x: f32 = event.position.x.into();
                let origin: f32 = bounds.origin.x.into();
                let t = ((x - origin) / width).clamp(0.0, 1.0);
                let idx = bucket_index_at(t, count);
                if this.observe_hover.as_ref().map(|h| h.bucket) != Some(idx) {
                    this.observe_hover = Some(ObserveHover { bucket: idx });
                    cx.notify();
                }
            })
        })
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if !*hovered && this.observe_hover.is_some() {
                this.observe_hover = None;
                cx.notify();
            }
        }))
        .child(
            canvas(
                {
                    let bounds_cell = bounds_cell.clone();
                    move |bounds, _, _| {
                        *bounds_cell.borrow_mut() = Some(bounds);
                        bounds
                    }
                },
                {
                    let painted = painted.clone();
                    let ticks = ticks.clone();
                    move |bounds, _, window, _| {
                        paint_status_chart(
                            window,
                            bounds,
                            &painted,
                            hover_idx,
                            nice_max,
                            &ticks,
                        );
                    }
                },
            )
            .w_full()
            .h_full(),
        );

    if let Some(bucket) = tooltip_bucket {
        if let Some(idx) = hover_idx {
            plot = plot.child(chart_tooltip(&bucket, idx, count, range));
        }
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .gap(px(2.))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .child(gutter)
                .child(plot),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .w_full()
                .child(div().w(px(Y_GUTTER)).flex_shrink_0())
                .child(x_row),
        )
}

fn chart_tooltip(
    bucket: &ChartBucket,
    index: usize,
    count: usize,
    range: ObserveRange,
) -> impl IntoElement {
    let flip = tooltip_flips_left(index, count);
    let header = format_bucket_range(bucket.start, bucket.end, range);
    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .w_full()
        .child(
            label(header, theme::FOOTNOTE, false)
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_color(theme::secondary_label()),
        );
    if let Some(values) = bucket.values {
        for group in [2usize, 1, 0] {
            let value = clamp_value(values[group]);
            let dim = value <= 0.0;
            let color = if dim {
                with_alpha(status_color(group), status_color(group).a * HOVER_DIM)
            } else {
                status_color(group)
            };
            let name = ["1–3xx", "4xx", "5xx"][group];
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .child(div().size(px(6.)).rounded_full().flex_shrink_0().bg(color))
                            .child(
                                label(name, theme::FOOTNOTE, false).text_color(if dim {
                                    theme::tertiary_label()
                                } else {
                                    theme::LABEL
                                }),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        label(
                            format!("{} req/s", compact_number(value)),
                            theme::FOOTNOTE,
                            false,
                        )
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_color(if dim {
                            theme::tertiary_label()
                        } else {
                            theme::LABEL
                        }),
                    ),
            );
        }
        body = body
            .child(
                div()
                    .w_full()
                    .h(px(1.))
                    .bg(theme::FILL_SECONDARY)
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w_full()
                    .child(
                        label("Total", theme::SUBHEADLINE, true)
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_color(theme::LABEL),
                    )
                    .child(div().flex_1())
                    .child(
                        label(
                            format!("{} req/s", compact_number(bucket_total(values))),
                            theme::SUBHEADLINE,
                            true,
                        )
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_color(theme::LABEL),
                    ),
            );
    } else {
        body = body.child(
            label("No data", theme::FOOTNOTE, false).text_color(theme::tertiary_label()),
        );
    }

    let left_t = if count == 0 {
        0.0
    } else {
        (index as f32 + 1.0) / count as f32
    };
    let right_t = if count == 0 {
        0.0
    } else {
        1.0 - index as f32 / count as f32
    };

    deferred(
        div()
            .absolute()
            .top(px(4.))
            .when(flip, |d| d.right(relative(right_t.clamp(0.0, 1.0))))
            .when(!flip, |d| d.left(relative(left_t.clamp(0.0, 1.0))))
            .when(flip, |d| d.mr(px(4.)))
            .when(!flip, |d| d.ml(px(4.)))
            .w(px(TOOLTIP_W))
            .rounded(px(theme::CONTROL_RADIUS))
            .bg(theme::GROUPED_BG)
            .border_1()
            .border_color(theme::FILL_SECONDARY)
            .shadow_sm()
            .px(px(8.))
            .py(px(6.))
            .child(body),
    )
}

fn paint_status_chart(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    buckets: &[ChartBucket],
    hover: Option<usize>,
    nice_max: f64,
    ticks: &[f64],
) {
    let x0: f32 = bounds.origin.x.into();
    let y0: f32 = bounds.origin.y.into();
    let w: f32 = bounds.size.width.into();
    let h: f32 = bounds.size.height.into();
    if w < 8.0 || h < 8.0 || buckets.is_empty() {
        return;
    }
    let p = |x: f32, y: f32| point(px(x), px(y));
    let count = buckets.len();
    let gaps = COLUMN_GAP * count.saturating_sub(1) as f32;
    let col_w = ((w - gaps) / count as f32).max(0.0);
    let stride = col_w + COLUMN_GAP;
    let y_at = |tick: f64| {
        if nice_max <= 0.0 {
            y0 + h
        } else {
            y0 + h * (1.0 - (tick / nice_max) as f32).clamp(0.0, 1.0)
        }
    };

    for tick in ticks {
        let y = y_at(*tick);
        let mut grid = PathBuilder::stroke(px(1.0));
        grid.move_to(p(x0, y));
        grid.line_to(p(x0 + w, y));
        if let Ok(built) = grid.build() {
            window.paint_path(built, with_alpha(theme::LABEL, 0.06));
        }
    }

    if let Some(idx) = hover {
        if idx < count && col_w > 0.0 {
            let left = x0 + idx as f32 * stride;
            fill_rect(window, &p, left, y0, col_w, h, with_alpha(theme::LABEL, 0.08));
        }
    }

    if nice_max > 0.0 && col_w > 0.0 {
        for (i, bucket) in buckets.iter().enumerate() {
            let Some(values) = bucket.values else {
                continue;
            };
            let total = bucket_total(values);
            if total <= 0.0 {
                continue;
            }
            let dim = hover.is_some_and(|idx| idx != i);
            let left = x0 + i as f32 * stride;
            let topmost = values.iter().rposition(|v| clamp_value(*v) > 0.0);
            let mut y = y0 + h;
            for (group, raw) in values.iter().enumerate() {
                let value = clamp_value(*raw);
                if value <= 0.0 {
                    continue;
                }
                let seg_h = (h as f64 * (value / nice_max)).clamp(0.0, h as f64) as f32;
                let top = (y - seg_h).max(y0);
                let mut color = status_color(group);
                if dim {
                    color.a *= HOVER_DIM;
                }
                let radius = if topmost == Some(group) {
                    COLUMN_RADIUS.min(col_w * 0.5).min(((y - top) * 0.5).max(0.0))
                } else {
                    0.0
                };
                let mut bar = PathBuilder::fill();
                rounded_top_rect(&mut bar, &p, left, top, col_w, y, radius);
                if let Ok(built) = bar.build() {
                    window.paint_path(built, color);
                }
                y = top;
            }
        }
    }

    if let Some(idx) = hover {
        if idx < count && col_w > 0.0 {
            let cx = x0 + idx as f32 * stride + col_w * 0.5;
            let mut guide = PathBuilder::stroke(px(1.0));
            guide.move_to(p(cx, y0));
            guide.line_to(p(cx, y0 + h));
            if let Ok(built) = guide.build() {
                window.paint_path(built, with_alpha(theme::LABEL, 0.45));
            }
        }
    }
}

fn fill_rect(
    window: &mut gpui::Window,
    p: &impl Fn(f32, f32) -> gpui::Point<gpui::Pixels>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Rgba,
) {
    let mut path = PathBuilder::fill();
    path.move_to(p(x, y));
    path.line_to(p(x + w, y));
    path.line_to(p(x + w, y + h));
    path.line_to(p(x, y + h));
    path.close();
    if let Ok(built) = path.build() {
        window.paint_path(built, color);
    }
}

/// Hover-only close, inside the card at 12,12 (mockup has no chrome at rest).
fn observe_close_btn(cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id("observe-close")
        .absolute()
        .top(px(CLOSE_INSET))
        .right(px(CLOSE_INSET))
        .size(px(CLOSE_GLYPH))
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.0)
        .group_hover("nook-observe-big", |s| s.opacity(1.0))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.close_observe_expanded(cx);
            }),
        )
        .child(lucide_color("x", CLOSE_GLYPH, theme::tertiary_label()))
}

fn observe_metric_caption(label: Option<&str>, settings: &AppSettings) -> String {
    let source = match settings.observe.source {
        ObserveSourceKind::Warmup => "warmUP /admin/metrics",
        ObserveSourceKind::Prometheus => "Prometheus",
        ObserveSourceKind::Grafana => "Grafana",
        ObserveSourceKind::Alertmanager => "Alertmanager",
        ObserveSourceKind::FmObserve => "fm-observe",
    };
    match label {
        Some(name) if !name.is_empty() => format!("{name} · {source}"),
        _ => format!("HTTP status · {source}"),
    }
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
}

/// Gallery card: 6px status dot + 12/15 title + 10/13 tertiary subtitle.
fn observe_event_row(ok: bool, title: String, subtitle: String) -> impl IntoElement {
    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .flex_shrink_0()
                .bg(if ok {
                    theme::SUCCESS
                } else {
                    theme::DESTRUCTIVE
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .overflow_hidden()
                .child(
                    label(title, theme::CALLOUT, false)
                        .text_color(theme::LABEL)
                        .overflow_hidden()
                        .text_ellipsis(),
                )
                .child(
                    label(subtitle, theme::FOOTNOTE, false)
                        .text_color(theme::TERTIARY_LABEL)
                        .overflow_hidden()
                        .text_ellipsis(),
                ),
        )
}

fn observe_event_rows(snap: &ObserveSnapshot) -> Vec<(bool, String, String)> {
    let mut rows = Vec::new();
    for alert in snap.alerts.iter().take(2) {
        let sub = if alert.summary.is_empty() {
            alert.severity.clone()
        } else {
            alert.summary.clone()
        };
        rows.push((false, alert.name.clone(), sub));
    }
    if rows.len() < 2 {
        for reading in &snap.metrics {
            if rows.len() >= 2 {
                break;
            }
            let ok = reading.error.is_none() && snap.connected;
            rows.push((ok, reading.label.clone(), metric_subtitle(reading)));
        }
    }
    if rows.is_empty() && snap.connected {
        rows.push((true, "warmup /metrics".into(), "just now".into()));
    }
    rows.truncate(2);
    rows
}

fn metric_subtitle(reading: &nook_core::observe::MetricReading) -> String {
    if let Some(err) = &reading.error {
        return err.clone();
    }
    if let Some(pt) = reading.series.first().and_then(|s| s.points.last()) {
        return relative_ago(pt.ts);
    }
    if let Some(value) = reading.last_value() {
        return nook_core::observe::format_sample(value);
    }
    "just now".into()
}

fn relative_ago(ts: f64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(ts);
    let secs = (now - ts).max(0.0) as u64;
    if secs < 15 {
        "just now".into()
    } else if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

/// Big metric numeral: mockup 34/36 weight 600 with an 11px tertiary unit.
fn observe_headline(text: String) -> gpui::Div {
    div()
        .text_size(px(34.0))
        .line_height(px(36.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme::LABEL)
        .child(text)
}

fn observe_unit(text: &str) -> gpui::Div {
    div()
        .text_size(px(theme::SUBHEADLINE.size))
        .line_height(px(26.0))
        .text_color(theme::tertiary_label())
        .child(text.to_string())
}

fn range_chip(option: ObserveRange, active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("range-{}", option.label())))
        .flex()
        .items_center()
        .justify_center()
        .px(px(10.))
        .py(px(4.))
        .rounded(px(8.))
        .bg(if active {
            theme::FILL_SECONDARY
        } else {
            theme::with_alpha(theme::ISLAND, 0.0)
        })
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if this.observe_hover.take().is_some() {
                    cx.notify();
                }
                if nook_core::settings::get_app_settings().observe.range != option {
                    nook_core::settings::tweak_app_settings(|s| {
                        nook_core::observe::set_range(&mut s.observe, option);
                    });
                    this.settings = nook_core::settings::get_app_settings();
                }
                this.refresh_observe(cx);
            }),
        )
        .child(
            label(option.label(), theme::SUBHEADLINE, active).text_color(if active {
                theme::LABEL
            } else {
                theme::TERTIARY_LABEL
            }),
        )
}

fn observe_error_row(err: &str, cx: &mut Context<Island>) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(lucide_color("triangle-alert", 14.0, theme::DESTRUCTIVE))
        .child(
            label(err.to_string(), theme::SUBHEADLINE, false)
                .flex_1()
                .min_w(px(0.))
                .text_color(theme::DESTRUCTIVE),
        )
        .child(text_btn("Retry", cx, |this, _, cx| {
            this.refresh_observe(cx);
        }))
}

fn chart_color(query: &str) -> Rgba {
    match query.trim() {
        "5xx" | "errors" => theme::DESTRUCTIVE,
        "4xx" => theme::WARNING,
        "slow" => theme::SUCCESS,
        _ => theme::accent(),
    }
}

fn status_group(query: &str) -> Option<usize> {
    match query.trim() {
        "1xx" | "2xx" | "3xx" => Some(0),
        "4xx" => Some(1),
        "5xx" => Some(2),
        _ => None,
    }
}

fn status_color(group: usize) -> Rgba {
    match group {
        0 => with_alpha(theme::LABEL, 0x5C as f32 / 255.0),
        1 => chart_color("4xx"),
        _ => chart_color("5xx"),
    }
}

fn status_series(snap: &ObserveSnapshot) -> Vec<StatusSeries> {
    snap.metrics
        .iter()
        .filter_map(|reading| {
            Some(StatusSeries {
                group: status_group(&reading.query)?,
                points: reading.series.first()?.points.clone(),
            })
        })
        .filter(|series| series.points.len() >= 2)
        .collect()
}

fn status_totals(snap: &ObserveSnapshot) -> [f64; 3] {
    let mut totals = [0.0; 3];
    for reading in &snap.metrics {
        if let Some(group) = status_group(&reading.query) {
            totals[group] += reading
                .window_total
                .or_else(|| reading.last_value())
                .unwrap_or(0.0);
        }
    }
    totals
}

fn status_legend(totals: [f64; 3], range: ObserveRange) -> impl IntoElement {
    // Mockup legend row: 5px dots, 11px tertiary labels, gap 16, with the
    // window (`last 15m · 30s step`) pinned right.
    let mut legend = div()
        .flex()
        .items_center()
        .gap(px(16.))
        .w_full()
        .flex_shrink_0();
    for (group, name) in ["1–3xx", "4xx", "5xx"].into_iter().enumerate() {
        legend = legend.child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(div().size(px(5.)).rounded_full().bg(status_color(group)))
                .child(
                    label(
                        format!("{name} {}", compact_number(totals[group])),
                        theme::SUBHEADLINE,
                        false,
                    )
                    .text_color(theme::tertiary_label()),
                ),
        );
    }
    legend
        .child(div().flex_1())
        .child(
            label(
                format!("last {} · {} step", range.label(), bucket_step(range)),
                theme::SUBHEADLINE,
                false,
            )
            .text_color(theme::tertiary_label()),
        )
}

/// Width of one chart column: the range split into [`BIG_COLUMNS`] (`30s` at 15m).
fn bucket_step(range: ObserveRange) -> String {
    let secs = range.seconds() / BIG_COLUMNS as i64;
    if secs >= 60 && secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

fn stacked_samples(series: &[StatusSeries], step: f64) -> Vec<StackedSample> {
    let tol = (step * 0.5).max(1e-9);
    let mut times: Vec<f64> = series
        .iter()
        .flat_map(|status| status.points.iter().map(|point| point.ts))
        .filter(|ts| ts.is_finite())
        .collect();
    if times.is_empty() {
        return Vec::new();
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut slots = Vec::new();
    for ts in times {
        match slots.last() {
            Some(&prev) if ts - prev < tol => {}
            _ => slots.push(ts),
        }
    }
    slots
        .into_iter()
        .map(|ts| {
            let mut values = [0.0; 3];
            for status in series {
                if let Some(sample) = nook_core::observe::point_at_ts(&status.points, ts) {
                    if (sample.ts - ts).abs() <= tol {
                        values[status.group] += clamp_value(sample.value);
                    }
                }
            }
            StackedSample { ts, values }
        })
        .collect()
}

fn with_alpha(color: Rgba, a: f32) -> Rgba {
    Rgba { a, ..color }
}

fn format_chart_time(ts: f64, with_seconds: bool) -> String {
    use chrono::{Local, TimeZone};
    if !ts.is_finite() {
        return String::new();
    }
    if let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() {
        if with_seconds {
            dt.format("%H:%M:%S").to_string()
        } else {
            dt.format("%H:%M").to_string()
        }
    } else {
        String::new()
    }
}

fn format_bucket_range(start: f64, end: f64, range: ObserveRange) -> String {
    let with_seconds = range.seconds() < 3600;
    format!(
        "{} – {}",
        format_chart_time(start, with_seconds),
        format_chart_time(end, with_seconds)
    )
}

fn rounded_top_rect(
    path: &mut PathBuilder,
    p: &impl Fn(f32, f32) -> gpui::Point<gpui::Pixels>,
    x: f32,
    top: f32,
    w: f32,
    bottom: f32,
    r: f32,
) {
    let r = r.max(0.0);
    path.move_to(p(x, bottom));
    path.line_to(p(x + w, bottom));
    if r < 0.4 {
        path.line_to(p(x + w, top));
        path.line_to(p(x, top));
        path.close();
        return;
    }
    let k = 0.552_284_8 * r;
    path.line_to(p(x + w, top + r));
    path.cubic_bezier_to(
        p(x + w - r, top),
        p(x + w, top + r - k),
        p(x + w - r + k, top),
    );
    path.line_to(p(x + r, top));
    path.cubic_bezier_to(p(x, top + r), p(x + r - k, top), p(x, top + r - k));
    path.close();
}

#[cfg(test)]
mod tests {
    use super::{
        bucket_index_at, bucket_step, compact_number, nice_ticks, stacked_samples, status_buckets,
        tooltip_flips_left, StackedSample, StatusSeries, BIG_COLUMNS,
    };
    use nook_core::observe::ObserveRange;
    use nook_core::observe::SamplePoint;

    #[test]
    fn status_samples_stack_success_and_errors() {
        let points = |a, b| {
            vec![
                SamplePoint { ts: 1.0, value: a },
                SamplePoint { ts: 2.0, value: b },
            ]
        };
        let samples = stacked_samples(
            &[
                StatusSeries {
                    group: 0,
                    points: points(10.0, 20.0),
                },
                StatusSeries {
                    group: 0,
                    points: points(1.0, 2.0),
                },
                StatusSeries {
                    group: 2,
                    points: points(3.0, 4.0),
                },
            ],
            1.0,
        );
        assert_eq!(samples[1].values, [22.0, 0.0, 4.0]);
    }

    #[test]
    fn stacked_samples_ignore_far_series() {
        let samples = stacked_samples(
            &[
                StatusSeries {
                    group: 0,
                    points: vec![
                        SamplePoint {
                            ts: 10.0,
                            value: 4.0,
                        },
                        SamplePoint {
                            ts: 20.0,
                            value: 5.0,
                        },
                    ],
                },
                StatusSeries {
                    group: 1,
                    points: vec![
                        SamplePoint {
                            ts: 1_000.0,
                            value: 99.0,
                        },
                        SamplePoint {
                            ts: 1_010.0,
                            value: 98.0,
                        },
                    ],
                },
            ],
            10.0,
        );
        assert_eq!(samples.len(), 4);
        assert_eq!(
            samples[0],
            StackedSample {
                ts: 10.0,
                values: [4.0, 0.0, 0.0],
            }
        );
        assert_eq!(
            samples[1],
            StackedSample {
                ts: 20.0,
                values: [5.0, 0.0, 0.0],
            }
        );
        assert_eq!(
            samples[2],
            StackedSample {
                ts: 1_000.0,
                values: [0.0, 99.0, 0.0],
            }
        );
        assert_eq!(
            samples[3],
            StackedSample {
                ts: 1_010.0,
                values: [0.0, 98.0, 0.0],
            }
        );
    }

    #[test]
    fn compact_number_matches_the_mockup() {
        assert_eq!(compact_number(2400.0), "2.4k");
        assert_eq!(compact_number(2000.0), "2k");
        assert_eq!(compact_number(84.0), "84");
        assert_eq!(compact_number(12.5), "12.5");
        assert_eq!(compact_number(f64::NAN), "—");
    }

    #[test]
    fn big_chart_always_has_thirty_buckets() {
        let range = ObserveRange::FifteenMinutes;
        assert_eq!(bucket_step(range), "30s");
        assert_eq!(bucket_step(ObserveRange::OneHour), "2m");
        assert_eq!(bucket_step(ObserveRange::FiveMinutes), "10s");
        let empty = status_buckets(&[], range, BIG_COLUMNS);
        assert_eq!(empty.len(), BIG_COLUMNS);
        assert!(empty.iter().all(|b| b.values.is_none()));
    }

    #[test]
    fn continuous_series_fills_interior_columns() {
        for range in ObserveRange::all() {
            let width = range.seconds() as f64 / BIG_COLUMNS as f64;
            let newest = 1_700_000_000.0;
            let last_start = (newest / width).floor() * width;
            let first_start = last_start - (BIG_COLUMNS as f64 - 1.0) * width;
            let step = range.step_seconds() as f64;
            let mut samples = Vec::new();
            let mut ts = first_start;
            while ts < last_start + width - 1e-9 {
                samples.push(StackedSample {
                    ts,
                    values: [1.0, 0.0, 0.0],
                });
                ts += step;
            }
            let buckets = status_buckets(&samples, range, BIG_COLUMNS);
            let first = buckets.iter().position(|b| b.values.is_some());
            let last = buckets.iter().rposition(|b| b.values.is_some());
            let (first, last) = (first.expect("expected samples"), last.expect("expected samples"));
            assert!(
                buckets[first..=last].iter().all(|b| b.values.is_some()),
                "empty interior column for {range:?}"
            );
        }
    }

    #[test]
    fn buckets_align_to_wall_clock() {
        let range = ObserveRange::FifteenMinutes;
        let newest = 10_000.0;
        let width = range.seconds() as f64 / BIG_COLUMNS as f64;
        let buckets = status_buckets(
            &[StackedSample {
                ts: newest,
                values: [1.0, 0.0, 0.0],
            }],
            range,
            BIG_COLUMNS,
        );
        let last_start = (newest / width).floor() * width;
        assert_eq!(buckets.len(), BIG_COLUMNS);
        assert!((buckets[BIG_COLUMNS - 1].start - last_start).abs() < 1e-9);
        assert!(newest >= buckets[BIG_COLUMNS - 1].start);
        assert!(newest < buckets[BIG_COLUMNS - 1].end);
        for (i, bucket) in buckets.iter().enumerate() {
            assert!((bucket.end - bucket.start - width).abs() < 1e-9);
            assert!(
                bucket.start.rem_euclid(width).abs() < 1e-6,
                "bucket {i} start {} not aligned",
                bucket.start
            );
        }
        let first_start = last_start - (BIG_COLUMNS as f64 - 1.0) * width;
        let edge = status_buckets(
            &[
                StackedSample {
                    ts: first_start - 1e-6,
                    values: [9.0, 0.0, 0.0],
                },
                StackedSample {
                    ts: first_start,
                    values: [2.0, 0.0, 0.0],
                },
                StackedSample {
                    ts: first_start + width,
                    values: [8.0, 0.0, 0.0],
                },
                StackedSample {
                    ts: last_start,
                    values: [4.0, 0.0, 0.0],
                },
                StackedSample {
                    ts: newest,
                    values: [4.0, 0.0, 0.0],
                },
            ],
            range,
            BIG_COLUMNS,
        );
        assert_eq!(edge[0].values, Some([2.0, 0.0, 0.0]));
        assert_eq!(edge[1].values, Some([8.0, 0.0, 0.0]));
        assert_eq!(edge[BIG_COLUMNS - 1].values, Some([4.0, 0.0, 0.0]));
    }

    #[test]
    fn nice_ticks_from_2437() {
        let (max, ticks) = nice_ticks(2437.0);
        assert_eq!(max, 3000.0);
        assert_eq!(ticks, vec![0.0, 1000.0, 2000.0, 3000.0]);
    }

    #[test]
    fn bucket_index_maps_ratio_edges() {
        assert_eq!(bucket_index_at(0.0, BIG_COLUMNS), 0);
        assert_eq!(bucket_index_at(1.0, BIG_COLUMNS), 29);
        assert_eq!(bucket_index_at(-0.2, BIG_COLUMNS), 0);
        assert_eq!(bucket_index_at(1.2, BIG_COLUMNS), 29);
    }

    #[test]
    fn tooltip_flips_after_sixty_percent() {
        assert!(!tooltip_flips_left(0, BIG_COLUMNS));
        assert!(!tooltip_flips_left(17, BIG_COLUMNS));
        assert!(tooltip_flips_left(18, BIG_COLUMNS));
        assert!(tooltip_flips_left(29, BIG_COLUMNS));
    }

    #[test]
    fn gallery_rows_prefer_alerts_then_metrics() {
        let mut snap = nook_core::observe::ObserveSnapshot::default();
        snap.connected = true;
        snap.alerts.push(nook_core::observe::FiringAlert {
            name: "403 Forbidden".into(),
            severity: "error".into(),
            summary: "/admin · 2m ago".into(),
        });
        snap.metrics.push(nook_core::observe::MetricReading {
            label: "warmup /metrics".into(),
            query: "total_requests".into(),
            chart: nook_core::observe::ObserveChartKind::Off,
            values: Vec::new(),
            series: Vec::new(),
            error: None,
            history: Vec::new(),
            window_total: None,
        });
        let rows = super::observe_event_rows(&snap);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].0);
        assert_eq!(rows[0].1, "403 Forbidden");
        assert_eq!(rows[0].2, "/admin · 2m ago");
        assert!(rows[1].0);
        assert_eq!(rows[1].1, "warmup /metrics");
    }

    #[test]
    fn relative_ago_uses_just_now_under_15s() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        assert_eq!(super::relative_ago(now), "just now");
        assert_eq!(super::relative_ago(now - 120.0), "2m ago");
    }
}
