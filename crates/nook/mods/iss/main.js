// The International Space Station on the world-clock's dot map (lit where the sun is up): where
// it is now, one orbit of its path (the last 20 minutes dim, the rest of the lap orange), and
// its altitude, speed and sunlight. From wheretheiss.at (keyless, about one request a second
// allowed); it's only asked while this page is open. Between answers the station glides along
// the fetched path: positions a few minutes apart, joined along great circles.
// props: { units: "metric" | "imperial" }

import { View, row, col, div, text, icon, shader } from "nook/ui";
import { island, location } from "nook";

const API = "https://api.wheretheiss.at/v1/satellites/25544";
// The map's latitudes, as world-clock draws them; the ISS stays within ±52°.
const TOP = 78;
const BOTTOM = -56;
const POLL_MS = 5000;
// One orbit takes about 92.7 minutes; the map shows exactly one, so it never overlaps the
// lap before. Fetched every 3 minutes, with half an hour to spare ahead.
const ORBIT_S = 92.7 * 60;
const PAST_S = 20 * 60;
const AHEAD_S = ORBIT_S - PAST_S;
const SPARE_S = 30 * 60;
const STEP_S = 180;
const EARTH_KM = 6371;

// Where the sun is overhead now (as world-clock works it out), for the map's day and night.
function sun(now) {
  const start = Date.UTC(now.getUTCFullYear(), 0, 0);
  const day = (now.getTime() - start) / 86400000;
  const dec = -23.44 * Math.cos((2 * Math.PI / 365) * (day + 10));
  const b = (2 * Math.PI * (day - 81)) / 364;
  const eot = 9.87 * Math.sin(2 * b) - 7.53 * Math.cos(b) - 1.5 * Math.sin(b);
  const hours = now.getUTCHours() + now.getUTCMinutes() / 60 + now.getUTCSeconds() / 3600;
  let lon = -15 * (hours - 12 + eot / 60);
  lon = ((lon + 540) % 360) - 180;
  return { dec, lon };
}

// QuickJS has no Intl: thousands by hand.
const grouped = (n) => String(Math.round(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ",");

function km(a, b) {
  const r = Math.PI / 180;
  const h = Math.sin(((b.lat - a.lat) * r) / 2) ** 2
    + Math.cos(a.lat * r) * Math.cos(b.lat * r) * Math.sin(((b.lon - a.lon) * r) / 2) ** 2;
  return 2 * EARTH_KM * Math.asin(Math.min(1, Math.sqrt(h)));
}

const point = (j) => ({ lat: j.latitude, lon: j.longitude, at: j.timestamp });

const RAD = Math.PI / 180;
const toVec = (p) => [Math.cos(p.lat * RAD) * Math.cos(p.lon * RAD), Math.cos(p.lat * RAD) * Math.sin(p.lon * RAD), Math.sin(p.lat * RAD)];

// The point `k` of the way from a to b along the great circle through them.
function slerp(a, b, k) {
  const [u, v] = [toVec(a), toVec(b)];
  const omega = Math.acos(Math.min(1, Math.max(-1, u[0] * v[0] + u[1] * v[1] + u[2] * v[2])));
  if (omega < 1e-6) return { lat: a.lat, lon: a.lon };
  const [s, t] = [Math.sin((1 - k) * omega) / Math.sin(omega), Math.sin(k * omega) / Math.sin(omega)];
  const x = u[0] * s + v[0] * t, y = u[1] * s + v[1] * t, z = u[2] * s + v[2] * t;
  return { lat: Math.asin(Math.min(1, Math.max(-1, z))) / RAD, lon: Math.atan2(y, x) / RAD };
}

function metric(name, caption, value) {
  return row().id(`m-${caption}`).gap(7)
    .child(icon(name, 13).color("#ffffffb3"))
    .child(col().gap(0)
      .child(text(caption).text_size(9).font_semibold().text_color("#ffffff80"))
      .child(text(value).text_size(12).font_medium().tabular_nums()));
}

export default class Iss extends View {
  init(props, cx) {
    this.imperial = props.units === "imperial";
    const saved = JSON.parse(localStorage.getItem("iss") ?? "null");
    this.now = saved?.now ?? null;
    this.track = saved?.track ?? [];
    this.error = null;
    this.busy = false;
    this.tracing = false;
    this.retrace = 0;
    this.asked = 0;
    // Once now, so the page opens on a fresh position and path; after that only while it's open.
    cx.spawn((cx) => this.locate(cx));
    cx.timer.after(1500, (cx) => this.stale() && cx.spawn((cx) => this.trace(cx)));
    cx.timer.every(1000, (cx) => {
      const i = island();
      if (!i?.expanded || i.selected !== "iss") return;
      if (!this.busy && Date.now() - this.asked >= POLL_MS) cx.spawn((cx) => this.locate(cx));
      if (this.stale()) cx.spawn((cx) => this.trace(cx));
      // The station moves every second, between answers too.
      cx.notify();
    });
  }

  // The path needs tracing again when it no longer reaches a lap ahead.
  stale() {
    const until = this.track.at(-1)?.at ?? 0;
    return !this.tracing && Date.now() >= this.retrace && until - Date.now() / 1000 < AHEAD_S;
  }

  // Where the station is at `t` (seconds), along the path; null outside it.
  along(t) {
    const p = this.track;
    for (let i = 0; i + 1 < p.length; i++) {
      if (p[i].at <= t && t <= p[i + 1].at) return slerp(p[i], p[i + 1], (t - p[i].at) / (p[i + 1].at - p[i].at || 1));
    }
    return null;
  }

  save() {
    localStorage.setItem("iss", JSON.stringify({ now: this.now, track: this.track }));
  }

  async locate(cx) {
    this.busy = true;
    this.asked = Date.now();
    try {
      const r = await fetch(`${API}?units=kilometers`);
      if (!r.ok) throw new Error(`wheretheiss.at answered ${r.status}`);
      const j = await r.json();
      this.now = { ...point(j), altitude: j.altitude, velocity: j.velocity, sunlit: j.visibility === "daylight" };
      this.error = null;
      this.save();
    } catch (e) {
      this.error = String(e.message ?? e);
    } finally {
      this.busy = false;
      cx.notify();
    }
  }

  // The path, ten positions a request (the API's limit), a request a second.
  async trace(cx) {
    this.tracing = true;
    try {
      const from = Math.floor(Date.now() / 1000) - PAST_S;
      const times = [];
      for (let s = 0; s <= PAST_S + AHEAD_S + SPARE_S; s += STEP_S) times.push(from + s);
      const track = [];
      for (let i = 0; i < times.length; i += 10) {
        if (i) await new Promise((done) => cx.timer.after(1100, () => done()));
        const r = await fetch(`${API}/positions?units=kilometers&timestamps=${times.slice(i, i + 10).join(",")}`);
        if (!r.ok) throw new Error(`wheretheiss.at answered ${r.status}`);
        track.push(...(await r.json()).map(point));
      }
      this.track = track;
      this.save();
      cx.notify();
    } catch (e) {
      console.warn(`path: ${e.message ?? e}`);
      // Again in a minute, not every second.
      this.retrace = Date.now() + 60_000;
    } finally {
      this.tracing = false;
    }
  }

  // This lap as dots on the whole minutes, so they stay put while the station passes them.
  dots(at, now) {
    const out = [];
    for (let t = Math.ceil((now - PAST_S) / 60) * 60; t <= now + AHEAD_S; t += 60) {
      const p = this.along(t);
      if (!p) continue;
      const [x, y] = at(p);
      out.push(div().id(`dot-${t}`).size(3).rounded_full().bg(t > now ? "#ff9f0acc" : "#ffffff40").pin(x - 1.5, y - 1.5, null, null));
    }
    return out;
  }

  expanded() {
    const s = sun(new Date());
    const w = island()?.page?.width ?? 528;
    const h = Math.round((w * (TOP - BOTTOM)) / 360);
    const at = (p) => [((p.lon + 180) / 360) * w, ((TOP - p.lat) / (TOP - BOTTOM)) * h];
    const now = Date.now() / 1000;
    // The world-clock's map (shipped beside this mod), so both read the same.
    const map = div().id("map").w_full().h(h).rounded(18).clip().bg("#05070b")
      .child(shader("../world-clock/map.wgsl", [s.dec, s.lon, TOP, BOTTOM]).id("world").absolute_fill().fps(0))
      .children(this.dots(at, now));

    const here = location();
    const me = here && !here.error ? { lat: here.latitude, lon: here.longitude } : null;
    if (me) {
      const [x, y] = at(me);
      map.child(div().id("me").size(7).rounded_full().bg("blue").border(1.5, "#000000").pin(x - 3.5, y - 3.5, null, null));
    }

    const n = this.now;
    // Along the path now, else the last answer.
    const pos = this.along(now) ?? n;
    if (pos) {
      const [x, y] = at(pos);
      // A new id on each side of the map, so crossing the date line jumps instead of sweeping
      // back across the world.
      const side = pos.lon > 90 ? "e" : pos.lon < -90 ? "w" : "m";
      map.child(div().id(`iss-glow-${side}`).size(20).rounded_full().bg("#ff9f0a4d").pulse(1.6).pin(x - 10, y - 10, null, null));
      map.child(div().id(`iss-${side}`).size(9).rounded_full().bg("#ffffff").border(2, "orange").pin(x - 4.5, y - 4.5, null, null));
    } else {
      map.child(text(this.error ? `Couldn't reach the ISS: ${this.error}` : "Finding the ISS…").text_size(12).text_color("muted").pin(16, null, null, 14));
    }

    const dist = (v) => (this.imperial ? v / 1.609344 : v);
    const unit = this.imperial ? "mi" : "km";
    const where = pos ? `${Math.abs(pos.lat).toFixed(1)}° ${pos.lat >= 0 ? "N" : "S"}, ${Math.abs(pos.lon).toFixed(1)}° ${pos.lon >= 0 ? "E" : "W"}` : "—";
    const stats = row().id("stats").w_full().px(6).justify_between()
      .child(metric("map-pin", "OVER", where))
      .child(metric("radar", "ALTITUDE", n ? `${grouped(dist(n.altitude))} ${unit}` : "—"))
      .child(metric("gauge", "SPEED", n ? `${grouped(dist(n.velocity))} ${unit}/h` : "—"))
      .child(me && pos
        ? metric("users", "FROM YOU", `${grouped(dist(km(me, pos)))} ${unit}`)
        : metric(n?.sunlit === false ? "moon" : "sun", "SUNLIGHT", n ? (n.sunlit ? "Daylight" : "Earth's shadow") : "—"));

    return col().id("iss-page").w_full().gap(10).py(4).child(map).child(stats);
  }
}
