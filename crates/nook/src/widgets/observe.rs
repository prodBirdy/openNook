//! Observe Nook pane: warmUP `/admin/metrics` plus Prometheus `/api/v1/query_range`.
//!
//! The 128pt gallery card is two status rows. Clicking it opens the full-width
//! chart view: one dashboard panel per picker chip (stacked, bars, line, stat).

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_pane, text_btn};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, deferred, div, point, prelude::*, px, relative, AnyElement, Bounds, Context, CursorStyle,
    MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, Rgba, ScrollHandle,
    SharedString,
};
use nook_core::observe::{
    MetricReading, ObserveDashboard, ObservePanel, ObserveRange, ObserveSnapshot,
    ObserveSourceKind, PanelKind, SamplePoint,
};
use nook_core::settings::AppSettings;
use std::cell::RefCell;
use std::rc::Rc;

/// Mockup Nook Row while Observe is expanded (`observe.html` height 240).
pub(crate) const OBSERVE_EXPANDED_BODY: f32 = 240.0;

thread_local! {
    static PANEL_PICKER_SCROLL: RefCell<ScrollHandle> = RefCell::new(ScrollHandle::default());
}

fn panel_picker_scroll() -> ScrollHandle {
    PANEL_PICKER_SCROLL.with_borrow(|h| h.clone())
}

/// One panel target resolved to chart data: display label/colour plus the
/// reading's range points (empty when the snapshot has no such reading).
#[derive(Clone)]
struct PanelSeries {
    label: String,
    expr: String,
    color: Rgba,
    points: Vec<SamplePoint>,
    total: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct PanelSample {
    ts: f64,
    values: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct PanelBucket {
    start: f64,
    end: f64,
    values: Option<Vec<f64>>,
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

    let pip_open = crate::pip::is_open(crate::pip::PipKind::Observe);
    card_shell("nook-observe")
        .group("nook-observe")
        .relative()
        .w_full()
        .cursor(CursorStyle::PointingHand)
        .active(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.open_observe_expanded(cx);
            }),
        )
        .child(body)
        .child(
            // No header row: Pop Out overlays top-right (8 pt inset), hover-only
            // unless that PiP is open (then accent-tinted and always visible).
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .when(!pip_open, |d| {
                    d.opacity(0.0)
                        .group_hover("nook-observe", |s| s.opacity(1.0))
                })
                .child(crate::island::media::pip_popout_btn_sized(
                    crate::pip::PipKind::Observe,
                    24.0,
                    12.0,
                    13.0,
                    cx,
                )),
        )
}

pub(crate) fn observe_big_view(
    snap: &ObserveSnapshot,
    settings: &AppSettings,
    hover: Option<&ObserveHover>,
    cx: &mut Context<Island>,
) -> gpui::Stateful<gpui::Div> {
    // Mockup `observe-big`: numeral + unit + range chips, caption, panel
    // picker, chart, legend. Panels come from `effective_dashboard`; the
    // selection persists in `chart_panel`. Empty data keeps the layout.
    let range = settings.observe.range;
    let mut chips = div().flex().items_center().gap(px(6.)).flex_shrink_0();
    for option in ObserveRange::all() {
        chips = chips.child(range_chip(option, option == range, cx));
    }

    let dashboard = nook_core::observe::effective_dashboard(&settings.observe);
    let title = nook_core::observe::effective_chart_panel(&settings.observe);
    let panel = title
        .as_deref()
        .and_then(|t| dashboard.panels.iter().find(|p| p.title == t))
        .or_else(|| dashboard.panels.first());
    let Some(panel) = panel else {
        return big_pane(
            "—".into(),
            None,
            "No panels".into(),
            None,
            None,
            div().flex_1().min_h(px(0.)).into_any_element(),
            None,
            chips,
            cx,
        );
    };

    let unit = panel
        .unit
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let expr = panel
                .targets
                .first()
                .map(|t| t.expr.as_str())
                .unwrap_or("");
            let derived = metric_unit(expr);
            (!derived.is_empty()).then(|| derived.to_string())
        });
    let unit_str = unit.as_deref().unwrap_or("");
    let caption = observe_metric_caption(Some(&panel.title), settings);
    let picker = panel_picker(&dashboard, &panel.title, cx).into_any_element();
    let series = panel_series(snap, panel);

    let (headline, delta, body, legend) = match panel.kind {
        PanelKind::Stacked => {
            let samples = panel_samples(&series, range.step_seconds() as f64);
            let buckets = panel_buckets(&samples, series.len(), range, BIG_COLUMNS);
            let hover_idx = hover.map(|h| h.bucket);
            let headline = match hovered_values(&buckets, hover_idx) {
                Some(values) => compact_number(panel_total(values)),
                None => {
                    let latest: f64 = series
                        .iter()
                        .filter_map(|s| s.points.last())
                        .map(|p| clamp_value(p.value))
                        .sum();
                    if latest > 0.0 {
                        compact_number(latest)
                    } else {
                        "—".into()
                    }
                }
            };
            (
                headline,
                None,
                time_chart(
                    ChartPaint::Stacked,
                    &buckets,
                    &series,
                    unit_str,
                    hover_idx,
                    range,
                    cx,
                )
                .into_any_element(),
                Some(panel_legend(&series, unit_str, range).into_any_element()),
            )
        }
        PanelKind::Bars => {
            let samples = panel_samples(&series, range.step_seconds() as f64);
            let buckets = panel_buckets(&samples, series.len(), range, BIG_COLUMNS);
            let hover_idx = hover.map(|h| h.bucket);
            let headline = match hovered_values(&buckets, hover_idx) {
                Some(values) => {
                    compact_number(clamp_value(values.first().copied().unwrap_or(0.0)))
                }
                None => series
                    .first()
                    .and_then(|s| s.points.last())
                    .map(|p| compact_number(clamp_value(p.value)))
                    .unwrap_or_else(|| "—".into()),
            };
            (
                headline,
                None,
                time_chart(
                    ChartPaint::Bars,
                    &buckets,
                    &series,
                    unit_str,
                    hover_idx,
                    range,
                    cx,
                )
                .into_any_element(),
                Some(panel_legend(&series, unit_str, range).into_any_element()),
            )
        }
        PanelKind::Line => {
            let samples = panel_samples(&series, range.step_seconds() as f64);
            let buckets = panel_buckets(&samples, series.len(), range, BIG_COLUMNS);
            let hover_idx = hover.map(|h| h.bucket);
            let headline = match hovered_values(&buckets, hover_idx) {
                Some(values) => {
                    compact_number(clamp_value(values.first().copied().unwrap_or(0.0)))
                }
                None => series
                    .first()
                    .and_then(|s| s.points.last())
                    .map(|p| compact_number(clamp_value(p.value)))
                    .unwrap_or_else(|| "—".into()),
            };
            (
                headline,
                None,
                time_chart(
                    ChartPaint::Line,
                    &buckets,
                    &series,
                    unit_str,
                    hover_idx,
                    range,
                    cx,
                )
                .into_any_element(),
                Some(panel_legend(&series, unit_str, range).into_any_element()),
            )
        }
        PanelKind::Stat => {
            let primary = series.first();
            let expr = primary.map(|s| s.expr.as_str()).unwrap_or("");
            let reading = nook_core::observe::reading_for(snap, expr);
            let headline = reading
                .and_then(|r| r.last_value())
                .map(compact_number)
                .unwrap_or_else(|| "—".into());
            (
                headline,
                stat_delta(reading, expr, range),
                stat_body(primary, cx).into_any_element(),
                Some(panel_legend(&series, unit_str, range).into_any_element()),
            )
        }
    };
    big_pane(
        headline,
        unit,
        caption,
        Some(picker),
        delta,
        body,
        legend,
        chips,
        cx,
    )
}

fn hovered_values(buckets: &[PanelBucket], hover: Option<usize>) -> Option<&Vec<f64>> {
    hover
        .and_then(|i| buckets.get(i))
        .and_then(|b| b.values.as_ref())
}

fn big_pane(
    headline: String,
    unit: Option<String>,
    caption: String,
    picker: Option<AnyElement>,
    delta: Option<(String, Rgba)>,
    body: AnyElement,
    legend: Option<AnyElement>,
    chips: gpui::Div,
    cx: &mut Context<Island>,
) -> gpui::Stateful<gpui::Div> {
    let mut number = div()
        .flex()
        .items_end()
        .gap(px(8.))
        .child(observe_headline(headline));
    if let Some(unit) = unit.filter(|u| !u.is_empty()) {
        number = number.child(observe_unit(&unit));
    }
    let mut left = div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(2.))
        .child(number);
    if let Some((text, color)) = delta {
        left = left.child(label(text, theme::FOOTNOTE, false).text_color(color));
    }
    let mut pane = nook_pane("nook-observe-big")
        .group("nook-observe-big")
        .relative()
        .w_full()
        .bg(theme::ISLAND)
        .rounded(px(theme::ROW_RADIUS))
        .border_1()
        .border_color(theme::FILL_TERTIARY)
        .p(px(16.))
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(16.))
                .flex_shrink_0()
                .w_full()
                .child(left)
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .flex_shrink_0()
                        .child(crate::island::media::pip_popout_btn_sized(
                            crate::pip::PipKind::Observe,
                            24.0,
                            12.0,
                            13.0,
                            cx,
                        ))
                        .child(chips),
                ),
        )
        .child(
            label(caption, theme::CALLOUT, false)
                .text_color(theme::secondary_label())
                .w_full()
                .flex_shrink_0(),
        );
    if let Some(picker) = picker {
        pane = pane.child(picker);
    }
    pane = pane.child(body);
    if let Some(legend) = legend {
        pane = pane.child(legend);
    }
    pane.child(observe_close_btn(cx))
}

fn panel_picker(
    dashboard: &ObserveDashboard,
    selected: &str,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut row = div()
        .id("observe-panel-picker")
        .track_scroll(&panel_picker_scroll())
        .flex()
        .items_center()
        .gap(px(6.))
        .w_full()
        .flex_shrink_0()
        .overflow_x_scroll()
        .overflow_y_hidden();
    for panel in &dashboard.panels {
        row = row.child(panel_chip(&panel.title, panel.title == selected, cx));
    }
    row
}

fn panel_chip(title: &str, active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    let title = title.to_string();
    let select = title.clone();
    div()
        .id(SharedString::from(format!("observe-panel-{title}")))
        .flex()
        .flex_shrink_0()
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
                nook_core::settings::tweak_app_settings(|s| {
                    nook_core::observe::set_chart_panel(&mut s.observe, Some(&select));
                });
                this.settings = nook_core::settings::get_app_settings();
                cx.notify();
            }),
        )
        .child(
            label(truncate_metric_label(&title), theme::SUBHEADLINE, active)
                .whitespace_nowrap()
                .text_color(if active {
                    theme::LABEL
                } else {
                    theme::TERTIARY_LABEL
                }),
        )
}

/// Legend row below the chart: per-target dot + label + window total, with
/// the window/step caption pinned right. Headline and legend use the unit.
fn panel_legend(series: &[PanelSeries], unit: &str, range: ObserveRange) -> impl IntoElement {
    let mut legend = div()
        .flex()
        .items_center()
        .gap(px(16.))
        .w_full()
        .flex_shrink_0();
    for s in series {
        let value = s.total.map(compact_number).unwrap_or_else(|| "—".into());
        let text = if unit.is_empty() || value == "—" {
            format!("{} {value}", s.label)
        } else {
            format!("{} {value} {unit}", s.label)
        };
        legend = legend.child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(div().size(px(5.)).rounded_full().bg(s.color))
                .child(
                    label(text, theme::SUBHEADLINE, false).text_color(theme::tertiary_label()),
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

/// Sparkline strip for stat panels: the primary target's range points as a
/// thin line with a last-point dot.
fn stat_body(primary: Option<&PanelSeries>, cx: &mut Context<Island>) -> impl IntoElement {
    let _ = cx;
    let (points, color) = primary
        .map(|s| (s.points.clone(), s.color))
        .unwrap_or_else(|| (Vec::new(), theme::tertiary_label()));
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .justify_center()
        .child(
            div()
                .w_full()
                .h(px(48.))
                .flex_shrink_0()
                .child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, _| {
                            paint_stat_sparkline(window, bounds, &points, color);
                        },
                    )
                    .w_full()
                    .h_full(),
                ),
        )
}

fn truncate_metric_label(label: &str) -> String {
    const LIMIT: usize = 18;
    if label.chars().count() > LIMIT {
        let short: String = label.chars().take(LIMIT).collect();
        format!("{short}…")
    } else {
        label.to_string()
    }
}


fn is_latency_query(query: &str) -> bool {
    let query = query.trim();
    matches!(query, "avg_ms" | "max_ms")
        || query.starts_with("route_avg_ms:")
        || query.starts_with("route_max_ms:")
}

fn is_error_query(query: &str) -> bool {
    let query = query.trim();
    matches!(query, "5xx" | "errors") || query.starts_with("route_errors:")
}

fn is_counter_like_query(query: &str) -> bool {
    let query = query.trim();
    matches!(
        query,
        "total_requests" | "requests" | "1xx" | "2xx" | "3xx" | "4xx" | "5xx" | "slow" | "errors"
    ) || query.starts_with("route:")
        || query.starts_with("route_errors:")
        || query.ends_with("_total")
        || query.ends_with("_count")
}

fn metric_unit(query: &str) -> &'static str {
    if is_latency_query(query) {
        "ms"
    } else if is_counter_like_query(query) {
        "req/s"
    } else {
        ""
    }
}

fn metric_color(query: &str) -> Rgba {
    if is_latency_query(query) {
        theme::accent()
    } else if is_error_query(query) {
        theme::DESTRUCTIVE
    } else {
        theme::LABEL
    }
}

/// Dark systemPurple / systemGray reference values for dashboard target
/// colours that have no theme constant.
const PANEL_PURPLE: Rgba = Rgba {
    r: 0.749,
    g: 0.353,
    b: 0.949,
    a: 1.0,
};
const PANEL_GRAY: Rgba = Rgba {
    r: 0.557,
    g: 0.557,
    b: 0.576,
    a: 1.0,
};

fn panel_palette() -> [Rgba; 6] {
    [
        theme::accent(),
        theme::SUCCESS,
        theme::WARNING,
        theme::DESTRUCTIVE,
        PANEL_PURPLE,
        PANEL_GRAY,
    ]
}

/// One [`PanelSeries`] per panel target, aligned with `panel.targets`.
fn panel_series(snap: &ObserveSnapshot, panel: &ObservePanel) -> Vec<PanelSeries> {
    let n = panel.targets.len();
    let palette = panel_palette();
    panel
        .targets
        .iter()
        .enumerate()
        .map(|(i, target)| {
            let expr = target.expr.trim();
            let reading = if expr.is_empty() {
                None
            } else {
                nook_core::observe::reading_for(snap, expr)
            };
            let label = target
                .legend
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .or_else(|| reading.map(|r| r.label.clone()))
                .unwrap_or_else(|| expr.to_string());
            let points = reading
                .and_then(|r| r.series.first())
                .map(|s| s.points.clone())
                .unwrap_or_default();
            let total = reading.and_then(|r| r.window_total.or_else(|| r.last_value()));
            PanelSeries {
                label,
                expr: expr.to_string(),
                color: panel_target_color(target.color.as_deref(), i, n, expr, &palette),
                points,
                total,
            }
        })
        .collect()
}

fn panel_target_color(
    raw: Option<&str>,
    index: usize,
    targets: usize,
    expr: &str,
    palette: &[Rgba; 6],
) -> Rgba {
    if let Some(name) = raw.map(str::trim).filter(|s| !s.is_empty()) {
        let lower = name.to_ascii_lowercase();
        if let Some(hex) = lower.strip_prefix('#') {
            if let Some(rgb) = parse_hex_rgb(hex) {
                return rgb;
            }
        } else {
            return match lower.as_str() {
                "green" => theme::SUCCESS,
                "blue" => theme::ACCENT,
                "orange" => theme::WARNING,
                "red" => theme::DESTRUCTIVE,
                "purple" => PANEL_PURPLE,
                "gray" | "grey" => PANEL_GRAY,
                "accent" => theme::accent(),
                _ => panel_color_fallback(index, targets, expr, palette),
            };
        }
    }
    panel_color_fallback(index, targets, expr, palette)
}

/// One series keeps the pinned-metric expression colour; several unnamed
/// targets step through the palette so they stay distinguishable.
fn panel_color_fallback(index: usize, targets: usize, expr: &str, palette: &[Rgba; 6]) -> Rgba {
    if targets == 1 {
        metric_color(expr)
    } else {
        palette[index % palette.len()]
    }
}

fn parse_hex_rgb(hex: &str) -> Option<Rgba> {
    if hex.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgba {
        r: byte(0)? as f32 / 255.0,
        g: byte(2)? as f32 / 255.0,
        b: byte(4)? as f32 / 255.0,
        a: 1.0,
    })
}

/// Align every target's points onto shared slots (same nearest-neighbour
/// rule as the old stacked chart), one value per target.
fn panel_samples(series: &[PanelSeries], step: f64) -> Vec<PanelSample> {
    let tol = (step * 0.5).max(1e-9);
    let mut times: Vec<f64> = series
        .iter()
        .flat_map(|s| s.points.iter().map(|point| point.ts))
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
    let n = series.len();
    slots
        .into_iter()
        .map(|ts| {
            let mut values = vec![0.0; n];
            for (s, value) in series.iter().zip(values.iter_mut()) {
                if let Some(sample) = nook_core::observe::point_at_ts(&s.points, ts) {
                    if (sample.ts - ts).abs() <= tol {
                        *value += clamp_value(sample.value);
                    }
                }
            }
            PanelSample { ts, values }
        })
        .collect()
}

/// Bucket samples into `count` wall-clock slices of the selected range,
/// ending at the bucket that contains the newest sample. Each bucket is the
/// per-target mean of samples in `[start, end)`; empty buckets stay `None`
/// except a one-poll carry-forward after the first real sample.
fn panel_buckets(
    samples: &[PanelSample],
    groups: usize,
    range: ObserveRange,
    count: usize,
) -> Vec<PanelBucket> {
    if count == 0 || groups == 0 {
        return vec![
            PanelBucket {
                start: 0.0,
                end: 0.0,
                values: None,
            };
            count
        ];
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

    let mut sums = vec![vec![0.0f64; groups]; count];
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
        for (sum, value) in sums[i].iter_mut().zip(&sample.values) {
            *sum += clamp_value(*value);
        }
        hits[i] += 1;
        last_hit_ts[i] = Some(last_hit_ts[i].map_or(sample.ts, |ts| ts.max(sample.ts)));
    }

    let mut buckets: Vec<PanelBucket> = (0..count)
        .map(|i| {
            let start = first_start + i as f64 * width;
            PanelBucket {
                start,
                end: start + width,
                values: (hits[i] > 0).then(|| {
                    sums[i]
                        .iter()
                        .map(|v| clamp_value(v / hits[i] as f64))
                        .collect()
                }),
            }
        })
        .collect();

    let mut times: Vec<f64> = samples
        .iter()
        .map(|sample| sample.ts)
        .filter(|ts| ts.is_finite())
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let max_gap = carry_gap(&times, range);
    let mut last_real: Option<(f64, Vec<f64>)> = None;
    for (bucket, hit_ts) in buckets.iter_mut().zip(last_hit_ts) {
        if let Some(values) = bucket.values.clone() {
            last_real = Some((hit_ts.unwrap_or(bucket.start), values));
        } else if let Some((ts, values)) = last_real.as_ref() {
            if bucket.start - *ts <= max_gap {
                bucket.values = Some(values.clone());
            }
        }
    }
    buckets
}

fn carry_gap(times: &[f64], range: ObserveRange) -> f64 {
    let step = (range.step_seconds() as f64).max(1.0);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartPaint {
    Stacked,
    Bars,
    Line,
}

/// Shared chart chrome (y-gutter ticks, x-axis, hover column + tooltip) for
/// the time-series panel kinds; only the canvas paint differs.
fn time_chart(
    paint: ChartPaint,
    buckets: &[PanelBucket],
    series: &[PanelSeries],
    unit: &str,
    hover_idx: Option<usize>,
    range: ObserveRange,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let data_max = buckets
        .iter()
        .filter_map(|bucket| bucket.values.as_ref())
        .map(|values| match paint {
            ChartPaint::Stacked => panel_total(values),
            _ => values
                .iter()
                .map(|v| clamp_value(*v))
                .fold(0.0, f64::max),
        })
        .fold(0.0, f64::max);
    let (nice_max, ticks) = nice_ticks(data_max);
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
    let tooltip_bucket = hover_idx.and_then(|i| buckets.get(i)).cloned();
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
                    let colors: Vec<Rgba> = series.iter().map(|s| s.color).collect();
                    let ticks = ticks.clone();
                    move |bounds, _, window, _| match paint {
                        ChartPaint::Stacked => paint_stacked_panel(
                            window, bounds, &painted, hover_idx, nice_max, &ticks, &colors,
                        ),
                        ChartPaint::Bars => paint_grouped_bars(
                            window, bounds, &painted, hover_idx, nice_max, &ticks, &colors,
                        ),
                        ChartPaint::Line => paint_line_panel(
                            window, bounds, &painted, hover_idx, nice_max, &ticks, &colors,
                        ),
                    }
                },
            )
            .w_full()
            .h_full(),
        );

    if let (Some(bucket), Some(idx)) = (tooltip_bucket, hover_idx) {
        plot = plot.child(panel_tooltip(
            &bucket,
            series,
            unit,
            idx,
            count,
            range,
            paint == ChartPaint::Stacked,
        ));
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

fn tooltip_row(dot: Rgba, name: String, text: String, dim: bool) -> impl IntoElement {
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
                .child(div().size(px(6.)).rounded_full().flex_shrink_0().bg(dot))
                .child(label(name, theme::FOOTNOTE, false).text_color(if dim {
                    theme::tertiary_label()
                } else {
                    theme::LABEL
                })),
        )
        .child(div().flex_1())
        .child(
            label(text, theme::FOOTNOTE, false)
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_color(if dim {
                    theme::tertiary_label()
                } else {
                    theme::LABEL
                }),
        )
}

fn tooltip_total_row(text: String) -> impl IntoElement {
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
            label(text, theme::SUBHEADLINE, true)
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_color(theme::LABEL),
        )
}

fn panel_tooltip(
    bucket: &PanelBucket,
    series: &[PanelSeries],
    unit: &str,
    index: usize,
    count: usize,
    range: ObserveRange,
    show_total: bool,
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
    if let Some(values) = &bucket.values {
        for (i, s) in series.iter().enumerate() {
            let value = clamp_value(values.get(i).copied().unwrap_or(0.0));
            let dim = value <= 0.0;
            let dot = if dim {
                with_alpha(s.color, s.color.a * HOVER_DIM)
            } else {
                s.color
            };
            let text = if unit.is_empty() {
                compact_number(value)
            } else {
                format!("{} {unit}", compact_number(value))
            };
            body = body.child(tooltip_row(dot, s.label.clone(), text, dim));
        }
        if show_total {
            let total = if unit.is_empty() {
                compact_number(panel_total(values))
            } else {
                format!("{} {unit}", compact_number(panel_total(values)))
            };
            body = body
                .child(
                    div()
                        .w_full()
                        .h(px(1.))
                        .bg(theme::FILL_SECONDARY)
                        .flex_shrink_0(),
                )
                .child(tooltip_total_row(total));
        }
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

fn paint_stacked_panel(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    buckets: &[PanelBucket],
    hover: Option<usize>,
    nice_max: f64,
    ticks: &[f64],
    colors: &[Rgba],
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
            let Some(values) = &bucket.values else {
                continue;
            };
            if panel_total(values) <= 0.0 {
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
                let mut color = colors.get(group).copied().unwrap_or(theme::LABEL);
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

fn paint_grouped_bars(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    buckets: &[PanelBucket],
    hover: Option<usize>,
    nice_max: f64,
    ticks: &[f64],
    colors: &[Rgba],
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
            let Some(values) = &bucket.values else {
                continue;
            };
            let n = values.len().max(1);
            let inner_gap = if n > 1 {
                (col_w * 0.06).min(2.0)
            } else {
                0.0
            };
            let sub_w = ((col_w - inner_gap * (n - 1) as f32) / n as f32).max(0.0);
            if sub_w <= 0.0 {
                continue;
            }
            let dim = hover.is_some_and(|idx| idx != i);
            for (s, raw) in values.iter().enumerate() {
                let value = clamp_value(*raw);
                if value <= 0.0 {
                    continue;
                }
                let x = x0 + i as f32 * stride + s as f32 * (sub_w + inner_gap);
                let seg_h = (h as f64 * (value / nice_max)).clamp(0.0, h as f64) as f32;
                let top = (y0 + h - seg_h).max(y0);
                let mut color = colors.get(s).copied().unwrap_or(theme::LABEL);
                if dim {
                    color.a *= HOVER_DIM;
                }
                let radius = 2.0f32.min(sub_w * 0.5).min(((y0 + h - top) * 0.5).max(0.0));
                let mut bar = PathBuilder::fill();
                rounded_top_rect(&mut bar, &p, x, top, sub_w, y0 + h, radius);
                if let Ok(built) = bar.build() {
                    window.paint_path(built, color);
                }
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

fn paint_line_panel(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    buckets: &[PanelBucket],
    hover: Option<usize>,
    nice_max: f64,
    ticks: &[f64],
    colors: &[Rgba],
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
    let y_at = |value: f64| {
        if nice_max <= 0.0 {
            y0 + h
        } else {
            y0 + h * (1.0 - (value / nice_max) as f32).clamp(0.0, 1.0)
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
        let centers: Vec<f32> = (0..count)
            .map(|i| x0 + i as f32 * stride + col_w * 0.5)
            .collect();
        for (s, color) in colors.iter().enumerate() {
            let mut path = PathBuilder::stroke(px(1.5));
            let mut pen_up = true;
            let mut last_pt: Option<(f32, f32)> = None;
            for (i, bucket) in buckets.iter().enumerate() {
                let Some(value) = bucket.values.as_ref().and_then(|v| v.get(s)) else {
                    pen_up = true;
                    continue;
                };
                let y = y_at(clamp_value(*value));
                if pen_up {
                    path.move_to(p(centers[i], y));
                    pen_up = false;
                } else {
                    path.line_to(p(centers[i], y));
                }
                last_pt = Some((centers[i], y));
            }
            if let Ok(built) = path.build() {
                window.paint_path(built, *color);
            }
            if let Some((lx, ly)) = last_pt {
                let mut dot = PathBuilder::fill();
                fill_circle(&mut dot, &p, lx, ly, 2.5);
                if let Ok(built) = dot.build() {
                    window.paint_path(built, *color);
                }
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

fn fill_circle(
    path: &mut PathBuilder,
    p: &impl Fn(f32, f32) -> gpui::Point<gpui::Pixels>,
    cx: f32,
    cy: f32,
    r: f32,
) {
    let k = 0.552_284_8 * r;
    path.move_to(p(cx + r, cy));
    path.cubic_bezier_to(p(cx + r, cy + k), p(cx + k, cy + r), p(cx, cy + r));
    path.cubic_bezier_to(p(cx - k, cy + r), p(cx - r, cy + k), p(cx - r, cy));
    path.cubic_bezier_to(p(cx - r, cy - k), p(cx - k, cy - r), p(cx, cy - r));
    path.cubic_bezier_to(p(cx + k, cy - r), p(cx + r, cy - k), p(cx + r, cy));
    path.close();
}

/// Delta vs the start of the range for stat panels. Counters report the
/// window increase; gauges compare the last sample against the first one.
fn stat_delta(
    reading: Option<&MetricReading>,
    expr: &str,
    range: ObserveRange,
) -> Option<(String, Rgba)> {
    let reading = reading?;
    reading.last_value()?;
    let bad_on_rise = is_error_query(expr) || is_latency_query(expr);
    let (delta, pct) = if let Some(total) = reading.window_total {
        (total, None)
    } else if let (Some(first), Some(now)) = (reading.history.first(), reading.last_value()) {
        let delta = now - first.value;
        let pct = if first.value.abs() > 1e-9 {
            Some(delta / first.value * 100.0)
        } else {
            None
        };
        (delta, pct)
    } else {
        return None;
    };
    if delta.abs() < 1e-9 {
        return Some((
            "no change vs range start".into(),
            theme::tertiary_label(),
        ));
    }
    let up = delta > 0.0;
    let color = if up == !bad_on_rise {
        theme::SUCCESS
    } else {
        theme::DESTRUCTIVE
    };
    let signed = format!("{}{}", if up { "+" } else { "-" }, compact_number(delta.abs()));
    let text = if reading.window_total.is_some() {
        format!("{signed} in last {}", range.label())
    } else if let Some(pct) = pct {
        format!("{signed} ({pct:+.0}%) vs range start")
    } else {
        format!("{signed} vs range start")
    };
    Some((text, color))
}

fn paint_stat_sparkline(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    points: &[SamplePoint],
    color: Rgba,
) {
    let w: f32 = bounds.size.width.into();
    let h: f32 = bounds.size.height.into();
    if w < 8.0 || h < 8.0 || points.len() < 2 {
        return;
    }
    let x0: f32 = bounds.origin.x.into();
    let y0: f32 = bounds.origin.y.into();
    let p = |x: f32, y: f32| point(px(x), px(y));
    let t0 = points.first().map(|s| s.ts).unwrap_or(0.0);
    let t1 = points.last().map(|s| s.ts).unwrap_or(t0);
    let span = (t1 - t0).max(1e-9);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in points {
        let v = clamp_value(s.value);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let vspan = (hi - lo).max(1e-9);
    let pad = 5.0f32;
    let mut path = PathBuilder::stroke(px(1.5));
    for (i, s) in points.iter().enumerate() {
        let x = x0 + ((s.ts - t0) / span) as f32 * w;
        let y = y0 + h - pad - ((clamp_value(s.value) - lo) / vspan) as f32 * (h - pad * 2.0);
        if i == 0 {
            path.move_to(p(x, y));
        } else {
            path.line_to(p(x, y));
        }
    }
    if let Ok(built) = path.build() {
        window.paint_path(built, color);
    }
    if let Some(last) = points.last() {
        let y = y0 + h - pad - ((clamp_value(last.value) - lo) / vspan) as f32 * (h - pad * 2.0);
        let mut dot = PathBuilder::fill();
        fill_circle(&mut dot, &p, (x0 + w - 2.0).max(x0), y, 2.5);
        if let Ok(built) = dot.build() {
            window.paint_path(built, color);
        }
    }
}

/// Columns in the big status chart
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

fn panel_total(values: &[f64]) -> f64 {
    values.iter().map(|v| clamp_value(*v)).sum()
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
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

/// Width of one chart column: the range split into [`BIG_COLUMNS`] (`30s` at 15m).
fn bucket_step(range: ObserveRange) -> String {
    let secs = range.seconds() / BIG_COLUMNS as i64;
    if secs >= 60 && secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
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
        bucket_index_at, bucket_step, carry_gap, compact_number, nice_ticks, panel_buckets,
        panel_samples, panel_series, stat_delta, tooltip_flips_left, PanelBucket, PanelSample,
        PanelSeries, BIG_COLUMNS,
    };
    use crate::theme;
    use nook_core::observe::{
        ChartPoint, MetricReading, ObserveChartKind, ObservePanel, ObserveRange, PanelKind,
        PanelTarget, RangeSeries, SamplePoint, SeriesValue,
    };

    fn series(label: &str, points: Vec<(f64, f64)>) -> PanelSeries {
        PanelSeries {
            label: label.into(),
            expr: label.into(),
            color: theme::SUCCESS,
            points: points
                .into_iter()
                .map(|(ts, value)| SamplePoint { ts, value })
                .collect(),
            total: None,
        }
    }

    #[test]
    fn panel_samples_align_series_to_slots() {
        let points = |a, b| vec![(1.0, a), (2.0, b)];
        let samples = panel_samples(
            &[
                series("a", points(10.0, 20.0)),
                series("b", points(1.0, 2.0)),
                series("c", points(3.0, 4.0)),
            ],
            1.0,
        );
        assert_eq!(samples.len(), 2);
        assert_eq!(
            samples[0],
            PanelSample {
                ts: 1.0,
                values: vec![10.0, 1.0, 3.0],
            }
        );
        assert_eq!(
            samples[1],
            PanelSample {
                ts: 2.0,
                values: vec![20.0, 2.0, 4.0],
            }
        );
    }

    #[test]
    fn panel_samples_drops_misaligned_series() {
        let samples = panel_samples(
            &[
                series("a", vec![(10.0, 4.0), (20.0, 5.0)]),
                series("b", vec![(1_000.0, 99.0), (1_010.0, 98.0)]),
            ],
            10.0,
        );
        assert_eq!(samples.len(), 4);
        assert_eq!(
            samples[0],
            PanelSample {
                ts: 10.0,
                values: vec![4.0, 0.0],
            }
        );
        assert_eq!(
            samples[1],
            PanelSample {
                ts: 20.0,
                values: vec![5.0, 0.0],
            }
        );
        assert_eq!(
            samples[2],
            PanelSample {
                ts: 1_000.0,
                values: vec![0.0, 99.0],
            }
        );
        assert_eq!(
            samples[3],
            PanelSample {
                ts: 1_010.0,
                values: vec![0.0, 98.0],
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
        let empty: Vec<PanelBucket> = panel_buckets(&[], 3, range, BIG_COLUMNS);
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
                samples.push(PanelSample {
                    ts,
                    values: vec![1.0, 0.0],
                });
                ts += step;
            }
            let buckets = panel_buckets(&samples, 2, range, BIG_COLUMNS);
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
        let buckets = panel_buckets(
            &[PanelSample {
                ts: newest,
                values: vec![1.0, 0.0],
            }],
            2,
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
        let edge = panel_buckets(
            &[
                PanelSample {
                    ts: first_start - 1e-6,
                    values: vec![9.0, 0.0],
                },
                PanelSample {
                    ts: first_start,
                    values: vec![2.0, 0.0],
                },
                PanelSample {
                    ts: first_start + width,
                    values: vec![8.0, 0.0],
                },
                PanelSample {
                    ts: last_start,
                    values: vec![4.0, 0.0],
                },
                PanelSample {
                    ts: newest,
                    values: vec![4.0, 0.0],
                },
            ],
            2,
            range,
            BIG_COLUMNS,
        );
        assert_eq!(edge[0].values, Some(vec![2.0, 0.0]));
        assert_eq!(edge[1].values, Some(vec![8.0, 0.0]));
        assert_eq!(edge[BIG_COLUMNS - 1].values, Some(vec![4.0, 0.0]));
    }

    #[test]
    fn carry_gap_defaults_to_poll_interval() {
        let range = ObserveRange::FifteenMinutes;
        assert_eq!(carry_gap(&[], range), 30.0);
        let times: Vec<f64> = (0..15).map(|i| i as f64 * 30.0).collect();
        assert_eq!(carry_gap(&times, range), 30.0);
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

    fn reading(query: &str, points: Vec<(f64, f64)>, total: Option<f64>) -> MetricReading {
        MetricReading {
            label: query.into(),
            query: query.into(),
            chart: ObserveChartKind::Bars,
            values: vec![SeriesValue {
                name: String::new(),
                value: points.last().map(|(_, v)| *v).unwrap_or(0.0),
            }],
            series: vec![RangeSeries {
                name: String::new(),
                points: points
                    .into_iter()
                    .map(|(ts, value)| SamplePoint { ts, value })
                    .collect(),
            }],
            error: None,
            history: Vec::new(),
            window_total: total,
        }
    }

    #[test]
    fn panel_series_resolves_targets_and_colors() {
        let snap = nook_core::observe::ObserveSnapshot {
            metrics: vec![reading("5xx", vec![(1.0, 2.0), (2.0, 3.0)], Some(5.0))],
            ..Default::default()
        };
        let panel = ObservePanel {
            title: "Errors".into(),
            kind: PanelKind::Bars,
            unit: Some("req/s".into()),
            targets: vec![
                PanelTarget {
                    expr: " 5xx ".into(),
                    legend: Some("server".into()),
                    color: Some("red".into()),
                },
                PanelTarget {
                    expr: "missing".into(),
                    legend: None,
                    color: Some("#ff8800".into()),
                },
            ],
        };
        let resolved = panel_series(&snap, &panel);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].label, "server");
        assert_eq!(resolved[0].expr, "5xx");
        assert_eq!(resolved[0].points.len(), 2);
        assert_eq!(resolved[0].total, Some(5.0));
        assert_eq!(resolved[0].color, theme::DESTRUCTIVE);
        assert_eq!(resolved[1].label, "missing");
        assert!(resolved[1].points.is_empty());
        assert_eq!(resolved[1].total, None);
        let orange = resolved[1].color;
        assert!((orange.r - 1.0).abs() < 1e-6);
        assert!((orange.g - 0x88 as f32 / 255.0).abs() < 1e-6);
        assert_eq!(orange.b, 0.0);
    }

    #[test]
    fn stat_delta_reports_window_increase_for_counters() {
        let mut with_total = reading("total_requests", vec![(1.0, 47_800.0)], None);
        with_total.values[0].value = 49_000.0;
        with_total.window_total = Some(1_200.0);
        let (text, color) = stat_delta(
            Some(&with_total),
            "total_requests",
            ObserveRange::FifteenMinutes,
        )
        .unwrap();
        assert!(text.contains("1.2k"), "{text}");
        assert!(text.contains("15m"), "{text}");
        assert_eq!(color, theme::SUCCESS);
    }

    #[test]
    fn stat_delta_compares_gauges_against_range_start() {
        let mut gauge = reading("avg_ms", vec![(1.0, 20.0), (2.0, 42.0)], None);
        gauge.values[0].value = 42.0;
        gauge.history = vec![
            ChartPoint { t: 0.0, value: 20.0 },
            ChartPoint {
                t: 1.0,
                value: 42.0,
            },
        ];
        let (text, color) =
            stat_delta(Some(&gauge), "avg_ms", ObserveRange::FifteenMinutes).unwrap();
        assert!(text.contains("22"), "{text}");
        assert!(text.contains("+110%"), "{text}");
        assert_eq!(color, theme::DESTRUCTIVE);
        assert!(stat_delta(None, "avg_ms", ObserveRange::FifteenMinutes).is_none());
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
