// Runs once in every mod's context, before the mod. Rust talks to the mod only through the
// `__nook_*` functions defined here and the two natives it installs first:
//   __host_call(op, json) -> json   synchronous (reads, timers, storage, commands)
//   __host_start(op, json) -> id    asynchronous; settles later through __nook_settle
"use strict";

const call = (op, value) => {
  const out = __host_call(op, JSON.stringify(value ?? null));
  return out === undefined || out === "" ? undefined : JSON.parse(out);
};

const pending = new Map();
function start(op, value) {
  return new Promise((resolve, reject) => {
    const id = __host_start(op, JSON.stringify(value ?? null));
    pending.set(id, { resolve, reject });
  });
}

// Handlers from the last two renders: a press that lands just after a re-render still finds
// the handler the person saw.
let handlers = new Map();
let previous = new Map();
let nextHandler = 1;
const timers = new Map();
const subscriptions = new Map();
let nextTimer = 1;

function describe(error) {
  if (error instanceof Error) return `${error.name}: ${error.message}\n${error.stack ?? ""}`.trim();
  return String(error);
}

globalThis.__nook_describe = describe;

function report(where, error) {
  call("error", `${where}: ${describe(error)}`);
}

const cx = {
  notify() { call("notify"); },
  timer: {
    every(ms, f) { return addTimer(ms, f, true); },
    after(ms, f) { return addTimer(ms, f, false); },
  },
  // Runs an async function with this cx; a rejection lands in the mod's error log.
  spawn(f) { Promise.resolve().then(() => f(cx)).catch((e) => report("spawn", e)); },
};

function addTimer(ms, f, repeat) {
  const id = nextTimer++;
  timers.set(id, { f, repeat });
  call("timer", { id, ms: Math.max(repeat ? 50 : 0, Number(ms) || 0), repeat });
  return () => { timers.delete(id); call("timer.cancel", { id }); };
}

function serialize(node) {
  if (node === null || node === undefined || node === false) return null;
  if (typeof node === "string" || typeof node === "number") return { t: "text", v: String(node) };
  if (typeof node.__node !== "function") throw new TypeError(`not an element: ${JSON.stringify(node)?.slice(0, 80)}`);
  const raw = node.__node();
  const out = { t: raw.t };
  if (raw.k !== undefined) out.k = raw.k;
  if (raw.v !== undefined) out.v = raw.v;
  if (Object.keys(raw.s).length) out.s = raw.s;
  if (raw.on) {
    out.on = {};
    for (const [event, f] of Object.entries(raw.on)) {
      const id = nextHandler++;
      handlers.set(id, f);
      out.on[event] = id;
    }
  }
  const children = raw.c.map(serialize).filter((c) => c !== null);
  if (children.length) out.c = children;
  return out;
}

function renderSlot(view, slot) {
  const f = view[slot];
  if (typeof f !== "function") return { none: true };
  try {
    const value = f.call(view, cx);
    if (value === null || value === undefined) return { value: null };
    if (slot === "activity") {
      const { leading, trailing, detail, priority, width, ...rest } = value;
      const extra = Object.keys(rest);
      if (extra.length) throw new TypeError(`activity() returns { leading, trailing, detail, priority, width }; unknown: ${extra.join(", ")}`);
      return { value: { leading: serialize(leading), trailing: serialize(trailing), detail: serialize(detail), priority: priority ?? 0, width: width ?? null } };
    }
    return { value: serialize(value) };
  } catch (e) {
    return { error: `${slot}(): ${describe(e)}` };
  }
}

let view = null;

globalThis.__nook_mount = (Class, propsJson) => {
  if (typeof Class !== "function") throw new TypeError("main.js must `export default class … extends View`");
  view = new Class();
  if (typeof view.init === "function") view.init(JSON.parse(propsJson), cx);
};

globalThis.__nook_render = () => {
  previous = handlers;
  handlers = new Map();
  let icon = null;
  if (typeof view.icon === "function") {
    try { icon = view.icon(cx) ?? null; } catch (e) { report("icon()", e); }
  }
  return JSON.stringify({ activity: renderSlot(view, "activity"), expanded: renderSlot(view, "expanded"), icon: icon === null ? null : String(icon) });
};

globalThis.__nook_dispatch = (id, payloadJson) => {
  const f = handlers.get(id) ?? previous.get(id);
  if (!f) return;
  try { f(JSON.parse(payloadJson), cx); } catch (e) { report("handler", e); }
};

globalThis.__nook_fire = (id) => {
  const t = timers.get(id);
  if (!t) return;
  if (!t.repeat) timers.delete(id);
  try { t.f(cx); } catch (e) { report("timer", e); }
};

globalThis.__nook_settle = (id, ok, json) => {
  const p = pending.get(id);
  if (!p) return;
  pending.delete(id);
  if (ok) p.resolve(JSON.parse(json));
  else p.reject(new Error(JSON.parse(json)));
};

globalThis.__nook_event = (id, json) => {
  const f = subscriptions.get(id);
  if (!f) return;
  try { f(JSON.parse(json), cx); } catch (e) { report("subscription", e); }
};

globalThis.__nook_unmount = () => {
  try { view?.unmount?.(cx); } catch (e) { report("unmount", e); }
};

function subscribe(op, args, f) {
  const id = call(op, args);
  subscriptions.set(id, f);
  return () => { subscriptions.delete(id); call("unsubscribe", { id }); };
}

const format = (args) => args.map((a) => (typeof a === "string" ? a : (() => { try { return JSON.stringify(a); } catch { return String(a); } })())).join(" ");
globalThis.console = {
  log: (...a) => call("log", { level: "info", text: format(a) }),
  info: (...a) => call("log", { level: "info", text: format(a) }),
  warn: (...a) => call("log", { level: "warn", text: format(a) }),
  error: (...a) => call("log", { level: "error", text: format(a) }),
};

globalThis.localStorage = {
  getItem: (k) => call("storage.get", { key: String(k) }) ?? null,
  setItem: (k, v) => { call("storage.set", { key: String(k), value: String(v) }); },
  removeItem: (k) => { call("storage.set", { key: String(k), value: null }); },
  clear: () => { call("storage.clear"); },
};

class Response {
  constructor(r) { this.status = r.status; this.ok = r.status >= 200 && r.status < 300; this.headers = r.headers; this._body = r.body; }
  async text() { return this._body; }
  async json() { return JSON.parse(this._body); }
}
globalThis.fetch = async (url, init = {}) => new Response(await start("fetch", {
  url: String(url), method: init.method ?? "GET", headers: init.headers ?? {}, body: init.body ?? null,
}));

// What the `nook` and `nook/ui` modules build on; not for mods.
globalThis.__nook = { call, start, subscribe };
