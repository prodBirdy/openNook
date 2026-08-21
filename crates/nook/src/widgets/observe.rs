//! Observe card: warmUP `/admin/metrics` plus optional Prometheus PromQL.
//!
//! Mini charts follow TanStack Charts' KPI sparkline language: monotone cubic
//! lines, faint y-grid, fading area fill, round stroke, end dot, and rounded
//! bars from a zero baseline.

use crate::island::ui::{label, slide_label, widget_shell};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, div, point, prelude::*, px, relative, Bounds, Context, CursorStyle, MouseMoveEvent,
    PathBuilder, Pixels, Rgba, SharedString,
};
use nook_core::observe::{ChartPoint, ObserveChartKind, ObserveSnapshot, ObserveWindow};
use nook_core::settings::AppSettings;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObserveHover {
    pub query: String,
    pub t: f32,
    pub value: f64,
}

pub(crate) fn observe_card(
    snap: &ObserveSnapshot,
    settings: &AppSettings,
    hover: Option<&ObserveHover>,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_1();
    let url = settings.observe.prometheus_url.trim();
    if url.is_empty() {
        body = body
            .child(label("No metrics URL", theme::CALLOUT, true))
            .child(label(
                "Set one in Settings to pin samples and see firing 5xx.",
                theme::SUBHEADLINE,
                false,
            ));
    } else {
        if let Some(err) = &snap.error {
            body = body.child(slide_label(err.clone(), theme::SUBHEADLINE, false).w_full());
        }
        if snap.has_outage() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(8.)).rounded_full().bg(theme::DESTRUCTIVE))
                    .child(label(
                        format!("{} firing", snap.firing_count()),
                        theme::CALLOUT,
                        true,
                    )),
            );
            for alert in snap.alerts.iter().take(2) {
                let line = if alert.summary.is_empty() {
                    alert.name.clone()
                } else {
                    format!("{} — {}", alert.name, alert.summary)
                };
                body = body.child(slide_label(line, theme::CALLOUT, true).w_full());
            }
        } else if snap.connected {
            body = body.child(label("No firing alerts", theme::SUBHEADLINE, false));
        }
        body = body.child(label(
            format!("Last {}", settings.observe.window.caption()),
            theme::SUBHEADLINE,
            false,
        ));
        for reading in snap.metrics.iter() {
            let value = reading.headline();
            let mut block = div().flex().flex_col().gap(px(3.)).w_full().child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap_2()
                    .child(
                        label(reading.label.clone(), theme::SUBHEADLINE, false)
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(label(value, theme::CALLOUT, true)),
            );
            if reading.chart != ObserveChartKind::Off {
                block = block.child(mini_chart(
                    reading.query.clone(),
                    reading.chart,
                    reading.history.clone(),
                    chart_color(&reading.query),
                    hover.filter(|h| h.query == reading.query),
                    settings.observe.window,
                    cx,
                ));
            }
            body = body.child(block);
        }
    }
    widget_shell("observe-scroll", body)
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

fn with_alpha(color: Rgba, a: f32) -> Rgba {
    Rgba { a, ..color }
}

fn mini_chart(
    query: String,
    kind: ObserveChartKind,
    series: Vec<ChartPoint>,
    color: Rgba,
    hover: Option<&ObserveHover>,
    window: ObserveWindow,
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let bounds_cell = Rc::new(RefCell::new(None::<Bounds<Pixels>>));
    let hover_pt = hover.and_then(|h| nook_core::observe::nearest_chart_point(&series, h.t));
    let tooltip = hover_pt.map(|pt| {
        format!(
            "{} · {}",
            nook_core::observe::format_chart_sample(&query, pt.value),
            nook_core::observe::format_chart_age(pt.t, window)
        )
    });
    let tooltip_t = hover_pt.map(|pt| pt.t).unwrap_or(0.0);

    let mut chart = div()
        .id(SharedString::from(format!("chart-{query}")))
        .relative()
        .flex()
        .w_full()
        .h(px(32.))
        .flex_shrink_0()
        .cursor(CursorStyle::Crosshair)
        .on_mouse_move({
            let bounds_cell = bounds_cell.clone();
            let series = series.clone();
            let query = query.clone();
            cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let Some(bounds) = *bounds_cell.borrow() else {
                    return;
                };
                let width: f32 = bounds.size.width.into();
                if width < 1.0 || series.is_empty() {
                    return;
                }
                let x: f32 = event.position.x.into();
                let origin: f32 = bounds.origin.x.into();
                let t = ((x - origin) / width).clamp(0.0, 1.0);
                let Some(pt) = nook_core::observe::nearest_chart_point(&series, t) else {
                    return;
                };
                let next = ObserveHover {
                    query: query.clone(),
                    t: pt.t,
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
                    let series = series.clone();
                    let hover_pt = hover_pt;
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
                        let values: Vec<f64> = series.iter().map(|pt| pt.value).collect();
                        let ys = scale_series(&values, from_zero);
                        if ys.is_empty() {
                            return;
                        }
                        let y_at = |t: f32| y0 + h * (1.0 - t);
                        let pts: Vec<(f32, f32)> = series
                            .iter()
                            .zip(ys.iter())
                            .map(|(pt, y)| (x0 + pt.t.clamp(0.0, 1.0) * w, y_at(*y)))
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
                        if let Some(hover) = hover_pt {
                            let hx = x0 + hover.t.clamp(0.0, 1.0) * w;
                            if let Some((_, hy)) = series
                                .iter()
                                .zip(pts.iter())
                                .find(|(pt, _)| (pt.t - hover.t).abs() < 1e-4)
                            {
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

    if let Some(text) = tooltip {
        let align_right = tooltip_t > 0.55;
        chart = chart.child(
            div()
                .absolute()
                .top(px(1.))
                .when(align_right, |d| d.right(px(2.)))
                .when(!align_right, |d| {
                    d.left(relative(tooltip_t.clamp(0.0, 0.55)))
                })
                .rounded(px(theme::CONTROL_RADIUS))
                .bg(theme::GROUPED_BG)
                .px_2()
                .py(px(1.))
                .child(label(text, theme::FOOTNOTE, true)),
        );
    }

    chart
}

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
    append_monotone(&mut area, &pts);
    let end = *pts.last().unwrap();
    area.line_to(p(end.0, baseline));
    area.close();
    if let Ok(built) = area.build() {
        window.paint_path(built, with_alpha(color, 0.22));
    }

    let mut glow = PathBuilder::fill();
    glow.move_to(p(pts[0].0, (pts[0].1 + baseline) * 0.5));
    glow.line_to(p(pts[0].0, pts[0].1));
    append_monotone(&mut glow, &pts);
    glow.line_to(p(end.0, (end.1 + baseline) * 0.5));
    glow.close();
    if let Ok(built) = glow.build() {
        window.paint_path(built, with_alpha(color, 0.16));
    }

    let mut line = PathBuilder::stroke(px(2.0));
    line.move_to(p(pts[0].0, pts[0].1));
    append_monotone(&mut line, &pts);
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
    let k = 0.552_284_75 * r;
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
    let k = 0.552_284_75 * r;
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
fn monotone_beziers(pts: &[(f32, f32)]) -> Vec<((f32, f32), (f32, f32), (f32, f32))> {
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
    use super::{monotone_beziers, scale_series};

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
}
