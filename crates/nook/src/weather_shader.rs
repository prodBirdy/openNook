//! Weather card backdrop: the Pencil `weather.glsl` scene, ported line by
//! line to Metal and rendered offscreen into a [`RenderImage`].
//!
//! GPUI has no custom-shader primitive, so the scene is drawn into a small
//! BGRA8 texture the size of the card in device pixels, read back, and handed
//! to `window.paint_image`. Device, queue, pipeline and texture are cached per
//! thread (all calls come from the main-thread paint path); the texture is
//! only rebuilt when the card size changes, and frames are capped at ~30 fps.
//! Any Metal failure is logged once and [`render`] returns `None` for good —
//! callers keep their gradient wash underneath, so the card never goes blank.

use gpui::RenderImage;
use std::sync::Arc;

/// `u_time` for a Reduce Motion still frame.
pub(crate) const STATIC_TIME: f32 = 12.0;

/// One painted frame. `stale` is the image this one replaced; the caller
/// drops it from the sprite atlas (`window.drop_image`) so frames don't pile
/// up in GPU memory.
pub(crate) struct Frame {
    pub image: Arc<RenderImage>,
    pub stale: Option<Arc<RenderImage>>,
}

/// Render (or reuse) the scene for `mood` (0..=11, the shader's `u_mood`)
/// at `width` × `height` device pixels. `animate` false renders a single
/// still at [`STATIC_TIME`] and keeps reusing it.
pub(crate) fn render(mood: u8, width: u32, height: u32, animate: bool) -> Option<Frame> {
    #[cfg(target_os = "macos")]
    {
        metal::render(mood, width, height, animate)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (mood, width, height, animate);
        None
    }
}

/// `weather.glsl` in Metal Shading Language. Same constants, same order of
/// operations; only the syntax changes:
/// - `gl_FragCoord` is bottom-left in GLSL, `[[position]]` top-left in Metal,
///   so `uv.y` is flipped to keep the image upright.
/// - GLSL `mod` floors (`x - y*floor(x/y)`), Metal `fmod` truncates, so
///   `glsl_mod` stands in.
/// - `smoothstep` with `edge0 > edge1` is used on purpose by the scene (a
///   falling edge); Metal leaves that undefined, so `ss` spells the formula out.
/// - `atan(y, x)` is `atan2`.
#[cfg(target_os = "macos")]
const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct Uniforms {
  float2 resolution;
  float time;
  float mood;
  float intensity;
  float dim;
};

struct VOut {
  float4 pos [[position]];
};

vertex VOut weather_vertex(uint vid [[vertex_id]]) {
  // One oversized triangle covers the whole target.
  float2 p = float2(float((vid << 1) & 2), float(vid & 2));
  VOut o;
  o.pos = float4(p * 2.0 - 1.0, 0.0, 1.0);
  return o;
}

static float glsl_mod(float x, float y) { return x - y * floor(x / y); }

static float ss(float e0, float e1, float x) {
  float t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

static float hash(float2 p) {
  p = fract(p * float2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

static float noise(float2 p) {
  float2 i = floor(p);
  float2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  float a = hash(i);
  float b = hash(i + float2(1.0, 0.0));
  float c = hash(i + float2(0.0, 1.0));
  float d = hash(i + float2(1.0, 1.0));
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

static float fbm(float2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 5; i++) {
    v += a * noise(p);
    p = p * 2.03 + float2(17.1, 9.2);
    a *= 0.5;
  }
  return v;
}

static float clouds(float2 p, float t, float cover, float speed) {
  float2 q = p * float2(1.4, 2.6) + float2(t * speed, 0.0);
  float n = fbm(q + fbm(q * 0.5 + t * 0.03));
  return ss(1.0 - cover, 1.0 - cover + 0.35, n);
}

static float rain(float2 p, float t, float cols, float speed, float seed) {
  p.x += p.y * 0.22;
  float c = floor(p.x * cols);
  float r = hash(float2(c, seed));
  float fx = fract(p.x * cols) - 0.5;
  float y = fract(p.y * 1.3 + t * speed * (0.8 + 0.5 * r) + r * 11.0);
  float streak = ss(0.0, 0.015, y) * (1.0 - ss(0.015, 0.16, y));
  float w = 1.0 - ss(0.0, 0.12, abs(fx));
  return streak * w * step(0.45, r);
}

static float snow(float2 p, float t, float scale, float seed) {
  p *= scale;
  p.y += t * 0.45 * (1.0 + seed * 0.3);
  p.x += sin(p.y * 0.9 + t * 0.8 + seed) * 0.25;
  float2 id = floor(p);
  float2 f = fract(p) - 0.5;
  float r = hash(id + seed);
  float2 o = float2(hash(id + 3.1 + seed), hash(id + 7.7 + seed)) - 0.5;
  float d = length(f - o * 0.6);
  return step(0.35, r) * ss(0.07 * (0.6 + r), 0.0, d);
}

static float stars(float2 p, float t) {
  float2 g = p * 38.0;
  float2 id = floor(g);
  float2 f = fract(g) - 0.5;
  float r = hash(id);
  float2 o = float2(hash(id + 1.3), hash(id + 4.9)) - 0.5;
  float d = length(f - o * 0.7);
  float tw = 0.55 + 0.45 * sin(t * (1.5 + r * 3.0) + r * 40.0);
  return step(0.86, r) * ss(0.06, 0.0, d) * tw;
}

static float gust(float2 p, float t, float aspect, float y0, float amp, float freq,
                  float speed, float len, float seed, float res_y) {
  float cycle = aspect + len + 0.6;
  float hx = glsl_mod(t * speed + seed * cycle, cycle) - 0.3;
  float along = hx - p.x;
  if (along < 0.0 || along > len) return 0.0;
  float k = along / len;
  float y = y0 + amp * sin(p.x * freq + seed * 6.2831 + t * 0.7)
          + amp * 0.4 * sin(p.x * freq * 2.3 - t * 0.9);
  float d = abs(p.y - y);
  float px = 1.0 / res_y;
  float th = mix(1.1, 0.25, k) * px * ss(0.0, 0.14, along);
  float line = 1.0 - ss(th, th + 1.2 * px, d);
  float glow = exp(-d / (4.0 * px)) * 0.10;
  float fade = (1.0 - ss(0.25, 1.0, k)) * ss(0.0, 0.18, along);
  return (line + glow) * fade;
}

fragment float4 weather_fragment(VOut in [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
  float2 frag = float2(in.pos.x, u.resolution.y - in.pos.y);
  float2 uv = frag / u.resolution;
  float aspect = u.resolution.x / u.resolution.y;
  float t = u.time;
  float m = floor(u.mood + 0.5);

  float3 top = float3(0.12, 0.40, 0.80);
  float3 bot = float3(0.36, 0.62, 0.90);
  float sun = 0.0, cloud = 0.0, cloudDark = 0.0, cloudSpeed = 0.02;
  float rainAmt = 0.0, snowAmt = 0.0, fogAmt = 0.0, windAmt = 0.0;
  float starAmt = 0.0, flashAmt = 0.0, heatAmt = 0.0, frostAmt = 0.0;
  float3 cloudTint = float3(1.0);

  if (m < 0.5) {
    sun = 1.0; cloud = 0.12;
  } else if (m < 1.5) {
    top = float3(0.26, 0.40, 0.58); bot = float3(0.50, 0.61, 0.74);
    sun = 0.35; cloud = 0.55; cloudDark = 0.15;
  } else if (m < 2.5) {
    top = float3(0.26, 0.29, 0.34); bot = float3(0.40, 0.43, 0.48);
    cloud = 0.9; cloudDark = 0.45; cloudTint = float3(0.78, 0.80, 0.84);
  } else if (m < 3.5) {
    top = float3(0.16, 0.21, 0.30); bot = float3(0.28, 0.34, 0.44);
    cloud = 0.8; cloudDark = 0.55; rainAmt = 1.0; cloudSpeed = 0.035;
    cloudTint = float3(0.70, 0.75, 0.84);
  } else if (m < 4.5) {
    top = float3(0.08, 0.07, 0.15); bot = float3(0.20, 0.17, 0.32);
    cloud = 0.9; cloudDark = 0.7; rainAmt = 1.2; flashAmt = 1.0; cloudSpeed = 0.06;
    cloudTint = float3(0.62, 0.56, 0.82);
  } else if (m < 5.5) {
    top = float3(0.30, 0.40, 0.56); bot = float3(0.50, 0.58, 0.72);
    cloud = 0.6; cloudDark = 0.1; snowAmt = 1.0;
  } else if (m < 6.5) {
    top = float3(0.28, 0.35, 0.46); bot = float3(0.45, 0.52, 0.62);
    cloud = 0.75; cloudDark = 0.35; rainAmt = 0.55; snowAmt = 0.55; cloudSpeed = 0.03;
  } else if (m < 7.5) {
    top = float3(0.30, 0.33, 0.38); bot = float3(0.44, 0.46, 0.50);
    cloud = 0.3; cloudDark = 0.2; fogAmt = 1.0;
  } else if (m < 8.5) {
    top = float3(0.17, 0.33, 0.50); bot = float3(0.40, 0.55, 0.68);
    windAmt = 1.0;
  } else if (m < 9.5) {
    top = float3(0.16, 0.38, 0.62); bot = float3(0.40, 0.64, 0.82);
    sun = 0.4; cloud = 0.15; frostAmt = 1.0;
  } else if (m < 10.5) {
    top = float3(0.02, 0.03, 0.10); bot = float3(0.09, 0.11, 0.28);
    starAmt = 1.0; cloud = 0.15; cloudDark = 0.6; cloudTint = float3(0.35, 0.38, 0.55);
  } else {
    top = float3(0.60, 0.26, 0.07); bot = float3(0.78, 0.44, 0.15);
    sun = 1.3; heatAmt = 1.0;
  }

  if (heatAmt > 0.0) {
    uv.x += sin(uv.y * 38.0 + t * 3.0) * 0.004 * (1.0 - uv.y) * heatAmt;
  }

  float2 p = float2(uv.x * aspect, uv.y);
  float3 col = mix(bot, top, ss(0.0, 1.0, uv.y));

  if (sun > 0.0) {
    float2 sp = float2(0.86 * aspect, 1.02);
    float2 dv = p - sp;
    float d = length(dv);
    float ang = atan2(dv.y, dv.x);
    float rays = 0.5 + 0.5 * sin(ang * 9.0 + t * 0.25) * sin(ang * 5.0 - t * 0.15);
    float3 sunCol = heatAmt > 0.0 ? float3(1.0, 0.86, 0.55) : float3(1.0, 0.95, 0.80);
    col += sunCol * exp(-d * 3.2) * 0.55 * sun;
    col += sunCol * rays * exp(-d * 1.8) * 0.12 * sun;
    col += sunCol * ss(0.13, 0.10, d) * 0.5 * sun;
  }

  if (starAmt > 0.0) {
    col += float3(0.9, 0.93, 1.0) * stars(p, t) * starAmt * ss(0.1, 0.7, uv.y);
    float2 mp = float2(0.90 * aspect, 0.90);
    float md = length(p - mp);
    float disc = ss(0.075, 0.066, md);
    float crater = fbm(p * 18.0) * 0.18;
    col = mix(col, float3(0.93, 0.92, 0.86) - crater, disc);
    col += float3(0.55, 0.60, 0.85) * exp(-md * 5.0) * 0.22;
  }

  float flash = 0.0;
  if (flashAmt > 0.0) {
    float ft = fract(t * 0.21);
    flash = (ss(0.0, 0.01, ft) * (1.0 - ss(0.01, 0.05, ft))
          + 0.6 * ss(0.07, 0.08, ft) * (1.0 - ss(0.08, 0.12, ft))) * flashAmt;
    col += float3(0.55, 0.50, 0.85) * flash * 0.45;
  }

  if (cloud > 0.0) {
    float c1 = clouds(p, t, cloud * 0.75, cloudSpeed);
    float c2 = clouds(p * 1.7 + 5.0, t, cloud * 0.6, cloudSpeed * 1.6);
    float shade = fbm(p * 3.0 + t * cloudSpeed);
    float3 cc = mix(cloudTint, cloudTint * 0.45, cloudDark * (0.6 + 0.4 * shade));
    cc += float3(0.7, 0.65, 1.0) * flash * 0.6;
    float mask = ss(-0.1, 0.9, uv.y);
    col = mix(col, cc, c2 * 0.45 * mask);
    col = mix(col, cc * 1.05, c1 * 0.75 * mask);
  }

  if (fogAmt > 0.0) {
    float f1 = fbm(float2(p.x * 1.3 - t * 0.05, p.y * 5.0));
    float f2 = fbm(float2(p.x * 2.1 + t * 0.03, p.y * 7.0 + 3.0));
    float bands = ss(0.35, 0.8, f1) * 0.55 + ss(0.4, 0.85, f2) * 0.35;
    float low = 1.0 - ss(0.0, 0.85, uv.y);
    col = mix(col, float3(0.66, 0.68, 0.72), clamp(bands * (0.4 + low), 0.0, 1.0) * fogAmt * 0.7);
  }

  if (windAmt > 0.0) {
    float2 q = float2(p.x * 0.7 - t * 0.14, p.y * 5.5);
    float w = fbm(q + float2(fbm(q * 0.6 - t * 0.05) * 1.6, 0.0));
    float wisp = ss(0.45, 0.85, w) * ss(0.05, 0.85, uv.y);
    col = mix(col, float3(0.90, 0.94, 1.0), wisp * 0.42 * windAmt);

    float ry = u.resolution.y;
    float g = 0.0;
    g += gust(p, t, aspect, 0.80, 0.045, 3.1, 0.34, 0.95, 0.10, ry);
    g += gust(p, t, aspect, 0.56, 0.060, 2.4, 0.27, 1.20, 0.55, ry) * 0.8;
    g += gust(p, t, aspect, 0.30, 0.040, 3.7, 0.40, 0.80, 0.82, ry) * 0.7;
    g += gust(p, t, aspect, 0.12, 0.050, 2.8, 0.31, 1.00, 0.33, ry) * 0.6;
    col += float3(0.94, 0.97, 1.0) * g * 0.5 * windAmt;

    float2 dg = float2(p.x * 5.0 - t * 1.9, p.y * 13.0);
    dg.y += sin(p.x * 3.0 + t * 1.3) * 0.6;
    float2 did = floor(dg);
    float2 df = fract(dg) - 0.5;
    float dr = hash(did + 21.0);
    float streak = length(float2(df.x * 0.28, df.y));
    col += float3(0.95, 0.98, 1.0) * step(0.93, dr) * ss(0.07, 0.0, streak) * 0.5 * windAmt;
  }

  if (rainAmt > 0.0) {
    float r = rain(p, t, 55.0, 1.6, 1.0) * 0.55 + rain(p * 1.4 + 3.0, t, 80.0, 1.2, 2.0) * 0.3;
    col += float3(0.75, 0.82, 0.95) * r * 0.8 * rainAmt;
  }

  if (snowAmt > 0.0) {
    float s = snow(p, t, 9.0, 0.0) + snow(p, t, 14.0, 1.0) * 0.7 + snow(p, t, 22.0, 2.0) * 0.45;
    col = mix(col, float3(1.0), clamp(s, 0.0, 1.0) * 0.9 * snowAmt);
  }

  if (frostAmt > 0.0) {
    float2 g = p * 26.0;
    float2 id = floor(g);
    float r = hash(id);
    float d = length(fract(g) - 0.5);
    float tw = pow(0.5 + 0.5 * sin(t * 2.0 + r * 50.0), 6.0);
    col += float3(0.85, 0.95, 1.0) * step(0.9, r) * ss(0.08, 0.0, d) * tw * frostAmt;
    float edge = 1.0 - ss(0.0, 0.35, min(min(uv.x, 1.0 - uv.x) * aspect, min(uv.y, 1.0 - uv.y)));
    col = mix(col, float3(0.85, 0.94, 1.0), edge * 0.18 * frostAmt * fbm(p * 6.0));
  }

  float3 base = float3(0.0);
  col = mix(base, col, u.intensity);
  col *= 1.0 - u.dim * (0.75 - 0.35 * uv.y);
  float2 vc = uv - 0.5;
  col *= 1.0 - dot(vc, vc) * 0.35;

  return float4(saturate(col), 1.0);
}
"#;

#[cfg(target_os = "macos")]
mod metal {
    use super::{Frame, MSL, STATIC_TIME};
    use gpui::RenderImage;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_foundation::NSString;
    use objc2_metal::{
        MTLBlitCommandEncoder, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder,
        MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction,
        MTLOrigin, MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder,
        MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
        MTLResource, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
        MTLTextureUsage,
    };
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    // `MTLCreateSystemDefaultDevice` lives behind CoreGraphics.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {}

    /// ~30 fps: newer frames inside this window reuse the last image.
    const FRAME_INTERVAL: Duration = Duration::from_millis(33);
    /// Guard against a runaway layout asking for a huge texture.
    const MAX_EDGE: u32 = 4096;
    /// Pencil defaults for the two tuning uniforms.
    const INTENSITY: f32 = 1.0;
    const DIM: f32 = 0.35;

    /// `constant Uniforms&` in the MSL above (float2 + 4 floats, 24 bytes).
    #[repr(C)]
    struct Uniforms {
        resolution: [f32; 2],
        time: f32,
        mood: f32,
        intensity: f32,
        dim: f32,
    }

    #[derive(Clone, Copy, PartialEq)]
    struct Key {
        mood: u8,
        width: u32,
        height: u32,
    }

    struct Target {
        texture: Retained<ProtocolObject<dyn MTLTexture>>,
        width: u32,
        height: u32,
    }

    struct Renderer {
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
        pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
        /// Discrete GPUs can't CPU-read a Shared texture; those render into
        /// a Managed one and blit-synchronize before `getBytes`.
        unified: bool,
        target: Option<Target>,
        origin: Instant,
        last: Option<(Key, Arc<RenderImage>, Instant)>,
    }

    enum State {
        Pending,
        Ready(Box<Renderer>),
        Failed,
    }

    thread_local! {
        static STATE: RefCell<State> = const { RefCell::new(State::Pending) };
    }

    pub(super) fn render(mood: u8, width: u32, height: u32, animate: bool) -> Option<Frame> {
        if width == 0 || height == 0 {
            return None;
        }
        let width = width.min(MAX_EDGE);
        let height = height.min(MAX_EDGE);
        STATE.with(|cell| {
            let mut state = cell.borrow_mut();
            if matches!(*state, State::Pending) {
                *state = match Renderer::new() {
                    Ok(renderer) => State::Ready(Box::new(renderer)),
                    Err(err) => {
                        log::warn!("weather shader unavailable, using the gradient wash: {err}");
                        State::Failed
                    }
                };
            }
            let State::Ready(renderer) = &mut *state else {
                return None;
            };
            match renderer.frame(Key { mood, width, height }, animate) {
                Ok(frame) => Some(frame),
                Err(err) => {
                    log::warn!("weather shader frame failed, using the gradient wash: {err}");
                    *state = State::Failed;
                    None
                }
            }
        })
    }

    impl Renderer {
        fn new() -> Result<Self, String> {
            let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
            let queue = device.newCommandQueue().ok_or("no Metal command queue")?;
            let library = device
                .newLibraryWithSource_options_error(&NSString::from_str(MSL), None)
                .map_err(|err| format!("MSL compile: {}", err.localizedDescription()))?;
            let vertex = library
                .newFunctionWithName(&NSString::from_str("weather_vertex"))
                .ok_or("weather_vertex missing")?;
            let fragment = library
                .newFunctionWithName(&NSString::from_str("weather_fragment"))
                .ok_or("weather_fragment missing")?;
            let desc = MTLRenderPipelineDescriptor::new();
            desc.setVertexFunction(Some(&vertex));
            desc.setFragmentFunction(Some(&fragment));
            let attachment = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
            attachment.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            let pipeline = device
                .newRenderPipelineStateWithDescriptor_error(&desc)
                .map_err(|err| format!("pipeline: {}", err.localizedDescription()))?;
            let unified = device.hasUnifiedMemory();
            Ok(Self {
                device,
                queue,
                pipeline,
                unified,
                target: None,
                origin: Instant::now(),
                last: None,
            })
        }

        fn frame(&mut self, key: Key, animate: bool) -> Result<Frame, String> {
            let now = Instant::now();
            if let Some((last_key, image, at)) = &self.last {
                let fresh = !animate || now.duration_since(*at) < FRAME_INTERVAL;
                if *last_key == key && fresh {
                    return Ok(Frame {
                        image: image.clone(),
                        stale: None,
                    });
                }
            }
            let time = if animate {
                now.duration_since(self.origin).as_secs_f32()
            } else {
                STATIC_TIME
            };
            let bgra = self.draw(key, time)?;
            let image = to_render_image(bgra, key.width, key.height)
                .ok_or("frame buffer size mismatch")?;
            let stale = self.last.replace((key, image.clone(), now)).map(|(_, old, _)| old);
            Ok(Frame { image, stale })
        }

        fn texture(&mut self, width: u32, height: u32) -> Result<&ProtocolObject<dyn MTLTexture>, String> {
            let stale = self
                .target
                .as_ref()
                .is_none_or(|t| t.width != width || t.height != height);
            if stale {
                let desc = unsafe {
                    MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                        MTLPixelFormat::BGRA8Unorm,
                        width as usize,
                        height as usize,
                        false,
                    )
                };
                desc.setUsage(MTLTextureUsage::RenderTarget);
                desc.setStorageMode(if self.unified {
                    MTLStorageMode::Shared
                } else {
                    MTLStorageMode::Managed
                });
                let texture = self
                    .device
                    .newTextureWithDescriptor(&desc)
                    .ok_or("texture allocation failed")?;
                self.target = Some(Target {
                    texture,
                    width,
                    height,
                });
            }
            Ok(&self.target.as_ref().expect("target just set").texture)
        }

        /// One pass of the full-screen triangle, then a synchronous read-back.
        /// The texture is at most a few hundred KB, so the wait is short.
        fn draw(&mut self, key: Key, time: f32) -> Result<Vec<u8>, String> {
            let unified = self.unified;
            let queue = self.queue.clone();
            let pipeline = self.pipeline.clone();
            let texture = self.texture(key.width, key.height)?;

            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            color.setTexture(Some(texture));
            color.setLoadAction(MTLLoadAction::DontCare);
            color.setStoreAction(MTLStoreAction::Store);
            color.setClearColor(MTLClearColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 1.0,
            });

            let buffer = queue.commandBuffer().ok_or("no command buffer")?;
            let encoder = buffer
                .renderCommandEncoderWithDescriptor(&pass)
                .ok_or("no render encoder")?;
            let uniforms = Uniforms {
                resolution: [key.width as f32, key.height as f32],
                time,
                mood: key.mood as f32,
                intensity: INTENSITY,
                dim: DIM,
            };
            encoder.setRenderPipelineState(&pipeline);
            unsafe {
                encoder.setFragmentBytes_length_atIndex(
                    NonNull::from(&uniforms).cast(),
                    std::mem::size_of::<Uniforms>(),
                    0,
                );
                encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
            }
            encoder.endEncoding();
            if !unified {
                let blit = buffer.blitCommandEncoder().ok_or("no blit encoder")?;
                let resource: &ProtocolObject<dyn MTLResource> = ProtocolObject::from_ref(texture);
                blit.synchronizeResource(resource);
                blit.endEncoding();
            }
            buffer.commit();
            buffer.waitUntilCompleted();

            let row = key.width as usize * 4;
            let mut bgra = vec![0u8; row * key.height as usize];
            let region = MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: key.width as usize,
                    height: key.height as usize,
                    depth: 1,
                },
            };
            unsafe {
                texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    NonNull::new(bgra.as_mut_ptr()).ok_or("null frame buffer")?.cast(),
                    row,
                    region,
                    0,
                );
            }
            Ok(bgra)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Key, Renderer, STATIC_TIME};

        /// Renders all 12 moods through the real Metal path (runtime MSL
        /// compile, offscreen pass, read-back) at 480×256 device px — the
        /// 240×128 card at 2x, as Pencil exports it — and writes
        /// `mood_00.png`..`mood_11.png` into `$WEATHER_SHADER_OUT`.
        ///
        /// `WEATHER_SHADER_OUT=/tmp/w cargo test -p nook render_moods_to_png -- --ignored`
        #[test]
        #[ignore]
        fn render_moods_to_png() {
            let Some(out) = std::env::var_os("WEATHER_SHADER_OUT") else {
                eprintln!("WEATHER_SHADER_OUT unset; skipping");
                return;
            };
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).expect("create WEATHER_SHADER_OUT");
            // `Renderer::new` carries the Metal compiler's own message.
            let mut renderer =
                Renderer::new().unwrap_or_else(|err| panic!("weather shader setup: {err}"));
            let (width, height) = (480u32, 256u32);
            for mood in 0..12u8 {
                let mut px = renderer
                    .draw(
                        Key {
                            mood,
                            width,
                            height,
                        },
                        STATIC_TIME,
                    )
                    .unwrap_or_else(|err| panic!("mood {mood}: {err}"));
                for bgra in px.chunks_exact_mut(4) {
                    bgra.swap(0, 2);
                }
                let path = out.join(format!("mood_{mood:02}.png"));
                image::RgbaImage::from_raw(width, height, px)
                    .expect("frame size")
                    .save(&path)
                    .unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
            }
        }
    }

    /// Same wrapping as the Mirror camera: keep BGRA — `RenderImage` wants
    /// that channel order. Alpha is always 255 (the shader writes 1.0).
    fn to_render_image(bgra: Vec<u8>, width: u32, height: u32) -> Option<Arc<RenderImage>> {
        use image::{ImageBuffer, Rgba};
        let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bgra)?;
        Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::MSL;

    #[test]
    fn msl_exports_both_entry_points() {
        assert!(MSL.contains("vertex VOut weather_vertex("));
        assert!(MSL.contains("fragment float4 weather_fragment("));
    }

    #[test]
    fn msl_keeps_every_mood_branch() {
        // 12 moods: 11 `m < n.5` tests plus the trailing else (Heat).
        let branches = (0..11)
            .filter(|n| MSL.contains(&format!("m < {n}.5")))
            .count();
        assert_eq!(branches, 11);
    }
}
