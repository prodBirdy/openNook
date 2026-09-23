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
    ObserveChartKind, ObserveRange, ObserveSnapshot, ObserveSourceKind, RangeSeries, SamplePoint,
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
pub(crate) struct ObserveHover {
    pub query: String,
    pub series: Option<String>,
    pub ts: f64,
    pub value: f64,
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
) -> impl IntoElement {
    // Mockup `observe-big` (nNzAv): numeral + unit + range chips, caption,
    // 30 stacked status columns, legend. No other chrome; the × only shows
    // on hover. Empty data keeps the layout with a "—" numeral.
    let _ = hover;
    let range = settings.observe.range;
    let mut chips = div().flex().items_center().gap(px(6.)).flex_shrink_0();
    for option in ObserveRange::all() {
        chips = chips.child(range_chip(option, option == range, cx));
    }

    let statuses = status_series(snap);
    let featured = snap.metrics.first();
    let headline = if !statuses.is_empty() {
        let latest: f64 = statuses
            .iter()
            .filter_map(|series| series.points.last())
            .map(|pt| pt.value.max(0.0))
            .sum();
        compact_number(latest)
    } else {
        featured
            .and_then(|m| m.last_value())
            .map(compact_number)
            .unwrap_or_else(|| "—".into())
    };
    let metric_label = if !statuses.is_empty() {
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

    let buckets = status_buckets(&stacked_samples(&statuses), range, BIG_COLUMNS);

    nook_pane("nook-observe-big")
        .group("nook-observe-big")
        .relative()
        .w_full()
        .h_full()
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
        .child(status_columns(&buckets))
        .child(status_legend(status_totals(snap), range))
        .child(observe_close_btn(cx))
}

/// Columns in the big status chart (mockup: always 30).
const BIG_COLUMNS: usize = 30;
const COLUMN_GAP: f32 = 3.0;
const COLUMN_RADIUS: f32 = 3.0;
const CLOSE_INSET: f32 = 12.0;
const CLOSE_GLYPH: f32 = 16.0;

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

/// Bucket stacked samples into `count` equal slices of the selected range,
/// ending at the newest sample. Each bucket is the per-group mean of the
/// samples that fall in it; empty buckets are `None` (no bar).
fn status_buckets(
    samples: &[StackedSample],
    range: ObserveRange,
    count: usize,
) -> Vec<Option<[f64; 3]>> {
    let mut sums = vec![[0.0f64; 3]; count];
    let mut hits = vec![0usize; count];
    if let Some(end) = samples.iter().map(|s| s.ts).reduce(f64::max) {
        let span = range.seconds().max(1) as f64;
        let start = end - span;
        for sample in samples {
            if sample.ts < start {
                continue;
            }
            let index = (((sample.ts - start) / span) * count as f64) as usize;
            let index = index.min(count.saturating_sub(1));
            for (sum, value) in sums[index].iter_mut().zip(sample.values) {
                *sum += value.max(0.0);
            }
            hits[index] += 1;
        }
    }
    sums.into_iter()
        .zip(hits)
        .map(|(sum, n)| (n > 0).then(|| sum.map(|v| v / n as f64)))
        .collect()
}

/// 30 flex columns, bottom-aligned, 3pt gaps. Each column stacks 1–3xx
/// (bottom), 4xx, 5xx (top); the column's 3pt radius clips the corners and
/// the topmost present segment carries the 3 3 0 0 radius.
fn status_columns(buckets: &[Option<[f64; 3]>]) -> impl IntoElement {
    let max = buckets
        .iter()
        .flatten()
        .map(|values| values.iter().sum::<f64>())
        .fold(0.0, f64::max);
    let mut chart = div()
        .flex()
        .items_end()
        .gap(px(COLUMN_GAP))
        .w_full()
        .flex_1()
        .min_h(px(0.));
    for bucket in buckets {
        let total = bucket.map(|v| v.iter().sum::<f64>()).unwrap_or(0.0);
        let mut column = div().flex_1().min_w(px(0.));
        if max > 0.0 && total > 0.0 {
            let values = bucket.unwrap_or_default();
            let top = values.iter().rposition(|v| *v > 0.0).unwrap_or(0);
            column = column
                .h(relative((total / max) as f32))
                .rounded(px(COLUMN_RADIUS))
                .overflow_hidden()
                .flex()
                .flex_col();
            // Top-down children: 5xx, 4xx, then 1–3xx at the bottom.
            for group in (0..3).rev() {
                let value = values[group];
                if value <= 0.0 {
                    continue;
                }
                column = column.child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .h(relative((value / total) as f32))
                        .bg(status_color(group))
                        .when(group == top, |d| d.rounded_t(px(COLUMN_RADIUS))),
                );
            }
        } else {
            column = column.h(px(0.));
        }
        chart = chart.child(column);
    }
    chart
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
    if let Some(pt) = reading
        .series
        .first()
        .and_then(|s| s.points.last())
    {
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

fn stacked_samples(series: &[StatusSeries]) -> Vec<StackedSample> {
    let Some(reference) = series.iter().max_by_key(|series| series.points.len()) else {
        return Vec::new();
    };
    reference
        .points
        .iter()
        .map(|point| {
            let mut values = [0.0; 3];
            for status in series {
                if let Some(sample) = nook_core::observe::point_at_ts(&status.points, point.ts) {
                    values[status.group] += sample.value.max(0.0);
                }
            }
            StackedSample {
                ts: point.ts,
                values,
            }
        })
        .collect()
}

fn with_alpha(color: Rgba, a: f32) -> Rgba {
    Rgba { a, ..color }
}

fn mini_chart(
    query: String,
    kind: ObserveChartKind,
    series: RangeSeries,
    multi: bool,
    color: Rgba,
    hover: Option<&ObserveHover>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let bounds_cell = Rc::new(RefCell::new(None::<Bounds<Pixels>>));
    let series_name = series.name.clone();
    let points = series.points;
    let hover_pt = hover.and_then(|h| {
        nook_core::observe::point_at_ts(&points, h.ts)
            .filter(|_| h.query == query)
            .cloned()
    });
    let tooltip_t = hover_pt
        .as_ref()
        .and_then(|pt| sample_t(&points, pt.ts))
        .unwrap_or(0.0);
    let painted = samples_as_chart(&points);

    let mut chart = div()
        .id(SharedString::from(format!("chart-{query}")))
        .relative()
        .flex()
        .w_full()
        .h(px(48.))
        .flex_shrink_0()
        .cursor(CursorStyle::Crosshair)
        .on_mouse_move({
            let bounds_cell = bounds_cell.clone();
            let points = points.clone();
            let query = query.clone();
            let series_name = series_name.clone();
            cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let Some(bounds) = *bounds_cell.borrow() else {
                    return;
                };
                let width: f32 = bounds.size.width.into();
                if width < 1.0 || points.is_empty() {
                    return;
                }
                let x: f32 = event.position.x.into();
                let origin: f32 = bounds.origin.x.into();
                let t = ((x - origin) / width).clamp(0.0, 1.0);
                let Some(pt) = nook_core::observe::point_at_ratio(&points, t) else {
                    return;
                };
                let next = ObserveHover {
                    query: query.clone(),
                    series: if multi && !series_name.is_empty() {
                        Some(series_name.clone())
                    } else {
                        None
                    },
                    ts: pt.ts,
                    value: pt.value,
                };
                if this.observe_hover.as_ref() != Some(&next) {
                    this.observe_hover = Some(next);
                    cx.notify();
                }
            })
        })
        .on_hover({
            let query = query.clone();
            cx.listener(move |this, hovered: &bool, _, cx| {
                if !*hovered
                    && this
                        .observe_hover
                        .as_ref()
                        .is_some_and(|h| h.query == query)
                {
                    this.observe_hover = None;
                    cx.notify();
                }
            })
        })
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
                    let hover_t = tooltip_t;
                    let show_hover = hover_pt.is_some();
                    move |bounds, _, window, _| {
                        let x0: f32 = bounds.origin.x.into();
                        let y0: f32 = bounds.origin.y.into();
                        let w: f32 = bounds.size.width.into();
                        let h: f32 = bounds.size.height.into();
                        if w < 8.0 || h < 8.0 {
                            return;
                        }
                        let p = |x: f32, y: f32| point(px(x), px(y));

                        let from_zero = kind == ObserveChartKind::Bars;
                        let values: Vec<f64> = painted.iter().map(|pt| pt.1).collect();
                        let ys = scale_series(&values, from_zero);
                        if ys.is_empty() {
                            return;
                        }
                        let y_at = |t: f32| y0 + h * (1.0 - t);
                        let pts: Vec<(f32, f32)> = painted
                            .iter()
                            .zip(ys.iter())
                            .map(|(pt, y)| (x0 + pt.0.clamp(0.0, 1.0) * w, y_at(*y)))
                            .collect();
                        match kind {
                            ObserveChartKind::Bars => {
                                paint_bars(window, x0, y0, w, h, &pts, color, p)
                            }
                            ObserveChartKind::Sparkline => {
                                paint_sparkline(window, x0, y0, w, h, &pts, color, p)
                            }
                            ObserveChartKind::Off => {}
                        }
                        if show_hover {
                            let hx = x0 + hover_t.clamp(0.0, 1.0) * w;
                            if let Some((_, hy)) = painted.iter().zip(pts.iter()).min_by(|a, b| {
                                (a.0 .0 - hover_t)
                                    .abs()
                                    .partial_cmp(&(b.0 .0 - hover_t).abs())
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            }) {
                                let mut rule = PathBuilder::stroke(px(1.0));
                                rule.move_to(p(hx, y0));
                                rule.line_to(p(hx, y0 + h));
                                if let Ok(built) = rule.build() {
                                    window.paint_path(built, with_alpha(theme::LABEL, 0.45));
                                }
                                fill_dot(window, hx, hy.1, 3.6, color);
                            }
                        }
                    }
                },
            )
            .w_full()
            .h_full(),
        );

    if let Some(pt) = hover_pt {
        let align_right = tooltip_t > 0.55;
        let mut pop = div().flex().flex_col().gap(px(1.));
        if let Some(name) = hover.and_then(|h| h.series.clone()) {
            pop = pop.child(label(name, theme::FOOTNOTE, false));
        }
        pop = pop
            .child(label(format_observe_ts(pt.ts), theme::FOOTNOTE, false))
            .child(label(
                nook_core::observe::format_chart_sample(&query, pt.value),
                theme::SUBHEADLINE,
                true,
            ));
        // Paint after ancestors so the card's overflow clip cannot hide the
        // popover. Sit just above the sparkline so it doesn't cover the cursor.
        chart = chart.child(deferred(
            div()
                .absolute()
                .bottom(px(40.))
                .when(align_right, |d| d.right(px(2.)))
                .when(!align_right, |d| {
                    d.left(relative(tooltip_t.clamp(0.0, 0.55)))
                })
                .rounded(px(theme::CONTROL_RADIUS))
                .bg(theme::GROUPED_BG)
                .border_1()
                .border_color(theme::FILL_SECONDARY)
                .shadow_sm()
                .px_2()
                .py(px(3.))
                .child(pop),
        ));
    }

    chart
}

/// x-ratio of `ts` across a series whose samples span `[t0, t0 + span]`.
fn span_t(t0: f64, span: f64, ts: f64) -> f32 {
    (((ts - t0) / span) as f32).clamp(0.0, 1.0)
}

fn samples_as_chart(points: &[SamplePoint]) -> Vec<(f32, f64)> {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Vec::new();
    };
    let t0 = first.ts;
    let span = (last.ts - t0).max(1e-9);
    points
        .iter()
        .map(|p| (span_t(t0, span, p.ts), p.value))
        .collect()
}

fn sample_t(points: &[SamplePoint], ts: f64) -> Option<f32> {
    let t0 = points.first()?.ts;
    let span = (points.last()?.ts - t0).max(1e-9);
    Some(span_t(t0, span, ts))
}

fn format_observe_ts(ts: f64) -> String {
    use chrono::{Local, TimeZone};
    if let Some(dt) = Local.timestamp_opt(ts as i64, 0).single() {
        dt.format("%H:%M:%S").to_string()
    } else {
        String::new()
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_bars(
    window: &mut gpui::Window,
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    pts: &[(f32, f32)],
    color: Rgba,
    p: impl Fn(f32, f32) -> gpui::Point<gpui::Pixels>,
) {
    let bar_w = (w / 60.0 * 0.8).max(1.5);
    let bottom = y0 + h;
    for &(x, top) in pts {
        let left = (x - bar_w * 0.5).max(x0);
        let r = (bar_w * 0.28).min(4.0).min(((bottom - top) * 0.5).max(0.0));
        let mut bar = PathBuilder::fill();
        rounded_top_rect(&mut bar, &p, left, top, bar_w, bottom, r);
        if let Ok(built) = bar.build() {
            window.paint_path(built, color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_sparkline(
    window: &mut gpui::Window,
    _x0: f32,
    y0: f32,
    _w: f32,
    h: f32,
    pts: &[(f32, f32)],
    color: Rgba,
    p: impl Fn(f32, f32) -> gpui::Point<gpui::Pixels>,
) {
    let baseline = y0 + h;

    if pts.len() == 1 {
        fill_dot(window, pts[0].0, pts[0].1, 2.4, color);
        return;
    }

    let mut area = PathBuilder::fill();
    area.move_to(p(pts[0].0, baseline));
    area.line_to(p(pts[0].0, pts[0].1));
    append_monotone(&mut area, pts);
    let end = *pts.last().unwrap();
    area.line_to(p(end.0, baseline));
    area.close();
    if let Ok(built) = area.build() {
        window.paint_path(built, with_alpha(color, 0.22));
    }

    let mut glow = PathBuilder::fill();
    glow.move_to(p(pts[0].0, (pts[0].1 + baseline) * 0.5));
    glow.line_to(p(pts[0].0, pts[0].1));
    append_monotone(&mut glow, pts);
    glow.line_to(p(end.0, (end.1 + baseline) * 0.5));
    glow.close();
    if let Ok(built) = glow.build() {
        window.paint_path(built, with_alpha(color, 0.16));
    }

    let mut line = PathBuilder::stroke(px(2.0));
    line.move_to(p(pts[0].0, pts[0].1));
    append_monotone(&mut line, pts);
    if let Ok(built) = line.build() {
        window.paint_path(built, color);
    }
    fill_dot(window, end.0, end.1, 2.4, color);
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

fn fill_dot(window: &mut gpui::Window, cx: f32, cy: f32, r: f32, color: Rgba) {
    let k = 0.552_284_8 * r;
    let p = |x: f32, y: f32| point(px(x), px(y));
    let mut path = PathBuilder::fill();
    path.move_to(p(cx, cy - r));
    path.cubic_bezier_to(p(cx + r, cy), p(cx + k, cy - r), p(cx + r, cy - k));
    path.cubic_bezier_to(p(cx, cy + r), p(cx + r, cy + k), p(cx + k, cy + r));
    path.cubic_bezier_to(p(cx - r, cy), p(cx - k, cy + r), p(cx - r, cy + k));
    path.cubic_bezier_to(p(cx, cy - r), p(cx - r, cy - k), p(cx - k, cy - r));
    path.close();
    if let Ok(built) = path.build() {
        window.paint_path(built, color);
    }
}

fn append_monotone(path: &mut PathBuilder, pts: &[(f32, f32)]) {
    for (c1, c2, to) in monotone_beziers(pts) {
        path.cubic_bezier_to(
            point(px(to.0), px(to.1)),
            point(px(c1.0), px(c1.1)),
            point(px(c2.0), px(c2.1)),
        );
    }
}

/// Fritsch–Carlson monotone cubic, same family as d3 `curveMonotoneX`.
type BezierSegment = ((f32, f32), (f32, f32), (f32, f32));

fn monotone_beziers(pts: &[(f32, f32)]) -> Vec<BezierSegment> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let mut dx = vec![0.0; n - 1];
    let mut m = vec![0.0; n - 1];
    for i in 0..n - 1 {
        dx[i] = pts[i + 1].0 - pts[i].0;
        let dy = pts[i + 1].1 - pts[i].1;
        m[i] = if dx[i].abs() < 1e-6 { 0.0 } else { dy / dx[i] };
    }
    let mut t = vec![0.0; n];
    t[0] = m[0];
    t[n - 1] = m[n - 2];
    for i in 1..n - 1 {
        t[i] = if m[i - 1] * m[i] <= 0.0 {
            0.0
        } else {
            (m[i - 1] + m[i]) / 2.0
        };
    }
    for i in 0..n - 1 {
        if m[i].abs() < 1e-8 {
            t[i] = 0.0;
            t[i + 1] = 0.0;
            continue;
        }
        let a = t[i] / m[i];
        let b = t[i + 1] / m[i];
        let s = a * a + b * b;
        if s > 9.0 {
            let tau = 3.0 / s.sqrt();
            t[i] = tau * a * m[i];
            t[i + 1] = tau * b * m[i];
        }
    }
    let mut out = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let d = dx[i] / 3.0;
        out.push((
            (pts[i].0 + d, pts[i].1 + t[i] * d),
            (pts[i + 1].0 - d, pts[i + 1].1 - t[i + 1] * d),
            pts[i + 1],
        ));
    }
    out
}

fn scale_series(series: &[f64], from_zero: bool) -> Vec<f32> {
    let finite: Vec<f64> = series.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return Vec::new();
    }
    let mut min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if from_zero && finite.iter().all(|v| *v >= 0.0) {
        min = 0.0;
    }
    let span = (max - min).max(1e-9);
    finite
        .into_iter()
        .map(|v| {
            let t = ((v - min) / span) as f32;
            0.06 + t.clamp(0.0, 1.0) * 0.88
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        bucket_step, compact_number, monotone_beziers, scale_series, stacked_samples,
        status_buckets, StackedSample, StatusSeries, BIG_COLUMNS,
    };
    use nook_core::observe::ObserveRange;
    use nook_core::observe::SamplePoint;

    #[test]
    fn scale_minmax_maps_into_padding() {
        let ys = scale_series(&[0.0, 10.0], false);
        assert!((ys[0] - 0.06).abs() < 1e-5);
        assert!((ys[1] - 0.94).abs() < 1e-5);
    }

    #[test]
    fn bars_scale_from_zero() {
        let ys = scale_series(&[5.0, 10.0], true);
        assert!(ys[0] < ys[1]);
        assert!(ys[0] > 0.06);
    }

    #[test]
    fn monotone_cubic_keeps_endpoints() {
        let pts = [(0.0, 10.0), (10.0, 0.0), (20.0, 8.0)];
        let segs = monotone_beziers(&pts);
        assert_eq!(segs.len(), 2);
        assert!((segs[1].2 .0 - 20.0).abs() < 1e-5);
        assert!((segs[1].2 .1 - 8.0).abs() < 1e-5);
        assert!(segs[0].0 .0 > pts[0].0);
        assert!(segs[0].0 .0 < pts[1].0);
    }

    #[test]
    fn status_samples_stack_success_and_errors() {
        let points = |a, b| {
            vec![
                SamplePoint { ts: 1.0, value: a },
                SamplePoint { ts: 2.0, value: b },
            ]
        };
        let samples = stacked_samples(&[
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
        ]);
        assert_eq!(samples[1].values, [22.0, 0.0, 4.0]);
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
        assert_eq!(status_buckets(&[], range, BIG_COLUMNS).len(), BIG_COLUMNS);
        assert!(status_buckets(&[], range, BIG_COLUMNS).iter().all(Option::is_none));

        let end = 10_000.0;
        let sample = |ts: f64, v: [f64; 3]| StackedSample { ts, values: v };
        let samples = [
            sample(end - 900.0 - 10.0, [99.0, 0.0, 0.0]), // before the window
            sample(end - 890.0, [10.0, 0.0, 0.0]),
            sample(end - 880.0, [20.0, 2.0, 0.0]),
            sample(end, [5.0, 1.0, 1.0]),
        ];
        let buckets = status_buckets(&samples, range, BIG_COLUMNS);
        assert_eq!(buckets.len(), BIG_COLUMNS);
        assert_eq!(buckets[0], Some([15.0, 1.0, 0.0]), "mean of the first slice");
        assert_eq!(buckets[BIG_COLUMNS - 1], Some([5.0, 1.0, 1.0]));
        assert!(buckets[1..BIG_COLUMNS - 1].iter().all(Option::is_none));
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
