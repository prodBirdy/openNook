//! Open-Meteo weather Nook pane.
//!
//! Expanded face matches Pencil: big temp, "Condition · City", H/L, and a
//! WIND / HUMIDITY / UV column over the Pencil scene shader (12 moods, see
//! `crate::weather_shader`). A flat mood wash sits underneath as the fallback
//! when Metal is unavailable.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane, text_btn};
use crate::island::Island;
use crate::theme;
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px, AnyElement, Context, Corners,
    Rgba,
};
use nook_core::weather::{self, WeatherSnapshot};

/// Metric column caption (Pencil uses 9/12; theme Footnote floors at 10).
const METRIC_CAPTION: theme::Text = theme::Text {
    size: 9.0,
    leading: 12.0,
    weight: gpui::FontWeight::NORMAL,
    emphasized: gpui::FontWeight::SEMIBOLD,
};

pub(crate) fn weather_card(island: &mut Island, cx: &mut Context<Island>) -> impl IntoElement {
    island.ensure_weather(cx);
    let body = match (&island.weather, &island.weather_error) {
        (Some(snap), _) => forecast_body(snap).into_any_element(),
        (None, Some(_)) => div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(nook_empty("cloud", "Couldn't load weather"))
            .child(text_btn("Try Again", cx, |this, _, cx| {
                this.refresh_weather(cx);
            }))
            .into_any_element(),
        // Weather follows the Mac's location; until the first fix lands the
        // card stays in its loading state (no stale manual city).
        (None, None) if island.settings.weather.location.coords().is_none() => {
            if weather::location_error().is_some() {
                nook_empty("map-pin", "Location is off").into_any_element()
            } else {
                nook_empty("map-pin", "Locating…").into_any_element()
            }
        }
        (None, None) => nook_empty("cloud-sun", "Loading…").into_any_element(),
    };
    // The scene lives on its own full-pane layer (see mood_shell), so
    // error and empty states stay on clean island black.
    let animate = !island.reduce_motion;
    match island.weather.as_ref() {
        Some(snap) => mood_shell(snap, body, animate),
        None => card_shell("nook-weather")
            .w_full()
            .child(body)
            .into_any_element(),
    }
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).relative().p(px(16.)).gap(px(10.))
}

/// Mockup card corner radius; the scene is clipped to it.
const CARD_RADIUS: f32 = 20.0;
/// Pencil card inner stroke `#FFFFFF14`, drawn over the scene.
const CARD_STROKE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0x14 as f32 / 255.0,
};

/// The scene fills the whole pane edge to edge on an absolute layer behind
/// the padded content — never an inset box — clipped to the card radius.
/// Layers, bottom up: mood wash (fallback), shader image, content, stroke.
fn mood_shell(snap: &WeatherSnapshot, body: AnyElement, animate: bool) -> AnyElement {
    let mood = weather_mood(snap);
    let fade = Rgba {
        a: 0.0,
        ..mood.wash
    };
    div()
        .id("nook-weather")
        .relative()
        .w_full()
        .h_full()
        .min_h(px(0.))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(CARD_RADIUS))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(CARD_RADIUS))
                .bg(linear_gradient(
                    225.,
                    linear_color_stop(mood.wash, 0.),
                    linear_color_stop(fade, 0.75),
                )),
        )
        .child(scene_layer(weather_scene(snap), animate))
        .child(
            div()
                .relative()
                .w_full()
                .h_full()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .p(px(16.))
                .gap(px(10.))
                .child(body),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(CARD_RADIUS))
                .border_1()
                .border_color(CARD_STROKE),
        )
        .into_any_element()
}

/// Shader image, sized to the pane in device pixels. Keeps requesting frames
/// while it is painted and animating; once the card leaves the tree nothing
/// asks again, so the scene stops on its own. Reduce Motion paints one still.
fn scene_layer(scene: u8, animate: bool) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let scale = window.scale_factor();
            let width = (f32::from(bounds.size.width) * scale).round().max(0.0) as u32;
            let height = (f32::from(bounds.size.height) * scale).round().max(0.0) as u32;
            let Some(frame) = crate::weather_shader::render(scene, width, height, animate) else {
                // Metal is unavailable: the mood wash below stays visible.
                return;
            };
            if let Some(old) = frame.stale {
                let _ = window.drop_image(old);
            }
            let _ = window.paint_image(
                bounds,
                Corners::all(px(CARD_RADIUS)),
                frame.image,
                0,
                false,
            );
            if animate {
                window.request_animation_frame();
            }
        },
    )
    .absolute()
    .inset_0()
}

fn display_text(text: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .text_size(px(26.))
        .line_height(px(30.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme::LABEL)
        .whitespace_nowrap()
        .child(text.into())
}

fn forecast_body(snap: &WeatherSnapshot) -> impl IntoElement {
    let place = if snap.location_name.is_empty() {
        snap.label().to_string()
    } else {
        snap.location_name.clone()
    };
    let range = match (snap.high, snap.low) {
        (Some(hi), Some(lo)) => Some((weather::format_temp(hi), weather::format_temp(lo))),
        _ => None,
    };
    let condition = format!("{} · {}", snap.label(), place);
    let wind = snap
        .wind_speed
        .map(|v| format!("{} km/h", v.round() as i64))
        .unwrap_or_else(|| "—".into());
    let humidity = snap
        .humidity
        .map(|h| format!("{h}%"))
        .unwrap_or_else(|| "—".into());
    let (uv_text, uv_color) = uv_display(snap.uv_index);

    div()
        .relative()
        .w_full()
        .h_full()
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(14.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(display_text(weather::format_temp(snap.temperature)))
                .child(
                    label(condition, theme::FOOTNOTE, false)
                        .text_color(theme::TERTIARY_LABEL)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap(),
                )
                .when_some(range, |d, (hi, lo)| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                label(format!("H {hi}"), theme::FOOTNOTE, false)
                                    .text_color(theme::SECONDARY_LABEL),
                            )
                            .child(
                                label(format!("L {lo}"), theme::FOOTNOTE, false)
                                    .text_color(theme::TERTIARY_LABEL),
                            ),
                    )
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(px(7.))
                .child(metric_row("wind", "WIND", wind, theme::LABEL))
                .child(metric_row("droplets", "HUMIDITY", humidity, theme::LABEL))
                .child(metric_row("sun", "UV", uv_text, uv_color)),
        )
}

struct Mood {
    wash: Rgba,
}

/// Twelve Pencil moods: clear, cloudy, overcast, rain, storm, snow, sleet,
/// fog, wind, cold, night, heat — picked from WMO + temp/wind/day.
fn weather_mood(snap: &WeatherSnapshot) -> Mood {
    let (rgb, alpha_u8) = mood_rgba(snap);
    Mood {
        wash: theme::rgba_from_u32(rgb, alpha_u8 as f32 / 255.0),
    }
}

/// Shader scene (`u_mood`): 0 Clear · 1 Cloudy · 2 Overcast · 3 Rain ·
/// 4 Storm · 5 Snow · 6 Sleet · 7 Fog · 8 Wind · 9 Cold · 10 Night · 11 Heat.
/// Storm / heat / cold / night / wind take priority over the WMO buckets.
fn weather_scene(snap: &WeatherSnapshot) -> u8 {
    let windy = snap.wind_speed.map(|w| w >= 40.0).unwrap_or(false);
    let hot = snap.temperature >= 30.0;
    let cold = snap.temperature <= -5.0;
    let code = snap.wmo_code;

    if (95..=99).contains(&code) {
        return 4;
    }
    if hot {
        return 11;
    }
    if cold {
        return 9;
    }
    if !snap.is_day && matches!(code, 0 | 1) {
        return 10;
    }
    if windy {
        return 8;
    }
    match code {
        0 | 1 => 0,
        2 => 1,
        3 => 2,
        45 | 48 => 7,
        56 | 57 | 66 | 67 => 6,
        51..=67 | 80..=82 => 3,
        71..=77 | 85..=86 => 5,
        _ => 1,
    }
}

/// Flat wash per scene — the Metal-less fallback and the compact glyph tint.
fn mood_rgba(snap: &WeatherSnapshot) -> (u32, u8) {
    match weather_scene(snap) {
        4 => (0x7A5CFF, 0x47),  // storm
        11 => (0xFF9F0A, 0x3D), // heat
        9 => (0x64D2FF, 0x3D),  // cold
        10 => (0x5E5CE6, 0x3D), // night
        8 => (0x2BD9C4, 0x3D),  // wind
        0 => (0x3AA3FF, 0x3D),  // clear
        2 => (0x6B707A, 0x3D),  // overcast
        7 => (0x9AA3AE, 0x33),  // fog
        6 => (0x6E86B0, 0x3D),  // sleet / freezing rain
        3 => (0x4C6FFF, 0x3D),  // rain
        5 => (0xA8C8FF, 0x3D),  // snow
        _ => (0x8E9AAF, 0x33),  // cloudy
    }
}

/// UV label + tint from the WHO index bands (Pencil: Low white, else orange).
/// 0–2 Low, 3–5 Mod, 6–7 High, 8–10 High (Very High band, mockup wording),
/// 11+ Extreme. Bare `0` when the index is zero.
fn uv_display(uv_index: Option<u8>) -> (String, Rgba) {
    let Some(idx) = uv_index else {
        return ("—".into(), theme::LABEL);
    };
    if idx == 0 {
        return ("0".into(), theme::LABEL);
    }
    let (word, color) = uv_level(idx);
    (format!("{idx} · {word}"), color)
}

fn uv_level(idx: u8) -> (&'static str, Rgba) {
    if idx <= 2 {
        ("Low", theme::LABEL)
    } else if idx <= 5 {
        ("Mod", theme::SYSTEM_ORANGE)
    } else if idx <= 10 {
        // 6–7 High, 8–10 Very High — mockup still prints "High".
        ("High", theme::SYSTEM_ORANGE)
    } else {
        ("Extreme", theme::SYSTEM_ORANGE)
    }
}

fn metric_row(
    icon: &'static str,
    name: &'static str,
    value: impl Into<gpui::SharedString>,
    value_color: gpui::Rgba,
) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(lucide_color(icon, 11.0, theme::TERTIARY_LABEL))
        .child(
            label(name, METRIC_CAPTION, false)
                .flex_1()
                .min_w(px(0.))
                .text_color(theme::TERTIARY_LABEL),
        )
        .child(
            label(value, theme::FOOTNOTE, true)
                .text_color(value_color)
                .flex_shrink_0(),
        )
}

fn compact_icon_tint(snap: &WeatherSnapshot) -> Rgba {
    // Compact cloudy/clear day glyph reads orange in Pencil; otherwise mood wash.
    if snap.is_day && matches!(snap.wmo_code, 0 | 1 | 2) {
        return theme::SYSTEM_ORANGE;
    }
    let (rgb, _) = mood_rgba(snap);
    theme::rgba_from_u32(rgb, 1.0)
}

pub(crate) fn compact_weather(island: &Island) -> AnyElement {
    let Some(snap) = island.weather.as_ref() else {
        return div().into_any_element();
    };
    if !island.settings.weather.enabled || !island.settings.weather.show_on_compact_face {
        return div().into_any_element();
    }
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color(snap.icon(), 17.0, compact_icon_tint(snap))),
        )
        .child(label(
            weather::format_temp(snap.temperature),
            theme::BODY,
            true,
        ))
        .into_any_element()
}

impl Island {
    pub(crate) fn weather_visible(&self) -> bool {
        if !self.settings.weather.enabled {
            return false;
        }
        if self.expanded {
            return true;
        }
        self.settings.weather.show_on_compact_face
    }

    pub(crate) fn ensure_weather(&mut self, cx: &mut Context<Self>) {
        if self.weather_inflight {
            return;
        }
        if !self.settings.weather.enabled {
            return;
        }
        if self.settings.weather.location.coords().is_none() {
            return;
        }
        if weather::is_fresh_for(&self.settings.weather) && self.weather.is_some() {
            return;
        }
        self.refresh_weather(cx);
    }

    pub(crate) fn refresh_weather(&mut self, cx: &mut Context<Self>) {
        if self.weather_inflight {
            return;
        }
        let settings = self.settings.weather.clone();
        if settings.location.coords().is_none() {
            return;
        }
        self.weather_inflight = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { nook_core::runtime().block_on(weather::fetch(&settings)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.weather_inflight = false;
                match result {
                    Ok(snap) => {
                        this.weather = Some(snap);
                        this.weather_error = None;
                    }
                    Err(err) => {
                        if this.weather.is_none() {
                            this.weather_error = Some(err);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(code: u8, temp: f64, is_day: bool, wind: Option<f64>) -> WeatherSnapshot {
        WeatherSnapshot {
            location_name: String::new(),
            latitude: 0.0,
            longitude: 0.0,
            units: Default::default(),
            temperature: temp,
            feels_like: temp,
            wmo_code: code,
            is_day,
            high: None,
            low: None,
            precip_probability: None,
            wind_speed: wind,
            humidity: None,
            uv_index: None,
            hourly: Vec::new(),
            fetched_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn scene_priority_branches() {
        // Storm beats heat, cold, night and wind.
        assert_eq!(weather_scene(&snap(95, 35.0, false, Some(60.0))), 4);
        assert_eq!(weather_scene(&snap(99, -10.0, true, None)), 4);
        // Heat at 30° and above, before cold / night / wind.
        assert_eq!(weather_scene(&snap(0, 30.0, false, Some(60.0))), 11);
        // Cold at -5° and below.
        assert_eq!(weather_scene(&snap(3, -5.0, true, Some(60.0))), 9);
        // Night only for clear codes after dark.
        assert_eq!(weather_scene(&snap(0, 12.0, false, None)), 10);
        assert_eq!(weather_scene(&snap(1, 12.0, false, Some(60.0))), 10);
        assert_eq!(weather_scene(&snap(3, 12.0, false, None)), 2);
        // Wind at 40 km/h and above, over the WMO buckets.
        assert_eq!(weather_scene(&snap(0, 12.0, true, Some(40.0))), 8);
        assert_eq!(weather_scene(&snap(61, 12.0, true, Some(45.0))), 8);
        assert_eq!(weather_scene(&snap(0, 12.0, true, Some(39.9))), 0);
    }

    #[test]
    fn scene_wmo_buckets() {
        let day = |code| weather_scene(&snap(code, 12.0, true, None));
        assert_eq!(day(0), 0);
        assert_eq!(day(1), 0);
        assert_eq!(day(2), 1);
        assert_eq!(day(3), 2);
        assert_eq!(day(45), 7);
        assert_eq!(day(48), 7);
        for code in [56, 57, 66, 67] {
            assert_eq!(day(code), 6, "sleet {code}");
        }
        for code in [51, 53, 55, 61, 63, 65, 80, 81, 82] {
            assert_eq!(day(code), 3, "rain {code}");
        }
        for code in [71, 73, 75, 77, 85, 86] {
            assert_eq!(day(code), 5, "snow {code}");
        }
        assert_eq!(day(42), 1);
    }

    #[test]
    fn fallback_wash_follows_the_scene() {
        assert_eq!(mood_rgba(&snap(95, 12.0, true, None)), (0x7A5CFF, 0x47));
        assert_eq!(mood_rgba(&snap(0, 12.0, false, None)), (0x5E5CE6, 0x3D));
        assert_eq!(mood_rgba(&snap(2, 12.0, true, None)), (0x8E9AAF, 0x33));
    }
}
