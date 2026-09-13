//! Open-Meteo weather Nook pane.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_display, nook_empty, nook_pane, text_btn};
use crate::island::Island;
use crate::theme;
use gpui::{div, prelude::*, px, AnyElement, Context};
use nook_core::weather::{self, WeatherSnapshot};

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
        (None, None) if island.settings.weather.location.coords().is_none() => {
            nook_empty("map-pin", "Choose a city in Settings").into_any_element()
        }
        (None, None) => nook_empty("cloud-sun", "Loading…").into_any_element(),
    };
    nook_pane("nook-weather").w_full().child(body)
}

fn forecast_body(snap: &WeatherSnapshot) -> impl IntoElement {
    let place = if snap.location_name.is_empty() {
        snap.label().to_string()
    } else {
        snap.location_name.clone()
    };
    let hi_lo = match (snap.high, snap.low) {
        (Some(hi), Some(lo)) => format!(
            "H {}  L {}",
            weather::format_temp(hi),
            weather::format_temp(lo)
        ),
        _ => snap.label().to_string(),
    };
    let mut hours = div()
        .flex()
        .items_end()
        .justify_between()
        .w_full()
        .gap(px(6.));
    for (i, hour) in snap.hourly.iter().take(6).enumerate() {
        hours = hours.child(hour_col(i, hour, hour_is_day(hour)));
    }
    div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .justify_between()
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(lucide_color(snap.icon(), 22.0, theme::LABEL))
                        .child(nook_display(weather::format_temp(snap.temperature))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(px(1.))
                        .child(label(place, theme::CALLOUT, true))
                        .child(label(hi_lo, theme::SUBHEADLINE, false)),
                ),
        )
        .child(hours)
        .child(label("Open-Meteo", theme::FOOTNOTE, false).text_color(theme::TERTIARY_LABEL))
}

fn hour_is_day(hour: &nook_core::weather::HourlyForecast) -> bool {
    // Snapshot has no sunrise/sunset; treat 06:00–20:00 local as day.
    hour.hour
        .parse::<u32>()
        .map(|h| (6..20).contains(&h))
        .unwrap_or(true)
}

fn hour_col(
    index: usize,
    hour: &nook_core::weather::HourlyForecast,
    is_day: bool,
) -> impl IntoElement {
    div()
        .id(("wx-h", index))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.))
        .child(label(hour.hour.clone(), theme::FOOTNOTE, false).text_color(theme::TERTIARY_LABEL))
        .child(lucide_color(
            weather::wmo_icon(hour.wmo_code, is_day),
            12.0,
            theme::SECONDARY_LABEL,
        ))
        .child(label(
            weather::format_temp(hour.temperature),
            theme::SUBHEADLINE,
            true,
        ))
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
        .gap(px(6.))
        .child(lucide_color(snap.icon(), 16.0, theme::LABEL))
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
