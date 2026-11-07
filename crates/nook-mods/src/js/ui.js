// The `nook/ui` module: elements a mod's views return. They're descriptions; Rust draws them
// with Rune, and springs every change between renders by element id.

class Element {
  constructor(t, v) {
    this.t = t;
    this.v = v;
    this.s = {};
    this.c = [];
    this.on = null;
    this.k = undefined;
  }
  __node() { return this; }
  set(key, value) { this.s[key] = value; return this; }

  /** Names the element so it animates between renders instead of being replaced. */
  id(key) { this.k = String(key); return this; }
  child(c) {
    if (c === null || c === undefined || c === false) return this;
    this.c.push(typeof c === "string" || typeof c === "number" ? text(c) : c);
    return this;
  }
  children(list) { for (const c of list ?? []) this.child(c); return this; }
  when(cond, f) { return cond ? (f(this) ?? this) : this; }

  // Size, in points. `w_full`/`h_full` fill the parent.
  w(v) { return this.set("w", v); }
  h(v) { return this.set("h", v); }
  size(v) { return this.set("w", v).set("h", v); }
  w_full() { return this.set("w", "full"); }
  h_full() { return this.set("h", "full"); }
  size_full() { return this.w_full().h_full(); }
  min_w(v) { return this.set("min_w", v); }
  max_w(v) { return this.set("max_w", v); }
  min_h(v) { return this.set("min_h", v); }
  max_h(v) { return this.set("max_h", v); }
  flex_1() { return this.set("grow", 1); }
  grow(v = 1) { return this.set("grow", v); }
  flex_none() { return this.set("shrink", 0); }
  shrink(v = 1) { return this.set("shrink", v); }

  // Spacing: padding as [top, right, bottom, left].
  pad(i, v) { const p = this.s.p ?? [0, 0, 0, 0]; for (const j of i) p[j] = v; return this.set("p", p); }
  p(v) { return this.pad([0, 1, 2, 3], v); }
  px(v) { return this.pad([1, 3], v); }
  py(v) { return this.pad([0, 2], v); }
  pt(v) { return this.pad([0], v); }
  pr(v) { return this.pad([1], v); }
  pb(v) { return this.pad([2], v); }
  pl(v) { return this.pad([3], v); }
  gap(v) { return this.set("gap", v); }
  /** Places the element over its parent's content, `null` insets left free (CSS absolute). */
  pin(left = null, top = null, right = null, bottom = null) { return this.set("pin", [left, top, right, bottom]); }
  /** Covers the parent: behind later siblings, e.g. a backdrop. */
  absolute_fill() { delete this.s.w; delete this.s.h; return this.pin(0, 0, 0, 0); }

  // Alignment of children (rows: vertical; columns: horizontal).
  items_start() { return this.set("items", "start"); }
  items_center() { return this.set("items", "center"); }
  items_end() { return this.set("items", "end"); }
  justify_start() { return this.set("justify", "start"); }
  justify_center() { return this.set("justify", "center"); }
  justify_end() { return this.set("justify", "end"); }
  justify_between() { return this.set("justify", "between"); }

  // Look. Colors: "#rgb", "#rrggbb", "#rrggbbaa", or a theme token ("fg", "muted", "accent", …).
  bg(c) { return this.set("bg", c); }
  /** A vertical gradient, top colour to bottom colour, over the background. */
  gradient(top, bottom) { return this.set("gradient", [top, bottom]); }
  rounded(r) { return this.set("rounded", r); }
  rounded_full() { return this.set("rounded", 9999); }
  border(width, color) { return this.set("border", [width, color]); }
  opacity(o) { return this.set("opacity", o); }
  shadow(y, blur, color) { return this.set("shadow", [y, blur, color]); }
  clip() { return this.set("clip", true); }
  hover_bg(c) { return this.set("hover_bg", c); }
  cursor_pointer() { return this.set("cursor", "pointer"); }

  // Text (on text elements, and inherited by nothing: style each text).
  text_size(v) { return this.set("text_size", v); }
  text_color(c) { return this.set("color", c); }
  font_weight(v) { return this.set("weight", v); }
  font_medium() { return this.font_weight(500); }
  font_semibold() { return this.font_weight(600); }
  font_bold() { return this.font_weight(700); }
  font_mono() { return this.set("mono", true); }
  tabular_nums() { return this.set("tnum", true); }
  truncate() { return this.set("truncate", true); }
  line_clamp(n) { return this.set("lines", n); }
  text_center() { return this.set("align", "center"); }
  text_right() { return this.set("align", "end"); }
  /** Icons: color (same as text_color). */
  color(c) { return this.set("color", c); }

  // Motion. Every change of a keyed element springs; these tune it.
  /** The spring for this element's layout changes: duration in seconds, bounce 0–1. */
  spring(duration, bounce = 0) { return this.set("spring", [duration, bounce]); }
  /** How the element enters and leaves: "fade", "scale", "blur", "slide". */
  enter(kind) { return this.set("enter", kind); }
  /** Pulse opacity forever with this period in seconds (a "live" dot). */
  pulse(period = 1.2) { return this.set("pulse", period); }
  /** Spin an icon forever with this period in seconds (a spinner). */
  spin(period = 1) { return this.set("spin", period); }
  scale(v) { return this.set("scale", v); }
  /** Shader elements: frames a second (default 30; 0 draws one still frame). */
  fps(n) { return this.set("fps", n); }
  offset(x, y) { return this.set("offset", [x, y]); }

  // Input. Handlers get (event, cx); call cx.notify() after changing what render reads.
  on_click(f) { this.on = { ...this.on, click: f }; return this.set("cursor", this.s.cursor ?? "pointer"); }
  on_hover(f) { this.on = { ...this.on, hover: f }; return this; }
}

/** A box; children stack vertically. */
export const div = () => new Element("box").set("dir", "col");
export const col = div;
export const v_flex = div;
/** A box; children sit side by side, vertically centered. */
export const row = () => new Element("box").set("dir", "row").set("items", "center");
export const h_flex = row;
export const text = (s) => new Element("text", String(s ?? ""));
/** A built-in icon by name (see icons in ui.d.ts), or "./file.svg" from the mod's folder. */
export const icon = (name, size = 16) => new Element("icon", String(name)).set("w", size).set("h", size);
/**
 * An image: "https://…", an absolute path, "data:image/png;base64,…", "./file.png" from the
 * mod's folder, or "app:<bundle id>" for an app's icon.
 */
export const image = (src) => new Element("image", String(src ?? ""));
export const spacer = () => new Element("box").set("grow", 1);
/**
 * A Metal fragment shader from the mod's folder ("./scene.metal"), drawn animated in its box.
 * `params` (up to 8 numbers) reach the shader as P(0)…P(7). See AGENTS.md › Shaders.
 */
export const shader = (src, params = []) => {
  if (params.length > 8) throw new RangeError("shader(): at most 8 params");
  return new Element("shader", String(src)).set("params", params.map(Number)).set("w", "full").set("h", 120);
};

/** Base class every mod's default export extends. */
export class View {}
