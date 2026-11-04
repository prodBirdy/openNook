// A countdown timer. Pick a length (presets, or a minute at a time), start it, and the closed
// island counts down beside the camera; at zero it chimes, opens briefly and shows a bell
// until it's stopped. The timer is kept in localStorage, so it survives a reload.
// props: { presets: [1, 3, 5, 10, 25, 60] (minutes), sound: "Glass" (/System/Library/Sounds) }

import { View, row, col, div, text, icon, shader } from "nook/ui";
import { exec, island, run } from "nook";

const MAX_MIN = 180;
// A done timer gives up ringing after this long.
const RING_MS = 60_000;

const pad = (n) => String(n).padStart(2, "0");

function clock(ms) {
  const s = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

const minutes = (ms) => {
  const m = Math.round(ms / 60000);
  return m >= 60 && m % 60 === 0 ? `${m / 60} h` : `${m} min`;
};

// `grow`: share the row's width with its siblings (centered), so the row fits any island.
function pill(id, label, onClick, { accent = false, selected = false, iconName = null, grow = false } = {}) {
  const fg = accent ? "#000000" : selected ? "orange" : "fg";
  return row().id(id).gap(5).px(12).py(6).rounded_full().justify_center()
    .when(grow, (r) => r.flex_1())
    .bg(accent ? "orange" : selected ? "#ff9f0a33" : "surface")
    .hover_bg(accent ? "#ffb340" : "raised")
    .on_click((_, cx) => onClick(cx))
    .when(iconName, (r) => r.child(icon(iconName, 12).color(fg)))
    .child(text(label).text_size(12).font_semibold().text_color(fg));
}

export default class Timer extends View {
  init(props, cx) {
    this.presets = (Array.isArray(props.presets) ? props.presets : [1, 3, 5, 10, 25, 60])
      .filter((m) => m > 0 && m <= MAX_MIN).slice(0, 6);
    this.sound = String(props.sound || "Glass").replace(/[^A-Za-z0-9 _-]/g, "");
    // state: "idle" | "running" | "paused" | "done"; endAt while running, left while paused.
    const saved = JSON.parse(localStorage.getItem("timer") ?? "null");
    this.t = saved ?? { state: "idle", duration: 5 * 60000, endAt: 0, left: 0, doneAt: 0 };
    this.shown = null;
    cx.timer.every(250, (cx) => this.tick(cx));
  }

  save() {
    localStorage.setItem("timer", JSON.stringify(this.t));
  }

  left() {
    const t = this.t;
    if (t.state === "running") return Math.max(0, t.endAt - Date.now());
    if (t.state === "paused") return t.left;
    if (t.state === "done") return 0;
    return t.duration;
  }

  tick(cx) {
    const t = this.t;
    if (t.state === "running" && Date.now() >= t.endAt) return this.finish(cx);
    if (t.state === "done" && Date.now() - t.doneAt > RING_MS) return this.reset(cx);
    if (t.state !== "running") return;
    // The open ring moves smoothly; closed, only the seconds change.
    const label = clock(this.left());
    if (island()?.expanded || label !== this.shown) {
      this.shown = label;
      cx.notify();
    }
  }

  finish(cx) {
    Object.assign(this.t, { state: "done", doneAt: Date.now() });
    this.save();
    run("island.peek", { mod: "timer", ms: 5000 });
    const chime = () => exec("afplay", [`/System/Library/Sounds/${this.sound}.aiff`]).catch(() => {});
    chime();
    cx.timer.after(1600, () => this.t.state === "done" && chime());
    cx.timer.after(3200, () => this.t.state === "done" && chime());
    cx.notify();
  }

  start(cx, duration = this.t.duration) {
    Object.assign(this.t, { state: "running", duration, endAt: Date.now() + duration, left: 0 });
    this.save();
    cx.notify();
  }

  pause(cx) {
    Object.assign(this.t, { state: "paused", left: this.left() });
    this.save();
    cx.notify();
  }

  resume(cx) {
    Object.assign(this.t, { state: "running", endAt: Date.now() + this.t.left });
    this.save();
    cx.notify();
  }

  reset(cx) {
    Object.assign(this.t, { state: "idle", endAt: 0, left: 0, doneAt: 0 });
    this.save();
    cx.notify();
  }

  // Idle, the length; running or paused, the time left (and the length with it).
  add(cx, ms) {
    const t = this.t;
    if (t.state === "idle") {
      t.duration = Math.min(Math.max(t.duration + ms, 60000), MAX_MIN * 60000);
    } else if (t.state === "running") {
      t.endAt += ms;
      t.duration += ms;
    } else if (t.state === "paused") {
      t.left += ms;
      t.duration += ms;
    }
    this.save();
    cx.notify();
  }

  activity() {
    const t = this.t;
    if (t.state === "done") {
      return {
        leading: icon("bell-ring", 15).color("orange").pulse(0.8),
        trailing: text("Done").text_size(12).font_semibold().text_color("orange"),
        detail: text(`${minutes(t.duration)} timer is done`).text_size(12).font_medium(),
        priority: 70,
      };
    }
    if (t.state !== "running" && t.state !== "paused") return null;
    const paused = t.state === "paused";
    return {
      leading: div().id("mini-ring").size(15).child(shader("./ring.wgsl", [this.left() / t.duration, 0.36, paused ? 1 : 0]).id("mini").absolute_fill().fps(0)),
      trailing: text(clock(this.left())).id("left").text_size(13).font_medium().tabular_nums().text_color(paused ? "muted" : "fg"),
      detail: text(paused ? `Paused · ${minutes(t.duration)}` : `Ends ${this.endsAt()}`).text_size(12).font_medium(),
      priority: paused ? 15 : 50,
    };
  }

  endsAt() {
    const end = new Date(this.t.state === "running" ? this.t.endAt : Date.now() + this.left());
    return `${pad(end.getHours())}:${pad(end.getMinutes())}`;
  }

  expanded() {
    const t = this.t;
    const left = this.left();
    const label = clock(left);
    const caption = t.state === "done" ? "Done" : t.state === "paused" ? "Paused" : t.state === "running" ? `Ends ${this.endsAt()}` : minutes(t.duration);
    const ring = div().id("ring").size(112).flex_none()
      .child(shader("./ring.wgsl", [t.state === "done" ? 1 : left / t.duration, 0.11, t.state === "paused" ? 1 : 0]).id("ring-shader").absolute_fill().fps(0))
      .child(col().id("ring-face").size_full().items_center().justify_center().gap(1)
        .child(text(label).id("ring-time").text_size(label.length > 5 ? 20 : 26).font_semibold().tabular_nums()
          .text_color(t.state === "paused" ? "muted" : "fg"))
        .child(text(caption).id("ring-caption").text_size(11).font_medium().text_color(t.state === "done" ? "orange" : "muted")))
      .when(t.state === "done", (r) => r.pulse(1.2));

    let controls;
    if (t.state === "idle") {
      // Presets three to a row: a single row runs past the edge of a narrow island.
      const preset = (m) => pill(`preset-${m}`, minutes(m * 60000), (cx) => { t.duration = m * 60000; this.save(); cx.notify(); }, { selected: t.duration === m * 60000, grow: true });
      const rows = [];
      for (let i = 0; i < this.presets.length; i += 3) rows.push(row().id(`presets-${i}`).w_full().gap(6).children(this.presets.slice(i, i + 3).map(preset)));
      controls = col().gap(8)
        .children(rows)
        .child(row().id("idle-actions").w_full().gap(6)
          .child(pill("less", "1 min", (cx) => this.add(cx, -60000), { iconName: "minus" }))
          .child(pill("more", "1 min", (cx) => this.add(cx, 60000), { iconName: "plus" }))
          .child(div().flex_1())
          .child(pill("start", "Start", (cx) => this.start(cx), { accent: true, iconName: "play-fill" })));
    } else if (t.state === "done") {
      controls = row().id("done-actions").w_full().gap(6)
        .child(pill("repeat", `Again · ${minutes(t.duration)}`, (cx) => this.start(cx), { iconName: "rotate-ccw" }))
        .child(div().flex_1())
        .child(pill("stop", "Stop", (cx) => this.reset(cx), { accent: true }));
    } else {
      const running = t.state === "running";
      controls = col().gap(10)
        .child(row().id("ends").gap(6)
          .child(icon("bell", 12).color("muted"))
          .child(text(running ? `Rings at ${this.endsAt()}` : `${minutes(t.duration)} timer, paused`).text_size(12).text_color("muted")))
        .child(row().id("run-actions").w_full().gap(6)
          .child(pill("plus-1", "1 min", (cx) => this.add(cx, 60000), { iconName: "plus" }))
          .child(pill("cancel", "Cancel", (cx) => this.reset(cx)))
          .child(div().flex_1())
          .child(running
            ? pill("pause", "Pause", (cx) => this.pause(cx), { accent: true, iconName: "pause-fill" })
            : pill("resume", "Resume", (cx) => this.resume(cx), { accent: true, iconName: "play-fill" })));
    }
    return row().id("timer").w_full().gap(20).px(4).py(6).items_center()
      .child(ring)
      .child(col().flex_1().child(controls));
  }
}
