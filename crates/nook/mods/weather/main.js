// Weather from Open-Meteo (keyless) for the Mac's location, or props.city. The page is the
// 1.x card: big temperature, condition · place, high and low, wind / humidity / UV, over an
// animated sky (scene.wgsl, twelve moods). Closed, it shows the temperature beside the camera
// when no other mod has something to say.
// props: { city: "Berlin" | null, units: "celsius" | "fahrenheit", compact: true }

import { View, row, col, div, text, icon, image, shader } from "nook/ui";
import { location } from "nook";

const STALE_MS = 30 * 60 * 1000;
// Mean synodic month and a known new moon (2000-01-06 18:14 UTC), for the night sky's phase.
const SYNODIC_DAYS = 29.530588853;
const NEW_MOON = Date.UTC(2000, 0, 6, 18, 14);
const RETRY_MS = 60 * 1000;

const LABEL = {
  0: "Clear", 1: "Mostly clear", 2: "Partly cloudy", 3: "Overcast", 45: "Fog", 48: "Rime fog",
  51: "Light drizzle", 53: "Drizzle", 55: "Heavy drizzle", 56: "Freezing drizzle", 57: "Heavy freezing drizzle",
  61: "Light rain", 63: "Rain", 65: "Heavy rain", 66: "Freezing rain", 67: "Heavy freezing rain",
  71: "Light snow", 73: "Snow", 75: "Heavy snow", 77: "Snow grains", 80: "Light showers", 81: "Showers",
  82: "Heavy showers", 85: "Light snow showers", 86: "Snow showers", 95: "Thunderstorm",
  96: "Thunderstorm + hail", 99: "Severe thunderstorm",
};

// The moon's age now as a fraction of its cycle: 0 new, 0.25 first quarter, 0.5 full.
function moonPhase(now = Date.now()) {
  const cycles = (now - NEW_MOON) / 86400000 / SYNODIC_DAYS;
  return cycles - Math.floor(cycles);
}

// The sky's mood: storm, heat, cold and wind beat the WMO buckets (as 1.x picked them).
function mood(w) {
  const c = w.code;
  if (c >= 95 && c <= 99) return 4;
  if (w.celsius >= 30) return 11;
  if (w.celsius <= -5) return 9;
  if ((w.wind ?? 0) >= 40) return 8;
  if (c === 0 || c === 1) return 0;
  if (c === 2) return 1;
  if (c === 3) return 2;
  if (c === 45 || c === 48) return 7;
  if ([56, 57, 66, 67].includes(c)) return 6;
  if ((c >= 51 && c <= 67) || (c >= 80 && c <= 82)) return 3;
  if ((c >= 71 && c <= 77) || c === 85 || c === 86) return 5;
  return 1;
}

// A flat wash under the sky, the color the card shows before the shader's first frame.
const WASH = ["#3aa3ff3d", "#8e9aaf33", "#6b707a3d", "#4c6fff3d", "#7a5cff47", "#a8c8ff3d", "#6e86b03d", "#9aa3ae33", "#2bd9c43d", "#64d2ff3d", "#5e5ce63d", "#ff9f0a3d"];

const temp = (v) => (v == null ? "–" : `${Math.round(v)}°`);

function uv(index) {
  if (index == null) return ["—", "fg"];
  if (index === 0) return ["0", "fg"];
  const word = index <= 2 ? "Low" : index <= 5 ? "Mod" : index <= 10 ? "High" : "Extreme";
  return [`${index} · ${word}`, index <= 2 ? "fg" : "orange"];
}

export default class Weather extends View {
  init(props, cx) {
    this.city = props.city || null;
    this.units = props.units === "fahrenheit" ? "fahrenheit" : "celsius";
    this.compact = props.compact !== false;
    this.weather = JSON.parse(localStorage.getItem("weather") ?? "null");
    this.error = null;
    this.busy = false;
    this.nextTry = 0;
    // Refresh when stale; until the first answer, check each second for a location.
    cx.timer.every(1000, (cx) => {
      const due = !this.weather || Date.now() - this.weather.at > STALE_MS || this.weather.units !== this.units;
      if (due && !this.busy && Date.now() >= this.nextTry) cx.spawn((cx) => this.refresh(cx));
    });
  }

  async place() {
    if (this.city) {
      const cached = JSON.parse(localStorage.getItem("place") ?? "null");
      if (cached?.query === this.city) return cached;
      const r = await (await fetch(`https://geocoding-api.open-meteo.com/v1/search?count=1&name=${encodeURIComponent(this.city)}`)).json();
      const hit = r.results?.[0];
      if (!hit) throw new Error(`No place called ${this.city}`);
      const place = { query: this.city, latitude: hit.latitude, longitude: hit.longitude, name: hit.name };
      localStorage.setItem("place", JSON.stringify(place));
      return place;
    }
    const loc = location();
    if (loc === null) return null; // still being found
    if (loc.error) throw new Error(loc.error);
    return loc;
  }

  async refresh(cx) {
    this.busy = true;
    try {
      const place = await this.place();
      if (!place) return;
      const url = "https://api.open-meteo.com/v1/forecast"
        + `?latitude=${place.latitude}&longitude=${place.longitude}`
        + "&current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,is_day,relative_humidity_2m,uv_index"
        + "&daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto"
        + `&temperature_unit=${this.units}`;
      const r = await fetch(url);
      if (!r.ok) throw new Error(`Open-Meteo answered ${r.status}`);
      const j = await r.json();
      const c = j.current;
      const f = this.units === "fahrenheit";
      this.weather = {
        at: Date.now(),
        units: this.units,
        place: place.name ?? null,
        lat: place.latitude ?? null,
        temperature: c.temperature_2m,
        celsius: f ? (c.temperature_2m - 32) / 1.8 : c.temperature_2m,
        code: c.weather_code,
        day: c.is_day === 1,
        wind: c.wind_speed_10m,
        humidity: c.relative_humidity_2m,
        uv: c.uv_index == null ? null : Math.round(c.uv_index),
        high: j.daily?.temperature_2m_max?.[0] ?? null,
        low: j.daily?.temperature_2m_min?.[0] ?? null,
      };
      localStorage.setItem("weather", JSON.stringify(this.weather));
      this.error = null;
    } catch (e) {
      this.error = String(e.message ?? e);
      this.nextTry = Date.now() + RETRY_MS;
    } finally {
      this.busy = false;
      cx.notify();
    }
  }

  activity() {
    const w = this.weather;
    if (!this.compact || !w) return null;
    return {
      leading: image("app:com.apple.weather").id("app").size(20),
      trailing: text(temp(w.temperature)).text_size(13).font_medium().tabular_nums(),
      detail: text(`${LABEL[w.code] ?? ""}${w.place ? ` · ${w.place}` : ""}`).text_size(12).text_color("muted").truncate(),
      // Below everything: the island is the weather's only when it's otherwise quiet.
      priority: 1,
    };
  }

  expanded() {
    const w = this.weather;
    if (!w) {
      const waiting = this.error ?? (this.city ? "Loading weather…" : "Finding your location…");
      return row().id("empty").gap(10).py(14)
        .child(icon(this.error ? "map-pin" : "cloud", 18).color("faint"))
        .child(text(waiting).text_color("muted").truncate().flex_1());
    }
    const scene = mood(w);
    const label = !w.day && (w.code === 0 || w.code === 1) ? "Clear night" : LABEL[w.code] ?? "";
    const [uvText, uvColor] = uv(w.uv);
    const metric = (name, caption, value, color = "fg") => row().gap(7)
      .child(icon(name, 13).color("#ffffffb3"))
      .child(col().gap(0)
        .child(text(caption).text_size(9).font_semibold().text_color("#ffffff80"))
        .child(text(value).text_size(12).font_medium().text_color(color).tabular_nums()));
    return div().id("card").w_full().h(128).rounded(22).clip().bg(WASH[scene])
      .child(shader("./scene.wgsl", [scene, w.day ? 0 : 1, 1, 0.35, moonPhase(), (w.lat ?? 0) < 0 ? -1 : 1]).id("sky").absolute_fill().rounded(22))
      .child(
        row().id("face").size_full().p(16).gap(14).items_center()
          .child(col().flex_1().gap(4)
            .child(text(temp(w.temperature)).text_size(26).font_semibold().tabular_nums())
            .child(text(`${label}${w.place ? ` · ${w.place}` : ""}`).text_size(12).text_color("#ffffffb3").truncate())
            .when(w.high != null && w.low != null, (c) => c.child(row().gap(6)
              .child(text(`H ${temp(w.high)}`).text_size(12).text_color("#ffffffd9"))
              .child(text(`L ${temp(w.low)}`).text_size(12).text_color("#ffffff99")))))
          .child(col().gap(7)
            .child(metric("wind", "WIND", w.wind == null ? "—" : `${Math.round(w.wind)} km/h`))
            .child(metric("droplets", "HUMIDITY", w.humidity == null ? "—" : `${w.humidity}%`))
            .child(metric("sun", "UV", uvText, uvColor))),
      );
  }
}
