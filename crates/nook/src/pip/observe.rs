//! Observe analytics floating window.

use super::chrome;
use super::PipView;
use crate::icons::lucide_color;
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, AnyElement, Context, Entity, FontWeight, Rgba, Window};
use nook_core::observe::{MetricReading, ObserveSourceKind, ObserveWindow};

pub use super::chrome::OBSERVE_W;

const PAD_TOP: f32 = 10.0;
const PAD_BOTTOM: f32 = 12.0;
const SECTION_GAP: f32 = 10.0;
const HEADER_H: f32 = 22.0;
const LABEL_H: f32 = 14.0;
const VALUE_H: f32 = 24.0;
const SPARK_H: f32 = 18.0;
const DELTA_H: f32 = 13.0;
const TILE_PAD: f32 = 8.0;
const TILE_GAP: f32 = 2.0;
const TILE_H: f32 =
    TILE_PAD * 2.0 + LABEL_H + TILE_GAP + VALUE_H + TILE_GAP + SPARK_H + TILE_GAP + DELTA_H;
const ERROR_H: f32 = 16.0;
const ROUTE_H: f32 = 15.0;
const SPARK_BARS: usize = 16;
const BAR_W: f32 = 2.0;
const BAR_GAP: f32 = 1.5;

pub fn render(
    view: &PipView,
    island: &Entity<Island>,
    window: &mut Window,
    cx: &mut Context<PipView>,
) -> AnyElement {
    let height = chrome::desired_height(view, island.read(cx));
    chrome::note_content_height(view, height, window, cx);
    let body = body_kind(island.read(cx));
    let header_el = header(view, island, cx);
    let mut column = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(SECTION_GAP))
        .child(header_el);
    match body {
        Body::Error(text) => {
            column = column.child(caption_row(text));
        }
        Body::Metrics { tiles, route } => {
            if tiles {
                column = column.child(tiles_row(island.read(cx)));
            }
            if route {
                let metric = busiest_route(&island.read(cx).observe.metrics).cloned();
                if let Some(metric) = metric {
                    column = column.child(route_row(&metric));
                }
            }
        }
    }
    column.into_any_element()
}

pub fn content_height(view: &PipView, island: &Island) -> f32 {
    let _ = view;
    let mut height = PAD_TOP + HEADER_H + PAD_BOTTOM;
    match body_kind(island) {
        Body::Error(_) => height += SECTION_GAP + ERROR_H,
        Body::Metrics { tiles, route } => {
            if tiles {
                height += SECTION_GAP + TILE_H;
            }
            if route {
                height += SECTION_GAP + ROUTE_H;
            }
        }
    }
    height
}

pub fn desired_height(view: &PipView, island: &Island) -> f32 {
    content_height(view, island)
}

enum Body {
    Error(String),
    Metrics { tiles: bool, route: bool },
}

fn body_kind(island: &Island) -> Body {
    if let Some(err) = island
        .observe
        .error
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        return Body::Error(err.to_string());
    }
    let tiles = !tile_metrics(&island.observe.metrics).is_empty();
    let route = busiest_route(&island.observe.metrics).is_some();
    Body::Metrics { tiles, route }
}

fn header(view: &PipView, island: &Entity<Island>, cx: &mut Context<PipView>) -> impl IntoElement {
    let config = nook_core::settings::get_app_settings().observe;
    let live = island.read(cx).observe.connected;
    let reduce_motion = island.read(cx).reduce_motion;
    div()
        .w_full()
        .h(px(HEADER_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(lucide_color("radar", 14.0, theme::SUCCESS))
        .child(
            div()
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_size(px(13.))
                .line_height(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child("Observe"),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(theme::SUBHEADLINE.size))
                .line_height(px(theme::SUBHEADLINE.leading))
                .font_weight(FontWeight::NORMAL)
                .text_color(theme::tertiary_label())
                .child(source_caption(config.source, config.window)),
        )
        .child(live_badge(live))
        .when(view.hovered, |row| {
            row.child(chrome::hover_controls(view, reduce_motion, cx))
        })
}

fn source_caption(source: ObserveSourceKind, window: ObserveWindow) -> String {
    format!("{} · {}", source_name(source), window.caption())
}

fn source_name(source: ObserveSourceKind) -> &'static str {
    match source {
        ObserveSourceKind::Warmup => "warmUP",
        ObserveSourceKind::Prometheus => "Prometheus",
        ObserveSourceKind::Grafana => "Grafana",
        ObserveSourceKind::Alertmanager => "Alertmanager",
        ObserveSourceKind::FmObserve => "fm-observe",
    }
}

fn live_badge(connected: bool) -> impl IntoElement {
    let color = if connected {
        theme::SUCCESS
    } else {
        theme::tertiary_label()
    };
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(div().size(px(6.)).rounded_full().flex_shrink_0().bg(color))
        .child(
            div()
                .whitespace_nowrap()
                .text_size(px(11.))
                .line_height(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(if connected { "Live" } else { "Offline" }),
        )
}

fn caption_row(text: String) -> impl IntoElement {
    div()
        .w_full()
        .h(px(ERROR_H))
        .flex_shrink_0()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_size(px(13.))
        .line_height(px(16.))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme::tertiary_label())
        .child(text)
}

fn tiles_row(island: &Island) -> impl IntoElement {
    let mut row = div().w_full().flex().flex_row().gap(px(8.)).flex_shrink_0();
    for metric in tile_metrics(&island.observe.metrics) {
        row = row.child(tile(metric));
    }
    row
}

fn tile(metric: &MetricReading) -> impl IntoElement {
    let color = bar_color(metric);
    let (delta, delta_color) = delta_caption(metric);
    div()
        .flex_1()
        .min_w(px(0.))
        .rounded(px(10.))
        .bg(theme::FILL_TERTIARY)
        .p(px(TILE_PAD))
        .flex()
        .flex_col()
        .gap(px(TILE_GAP))
        .child(
            div()
                .h(px(LABEL_H))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(11.))
                .line_height(px(LABEL_H))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::tertiary_label())
                .child(metric.label.clone()),
        )
        .child(
            div()
                .h(px(VALUE_H))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(20.))
                .line_height(px(VALUE_H))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(metric.headline()),
        )
        .child(sparkline(metric, color))
        .child(
            div()
                .h(px(DELTA_H))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(10.))
                .line_height(px(DELTA_H))
                .font_weight(FontWeight::MEDIUM)
                .text_color(delta_color)
                .child(delta),
        )
}

fn sparkline(metric: &MetricReading, color: Rgba) -> impl IntoElement {
    let values = spark_tail(metric);
    let max = values.iter().copied().fold(0.0_f32, f32::max);
    let mut row = div()
        .w_full()
        .h(px(SPARK_H))
        .flex_shrink_0()
        .flex()
        .items_end()
        .gap(px(BAR_GAP));
    for value in values {
        let height = bar_height(value, max);
        row = row.child(
            div()
                .w(px(BAR_W))
                .h(px(height))
                .flex_shrink_0()
                .rounded(px(1.))
                .bg(color)
                .opacity(0.85),
        );
    }
    row
}

fn route_row(metric: &MetricReading) -> impl IntoElement {
    div()
        .w_full()
        .h(px(ROUTE_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(12.))
                .line_height(px(ROUTE_H))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::secondary_label())
                .child(metric.label.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_size(px(12.))
                .line_height(px(ROUTE_H))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::LABEL)
                .child(metric.headline()),
        )
}

fn tile_metrics(metrics: &[MetricReading]) -> Vec<&MetricReading> {
    metrics
        .iter()
        .filter(|metric| !metric.query.starts_with("route:"))
        .take(3)
        .collect()
}

fn busiest_route(metrics: &[MetricReading]) -> Option<&MetricReading> {
    metrics
        .iter()
        .filter(|metric| metric.query.starts_with("route:"))
        .max_by(|a, b| {
            route_value(a)
                .partial_cmp(&route_value(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn route_value(metric: &MetricReading) -> f64 {
    metric
        .window_total
        .or_else(|| metric.last_value())
        .unwrap_or(f64::NEG_INFINITY)
}

fn spark_tail(metric: &MetricReading) -> Vec<f32> {
    let values = metric.sparkline_values();
    let start = values.len().saturating_sub(SPARK_BARS);
    values[start..]
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect()
}

fn bar_height(value: f32, max: f32) -> f32 {
    if max <= f32::EPSILON {
        return 2.0;
    }
    let height = (value / max * SPARK_H).clamp(0.0, SPARK_H);
    if value > 0.0 {
        height.max(2.0)
    } else {
        height
    }
}

fn bar_color(metric: &MetricReading) -> Rgba {
    if is_error_metric(metric) {
        theme::DESTRUCTIVE
    } else if is_latency_metric(metric) {
        theme::accent()
    } else {
        theme::LABEL
    }
}

fn is_error_metric(metric: &MetricReading) -> bool {
    let label = metric.label.to_ascii_lowercase();
    let query = metric.query.to_ascii_lowercase();
    contains_error(&label) || contains_error(&query)
}

fn contains_error(text: &str) -> bool {
    text.contains("5xx") || text.contains("error")
}

fn is_latency_metric(metric: &MetricReading) -> bool {
    metric.label.to_ascii_lowercase().contains("ms")
        || metric.query.to_ascii_lowercase().contains("ms")
}

fn delta_caption(metric: &MetricReading) -> (String, Rgba) {
    let values = spark_tail(metric);
    if is_error_metric(metric) {
        if values.is_empty() {
            return ("—".into(), theme::tertiary_label());
        }
        let spikes = spike_count(&values);
        let text = if spikes == 1 {
            "1 spike".into()
        } else {
            format!("{spikes} spikes")
        };
        let color = if spikes > 0 {
            theme::DESTRUCTIVE
        } else {
            theme::tertiary_label()
        };
        return (text, color);
    }
    if is_latency_metric(metric) {
        let text = match percentile(values, 0.95) {
            Some(value) => format!("p95 {}", nook_core::observe::format_sample(value as f64)),
            None => "p95 —".into(),
        };
        return (text, theme::tertiary_label());
    }
    match percent_change(&values) {
        Some(pct) if pct >= 0.5 => (format!("+{pct:.0}%"), theme::SUCCESS),
        Some(pct) if pct <= -0.5 => (format!("{pct:.0}%"), theme::DESTRUCTIVE),
        Some(_) => ("0%".into(), theme::tertiary_label()),
        None => ("—".into(), theme::tertiary_label()),
    }
}

fn spike_count(values: &[f32]) -> usize {
    let mid = median(values);
    let threshold = mid * 2.0;
    values
        .iter()
        .filter(|value| **value > 0.0 && **value > threshold)
        .count()
}

fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) * 0.5
    } else {
        sorted[mid]
    }
}

fn percentile(values: Vec<f32>, p: f32) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted.get(idx.min(sorted.len() - 1)).copied()
}

fn percent_change(values: &[f32]) -> Option<f32> {
    let first = *values.first()?;
    let last = *values.last()?;
    if !first.is_finite() || !last.is_finite() || first.abs() < f32::EPSILON {
        return None;
    }
    Some((last - first) / first.abs() * 100.0)
}
