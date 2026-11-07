// The playing track: artwork and a level meter beside the camera while it plays; artwork,
// title, progress and controls on its page. Clone this folder to make your own player.

import { View, row, col, text, icon, image, div, shader } from "nook/ui";
import { media, island, run } from "nook";

// "#rrggbb" as the 0–1 channels a shader takes.
const rgb = (hex) => { const n = parseInt(hex.slice(1), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => c / 255); };

const clock = (s) => {
  if (s == null || !isFinite(s)) return "–:––";
  const t = Math.max(0, Math.floor(s));
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, "0")}`;
};

// Where playback is now: the published position plus the time since it was sampled.
const position = (m) => (m.elapsed ?? 0) + (m.playing && m.sampledAt ? (Date.now() - m.sampledAt) / 1000 : 0);

// A cover colour bright enough to read on the black island: dark ones keep their hue and
// are lifted to about half brightness.
function lift(hex) {
  const n = parseInt(hex.slice(1), 16);
  let [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  if (lum < 0.45) {
    const k = Math.min(0.45 / Math.max(lum, 0.02), 4);
    [r, g, b] = [r, g, b].map((c) => Math.min(255, Math.round(c * k + (k > 3 ? 40 : 0))));
  }
  return `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

// The app playing, as its icon: the tab shows it.
const appIcon = (m) => `app:${m?.bundleId || "com.apple.Music"}`;

function art(m, size, radius) {
  const src = m.artwork ?? (m.bundleId ? `app:${m.bundleId}` : null);
  return src
    ? image(src).id(`art-${size}`).size(size).rounded(radius).clip()
    : div().id(`art-${size}`).size(size).rounded(radius).bg("raised").items_center().justify_center().child(icon("music", size * 0.5).color("muted"));
}

function button(name, size, command) {
  return row().id(`btn-${command}`).size(size + 14).rounded_full().items_center().justify_center()
    .hover_bg("raised")
    .on_click(() => run(command))
    .child(icon(name, size));
}

export default class NowPlaying extends View {
  init(_props, cx) {
    // Tick the progress bar while the page is open and something plays.
    cx.timer.every(1000, (cx) => {
      if (island()?.expanded && media()?.playing) cx.notify();
    });
  }

  activity() {
    const m = media();
    if (!m?.playing) return null;
    // The level meter (meter.wgsl): bars out from the middle, as loud as what's playing, in
    // the cover's colours. 60 fps: the bars follow the beat.
    const [top, , low] = (m.colors ?? ["#30d158", "#30d158", "#30d158"]).map(lift);
    const meter = shader("./meter.wgsl", [...rgb(top), ...rgb(low), m.playing ? 1 : 0]).id("meter").w(18).h(14).fps(60);
    const line = [m.title, m.artist].filter(Boolean).join(" — ");
    return { leading: art(m, 20, 5), trailing: meter, detail: text(line).text_size(12).font_medium().truncate(), priority: 10 };
  }

  icon() {
    return appIcon(media());
  }

  expanded() {
    const m = media();
    if (!m) {
      return row().id("idle").gap(10).py(14).child(icon("music", 18).color("faint")).child(text("Nothing playing").text_color("muted"));
    }
    const now = position(m);
    const ratio = m.duration ? Math.min(1, now / m.duration) : 0;
    const progress = row().id("progress").w_full().h(4).rounded_full().bg("raised").clip()
      .child(div().id("progress-fill").grow(Math.max(ratio, 0.0001)).h_full().bg("fg").spring(0.9))
      .child(div().id("progress-rest").grow(Math.max(1 - ratio, 0.0001)).h_full());
    return row().id("player").w_full().gap(14).py(6).items_center()
      .child(art(m, 72, 14))
      // min_w(0): a long title truncates instead of pushing the controls off the page.
      .child(
        col().flex_1().min_w(0).gap(4)
          .child(text(m.title ?? "").text_size(15).font_semibold().truncate())
          .child(text([m.artist, m.album].filter(Boolean).join(" — ")).text_size(12.5).text_color("muted").truncate())
          .child(div().h(6))
          .child(progress)
          .child(
            row().w_full().justify_between()
              .child(text(clock(now)).text_size(10.5).text_color("faint").tabular_nums())
              .child(text(clock(m.duration)).text_size(10.5).text_color("faint").tabular_nums()),
          ),
      )
      .child(
        row().flex_none().gap(2)
          .child(button("skip-back-fill", 16, "media.previous"))
          .child(button(m.playing ? "pause-fill" : "play-fill", 20, "media.toggle"))
          .child(button("skip-forward-fill", 16, "media.next")),
      );
  }
}
