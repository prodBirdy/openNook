// A world clock: the world in dots, lit where the sun is up right now (map.wgsl), with your
// cities pinned on it and their local times. Cities come from props (names, geocoded once
// with Open-Meteo, which also gives their time zones). Where you are (location(): this Mac,
// or the place set in Settings › General › Location) gets a blue pin of its own.
// props: { cities: ["Tokyo", "Berlin", …], hours: 24 | 12 }

import { View, row, col, div, text, icon, shader } from "nook/ui";
import { exec, island, location } from "nook";

// The map's latitudes, top and bottom: past them is ice and ocean.
const TOP = 78;
const BOTTOM = -56;

// Where the sun is overhead now: its declination and longitude, in degrees (good to a
// fraction of a degree, plenty for a dot map).
function sun(now) {
  const start = Date.UTC(now.getUTCFullYear(), 0, 0);
  const day = (now.getTime() - start) / 86400000;
  const dec = -23.44 * Math.cos((2 * Math.PI / 365) * (day + 10));
  const b = (2 * Math.PI * (day - 81)) / 364;
  const eot = 9.87 * Math.sin(2 * b) - 7.53 * Math.cos(b) - 1.5 * Math.sin(b); // minutes
  const hours = now.getUTCHours() + now.getUTCMinutes() / 60 + now.getUTCSeconds() / 3600;
  let lon = -15 * (hours - 12 + eot / 60);
  lon = ((lon + 540) % 360) - 180;
  return { dec, lon };
}

const pad = (n) => String(n).padStart(2, "0");

export default class WorldClock extends View {
  init(props, cx) {
    this.names = (props.cities ?? []).slice(0, 8);
    this.twelve = props.hours === 12;
    this.cities = JSON.parse(localStorage.getItem("cities") ?? "[]").filter((c) => this.names.includes(c.query));
    this.offsets = {};
    cx.spawn((cx) => this.locate(cx));
    // The sun moves a dot every eight minutes; the clocks, every minute.
    cx.timer.every(30_000, (cx) => cx.notify());
    // A new place (set in Settings) may bring a time zone we haven't looked up yet.
    cx.timer.every(2000, (cx) => {
      const tz = this.here()?.tz;
      if (tz && this.offsets[tz] === undefined && !this.zoning) cx.spawn((cx) => this.zones(cx));
    });
    cx.timer.every(10 * 60_000, (cx) => cx.spawn((cx) => this.zones(cx)));
  }

  async locate(cx) {
    const known = new Map(this.cities.map((c) => [c.query, c]));
    for (const name of this.names) {
      if (known.has(name)) continue;
      try {
        const r = await (await fetch(`https://geocoding-api.open-meteo.com/v1/search?count=1&name=${encodeURIComponent(name)}`)).json();
        const hit = r.results?.[0];
        if (hit) known.set(name, { query: name, name: hit.name, lat: hit.latitude, lon: hit.longitude, tz: hit.timezone });
      } catch (e) {
        console.warn(`couldn't find ${name}: ${e.message ?? e}`);
      }
    }
    this.cities = this.names.map((n) => known.get(n)).filter(Boolean);
    localStorage.setItem("cities", JSON.stringify(this.cities));
    await this.zones(cx);
  }

  // Each zone's UTC offset now (DST included), from the system's own time zone database.
  async zones(cx) {
    const tzs = [...new Set([...this.cities, this.here()].map((c) => c?.tz).filter(Boolean))];
    if (!tzs.length) return cx.notify();
    this.zoning = true;
    try {
      const script = tzs.map((z) => `TZ='${z.replace(/'/g, "")}' date +%z`).join("; ");
      const r = await exec("/bin/sh", ["-c", script]);
      r.stdout.trim().split("\n").forEach((line, i) => {
        const m = line.trim().match(/^([+-])(\d\d)(\d\d)$/);
        if (m) this.offsets[tzs[i]] = (m[1] === "-" ? -1 : 1) * (Number(m[2]) * 60 + Number(m[3]));
      });
    } finally {
      this.zoning = false;
    }
    cx.notify();
  }

  // Where you are, as a city: null until location() knows (or when it can't).
  here() {
    const l = location();
    if (!l || l.error || l.latitude == null) return null;
    return { query: "here", name: l.name ?? "You", lat: l.latitude, lon: l.longitude, tz: l.timezone ?? null, here: true, live: !!l.live };
  }

  time(city, now) {
    // Where this Mac is, its clock is the time.
    const offset = city.here && (city.live || !city.tz) ? undefined : this.offsets[city.tz];
    if (offset === undefined && !(city.here && (city.live || !city.tz))) return "–:––";
    const local = offset === undefined ? null : new Date(now.getTime() + offset * 60000);
    const h = local ? local.getUTCHours() : now.getHours(), m = local ? local.getUTCMinutes() : now.getMinutes();
    if (!this.twelve) return `${pad(h)}:${pad(m)}`;
    return `${h % 12 || 12}:${pad(m)} ${h < 12 ? "am" : "pm"}`;
  }

  expanded() {
    const now = new Date();
    const s = sun(now);
    const w = island()?.page?.width ?? 528;
    const h = Math.round((w * (TOP - BOTTOM)) / 360);
    const map = div().id("map").w_full().h(h).rounded(18).clip().bg("#05070b")
      .child(shader("./map.wgsl", [s.dec, s.lon, TOP, BOTTOM]).id("world").absolute_fill().fps(0));
    // Labels go beside their pins, moving up or down a row when one would cover another.
    const LABEL_H = 22;
    const at = (c) => [((c.lon + 180) / 360) * w, ((TOP - c.lat) / (TOP - BOTTOM)) * h];
    // Your place, unless a city of yours is the same place.
    const here = this.here();
    const same = (c) => here && (c.name.toLowerCase() === here.name.toLowerCase() || (Math.abs(c.lat - here.lat) < 0.3 && Math.abs(c.lon - here.lon) < 0.3));
    const cities = here ? [...this.cities.filter((c) => !same(c)), here] : this.cities;
    // Every pin is in the way of every label; placed labels join them.
    const placed = cities.map((c) => { const [x, y] = at(c); return { left: x - 5, top: y - 5 - (LABEL_H - 10), width: 10, pin: true }; });
    const pins = [];
    for (const c of [...cities].sort((a, b) => a.lon - b.lon)) {
      const [x, y] = at(c);
      const time = this.time(c, now);
      const width = (c.name.length + time.length) * 6.6 + 22 + (c.here ? 14 : 0);
      const right = x + 7 + width > w - 4;
      const left = right ? x - 7 - width : x + 7;
      let top = y - LABEL_H / 2;
      for (const shift of [0, -LABEL_H - 2, LABEL_H + 2, -2 * (LABEL_H + 2), 2 * (LABEL_H + 2)]) {
        const t = Math.min(Math.max(y - LABEL_H / 2 + shift, 4), h - LABEL_H - 4);
        const clear = (p) => left + width < p.left || left > p.left + p.width || t + LABEL_H < p.top || t > p.top + LABEL_H;
        // Its own pin sits beside it, never under it.
        if (placed.every((p) => clear(p) || (p.pin && Math.abs(p.left + 5 - x) < 0.5))) {
          top = t;
          break;
        }
      }
      placed.push({ left, top, width });
      const label = row().gap(5).px(7).py(3).rounded(7).bg("#000000b3")
        .when(c.here, (r) => r.child(icon("map-pin", 10).color("blue")))
        .child(text(c.name).text_size(11).font_semibold())
        .child(text(time).text_size(11).text_color("muted").tabular_nums());
      map.child(label.id(`label-${c.query}`).pin(right ? null : left, top, right ? w - x + 7 : null, null));
      pins.push(div().id(`pin-${c.query}`).size(7).rounded_full().bg(c.here ? "blue" : "fg").border(1.5, "#000000").pin(x - 3.5, y - 3.5, null, null));
    }
    // Pins over labels: a pin always shows.
    map.children(pins);
    if (!cities.length) {
      map.child(text("Add cities in layout.json › mods › world-clock › props, or set your place in Settings › General › Location").text_size(12).text_color("muted").pin(16, null, null, 14));
    }
    return col().id("world-clock").w_full().py(4).child(map);
  }
}
