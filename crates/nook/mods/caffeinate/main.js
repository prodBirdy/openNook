// Caffeinate: keeps the Mac awake (macOS's own `caffeinate`) for a while or until stopped.
// Closed, the island shows a cup and the time left while it's on. caffeinate runs on its own,
// so a reload or a restart of openNook finds it again; it waits on openNook (-w), so quitting
// openNook lets the Mac sleep as usual.
//
// "Lid closed too" goes further: closing the lid sleeps a Mac whatever caffeinate says, so it
// turns on `pmset disablesleep`, which needs an administrator's password (macOS's own dialog,
// once). One small root shell does it and turns it off again by itself when the time is up,
// when it's turned off here, or when openNook quits or crashes: no second password, which a
// closed lid couldn't type anyway. If the Mac restarts meanwhile the setting outlives that
// shell; the page then says so and offers to restore sleep.
// props: { presets: [15, 30, 60, 120, 240, 0] (minutes; 0 = until stopped), display: true
//          (keep the display on too; false lets it sleep while the Mac stays up) }

import { View, row, col, div, text, icon } from "nook/ui";
import { exec, appHome, readTextFile, island, home } from "nook";

// The root shell: sleep off, then wait for the stop file ($0), openNook ($1, 0 for none) or the
// deadline ($2, seconds since 1970, 0 for none), then sleep on again. It never deletes the stop
// file (root removing a path the user can swap for a link): the mod clears it before each start.
const LID_SCRIPT = 'pmset -a disablesleep 1; (while [ ! -e "$0" ] && { [ "$1" = 0 ] || kill -0 "$1" 2>/dev/null; } && { [ "$2" = 0 ] || [ "$(date +%s)" -lt "$2" ]; }; do sleep 5; done; pmset -a disablesleep 0) >/dev/null 2>&1 & echo $!';
const shellQuote = (s) => `'${String(s).replace(/'/g, "'\\''")}'`;
const appleQuote = (s) => `"${String(s).replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;

// A shell command run as root, through macOS's password dialog. Rejects when it's cancelled.
async function asAdmin(command, prompt) {
  const { code, stdout, stderr } = await exec("osascript", ["-e", `do shell script ${appleQuote(command)} with administrator privileges with prompt ${appleQuote(prompt)}`]);
  if (code !== 0) throw new Error(/-128|cancel/i.test(stderr) ? "cancelled" : stderr.trim() || `osascript exited ${code}`);
  return stdout.trim();
}

async function sleepDisabled() {
  const { stdout } = await exec("pmset", ["-g"]).catch(() => ({ stdout: "" }));
  return /SleepDisabled\s+1/.test(stdout);
}

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
    this.lid = localStorage.getItem("lid") === "1";
    this.lidError = null;
    // Sleep is still off from a session that didn't get to turn it back on (the Mac restarted).
    this.stuck = false;
    this.stopFile = `${home()}/Library/Caches/openNook/caffeinate-lid.stop`;
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
    if (this.busy) return;
    if (this.on && !(await this.alive(this.on.pid))) {
      this.on = null;
      this.save();
      cx.notify();
    }
    if (this.on?.lid && !(await this.lidAlive(this.on.lid))) {
      this.on.lid = null;
      this.save();
      cx.notify();
    }
    // Only sleep this mod turned off counts as stuck: someone's own pmset setting is theirs.
    const stuck = localStorage.getItem("lidSet") === "1" && !this.on?.lid && (await sleepDisabled());
    if (localStorage.getItem("lidSet") === "1" && !(await sleepDisabled())) localStorage.removeItem("lidSet");
    if (stuck !== this.stuck) {
      this.stuck = stuck;
      cx.notify();
    }
  }

  async lidAlive(pid) {
    const { stdout } = await exec("ps", ["-p", String(pid), "-o", "comm="]).catch(() => ({ stdout: "" }));
    return /\bsh$/.test(stdout.trim());
  }

  // Sleep off until this session ends: the root shell, after one password.
  async startLid(nook, endsAt) {
    await exec("mkdir", ["-p", this.stopFile.replace(/\/[^/]+$/, "")]);
    await exec("rm", ["-f", this.stopFile]);
    const args = [this.stopFile, /^\d+$/.test(nook ?? "") ? nook : "0", endsAt ? String(Math.ceil(endsAt / 1000)) : "0"];
    const command = `/bin/sh -c ${shellQuote(LID_SCRIPT)} ${args.map(shellQuote).join(" ")}`;
    const pid = Number(await asAdmin(command, "openNook needs your password to keep your Mac awake with the lid closed."));
    if (!pid) throw new Error("the helper didn't start");
    localStorage.setItem("lidSet", "1");
    return pid;
  }

  // The root shell sees the stop file within five seconds and turns sleep back on.
  async stopLid() {
    await exec("touch", [this.stopFile]).catch(() => {});
  }

  setLid(cx, on) {
    this.lid = on;
    this.lidError = null;
    if (on) localStorage.setItem("lid", "1");
    else localStorage.removeItem("lid");
    cx.notify();
    if (!this.on) return;
    cx.spawn(async (cx) => {
      try {
        if (on && !this.on.lid) {
          const nook = (await readTextFile(`${appHome()}/opennook.pid`))?.trim();
          this.on.lid = await this.startLid(nook, this.on.endsAt);
        } else if (!on && this.on.lid) {
          await this.stopLid();
          this.on.lid = null;
        }
        this.save();
      } catch (e) {
        this.lid = false;
        localStorage.removeItem("lid");
        this.lidError = e.message === "cancelled" ? null : `Couldn't keep it awake with the lid closed: ${e.message}`;
      }
      cx.notify();
    });
  }

  restoreSleep(cx) {
    cx.spawn(async (cx) => {
      try {
        await asAdmin("pmset -a disablesleep 0", "openNook needs your password to let your Mac sleep again.");
        localStorage.removeItem("lidSet");
        this.stuck = false;
      } catch (e) {
        if (e.message !== "cancelled") this.lidError = e.message;
      }
      cx.notify();
    });
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
        const endsAt = minutes ? Date.now() + minutes * 60_000 : 0;
        this.on = { pid, endsAt, minutes, display: this.display, lid: null };
        this.save();
        if (this.lid) {
          try {
            this.on.lid = await this.startLid(nook, endsAt);
            this.save();
          } catch (e) {
            // Awake with the lid open still holds; only the lid part is off.
            this.lid = false;
            localStorage.removeItem("lid");
            this.lidError = e.message === "cancelled" ? null : `Couldn't keep it awake with the lid closed: ${e.message}`;
          }
        }
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
    if (this.on?.lid) await this.stopLid();
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

    const lidOn = this.lid;
    const lid = row().id("lid").w_full().gap(10).px(4).py(2)
      .on_click((_, cx) => this.setLid(cx, !this.lid))
      .child(icon("laptop", 14).color(lidOn ? "orange" : "muted"))
      .child(col().flex_1().gap(1)
        .child(text("Lid closed too").text_size(12).text_color(lidOn ? "fg" : "muted"))
        .child(text(on?.lid ? "Sleep is off until this ends" : "Asks for your password; keep it out of a bag").text_size(11).text_color("faint")))
      .child(div().id("lid-switch").w(34).h(20).rounded_full().bg(lidOn ? "orange" : "raised").p(2)
        .when(lidOn, (d) => d.items_end())
        .child(div().id("lid-knob").size(16).rounded_full().bg("#ffffff")));

    const stuck = this.stuck && row().id("stuck").w_full().gap(10).px(12).py(9).rounded(12).bg("#ff9f0a1f")
      .child(icon("triangle-alert", 14).color("orange"))
      .child(text("Your Mac still won't sleep, even with the lid closed: the last session ended without turning sleep back on.").flex_1().text_size(12).line_clamp(3))
      .child(row().id("restore").px(12).py(6).rounded_full().bg("orange").hover_bg("#ffb340")
        .on_click((_, cx) => this.restoreSleep(cx))
        .child(text("Restore sleep").text_size(12).font_semibold().text_color("#000000")));

    return col().id("caffeinate").w_full().gap(12).px(4).py(8)
      .child(stuck || null)
      .child(head)
      .child(col().id("presets").w_full().gap(6).children(rows))
      .child(screen)
      .child(lid)
      .when(this.lidError, (c) => c.child(text(this.lidError).id("lid-error").px(4).text_size(11).line_clamp(2).text_color("orange")));
  }
}
