// The timer's ring: a track, and an arc clockwise from twelve o'clock with round ends.
// P(0) the part left (0–1), P(1) the ring's width as a fraction of its radius, P(2) 1 while
// paused (the arc greys).

fn scene(frag: vec2f, res: vec2f, t: f32) -> vec4f {
  let p = frag - res * 0.5;
  let radius = min(res.x, res.y) * 0.5;
  let width = radius * P(1);
  let mid = radius - width * 0.5;
  let d = length(p);
  let track = abs(d - mid) - width * 0.5;

  let left = clamp(P(0), 0.0, 1.0);
  let turn = fract(atan2(p.x, -p.y) / 6.2831853 + 1.0);
  let end = left * 6.2831853;
  let start_cap = length(p - vec2f(0.0, -mid)) - width * 0.5;
  let end_cap = length(p - vec2f(sin(end), -cos(end)) * mid) - width * 0.5;
  var arc = min(start_cap, end_cap);
  if turn <= left { arc = min(arc, track); }
  if left <= 0.0 { arc = 1e4; }

  let track_a = clamp(0.5 - track, 0.0, 1.0);
  let arc_a = clamp(0.5 - arc, 0.0, 1.0);
  let orange = vec3f(1.0, 0.624, 0.039);
  let tint = mix(orange, vec3f(0.56, 0.56, 0.58), clamp(P(2), 0.0, 1.0));
  let under = vec4f(vec3f(1.0), 0.14 * track_a);
  let a = arc_a + under.a * (1.0 - arc_a);
  if a <= 0.0 { return vec4f(0.0); }
  return vec4f((tint * arc_a + under.rgb * under.a * (1.0 - arc_a)) / a, a);
}
