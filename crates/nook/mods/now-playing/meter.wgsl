// The closed island's level meter: five 2-point bars 2 points apart, growing and shrinking
// about their middle line, up to the box's height, as loud as what's playing (LEVEL: the
// Mac's own audio in five bands). The bass is the middle bar, the mids beside it, the treble
// outside. P(0..2) and P(3..5) are the bars' top and bottom colours (sRGB, 0–1); P(6) is 1
// while playing (paused rests at 7/12/9/14/8).

// Which band each bar shows, left to right.
fn band(i: i32) -> i32 {
  switch i {
    case 0: { return 3; }
    case 1: { return 1; }
    case 2: { return 0; }
    case 3: { return 2; }
    default: { return 4; }
  }
}

fn scene(frag: vec2f, res: vec2f, t: f32) -> vec4f {
  // 5 bars and 4 gaps of equal width: 9 units, a bar on every even one.
  let unit = res.x / 9.0;
  let slot = floor(frag.x / unit);
  let i = i32(slot / 2.0);
  if (i32(slot) % 2 == 1 || i > 4) { return vec4f(0.0); }
  var rest = array<f32, 5>(0.5, 12.0 / 14.0, 9.0 / 14.0, 1.0, 8.0 / 14.0);
  let floor_h = 2.0 / 14.0;
  let s = select(rest[i], floor_h + (1.0 - floor_h) * clamp(LEVEL(band(i)), 0.0, 1.0), P(6) > 0.5);
  let h = res.y * s;
  let top = (res.y - h) * 0.5;
  let y = frag.y - top;
  // Round ends: a capsule as wide as the bar.
  let cx = (slot + 0.5) * unit;
  let r = unit * 0.5;
  let cy = clamp(frag.y, top + r, top + h - r);
  let d = length(vec2f(frag.x - cx, frag.y - cy)) - r;
  let a = clamp(0.5 - d, 0.0, 1.0);
  let k = clamp(y / max(h, 1.0), 0.0, 1.0);
  let c = mix(vec3f(P(0), P(1), P(2)), vec3f(P(3), P(4), P(5)), k);
  return vec4f(c, a);
}
