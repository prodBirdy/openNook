// Caffeinate: keeps the Mac awake (macOS's own `caffeinate`) for a while or until stopped.
// Closed, the island shows a cup and the time left while it's on. caffeinate runs on its own,
// so a reload or a restart of openNook finds it again; it waits on openNook (-w), so quitting
// openNook lets the Mac sleep as usual.
// props: { presets: [15, 30, 60, 120, 240, 0] (minutes; 0 = until stopped), display: true
//          (keep the display on too; false lets it sleep while the Mac stays up) }

import { View, row, col, div, text, icon } from "nook/ui";
import { exec, appHome, readTextFile, island } from "nook";

const pad = (n) => String(n).padStart(2, "0");

function left(ms) {
  const s = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

// Closed: whole minutes are enough ("1h 24m", "12m"), the last minute in seconds.
function short(ms) {
  const s = Math.max(0, Math.ceil(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.ceil(s / 60);
  return m >= 60 ? `${Math.floor(m / 60)}h${m % 60 ? ` ${m % 60}m` : ""}` : `${m}m`;
}

const label = (min) => (min === 0 ? "Until off" : min >= 60 && min % 60 === 0 ? `${min / 60} h` : `${min} min`);

const clockAt = (t) => { const d = new Date(t); return `${pad(d.getHours())}:${pad(d.getMinutes())}`; };

export default class Caffeinate extends View {
  init(props, cx) {
    this.presets = (Array.isArray(props.presets) ? props.presets : [15, 30, 60, 120, 240, 0]).filter((m) => m >= 0).slice(0, 9);
    this.display = props.display !== false;
    // { pid, endsAt (ms; 0 = until stopped), minutes, display }
    this.on = JSON.parse(localStorage.getItem("on") ?? "null");
    this.busy = false;
    this.shown = null;
    cx.spawn((cx) => this.check(cx));
    cx.timer.every(1000, (cx) => this.tick(cx));
    // caffeinate may have gone without us (killed, or openNook restarted with a new pid).
    cx.timer.every(10_000, (cx) => cx.spawn((cx) => this.check(cx)));
  }

  save() {
    if (this.on) localStorage.setItem("on", JSON.stringify(this.on));
    else localStorage.removeItem("on");
  }

  async alive(pid) {
    const { stdout } = await exec("ps", ["-p", String(pid), "-o", "comm="]).catch(() => ({ stdout: "" }));
    return stdout.trim().endsWith("caffeinate");
  }

  async check(cx) {
    if (!this.on || this.busy) return;
    if (!(await this.alive(this.on.pid))) {
      this.on = null;
      this.save();
      cx.notify();
    }
  }

  tick(cx) {
    if (!this.on) return;
    if (this.on.endsAt && Date.now() >= this.on.endsAt) {
      // caffeinate's own -t has ended it; check() confirms soon after.
      this.on = null;
      this.save();
      cx.notify();
      return;
    }
    // Closed, the label moves a minute at a time; open, the seconds.
    const shown = this.on.endsAt ? (island()?.expanded ? left(this.on.endsAt - Date.now()) : short(this.on.endsAt - Date.now())) : "∞";
    if (shown !== this.shown) {
      this.shown = shown;
      cx.notify();
    }
  }

  start(cx, minutes) {
    if (this.busy) return;
    this.busy = true;
    cx.spawn(async (cx) => {
      try {
        await this.kill();
        const nook = (await readTextFile(`${appHome()}/opennook.pid`))?.trim();
        const args = ["-i", ...(this.display ? ["-d"] : []), ...(/^\d+$/.test(nook ?? "") ? ["-w", nook] : []), ...(minutes ? ["-t", String(minutes * 60)] : [])];
        // In the background with its output closed, so exec returns at once and caffeinate
        // outlives the shell (exec stops what it waits on after 30 s).
        const { stdout } = await exec("/bin/sh", ["-c", 'caffeinate "$@" >/dev/null 2>&1 & echo $!', "sh", ...args]);
        const pid = Number(stdout.trim());
        if (!pid) throw new Error("caffeinate didn't start");
        this.on = { pid, endsAt: minutes ? Date.now() + minutes * 60_000 : 0, minutes, display: this.display };
        this.save();
      } catch (e) {
        console.error(`start: ${e.message}`);
      } finally {
        this.busy = false;
        this.shown = null;
        cx.notify();
      }
    });
  }

  stop(cx) {
    cx.spawn(async (cx) => {
      await this.kill();
      cx.notify();
    });
  }

  async kill() {
    const pid = this.on?.pid;
    this.on = null;
    this.save();
    if (pid && (await this.alive(pid))) await exec("kill", [String(pid)]).catch(() => {});
  }

  activity() {
    if (!this.on) return null;
    const ms = this.on.endsAt - Date.now();
    return {
      leading: icon("./coffee.svg", 15).color("orange"),
      trailing: text(this.on.endsAt ? short(ms) : "∞").id("left").text_size(13).font_medium().tabular_nums().text_color("orange"),
      detail: text(this.on.endsAt ? `Awake until ${clockAt(this.on.endsAt)}` : "Awake until you turn it off").text_size(12).font_medium(),
      priority: 15,
    };
  }

  pill(id, title, onClick, { selected = false, accent = false } = {}) {
    return row().id(id).flex_1().px(12).py(7).rounded_full().justify_center()
      .bg(accent ? "orange" : selected ? "#ff9f0a33" : "surface").hover_bg(accent ? "#ffb340" : "raised")
      .on_click((_, cx) => onClick(cx))
      .child(text(title).text_size(12).font_semibold().text_color(accent ? "#000000" : selected ? "orange" : "fg"));
  }

  expanded() {
    const on = this.on;
    const status = !on ? "Your Mac sleeps as usual"
      : on.endsAt ? `${left(on.endsAt - Date.now())} left · until ${clockAt(on.endsAt)}`
      : "Until you turn it off";

    const head = row().id("head").w_full().gap(14).px(4)
      .child(div().id("cup").size(46).flex_none().rounded_full().items_center().justify_center()
        .bg(on ? "#ff9f0a2e" : "surface").border(1, on ? "#ff9f0a66" : "border")
        .child(icon("./coffee.svg", 22).color(on ? "orange" : "muted").when(on, (i) => i.pulse(2.4))))
      .child(col().flex_1().gap(2)
        .child(text(on ? "Keeping your Mac awake" : "Keep awake").text_size(16).font_semibold())
        .child(text(status).id("status").text_size(12).tabular_nums().text_color(on ? "orange" : "muted")))
      .when(on, (r) => r.child(row().id("stop").px(14).py(7).rounded_full().bg("surface").hover_bg("raised")
        .on_click((_, cx) => this.stop(cx))
        .child(text("Turn off").text_size(12).font_semibold())));

    // Three to a row, so they fit a narrow island.
    const rows = [];
    for (let i = 0; i < this.presets.length; i += 3) {
      rows.push(row().id(`presets-${i}`).w_full().gap(6).children(this.presets.slice(i, i + 3).map((m) =>
        this.pill(`p-${m}`, label(m), (cx) => this.start(cx, m), { selected: on?.minutes === m }))));
    }

    const screen = row().id("display").w_full().gap(10).px(4).py(2)
      .on_click((_, cx) => { this.display = !this.display; cx.notify(); if (this.on) this.start(cx, this.on.endsAt ? Math.max(1, Math.ceil((this.on.endsAt - Date.now()) / 60_000)) : 0); })
      .child(icon(this.display ? "monitor" : "moon", 14).color("muted"))
      .child(text(this.display ? "Display stays on too" : "Display may sleep; the Mac stays awake").flex_1().text_size(12).text_color("muted"))
      .child(div().id("switch").w(34).h(20).rounded_full().bg(this.display ? "orange" : "raised").p(2)
        .when(this.display, (d) => d.items_end())
        .child(div().id("knob").size(16).rounded_full().bg("#ffffff")));

    return col().id("caffeinate").w_full().gap(12).px(4).py(8)
      .child(head)
      .child(col().id("presets").w_full().gap(6).children(rows))
      .child(screen);
  }
}
