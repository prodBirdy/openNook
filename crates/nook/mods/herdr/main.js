// Herdr (herdrdev/herdr): the agents in your Herdr window (`herdr-app ls`), or in herdr-core
// (`hc ls --json`) while the window is closed.
// Closed, the island says who needs you (blocked), who finished while you weren't looking
// (done), or how many are working. Open, every agent, most urgent first; click one to jump to
// it in the Herdr window (`herdr-app do agent.focus`).
// props: { peekOnBlocked: true } opens the island briefly when an agent starts waiting on you.

import { View, row, col, text, icon, image, div } from "nook/ui";
import { exec, run, home, island } from "nook";

const URGENCY = { blocked: 0, done: 1, working: 2, idle: 3 };
const LABEL = { blocked: "Needs you", done: "Done", working: "Working", idle: "Idle" };
const COLOR = { blocked: "orange", done: "green", working: "accent", idle: "muted" };
const TINT = { claude: "#d97757", gemini: "#4c8bf5" };

// Logos Herdr ships (~/.config/herdr-app/icons), read once.
let herdrIcons = new Set();

// Herdr's agent ids ("claude-code", "codex", …) to a logo: openNook's, else Herdr's, else a
// terminal.
function logo(agent) {
  const id = (agent ?? "").replace(/-code$/, "").replace(/-cli$/, "");
  const shipped = ["claude", "codex", "cursor", "gemini", "grok", "opencode", "pi"];
  const name = shipped.includes(id) ? `agent-${id}` : herdrIcons.has(id) ? `${home()}/.config/herdr-app/icons/${id}.svg` : "terminal";
  return { name, tint: TINT[id] ?? "fg" };
}

const KIND = { "claude-code": "Claude Code", codex: "Codex", opencode: "OpenCode", cursor: "Cursor", gemini: "Gemini", grok: "Grok", pi: "Pi", amp: "Amp", fx: "fx" };
const kind = (agent) => KIND[agent] ?? agent ?? "agent";
// Its name, else its space's (the window names spaces after their folder), else its folder.
const title = (t) => t.name || t.space || (t.cwd ?? "").split("/").filter(Boolean).pop() || t.address;

export default class Herdr extends View {
  init(props, cx) {
    this.peek = props.peekOnBlocked !== false;
    this.agents = [];
    this.missing = false;
    this.error = null;
    this.blocked = new Set();
    const tick = (cx) => cx.spawn((cx) => this.refresh(cx));
    cx.spawn(async (cx) => {
      const r = await exec("ls", [`${home()}/.config/herdr-app/icons`]).catch(() => null);
      herdrIcons = new Set((r?.stdout ?? "").split("\n").filter((f) => f.endsWith(".svg")).map((f) => f.slice(0, -4)));
      cx.notify();
    });
    tick(cx);
    // Faster while someone's looking.
    cx.timer.every(1500, (cx) => {
      this.beat = (this.beat ?? 0) + 1;
      if (island()?.expanded || this.beat % 2 === 0) tick(cx);
    });
  }

  // What the Herdr window shows (`herdr-app ls`): its spaces, tabs and agents, a sub-agent
  // counting toward its parent. With the window closed, the core's list (`hc ls`) without
  // sub-agents, which the window would have folded into their parents.
  async list() {
    const shown = await exec("herdr-app", ["ls"]).catch(() => null);
    if (shown?.code === 0) {
      const agents = [];
      for (const space of JSON.parse(shown.stdout).spaces ?? []) {
        for (const tab of space.tabs ?? []) {
          for (const t of tab.terminals ?? []) {
            const subs = (t.sub_agents ?? []).map((a) => a.status).filter(Boolean);
            const statuses = [t.status, ...subs].filter(Boolean);
            if (!t.app || !statuses.length) continue;
            const status = statuses.sort((a, b) => (URGENCY[a] ?? 9) - (URGENCY[b] ?? 9))[0];
            agents.push({ address: t.terminal, agent: t.app, name: t.name, status, space: space.name, where: `tab ${tab.name}`, subs: subs.length });
          }
        }
      }
      return agents;
    }
    const r = await exec("hc", ["ls", "--json"]);
    if (r.code !== 0) throw new Error(r.stderr.trim() || `hc exited ${r.code}`);
    return (JSON.parse(r.stdout).terminals ?? [])
      .filter((t) => t.agent && t.state === "live" && !t.parent)
      .map((t) => ({ address: t.address, agent: t.agent, name: t.name, status: t.status, where: t.where, cwd: t.cwd, turn: t.turn }));
  }

  async refresh(cx) {
    if (this.busy) return;
    this.busy = true;
    try {
      const agents = await this.list();
      const blocked = new Set(agents.filter((t) => t.status === "blocked").map((t) => t.address));
      if (this.peek && [...blocked].some((a) => !this.blocked.has(a))) run("island.peek", { ms: 4000 });
      this.blocked = blocked;
      const next = JSON.stringify(agents);
      if (next !== this.last || this.error || this.missing) {
        this.last = next;
        this.agents = agents;
        this.error = null;
        this.missing = false;
        cx.notify();
      }
    } catch (e) {
      const msg = String(e.message ?? e);
      const missing = /No such file|not found|can't start|ENOENT/i.test(msg);
      if (missing !== this.missing || msg !== this.error) {
        this.missing = missing;
        this.error = msg;
        cx.notify();
      }
    } finally {
      this.busy = false;
    }
  }

  activity() {
    const by = (s) => this.agents.filter((t) => t.status === s);
    const blocked = by("blocked"), done = by("done"), working = by("working");
    const face = (t) => { const l = logo(t.agent); return icon(l.name, 15).color(l.tint); };
    if (blocked.length) {
      return {
        leading: face(blocked[0]),
        trailing: row().id("blocked").gap(5).child(div().size(7).rounded_full().bg("orange").pulse(0.9))
          .when(blocked.length > 1, (r) => r.child(text(String(blocked.length)).text_size(12).font_semibold().tabular_nums())),
        detail: text(`${title(blocked[0])} needs you`).text_size(12).font_medium().truncate(),
        priority: 60,
      };
    }
    if (done.length) {
      return {
        leading: face(done[0]),
        trailing: row().id("done").gap(4).child(icon("check", 13).color("green"))
          .when(done.length > 1, (r) => r.child(text(String(done.length)).text_size(12).font_semibold().tabular_nums())),
        detail: text(`${title(done[0])} is done`).text_size(12).font_medium().truncate(),
        priority: 30,
      };
    }
    if (working.length) {
      return {
        leading: face(working[0]),
        trailing: row().id("working").gap(6).child(div().size(6).rounded_full().bg("accent").pulse(1.1))
          .when(working.length > 1, (r) => r.child(text(String(working.length)).text_size(12).font_semibold().tabular_nums())),
        detail: text(working.length === 1 ? `${title(working[0])} is working` : `${working.length} agents working`).text_size(12).font_medium().truncate(),
        priority: 20,
      };
    }
    return null;
  }

  expanded() {
    if (this.missing) {
      return row().id("missing").gap(10).py(14)
        .child(icon("terminal", 18).color("faint"))
        .child(text("Herdr isn't installed").text_color("muted").flex_1())
        .child(row().px(10).py(5).rounded(8).bg("surface").hover_bg("raised")
          .on_click(() => run("open", { target: "https://github.com/herdrdev/herdr" }))
          .child(text("Get Herdr").text_size(12).font_medium()));
    }
    if (!this.agents.length) {
      return row().id("none").gap(10).py(14)
        .child(icon("terminal", 18).color("faint"))
        .child(text(this.error ? `Herdr: ${this.error}` : "No agents in Herdr").text_color("muted").truncate().flex_1())
        .child(this.openButton());
    }
    const sorted = [...this.agents].sort((a, b) => (URGENCY[a.status] ?? 9) - (URGENCY[b.status] ?? 9) || title(a).localeCompare(title(b)));
    return col().id("list").w_full().gap(2).py(2)
      .children(sorted.slice(0, 6).map((t) => this.row(t)))
      .child(row().w_full().pt(4).px(8).items_center()
        .child(text(sorted.length > 6 ? `and ${sorted.length - 6} more` : "").text_size(11).text_color("faint").flex_1())
        .child(this.openButton()));
  }

  openButton() {
    return row().id("open-herdr").gap(6).pl(7).pr(10).py(5).rounded(8).bg("surface").hover_bg("raised")
      .on_click(() => run("open", { target: "/Applications/Herdr.app" }))
      .child(image("app:dev.herdr.app").size(16))
      .child(text("Open Herdr").text_size(12).font_medium());
  }

  row(t) {
    const l = logo(t.agent);
    const status = t.status ?? "idle";
    return row().id(`t-${t.address}`).w_full().gap(10).px(8).py(7).rounded(10)
      .hover_bg("surface")
      .on_click(() => {
        exec("herdr-app", ["do", "agent.focus", "--terminal", t.address]);
        run("open", { target: "/Applications/Herdr.app" });
        run("island.collapse");
      })
      .child(row().size(26).rounded(7).bg("surface").items_center().justify_center().child(icon(l.name, 15).color(l.tint)))
      .child(col().flex_1().gap(1)
        .child(text(title(t)).text_size(13).font_medium().truncate())
        .child(text([kind(t.agent), t.name ? t.space : null, t.where, t.subs ? `${t.subs} sub-agent${t.subs > 1 ? "s" : ""}` : null, t.turn ? `turn ${t.turn}` : null].filter(Boolean).join(" · ")).text_size(11).text_color("muted").truncate()))
      .child(row().gap(5).px(8).py(3).rounded_full().bg("surface")
        .when(status === "working" || status === "blocked", (r) => r.child(div().size(6).rounded_full().bg(COLOR[status]).pulse(1.1)))
        .child(text(LABEL[status] ?? status).text_size(11).font_medium().text_color(COLOR[status] ?? "muted")));
  }
}
