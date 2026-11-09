// Apple Reminders. The page shows one list at a time (pills switch it): tick a reminder off,
// tick it again within a few seconds to undo, type a new one into the field under the list
// (Return adds it and keeps the field for the next). Closed, the island shows how many
// reminders are due today or overdue, across every list.
// It asks Reminders for nothing until its page is first opened, so macOS's Automation prompt
// comes when someone goes to Reminders, not at openNook's first launch; once it has read the
// lists, it keeps them fresh in the background.
// props: { max: 7 (rows on the page), showDue: true (the closed-island count) }

import { View, row, col, div, text, icon, image, spacer, InputState, Input } from "nook/ui";
import { exec, island, modDir, run } from "nook";

const REFRESH_MS = 30_000;
// A ticked reminder stays (struck through) this long, so a second click can undo it.
const LINGER_MS = 3_000;
const DAY = 86_400_000;
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const pad = (n) => String(n).padStart(2, "0");
const startOfDay = (t) => { const d = new Date(t); d.setHours(0, 0, 0, 0); return d.getTime(); };

// Due today or earlier (all-day reminders count for the whole day).
const isDue = (r) => r.due != null && (r.allDay ? startOfDay(r.due) <= startOfDay(Date.now()) : r.due < startOfDay(Date.now()) + DAY);
const isOverdue = (r) => r.due != null && (r.allDay ? startOfDay(r.due) < startOfDay(Date.now()) : r.due < Date.now());

function dueLabel(r) {
  const d = new Date(r.due);
  const days = Math.round((startOfDay(r.due) - startOfDay(Date.now())) / DAY);
  const time = r.allDay ? "" : ` ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (days === 0) return r.allDay ? "Today" : time.trim();
  if (days === 1) return `Tomorrow${time}`;
  if (days === -1) return `Yesterday${time}`;
  if (days > 1 && days < 7) return `${WEEKDAYS[d.getDay()]}${time}`;
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

// Undated last; then by date, then by priority (1 is high, 0 none).
const rank = (p) => (p > 0 ? p : 10);
const byDue = (a, b) => (a.due ?? Infinity) - (b.due ?? Infinity) || rank(a.priority) - rank(b.priority);

function osa(...args) {
  return exec("osascript", ["-l", "JavaScript", `${modDir()}/reminders.jxa`, ...args]);
}

export default class Reminders extends View {
  init(props, cx) {
    this.max = Math.max(1, Number(props.max) || 7);
    this.showDue = props.showDue !== false;
    this.lists = JSON.parse(localStorage.getItem("lists") ?? "null");
    this.selected = localStorage.getItem("selected");
    this.error = null;
    // id → true while a ticked reminder lingers.
    this.ticked = {};
    this.busy = false;
    this.wasOpen = false;
    // Reminders has answered once: background refreshes may ask it without the page open.
    this.connected = localStorage.getItem("connected") === "1";
    this.field = InputState.new({ placeholder: "New reminder" });
    this.field.on("submit", (_, cx) => this.add(cx));
    this.field.on("change", (_, cx) => cx.notify());
    this.field.on("focus", (_, cx) => cx.notify());
    this.field.on("blur", (_, cx) => cx.notify());
    if (this.connected) cx.spawn((cx) => this.refresh(cx));
    cx.timer.every(REFRESH_MS, (cx) => { if (this.connected) cx.spawn((cx) => this.refresh(cx)); });
    // Fresh when the island opens on this page (the first time, that's what asks for access).
    cx.timer.every(1000, (cx) => {
      const open = island()?.expanded && island()?.selected === "reminders";
      if (open && !this.wasOpen) cx.spawn((cx) => this.refresh(cx));
      this.wasOpen = open;
    });
  }

  async refresh(cx) {
    if (this.busy) return;
    this.busy = true;
    try {
      const { code, stdout, stderr } = await osa("lists").catch((e) => ({ code: -1, stdout: "", stderr: String(e?.message ?? e) }));
      if (code !== 0) {
        this.error = /not allowed|-1743/i.test(stderr)
          ? "openNook may not control Reminders. Allow it in System Settings › Privacy & Security › Automation."
          : stderr.trim().split("\n").pop() || "Reminders didn't answer.";
      } else {
        this.error = null;
        if (!this.connected) localStorage.setItem("connected", "1");
        this.connected = true;
        const lists = JSON.parse(stdout);
        // Keep lingering ticks on screen until they've had their moment.
        for (const l of lists) {
          const old = this.lists?.find((o) => o.id === l.id);
          const lingering = old?.items.filter((r) => this.ticked[r.id] && !l.items.some((n) => n.id === r.id)) ?? [];
          l.items = [...l.items, ...lingering].sort(byDue);
        }
        this.lists = lists;
        localStorage.setItem("lists", JSON.stringify(lists));
      }
    } finally {
      this.busy = false;
    }
    cx.notify();
  }

  list() {
    const lists = this.lists ?? [];
    return lists.find((l) => l.id === this.selected) ?? lists.find((l) => l.items.length) ?? lists[0] ?? null;
  }

  select(cx, id) {
    this.selected = id;
    localStorage.setItem("selected", id);
    cx.notify();
  }

  toggle(cx, r) {
    const done = !this.ticked[r.id];
    if (done) this.ticked[r.id] = true;
    else delete this.ticked[r.id];
    cx.notify();
    cx.spawn(async (cx) => {
      const { code, stderr } = await osa("complete", r.id, String(done)).catch((e) => ({ code: -1, stderr: String(e?.message ?? e) }));
      if (code !== 0) {
        delete this.ticked[r.id];
        this.error = stderr.trim().split("\n").pop() || "Couldn't change that reminder.";
        cx.notify();
      }
    });
    if (done) {
      cx.timer.after(LINGER_MS, (cx) => {
        if (!this.ticked[r.id]) return;
        delete this.ticked[r.id];
        for (const l of this.lists ?? []) l.items = l.items.filter((i) => i.id !== r.id);
        localStorage.setItem("lists", JSON.stringify(this.lists));
        cx.notify();
      });
    }
  }

  add(cx) {
    const list = this.list();
    const title = this.field.value().trim();
    if (!list || !title) return;
    this.field.set_value("");
    const temp = { id: `pending-${Date.now()}`, name: title, priority: 0, flagged: false, due: null, allDay: false, pending: true };
    list.items.push(temp);
    cx.notify();
    cx.spawn(async (cx) => {
      const { code, stdout, stderr } = await osa("add", list.id, title).catch((e) => ({ code: -1, stdout: "", stderr: String(e?.message ?? e) }));
      if (code !== 0 || stdout.trim() !== "ok") {
        this.error = stderr.trim().split("\n").pop() || "Couldn't add that reminder.";
        list.items = list.items.filter((r) => r !== temp);
        // Give the title back, so nothing typed is lost.
        if (!this.field.value()) this.field.set_value(title);
        cx.notify();
        return;
      }
      await this.refresh(cx);
    });
  }

  // Due today or overdue, not ticked, across every list.
  due() {
    return (this.lists ?? []).flatMap((l) => l.items.filter((r) => !this.ticked[r.id] && isDue(r))).sort(byDue);
  }

  activity() {
    if (!this.showDue) return null;
    const due = this.due();
    if (!due.length) return null;
    const late = due.some(isOverdue);
    return {
      leading: icon("list-checks", 15).color(late ? "red" : "orange"),
      trailing: text(String(due.length)).id("due-count").text_size(13).font_semibold().tabular_nums().text_color(late ? "red" : "fg"),
      detail: text(due.length === 1 ? due[0].name : `${due[0].name} · ${due.length - 1} more`).text_size(12).font_medium().truncate(),
      priority: late ? 35 : 25,
    };
  }

  item(r, color) {
    const done = !!this.ticked[r.id];
    const late = !done && isOverdue(r);
    const check = div().id(`check-${r.id}`).size(18).flex_none().rounded_full().items_center().justify_center()
      .border(1.5, done ? color : "faint")
      .when(done, (d) => d.bg(color).child(icon("check", 11).color("#000000").enter("scale")));
    const marks = r.priority > 0 ? "!".repeat(r.priority <= 4 ? 3 : r.priority === 5 ? 2 : 1) : null;
    return row().id(`item-${r.id}`).w_full().gap(10).px(8).py(6).rounded(10).hover_bg("surface").enter("fade")
      .when(!r.pending, (el) => el.on_click((_, cx) => this.toggle(cx, r)))
      .when(r.pending, (el) => el.opacity(0.6))
      .child(check)
      .when(marks, (el) => el.child(text(marks).text_size(13).font_bold().text_color(done ? "faint" : "orange")))
      .child(text(r.name).flex_1().text_size(13).truncate().text_color(done ? "faint" : "fg"))
      .when(r.flagged, (el) => el.child(div().size(6).rounded_full().bg("orange")))
      .when(r.due != null, (el) => el.child(text(dueLabel(r)).text_size(11).font_medium().tabular_nums()
        .text_color(done ? "faint" : late ? "red" : "muted")));
  }

  button(id, glyph, onClick) {
    return div().id(id).size(26).rounded_full().items_center().justify_center().bg("surface").hover_bg("raised")
      .on_click((_, cx) => onClick(cx))
      .child(glyph);
  }

  pills(lists, current) {
    // Four to a row, so many lists still fit a narrow island.
    const pill = (l) => {
      const on = l.id === current.id;
      const color = l.color ?? "blue";
      const open = l.items.filter((r) => !this.ticked[r.id]).length;
      return row().id(`pill-${l.id}`).flex_1().gap(6).px(10).py(5).rounded_full().justify_center()
        .bg(on ? "raised" : "surface").hover_bg("raised")
        .on_click((_, cx) => this.select(cx, l.id))
        .child(div().size(7).flex_none().rounded_full().bg(color))
        .child(text(l.name).text_size(12).font_medium().truncate().text_color(on ? "fg" : "muted"))
        .when(open, (p) => p.child(text(String(open)).text_size(11).tabular_nums().text_color("faint")));
    };
    const rows = [];
    for (let i = 0; i < lists.length; i += 4) rows.push(row().id(`pills-${i}`).w_full().gap(6).children(lists.slice(i, i + 4).map(pill)));
    return col().id("pills").w_full().gap(6).children(rows);
  }

  expanded() {
    const lists = this.lists ?? [];
    const list = this.list();
    const page = col().id("reminders").w_full().gap(10).px(4).py(6);

    if (!list) {
      return page.child(row().id("empty").gap(8).py(12).justify_center()
        .child(icon(this.error ? "triangle-alert" : "list-checks", 16).color(this.error ? "orange" : "muted"))
        .child(text(this.error ?? "Loading Reminders…").flex_1().text_size(13).line_clamp(2).text_color("muted")));
    }

    const color = list.color ?? "blue";
    const items = list.items;
    const open = items.filter((r) => !this.ticked[r.id]).length;
    const header = row().id("header").w_full().gap(8).px(4)
      .child(text(list.name).id("list-name").text_size(17).font_bold().text_color(color).truncate())
      .child(text(open ? String(open) : "").id("list-count").text_size(15).font_semibold().tabular_nums().text_color("muted"))
      .child(spacer())
      .child(this.button("open-app", image("app:com.apple.reminders").size(16), () => exec("open", ["-a", "Reminders"])))
      .child(this.button("add", icon("plus", 14).color("fg"), () => this.field.focus()));

    const body = col().id(`items-${list.id}`).w_full().gap(1);
    if (!items.length) {
      body.child(row().id("all-done").w_full().gap(8).py(14).justify_center()
        .child(icon("check", 15).color(color))
        .child(text("All done").text_size(13).text_color("muted")));
    } else {
      body.children(items.slice(0, this.max).map((r) => this.item(r, color)));
      if (items.length > this.max) {
        body.child(row().id("more").px(8).py(4).rounded(10).hover_bg("surface")
          .on_click(() => exec("open", ["-a", "Reminders"]))
          .child(text(`${items.length - this.max} more in Reminders`).text_size(12).text_color("muted")));
      }
    }

    const typing = this.field.is_focused();
    const composer = row().id("composer").w_full().gap(10).px(8).py(6).rounded(10)
      .bg(typing ? "surface" : "#00000000")
      .border(1, typing ? color : "#00000000")
      .child(div().id("composer-dot").size(18).flex_none().rounded_full().items_center().justify_center()
        .border(1.5, typing ? color : "faint")
        .child(icon("plus", 11).color(typing ? color : "faint")))
      .child(Input.new(this.field).id("new-reminder").flex_1().text_size(13))
      .when(typing && this.field.value().trim(), (r) => r.child(text("↩").id("composer-hint").text_size(12).text_color("muted")));

    return page
      .child(header)
      .when(lists.length > 1, (p) => p.child(this.pills(lists, list)))
      .child(body)
      .child(composer)
      .when(this.error, (p) => p.child(text(this.error).id("error").text_size(11).line_clamp(2).text_color("orange")));
  }
}
