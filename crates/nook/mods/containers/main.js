// Containers: a quick look at Apple's `container` runtime. Closed, the island shows a box and
// how many containers are running; open, each container with its image, address, uptime, CPU
// and memory, and a button to start or stop it.
// `container list` is quick and read every few seconds while the page is open; `container
// stats` samples for about two seconds, so it runs in the background and CPU is worked out
// from the change in cumulative CPU time between two samples.
// props: { showBuilder: true (list the image builder's container; it never counts as running
//          in the closed island) }

import { View, row, col, div, text, icon } from "nook/ui";
import { exec, island, run } from "nook";

const BUILDER_ROLE = "com.apple.container.resource.role";
const MAX_CARDS = 8;

// "docker.io/library/postgres:16" → "postgres:16"; "ghcr.io/apple/x/builder:0.13" → "apple/x/builder:0.13".
const shortImage = (ref = "") => ref.replace(/^docker\.io\/(library\/)?/, "").replace(/^[^/]+\.[^/]+\//, "").replace(/@sha256:.*/, "");

// A container made without --name is called by a UUID; its first block is enough to tell it apart.
const name = (id) => (/^[0-9a-f]{8}-[0-9a-f]{4}-/i.test(id) ? id.slice(0, 8) : id);

function bytes(n) {
  if (!(n >= 0)) return "–";
  const gb = n / 2 ** 30;
  return gb >= 1 ? `${gb.toFixed(gb >= 10 ? 0 : 1)} GB` : `${Math.round(n / 2 ** 20)} MB`;
}

function since(iso) {
  const s = (Date.now() - Date.parse(iso)) / 1000;
  if (!(s >= 0)) return null;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

function ports(list = []) {
  return list.map((p) => {
    const host = p.hostPort ?? p.host?.port;
    const inner = p.containerPort ?? p.container?.port;
    return host && inner ? (host === inner ? `:${host}` : `:${host}→${inner}`) : null;
  }).filter(Boolean).join(" ");
}

function parse(c) {
  const cf = c.configuration ?? {};
  const st = c.status ?? {};
  return {
    id: c.id ?? cf.id,
    running: st.state === "running",
    state: st.state ?? "unknown",
    image: shortImage(cf.image?.reference),
    ip: st.networks?.[0]?.ipv4Address?.split("/")[0] ?? null,
    ports: ports(cf.publishedPorts),
    started: st.startedDate ?? null,
    memLimit: cf.resources?.memoryInBytes ?? null,
    builder: !!cf.labels?.[BUILDER_ROLE],
  };
}

export default class Containers extends View {
  init(props, cx) {
    this.showBuilder = props.showBuilder !== false;
    this.list = null; // null until the first read
    this.error = null; // why the list couldn't be read
    this.missing = false; // no `container` CLI on this Mac
    this.stats = {}; // id → { cpu (% of a core), mem, memLimit }
    this.samples = {}; // id → { usec, at }: the last cumulative CPU reading
    this.busy = new Set(); // ids being started or stopped
    this.starting = false; // the container system itself
    this.listing = false;
    this.sampling = false;
    this.listedAt = 0;

    cx.spawn((cx) => this.refresh(cx));
    // Every 3 s with the page open, every 15 s otherwise (the closed island only needs the count).
    cx.timer.every(3000, (cx) => {
      if (island()?.expanded || Date.now() - this.listedAt > 15_000) cx.spawn((cx) => this.refresh(cx));
    });
  }

  async refresh(cx) {
    if (this.listing) return;
    this.listing = true;
    try {
      const { code, stdout, stderr } = await exec("container", ["list", "--all", "--format", "json"]);
      if (code !== 0) throw new Error((stderr || stdout).trim().split("\n").pop() || `container list exited ${code}`);
      const list = JSON.parse(stdout || "[]").map(parse).filter((c) => this.showBuilder || !c.builder);
      list.sort((a, b) => (b.running - a.running) || (a.builder - b.builder) || a.id.localeCompare(b.id));
      this.list = list;
      this.error = null;
      this.missing = false;
    } catch (e) {
      this.missing = /No such file|os error 2/i.test(e.message);
      this.error = this.missing ? "Apple's container CLI isn't installed" : e.message;
      this.list = this.list ?? [];
      this.list.forEach((c) => (c.running = false));
    } finally {
      this.listing = false;
      this.listedAt = Date.now();
      cx.notify();
    }
    if (this.list.some((c) => c.running)) cx.spawn((cx) => this.sample(cx));
  }

  async sample(cx) {
    if (this.sampling) return;
    this.sampling = true;
    try {
      const { code, stdout } = await exec("container", ["stats", "--no-stream", "--format", "json"]);
      if (code !== 0) return;
      const now = Date.now();
      const stats = {};
      for (const s of JSON.parse(stdout || "[]")) {
        const prev = this.samples[s.id];
        const cpu = prev && now > prev.at ? ((s.cpuUsageUsec - prev.usec) / ((now - prev.at) * 1000)) * 100 : this.stats[s.id]?.cpu ?? null;
        this.samples[s.id] = { usec: s.cpuUsageUsec, at: now };
        stats[s.id] = { cpu: cpu == null ? null : Math.max(0, cpu), mem: s.memoryUsageBytes, memLimit: s.memoryLimitBytes };
      }
      this.stats = stats;
      cx.notify();
      if (Object.values(stats).some((s) => s.cpu == null)) cx.timer.after(1000, (cx) => cx.spawn((cx) => this.sample(cx)));
    } catch (e) {
      console.error(`stats: ${e.message}`);
    } finally {
      this.sampling = false;
    }
  }

  act(cx, c) {
    if (this.busy.has(c.id)) return;
    this.busy.add(c.id);
    cx.notify();
    cx.spawn(async (cx) => {
      try {
        const { code, stderr } = await exec("container", [c.running ? "stop" : "start", c.id]);
        if (code !== 0) console.error(`${c.running ? "stop" : "start"} ${c.id}: ${stderr.trim()}`);
        if (c.running) delete this.samples[c.id];
      } finally {
        this.busy.delete(c.id);
        await this.refresh(cx);
      }
    });
  }

  startSystem(cx) {
    if (this.starting) return;
    this.starting = true;
    cx.notify();
    cx.spawn(async (cx) => {
      try {
        const { code, stderr } = await exec("container", ["system", "start"]);
        if (code !== 0) console.error(`system start: ${stderr.trim()}`);
      } finally {
        this.starting = false;
        await this.refresh(cx);
      }
    });
  }

  running() {
    return (this.list ?? []).filter((c) => c.running && !c.builder);
  }

  activity() {
    const up = this.running();
    if (!up.length) return null;
    return {
      leading: icon("./box.svg", 15).color("blue"),
      trailing: text(String(up.length)).id("count").text_size(13).font_medium().tabular_nums().text_color("blue"),
      detail: text(up.map((c) => name(c.id)).join(" · ")).text_size(12).font_medium().truncate(),
      priority: 10,
    };
  }

  button(id, label, onClick, { busy = false, primary = false } = {}) {
    return row().id(id).flex_none().gap(5).px(12).py(6).rounded_full()
      .bg(primary ? "#0a84ff33" : "raised").hover_bg(primary ? "#0a84ff55" : "#ffffff26")
      .on_click((_, cx) => onClick(cx))
      .child(busy ? icon("rotate-ccw", 11).color("muted").spin(1) : null)
      .child(text(label).text_size(12).font_semibold().text_color(busy ? "muted" : primary ? "blue" : "fg"));
  }

  meter(id, frac, color) {
    return div().id(id).w(52).h(4).rounded_full().bg("raised").clip()
      .child(div().id(`${id}-fill`).h(4).w(Math.max(2, Math.min(1, frac || 0) * 52)).rounded_full().bg(color));
  }

  card(c) {
    const busy = this.busy.has(c.id);
    const s = this.stats[c.id];
    const meta = [c.image, c.running && c.ip, c.ports, c.running && c.started && since(c.started) && `up ${since(c.started)}`].filter(Boolean).join(" · ");
    const memLimit = s?.memLimit ?? c.memLimit;

    const stats = c.running && s
      ? row().id(`${c.id}-stats`).flex_none().gap(12)
        .child(col().id(`${c.id}-cpu`).w(52).gap(4).items_end()
          .child(text(s.cpu == null ? "–" : `${s.cpu < 10 ? s.cpu.toFixed(1) : Math.round(s.cpu)}%`).text_size(12).font_medium().tabular_nums())
          .child(this.meter(`${c.id}-cpu-bar`, (s.cpu ?? 0) / 100, (s.cpu ?? 0) > 80 ? "orange" : "green")))
        .child(col().id(`${c.id}-mem`).w(52).gap(4).items_end()
          .child(text(bytes(s.mem)).text_size(12).font_medium().tabular_nums())
          .child(this.meter(`${c.id}-mem-bar`, s.mem / memLimit, s.mem / memLimit > 0.85 ? "orange" : "blue")))
      : null;

    return row().id(`c-${c.id}`).w_full().gap(10).px(12).py(9).rounded(12).bg("surface")
      .when(!c.running, (r) => r.opacity(0.6))
      .child(div().id(`${c.id}-dot`).size(8).rounded_full().bg(c.running ? "green" : "faint").when(c.running, (d) => d.pulse(2.4)))
      .child(col().flex_1().min_w(0).gap(2)
        .child(row().w_full().min_w(0).gap(6)
          .child(text(name(c.id)).shrink().min_w(0).text_size(13).font_semibold().truncate())
          .child(c.builder ? text("builder").text_size(10).font_medium().text_color("muted").px(5).py(1).rounded(4).bg("raised") : null))
        .child(text(meta || c.state).text_size(11).text_color("muted").truncate()))
      .child(stats)
      .child(this.button(`${c.id}-act`, busy ? (c.running ? "Stopping" : "Starting") : c.running ? "Stop" : "Start", (cx) => this.act(cx, c), { busy }));
  }

  expanded() {
    const list = this.list ?? [];
    const up = list.filter((c) => c.running).length;
    const down = list.length - up;
    const summary = this.error ? this.error
      : this.list == null ? "Looking…"
      : !list.length ? "No containers"
      : [up && `${up} running`, down && `${down} stopped`].filter(Boolean).join(" · ");
    const systemDown = this.error && !this.missing;
    const more = list.length - MAX_CARDS;

    const head = row().id("head").w_full().gap(10).px(4)
      .child(icon("./box.svg", 18).color(up ? "blue" : "muted"))
      .child(col().flex_1().gap(1)
        .child(text("Containers").text_size(15).font_semibold())
        .child(text(summary).id("summary").text_size(12).text_color(this.error && !this.missing ? "orange" : "muted").truncate()))
      .when(systemDown, (r) => r.child(this.button("system", this.starting ? "Starting" : "Start system", (cx) => this.startSystem(cx), { busy: this.starting, primary: true })))
      .when(this.missing, (r) => r.child(this.button("get", "Get container", () => run("open", { target: "https://github.com/apple/container/releases" }), { primary: true })));

    return col().id("containers").w_full().gap(10).px(4).py(8)
      .child(head)
      .when(list.length, (c) => c.child(col().id("list").w_full().gap(6).children(list.slice(0, MAX_CARDS).map((x) => this.card(x)))))
      .when(more > 0, (c) => c.child(text(`${more} more · container list --all`).id("more").px(4).text_size(11).text_color("faint")));
  }
}
