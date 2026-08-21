//! Observe Nook pane: warmUP `/admin/metrics` plus Prometheus `/api/v1/query_range`.
//!
//! Calendar-style range chips, a featured headline, and one Nightwatch-style
//! status chart with a hover popover.

use crate::island::ui::{label, nook_display, nook_empty, nook_pane, slide_label};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, deferred, div, point, prelude::*, px, relative, Bounds, Context, CursorStyle,
    MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, Rgba, SharedString,
};
use nook_core::observe::{
    ObserveChartKind, ObserveRange, ObserveSnapshot, RangeSeries, SamplePoint,
};
use nook_core::settings::AppSettings;
use std::cell::RefCell;
use std::rc::Rc;

const STATUS_CHART_ID: &str = "http-status";

#[derive(Clone)]
struct StatusSeries {
    group: usize,
    points: Vec<SamplePoint>,
}

#[derive(Clone, Debug, PartialEq)]
struct StackedSample {
    t: f32,
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
    let range = settings.observe.range;
    let mut chips = div().flex().items_end().gap(px(10.));
    for option in ObserveRange::all() {
        let active = option == range;
        chips = chips.child(range_chip(option, active, cx));
    }

    let url = settings.observe.prometheus_url.trim();
    let featured = snap.metrics.first();
    let raw_headline = featured.map(|m| m.headline()).unwrap_or_else(|| "—".into());
    let big_headline = (raw_headline.len() <= 8).then(|| raw_headline.clone());
    let featured_label = featured.map(|m| m.label.clone());

    let mut body = div().flex().flex_col().flex_1().min_h(px(0.));
    if url.is_empty() {
        body = body.child(nook_empty("activity", "No metrics URL"));
    } else if snap.metrics.is_empty() {
        if let Some(err) = &snap.error {
            body = body.child(slide_label(err.clone(), theme::SUBHEADLINE, false).w_full());
        } else {
            body = body.child(nook_empty("activity", "No samples"));
        }
    } else {
        if let Some(err) = &snap.error {
            body = body.child(slide_label(err.clone(), theme::SUBHEADLINE, false).w_full());
        }
        let statuses = status_series(snap);
        let has_statuses = !statuses.is_empty();
        if has_statuses {
            body = body
                .child(status_chart(
                    statuses,
                    hover.filter(|h| h.query == STATUS_CHART_ID),
                    cx,
                ))
                .child(status_legend(status_totals(snap)));
        } else if let Some(reading) = featured {
            let series = reading.series.first().cloned();
            let multi = reading.series.len() > 1;
            if reading.chart != ObserveChartKind::Off {
                if let Some(series) = series.filter(|s| s.points.len() >= 2) {
                    let color = chart_color(&reading.query);
                    body = body.child(mini_chart(
                        reading.query.clone(),
                        reading.chart,
                        series,
                        multi,
                        color,
                        hover.filter(|h| h.query == reading.query),
                        cx,
                    ));
                }
            }
        }
        for reading in snap.metrics.iter().skip(1).take(1) {
            if has_statuses {
                break;
            }
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pt(px(6.))
                    .child(label(reading.label.clone(), theme::SUBHEADLINE, false))
                    .child(label(reading.headline(), theme::CALLOUT, true)),
            );
        }
    }

    nook_pane("nook-observe")
        .w_full()
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(16.))
                .flex_shrink_0()
                .when_some(big_headline, |d, text| d.child(nook_display(text)))
                .when(featured.is_some() && raw_headline.len() > 8, |d| {
                    d.child(label(raw_headline.clone(), theme::TITLE_3, true))
                })
                .child(chips),
        )
        .when_some(featured_label, |d, name| {
            d.child(
                div()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme::SECONDARY_LABEL)
                    .child(name),
            )
        })
        .child(body)
}

fn range_chip(option: ObserveRange, active: bool, cx: &mut Context<Island>) -> impl IntoElement {
    let (unit, num) = range_parts(option);
    let color = if active {
        theme::accent()
    } else {
        theme::LABEL
    };
    let label_color = if active {
        theme::accent()
    } else {
        theme::SECONDARY_LABEL
    };
    div()
        .id(SharedString::from(format!("range-{}", option.label())))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.))
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
            div()
                .text_size(px(9.))
                .line_height(px(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(label_color)
                .child(unit),
        )
        .child(
            div()
                .text_size(px(15.))
                .line_height(px(18.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(num),
        )
}

fn range_parts(option: ObserveRange) -> (&'static str, &'static str) {
    match option {
        ObserveRange::FiveMinutes => ("M", "05"),
        ObserveRange::FifteenMinutes => ("M", "15"),
        ObserveRange::OneHour => ("H", "01"),
        ObserveRange::SixHours => ("H", "06"),
    }
}

fn rgba_white(a: f32) -> Rgba {
    Rgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a,
    }
}

fn chart_color(query: &str) -> Rgba {
    match query.trim() {
        "5xx" | "errors" => theme::DESTRUCTIVE,
        "4xx" => Rgba {
            r: 1.0,
            g: 0.608,
            b: 0.396,
            a: 1.0,
        },
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
        0 => with_alpha(theme::LABEL, 0.36),
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

fn status_legend(totals: [f64; 3]) -> impl IntoElement {
    let mut legend = div()
        .flex()
        .items_center()
        .justify_between()
        .gap_1()
        .pt(px(3.));
    for (group, name) in ["1–3xx", "4xx", "5xx"].into_iter().enumerate() {
        legend = legend.child(
            div()
                .flex()
                .items_center()
                .gap(px(3.))
                .child(div().size(px(5.)).rounded_full().bg(status_color(group)))
                .child(label(
                    format!(
                        "{name} {}",
                        nook_core::observe::format_sample(totals[group])
                    ),
                    theme::FOOTNOTE,
                    false,
                )),
        );
    }
    legend
}

fn stacked_samples(series: &[StatusSeries]) -> Vec<StackedSample> {
    let Some(reference) = series.iter().max_by_key(|series| series.points.len()) else {
        return Vec::new();
    };
    let (Some(first), Some(last)) = (reference.points.first(), reference.points.last()) else {
        return Vec::new();
    };
    let span = (last.ts - first.ts).max(1e-9);
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
                t: span_t(first.ts, span, point.ts),
                ts: point.ts,
                values,
            }
        })
        .collect()
}

fn status_chart(
    series: Vec<StatusSeries>,
    hover: Option<&ObserveHover>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let bounds_cell = Rc::new(RefCell::new(None::<Bounds<Pixels>>));
    let samples = stacked_samples(&series);
    let hover_sample = hover.and_then(|hover| {
        samples.iter().min_by(|a, b| {
            (a.ts - hover.ts)
                .abs()
                .partial_cmp(&(b.ts - hover.ts).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    let hover_t = hover_sample.map(|sample| sample.t).unwrap_or(0.0);

    let mut chart = div()
        .id(STATUS_CHART_ID)
        .relative()
        .flex()
        .w_full()
        .h(px(48.))
        .flex_shrink_0()
        .cursor(CursorStyle::Crosshair)
        .on_mouse_move({
            let bounds_cell = bounds_cell.clone();
            let samples = samples.clone();
            cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let Some(bounds) = *bounds_cell.borrow() else {
                    return;
                };
                let width: f32 = bounds.size.width.into();
                if width < 1.0 || samples.is_empty() {
                    return;
                }
                let x: f32 = event.position.x.into();
                let origin: f32 = bounds.origin.x.into();
                let index = ((((x - origin) / width).clamp(0.0, 1.0) * (samples.len() - 1) as f32)
                    .round() as usize)
                    .min(samples.len() - 1);
                let sample = &samples[index];
                let next = ObserveHover {
                    query: STATUS_CHART_ID.into(),
                    series: None,
                    ts: sample.ts,
                    value: sample.values.iter().sum(),
                };
                if this.observe_hover.as_ref() != Some(&next) {
                    this.observe_hover = Some(next);
                    cx.notify();
                }
            })
        })
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if !*hovered
                && this
                    .observe_hover
                    .as_ref()
                    .is_some_and(|hover| hover.query == STATUS_CHART_ID)
            {
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
                    let samples = samples.clone();
                    let show_hover = hover_sample.is_some();
                    move |bounds, _, window, _| {
                        let x0: f32 = bounds.origin.x.into();
                        let y0: f32 = bounds.origin.y.into();
                        let w: f32 = bounds.size.width.into();
                        let h: f32 = bounds.size.height.into();
                        if w < 8.0 || h < 8.0 || samples.is_empty() {
                            return;
                        }
                        let p = |x: f32, y: f32| point(px(x), px(y));
                        let max = samples
                            .iter()
                            .map(|sample| sample.values.iter().sum::<f64>())
                            .fold(0.0, f64::max);
                        if max <= 0.0 {
                            return;
                        }
                        for t in [0.0, 0.5, 1.0] {
                            let y = y0 + h * t;
                            let mut line = PathBuilder::stroke(px(1.0));
                            line.move_to(p(x0, y));
                            line.line_to(p(x0 + w, y));
                            if let Ok(path) = line.build() {
                                window.paint_path(path, with_alpha(theme::LABEL, 0.11));
                            }
                        }
                        let bar_w = (w / samples.len() as f32 * 0.62).clamp(1.5, 8.0);
                        for sample in &samples {
                            let x = x0 + sample.t * w;
                            let left = (x - bar_w * 0.5).clamp(x0, x0 + w - bar_w);
                            let mut bottom = y0 + h;
                            let top_group = sample
                                .values
                                .iter()
                                .rposition(|value| *value > 0.0)
                                .unwrap_or(0);
                            for (group, value) in sample.values.into_iter().enumerate() {
                                if value <= 0.0 {
                                    continue;
                                }
                                let top = bottom - (value / max) as f32 * h * 0.94;
                                let mut bar = PathBuilder::fill();
                                rounded_top_rect(
                                    &mut bar,
                                    &p,
                                    left,
                                    top,
                                    bar_w,
                                    bottom,
                                    if group == top_group { 1.5 } else { 0.0 },
                                );
                                if let Ok(path) = bar.build() {
                                    window.paint_path(path, status_color(group));
                                }
                                bottom = top;
                            }
                        }
                        if show_hover {
                            let x = x0 + hover_t.clamp(0.0, 1.0) * w;
                            let mut rule = PathBuilder::stroke(px(1.0));
                            rule.move_to(p(x, y0));
                            rule.line_to(p(x, y0 + h));
                            if let Ok(path) = rule.build() {
                                window.paint_path(path, with_alpha(theme::LABEL, 0.45));
                            }
                        }
                    }
                },
            )
            .w_full()
            .h_full(),
        );

    if let Some(sample) = hover_sample {
        let mut pop = div().flex().flex_col().gap(px(1.)).child(label(
            format_observe_ts(sample.ts),
            theme::FOOTNOTE,
            false,
        ));
        for (group, name) in ["1–3xx", "4xx", "5xx"].into_iter().enumerate() {
            pop = pop.child(label(
                format!(
                    "{name} {}/min",
                    nook_core::observe::format_sample(sample.values[group])
                ),
                theme::FOOTNOTE,
                group > 0,
            ));
        }
        chart = chart.child(deferred(
            div()
                .absolute()
                .bottom(px(40.))
                .when(hover_t > 0.55, |d| d.right(px(2.)))
                .when(hover_t <= 0.55, |d| {
                    d.left(relative(hover_t.clamp(0.0, 0.55)))
                })
                .rounded(px(theme::CONTROL_RADIUS))
                .bg(theme::GROUPED_BG)
                .border_1()
                .border_color(rgba_white(0.2))
                .shadow_sm()
                .px_2()
                .py(px(3.))
                .child(pop),
        ));
    }
    chart
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
                        let grid = with_alpha(theme::LABEL, 0.11);
                        for t in [0.0, 0.5, 1.0] {
                            let y = y0 + h * t;
                            let mut line = PathBuilder::stroke(px(1.0));
                            line.move_to(p(x0, y));
                            line.line_to(p(x0 + w, y));
                            if let Ok(built) = line.build() {
                                window.paint_path(built, grid);
                            }
                        }

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
                .border_color(rgba_white(0.2))
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
    use super::{monotone_beziers, scale_series, stacked_samples, StatusSeries};
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
}
