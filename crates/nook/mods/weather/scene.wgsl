// The weather card's sky: twelve moods (P(0): 0 clear, 1 cloudy, 2 overcast, 3 rain, 4 storm,
// 5 snow, 6 sleet, 7 fog, 8 wind, 9 cold, 10 night, 11 heat), P(1) night 0–1, P(2) intensity,
// P(3) dim, P(4) the moon's phase 0–1 (0 new, 0.5 full), P(5) -1 south of the equator. Ported from openNook 1.x's Metal scene (originally weather.glsl).

fn glsl_mod(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }

// smoothstep that also takes a falling edge (e0 > e1), which the scene uses on purpose.
fn ss(e0: f32, e1: f32, x: f32) -> f32 {
  let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

fn hash(p0: vec2f) -> f32 {
  var p = fract(p0 * vec2f(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

fn noise(p: vec2f) -> f32 {
  let i = floor(p);
  var f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  let a = hash(i);
  let b = hash(i + vec2f(1.0, 0.0));
  let c = hash(i + vec2f(0.0, 1.0));
  let d = hash(i + vec2f(1.0, 1.0));
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn fbm(p0: vec2f) -> f32 {
  var p = p0;
  var v = 0.0;
  var a = 0.5;
  for (var i = 0; i < 5; i++) {
    v += a * noise(p);
    p = p * 2.03 + vec2f(17.1, 9.2);
    a *= 0.5;
  }
  return v;
}

fn clouds(p: vec2f, t: f32, cover: f32, speed: f32) -> f32 {
  let q = p * vec2f(1.4, 2.6) + vec2f(t * speed, 0.0);
  let n = fbm(q + fbm(q * 0.5 + t * 0.03));
  return ss(1.0 - cover, 1.0 - cover + 0.35, n);
}

fn rain(p0: vec2f, t: f32, cols: f32, speed: f32, seed: f32) -> f32 {
  var p = p0;
  p.x += p.y * 0.22;
  let c = floor(p.x * cols);
  let r = hash(vec2f(c, seed));
  let fx = fract(p.x * cols) - 0.5;
  let y = fract(p.y * 1.3 + t * speed * (0.8 + 0.5 * r) + r * 11.0);
  let streak = ss(0.0, 0.015, y) * (1.0 - ss(0.015, 0.16, y));
  let w = 1.0 - ss(0.0, 0.12, abs(fx));
  return streak * w * step(0.45, r);
}

fn snow(p0: vec2f, t: f32, scale: f32, seed: f32) -> f32 {
  var p = p0 * scale;
  p.y += t * 0.45 * (1.0 + seed * 0.3);
  p.x += sin(p.y * 0.9 + t * 0.8 + seed) * 0.25;
  let id = floor(p);
  let f = fract(p) - 0.5;
  let r = hash(id + seed);
  let o = vec2f(hash(id + 3.1 + seed), hash(id + 7.7 + seed)) - 0.5;
  let d = length(f - o * 0.6);
  return step(0.35, r) * ss(0.07 * (0.6 + r), 0.0, d);
}

fn stars(p: vec2f, t: f32) -> f32 {
  let g = p * 38.0;
  let id = floor(g);
  let f = fract(g) - 0.5;
  let r = hash(id);
  let o = vec2f(hash(id + 1.3), hash(id + 4.9)) - 0.5;
  let d = length(f - o * 0.7);
  let tw = 0.55 + 0.45 * sin(t * (1.5 + r * 3.0) + r * 40.0);
  return step(0.86, r) * ss(0.06, 0.0, d) * tw;
}

// One of the moon's seas: a soft dark patch around `c` on the face, `r` across.
fn sea(q: vec2f, c: vec2f, r: vec2f, edge: f32) -> f32 {
  return ss(1.0, 0.55, length((q - c) / r) + edge);
}

// The moon, `r` across at `c`: a sphere lit from the sun's side for `phase`, with the near
// side's maria where they are, a soft terminator and earthshine on the dark part. `hemi` -1
// turns it over, as it looks from the southern hemisphere.
struct Moon { day: vec3f, lit: f32, cover: f32, albedo: f32 }

fn moon(p: vec2f, c: vec2f, r: f32, phase: f32, hemi: f32, px: f32) -> Moon {
  let q = (p - c) / r * hemi;
  let d = length(q);
  let cover = ss(1.0 + px / r, 1.0 - px / r, d);
  if (cover <= 0.0) { return Moon(vec3f(0.0), 0.0, 0.0, 0.0); }
  let n = vec3f(q, sqrt(max(1.0 - d * d, 0.0)));
  let a = phase * 6.2831853;
  // Waxing, the sun is to the right (+x); full, it's behind the viewer (+z).
  let lit = ss(-0.04, 0.16, dot(n, vec3f(sin(a), 0.0, -cos(a))));
  let edge = (noise(q * 4.0 + 7.3) - 0.5) * 0.35;
  var seas = sea(q, vec2f(-0.52, 0.02), vec2f(0.34, 0.50), edge);   // Oceanus Procellarum
  seas = max(seas, sea(q, vec2f(-0.26, 0.44), vec2f(0.27, 0.22), edge));  // Imbrium
  seas = max(seas, sea(q, vec2f(0.20, 0.40), vec2f(0.15, 0.15), edge));   // Serenitatis
  seas = max(seas, sea(q, vec2f(0.36, 0.10), vec2f(0.19, 0.16), edge));   // Tranquillitatis
  seas = max(seas, sea(q, vec2f(0.70, 0.30), vec2f(0.10, 0.09), edge));   // Crisium
  seas = max(seas, sea(q, vec2f(0.56, -0.16), vec2f(0.12, 0.15), edge));  // Fecunditatis
  seas = max(seas, sea(q, vec2f(0.33, -0.26), vec2f(0.08, 0.08), edge));  // Nectaris
  seas = max(seas, sea(q, vec2f(-0.20, -0.36), vec2f(0.17, 0.13), edge)); // Nubium
  seas = max(seas, sea(q, vec2f(-0.48, -0.40), vec2f(0.08, 0.08), edge)); // Humorum
  let highlands = fbm(q * 6.0 + 11.0);
  let tycho = ss(0.07, 0.0, length(q - vec2f(-0.12, -0.72)));
  var albedo = 0.88 - 0.36 * seas - 0.14 * highlands + 0.12 * tycho;
  // Nearly flat when full, a little dimmer toward the limb.
  albedo *= 0.84 + 0.16 * n.z;
  return Moon(vec3f(0.97, 0.95, 0.89) * albedo, lit, cover, albedo);
}

fn gust(p: vec2f, t: f32, aspect: f32, y0: f32, amp: f32, freq: f32, speed: f32, len: f32, seed: f32, res_y: f32) -> f32 {
  let cycle = aspect + len + 0.6;
  let hx = glsl_mod(t * speed + seed * cycle, cycle) - 0.3;
  let along = hx - p.x;
  if (along < 0.0 || along > len) { return 0.0; }
  let k = along / len;
  let y = y0 + amp * sin(p.x * freq + seed * 6.2831 + t * 0.7) + amp * 0.4 * sin(p.x * freq * 2.3 - t * 0.9);
  let d = abs(p.y - y);
  let px = 1.0 / res_y;
  let th = mix(1.1, 0.25, k) * px * ss(0.0, 0.14, along);
  let line = 1.0 - ss(th, th + 1.2 * px, d);
  let glow = exp(-d / (4.0 * px)) * 0.10;
  let fade = (1.0 - ss(0.25, 1.0, k)) * ss(0.0, 0.18, along);
  return (line + glow) * fade;
}

fn scene(frag_in: vec2f, res: vec2f, t: f32) -> vec4f {
  // The original works bottom-up, as GLSL's gl_FragCoord does.
  let frag = vec2f(frag_in.x, res.y - frag_in.y);
  var uv = frag / res;
  let aspect = res.x / res.y;
  let m = floor(P(0) + 0.5);

  var top = vec3f(0.12, 0.40, 0.80);
  var bot = vec3f(0.36, 0.62, 0.90);
  var sun = 0.0; var cloud = 0.0; var cloudDark = 0.0; var cloudSpeed = 0.02;
  var rainAmt = 0.0; var snowAmt = 0.0; var fogAmt = 0.0; var windAmt = 0.0;
  var starAmt = 0.0; var moonAmt = 0.0; var flashAmt = 0.0; var heatAmt = 0.0; var frostAmt = 0.0;
  var cloudTint = vec3f(1.0);

  if (m < 0.5) {
    sun = 1.0; cloud = 0.12;
  } else if (m < 1.5) {
    top = vec3f(0.26, 0.40, 0.58); bot = vec3f(0.50, 0.61, 0.74);
    sun = 0.35; cloud = 0.55; cloudDark = 0.15;
  } else if (m < 2.5) {
    top = vec3f(0.26, 0.29, 0.34); bot = vec3f(0.40, 0.43, 0.48);
    cloud = 0.9; cloudDark = 0.45; cloudTint = vec3f(0.78, 0.80, 0.84);
  } else if (m < 3.5) {
    top = vec3f(0.16, 0.21, 0.30); bot = vec3f(0.28, 0.34, 0.44);
    cloud = 0.8; cloudDark = 0.55; rainAmt = 1.0; cloudSpeed = 0.035;
    cloudTint = vec3f(0.70, 0.75, 0.84);
  } else if (m < 4.5) {
    top = vec3f(0.08, 0.07, 0.15); bot = vec3f(0.20, 0.17, 0.32);
    cloud = 0.9; cloudDark = 0.7; rainAmt = 1.2; flashAmt = 1.0; cloudSpeed = 0.06;
    cloudTint = vec3f(0.62, 0.56, 0.82);
  } else if (m < 5.5) {
    top = vec3f(0.30, 0.40, 0.56); bot = vec3f(0.50, 0.58, 0.72);
    cloud = 0.6; cloudDark = 0.1; snowAmt = 1.0;
  } else if (m < 6.5) {
    top = vec3f(0.28, 0.35, 0.46); bot = vec3f(0.45, 0.52, 0.62);
    cloud = 0.75; cloudDark = 0.35; rainAmt = 0.55; snowAmt = 0.55; cloudSpeed = 0.03;
  } else if (m < 7.5) {
    top = vec3f(0.30, 0.33, 0.38); bot = vec3f(0.44, 0.46, 0.50);
    cloud = 0.3; cloudDark = 0.2; fogAmt = 1.0;
  } else if (m < 8.5) {
    top = vec3f(0.17, 0.33, 0.50); bot = vec3f(0.40, 0.55, 0.68);
    windAmt = 1.0;
  } else if (m < 9.5) {
    top = vec3f(0.16, 0.38, 0.62); bot = vec3f(0.40, 0.64, 0.82);
    sun = 0.4; cloud = 0.15; frostAmt = 1.0;
  } else if (m < 10.5) {
    top = vec3f(0.02, 0.03, 0.10); bot = vec3f(0.09, 0.11, 0.28);
    starAmt = 1.0; moonAmt = 1.0; cloud = 0.15; cloudDark = 0.6; cloudTint = vec3f(0.35, 0.38, 0.55);
  } else {
    top = vec3f(0.60, 0.26, 0.07); bot = vec3f(0.78, 0.44, 0.15);
    sun = 1.3; heatAmt = 1.0;
  }

  let night = clamp(P(1), 0.0, 1.0);
  if (night > 0.0) {
    top = mix(top, top * 0.16 + vec3f(0.010, 0.018, 0.060), night);
    bot = mix(bot, bot * 0.26 + vec3f(0.020, 0.030, 0.090), night);
    // A partly clouded night still has its moon; the clouds drift over it.
    moonAmt = max(moonAmt, min(sun * 2.5, 1.0) * night);
    sun *= 1.0 - night;
    starAmt = max(starAmt, night * clamp(1.0 - cloud * 1.15 - fogAmt * 0.5, 0.0, 1.0));
    cloudTint = mix(cloudTint, cloudTint * vec3f(0.46, 0.50, 0.64), night);
  }
  let nightFade = 1.0 - 0.4 * night;

  if (heatAmt > 0.0) {
    uv.x += sin(uv.y * 38.0 + t * 3.0) * 0.004 * (1.0 - uv.y) * heatAmt;
  }

  let p = vec2f(uv.x * aspect, uv.y);
  var col = mix(bot, top, ss(0.0, 1.0, uv.y));

  if (sun > 0.0) {
    let sp = vec2f(0.86 * aspect, 1.02);
    let dv = p - sp;
    let d = length(dv);
    let ang = atan2(dv.y, dv.x);
    let rays = 0.5 + 0.5 * sin(ang * 9.0 + t * 0.25) * sin(ang * 5.0 - t * 0.15);
    let sunCol = select(vec3f(1.0, 0.95, 0.80), vec3f(1.0, 0.86, 0.55), heatAmt > 0.0);
    col += sunCol * exp(-d * 3.2) * 0.55 * sun;
    col += sunCol * rays * exp(-d * 1.8) * 0.12 * sun;
    col += sunCol * ss(0.13, 0.10, d) * 0.5 * sun;
  }

  var behindMoon = 0.0;
  if (moonAmt > 0.0) {
    // In the open sky between the temperature and the wind / humidity / UV column.
    let r = 0.105;
    let mp = vec2f(max(aspect - 1.2, aspect * 0.55), 0.72);
    let md = length(p - mp);
    let phase = fract(P(4));
    let full = 0.5 - 0.5 * cos(phase * 6.2831853);
    let hemi = select(1.0, -1.0, P(5) < 0.0);
    // Its halo, as bright as the lit part of it.
    col += vec3f(0.55, 0.62, 0.85) * (exp(-max(md - r, 0.0) * 16.0) * 0.16 + exp(-md * 4.0) * 0.05) * full * moonAmt;
    let m = moon(p, mp, r, phase, hemi, 1.0 / res.y);
    // The sky's glow is in front of the moon: its dark part keeps the sky's color, with a
    // faint earthshine, and only hides the stars behind it.
    behindMoon = m.cover * moonAmt;
    col += vec3f(0.030, 0.034, 0.050) * m.albedo * behindMoon;
    col = mix(col, m.day, m.lit * behindMoon);
  }

  if (starAmt > 0.0) {
    col += vec3f(0.9, 0.93, 1.0) * stars(p, t) * starAmt * ss(0.1, 0.7, uv.y) * (1.0 - behindMoon);
  }

  var flash = 0.0;
  if (flashAmt > 0.0) {
    let ft = fract(t * 0.21);
    flash = (ss(0.0, 0.01, ft) * (1.0 - ss(0.01, 0.05, ft)) + 0.6 * ss(0.07, 0.08, ft) * (1.0 - ss(0.08, 0.12, ft))) * flashAmt;
    col += vec3f(0.55, 0.50, 0.85) * flash * 0.45;
  }

  if (cloud > 0.0) {
    let c1 = clouds(p, t, cloud * 0.75, cloudSpeed);
    let c2 = clouds(p * 1.7 + 5.0, t, cloud * 0.6, cloudSpeed * 1.6);
    let shade = fbm(p * 3.0 + t * cloudSpeed);
    var cc = mix(cloudTint, cloudTint * 0.45, cloudDark * (0.6 + 0.4 * shade));
    cc += vec3f(0.7, 0.65, 1.0) * flash * 0.6;
    let mask = ss(-0.1, 0.9, uv.y);
    col = mix(col, cc, c2 * 0.45 * mask);
    col = mix(col, cc * 1.05, c1 * 0.75 * mask);
  }

  if (fogAmt > 0.0) {
    let f1 = fbm(vec2f(p.x * 1.3 - t * 0.05, p.y * 5.0));
    let f2 = fbm(vec2f(p.x * 2.1 + t * 0.03, p.y * 7.0 + 3.0));
    let bands = ss(0.35, 0.8, f1) * 0.55 + ss(0.4, 0.85, f2) * 0.35;
    let low = 1.0 - ss(0.0, 0.85, uv.y);
    col = mix(col, mix(vec3f(0.66, 0.68, 0.72), vec3f(0.20, 0.22, 0.29), night), clamp(bands * (0.4 + low), 0.0, 1.0) * fogAmt * 0.7);
  }

  if (windAmt > 0.0) {
    let q = vec2f(p.x * 0.7 - t * 0.14, p.y * 5.5);
    let w = fbm(q + vec2f(fbm(q * 0.6 - t * 0.05) * 1.6, 0.0));
    let wisp = ss(0.45, 0.85, w) * ss(0.05, 0.85, uv.y);
    col = mix(col, vec3f(0.90, 0.94, 1.0) * nightFade, wisp * 0.42 * windAmt);

    let ry = res.y;
    var g = 0.0;
    g += gust(p, t, aspect, 0.80, 0.045, 3.1, 0.34, 0.95, 0.10, ry);
    g += gust(p, t, aspect, 0.56, 0.060, 2.4, 0.27, 1.20, 0.55, ry) * 0.8;
    g += gust(p, t, aspect, 0.30, 0.040, 3.7, 0.40, 0.80, 0.82, ry) * 0.7;
    g += gust(p, t, aspect, 0.12, 0.050, 2.8, 0.31, 1.00, 0.33, ry) * 0.6;
    col += vec3f(0.94, 0.97, 1.0) * g * 0.5 * windAmt * nightFade;

    var dg = vec2f(p.x * 5.0 - t * 1.9, p.y * 13.0);
    dg.y += sin(p.x * 3.0 + t * 1.3) * 0.6;
    let did = floor(dg);
    let df = fract(dg) - 0.5;
    let dr = hash(did + 21.0);
    let streak = length(vec2f(df.x * 0.28, df.y));
    col += vec3f(0.95, 0.98, 1.0) * step(0.93, dr) * ss(0.07, 0.0, streak) * 0.5 * windAmt * nightFade;
  }

  if (rainAmt > 0.0) {
    let r = rain(p, t, 55.0, 1.6, 1.0) * 0.55 + rain(p * 1.4 + 3.0, t, 80.0, 1.2, 2.0) * 0.3;
    col += vec3f(0.75, 0.82, 0.95) * r * 0.8 * rainAmt * nightFade;
  }

  if (snowAmt > 0.0) {
    let s = snow(p, t, 9.0, 0.0) + snow(p, t, 14.0, 1.0) * 0.7 + snow(p, t, 22.0, 2.0) * 0.45;
    col = mix(col, vec3f(1.0) * (1.0 - 0.2 * night), clamp(s, 0.0, 1.0) * 0.9 * snowAmt);
  }

  if (frostAmt > 0.0) {
    let g = p * 26.0;
    let id = floor(g);
    let r = hash(id);
    let d = length(fract(g) - 0.5);
    let tw = pow(0.5 + 0.5 * sin(t * 2.0 + r * 50.0), 6.0);
    col += vec3f(0.85, 0.95, 1.0) * step(0.9, r) * ss(0.08, 0.0, d) * tw * frostAmt;
    let edge = 1.0 - ss(0.0, 0.35, min(min(uv.x, 1.0 - uv.x) * aspect, min(uv.y, 1.0 - uv.y)));
    col = mix(col, vec3f(0.85, 0.94, 1.0) * nightFade, edge * 0.18 * frostAmt * fbm(p * 6.0));
  }

  col = mix(vec3f(0.0), col, P(2));
  col *= 1.0 - P(3) * (0.75 - 0.35 * uv.y);
  let vc = uv - 0.5;
  col *= 1.0 - dot(vc, vc) * 0.35;
  return vec4f(clamp(col, vec3f(0.0), vec3f(1.0)), 1.0);
}
