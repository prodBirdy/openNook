//! Mods' shaders, drawn by Rune's compositor straight into the frame. A `shader()` element is an
//! `El::effect` slot; at each slot this draws one quad with the mod's WGSL `scene` function, in
//! the slot's pass, and applies the slot's clip and rounded clip itself (Rune's `CLIP_WGSL`): only
//! inside an isolated layer, such as an enter fade, does Rune clip the quad for it. Nothing is
//! read back.
//!
//! The mod's file defines
//!   `fn scene(frag: vec2f, res: vec2f, t: f32) -> vec4f`
//! (frag in points-scaled device pixels from the element's top-left, res its size, t seconds;
//! straight-alpha sRGB out) and may call `P(i)` for the element's params.

use crate::island::Island;
use rune::compose::{Compositor, Frame};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// After the mod's source, so naga's line numbers are the mod's own.
const WRAPPER: &str = r#"
struct NookSlot { rect: vec4f, surface: vec4f, p0: vec4f, p1: vec4f, radii: vec4f, clip: vec4f, fade: vec4u, l0: vec4f, l1: vec4f }
@group(0) @binding(0) var<uniform> nook: NookSlot;

fn P(i: i32) -> f32 {
  if i < 4 { return nook.p0[i]; }
  return nook.p1[i - 4];
}

fn LEVEL(i: i32) -> f32 {
  if i < 4 { return nook.l0[i]; }
  return nook.l1[i - 4];
}

struct NookOut { @builtin(position) pos: vec4f }

@vertex fn nook_vs(@builtin(vertex_index) vi: u32) -> NookOut {
  var corners = array<vec2f, 6>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0));
  let px = nook.rect.xy + corners[vi] * nook.rect.zw;
  var o: NookOut;
  o.pos = vec4f(px.x / nook.surface.x * 2.0 - 1.0, 1.0 - px.y / nook.surface.y * 2.0, 0.0, 1.0);
  return o;
}

fn nook_rounded(p: vec2f, size: vec2f, r4: vec4f) -> f32 {
  let c = p - size * 0.5;
  var r = r4.x;
  if c.x >= 0.0 && c.y < 0.0 { r = r4.y; }
  if c.x >= 0.0 && c.y >= 0.0 { r = r4.z; }
  if c.x < 0.0 && c.y >= 0.0 { r = r4.w; }
  let q = abs(c) - size * 0.5 + vec2f(r);
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn nook_linear(c: vec3f) -> vec3f {
  return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

@fragment fn nook_fs(i: NookOut) -> @location(0) vec4f {
  let frag = i.pos.xy - nook.rect.xy;
  let c = scene(frag, nook.rect.zw, nook.surface.z);
  let own = clamp(0.5 - nook_rounded(frag, nook.rect.zw, nook.radii), 0.0, 1.0);
  let a = clamp(c.a, 0.0, 1.0) * own * nook_clip_alpha(i.pos.xy, nook.clip, nook.fade);
  var rgb = clamp(c.rgb, vec3f(0.0), vec3f(1.0));
  if nook.surface.w < 0.5 { rgb = nook_linear(rgb); }
  return vec4f(rgb * a, a);
}
"#;

/// Marks for shader slots start here, clear of anything else that marks elements.
const FIRST_MARK: u32 = 1 << 24;
const MAX_SCENES: usize = 64;
/// How often a shader file is looked at for edits.
const RESTAT: Duration = Duration::from_millis(500);
/// Bytes of one slot's uniforms (9 × vec4), padded to the offset alignment.
const SLOT_BYTES: u64 = 256;

#[derive(Clone, Debug)]
struct Scene {
    path: PathBuf,
    name: String,
    params: [f32; 8],
    animate: bool,
}

#[derive(Default)]
struct Registry {
    marks: HashMap<String, u32>,
    scenes: HashMap<u32, Scene>,
    next: u32,
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

/// The effect mark for a shader element (mod, file, element id), with its latest params.
pub fn mark(name: &str, path: &Path, id: Option<&str>, params: &[f32], fps: f32) -> u32 {
    let reg = REGISTRY.get_or_init(Default::default);
    let mut reg = reg.lock().unwrap();
    let key = format!("{name}\0{}\0{}", path.display(), id.unwrap_or_default());
    if reg.marks.len() >= MAX_SCENES && !reg.marks.contains_key(&key) {
        reg.marks.clear();
        reg.scenes.clear();
    }
    let next = FIRST_MARK + reg.next;
    let mark = *reg.marks.entry(key).or_insert(next);
    if mark == next {
        reg.next += 1;
    }
    let mut p = [0.0; 8];
    for (slot, v) in p.iter_mut().zip(params) {
        *slot = *v;
    }
    reg.scenes.insert(mark, Scene { path: path.to_path_buf(), name: name.to_owned(), params: p, animate: fps > 0.0 });
    mark
}

/// The whole module for a mod's shader source, or the error naga gives for it.
pub fn source(path: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let full = format!("{src}\n{}\n{WRAPPER}", clip_wgsl());
    let module = naga::front::wgsl::parse_str(&full).map_err(|e| e.emit_to_string_with_path(&full, &path.to_string_lossy()))?;
    // Rune's clip maths unpacks its fade widths with `unpack2x16float`, as its own shaders do.
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::SHADER_FLOAT16_IN_FLOAT32)
        .validate(&module)
        .map_err(|e| e.emit_to_string_with_path(&full, &path.to_string_lossy()))?;
    Ok(full)
}

/// Rune's clip maths with its names prefixed, so a mod may name its own `ramp` or `sd_rounded`.
fn clip_wgsl() -> String {
    ["clip_alpha", "unpack_clip_radii", "sd_rounded", "sd_continuous", "CONTINUOUS_K", "ramp"]
        .iter()
        .fold(rune::render::CLIP_WGSL.to_owned(), |s, name| s.replace(name, &format!("nook_{name}")))
}

struct Pipeline {
    stamp: Option<(SystemTime, u64)>,
    checked: Instant,
    /// None when the source didn't compile (logged once per edit).
    pipeline: Option<wgpu::RenderPipeline>,
    /// The mod's source calls `LEVEL`: drawing it keeps the system audio tap listening.
    listens: bool,
}

#[derive(Default)]
pub struct Shaders {
    layout: Option<(wgpu::BindGroupLayout, wgpu::PipelineLayout)>,
    pipelines: HashMap<(PathBuf, wgpu::TextureFormat), Pipeline>,
    pool: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    origin: Option<Instant>,
    animating: bool,
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

impl Shaders {
    fn layout(&mut self, device: &wgpu::Device) -> &(wgpu::BindGroupLayout, wgpu::PipelineLayout) {
        self.layout.get_or_insert_with(|| {
            let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nook shader slot"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                }],
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nook shader"),
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            (bgl, layout)
        })
    }

    fn pipeline(&mut self, device: &wgpu::Device, scene: &Scene, format: wgpu::TextureFormat) -> Option<&wgpu::RenderPipeline> {
        let key = (scene.path.clone(), format);
        let fresh = self.pipelines.get(&key).is_some_and(|p| p.checked.elapsed() < RESTAT || p.stamp == stamp(&scene.path));
        if fresh {
            if let Some(p) = self.pipelines.get_mut(&key).filter(|p| p.checked.elapsed() >= RESTAT) {
                p.checked = Instant::now();
            }
            return self.pipelines.get(&key).and_then(|p| p.pipeline.as_ref());
        }
        let layout = self.layout(device).1.clone();
        let listens = std::fs::read_to_string(&scene.path).is_ok_and(|s| s.contains("LEVEL("));
        let pipeline = match source(&scene.path) {
            Ok(full) => {
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("nook mod shader"),
                    source: wgpu::ShaderSource::Wgsl(full.into()),
                });
                Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("nook mod shader"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState { module: &module, entry_point: Some("nook_vs"), compilation_options: Default::default(), buffers: &[] },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("nook_fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                    }),
                    multiview_mask: None,
                    cache: None,
                }))
            }
            Err(e) => {
                let logs = crate::home::config().logs;
                nook_mods::files::log(&logs, &scene.name, "error", &format!("shader: {e}"));
                log::warn!("{}: shader: {e}", scene.name);
                None
            }
        };
        self.pipelines.insert(key.clone(), Pipeline { stamp: stamp(&scene.path), checked: Instant::now(), pipeline, listens });
        self.pipelines.get(&key).and_then(|p| p.pipeline.as_ref())
    }

    fn slot(&mut self, device: &wgpu::Device, i: usize) -> &(wgpu::Buffer, wgpu::BindGroup) {
        while self.pool.len() <= i {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nook shader slot"),
                size: SLOT_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bgl = &self.layout(device).0;
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("nook shader slot"),
                layout: bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            self.pool.push((buffer, group));
        }
        &self.pool[i]
    }
}

impl Compositor<Island> for Shaders {
    fn render(&mut self, f: &mut Frame<'_, Island>) {
        let (device, queue) = (f.device, f.queue);
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        let t = self.origin.get_or_insert_with(Instant::now).elapsed().as_secs_f32();
        let scenes: HashMap<u32, Scene> = REGISTRY.get().map(|r| r.lock().unwrap().scenes.clone()).unwrap_or_default();
        let mut used = 0;
        let mut animating = false;
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            let Some(scene) = scenes.get(&site.mark) else { return };
            if self.pipeline(device, scene, site.format).is_none() {
                return;
            }
            let i = &site.instance;
            let rect = [i.rect[0] - site.origin[0], i.rect[1] - site.origin[1], i.rect[2], i.rect[3]];
            let time = if scene.animate { t } else { 12.0 };
            let p = scene.params;
            // Clip in this pass's pixels, like `frag`; radii and fade offsets are relative to it.
            let clip = [i.clip[0] - site.origin[0], i.clip[1] - site.origin[1], i.clip[2] - site.origin[0], i.clip[3] - site.origin[1]];
            let listens = self.pipelines.get(&(scene.path.clone(), site.format)).is_some_and(|p| p.listens);
            let l = if listens { nook_core::audio_levels::levels() } else { [0.0; nook_core::audio_levels::BANDS] };
            let uniforms: [f32; 36] = [
                rect[0], rect[1], rect[2], rect[3],
                site.size[0] as f32, site.size[1] as f32, time, if site.srgb_blend { 1.0 } else { 0.0 },
                p[0], p[1], p[2], p[3],
                p[4], p[5], p[6], p[7],
                i.radii[0], i.radii[1], i.radii[2], i.radii[3],
                clip[0], clip[1], clip[2], clip[3],
                f32::from_bits(i.fade[0]), f32::from_bits(i.fade[1]), f32::from_bits(i.fade[2]), f32::from_bits(i.fade[3]),
                l[0], l[1], l[2], l[3],
                l[4], 0.0, 0.0, 0.0,
            ];
            let bytes: Vec<u8> = uniforms.iter().flat_map(|v| v.to_bits().to_ne_bytes()).collect();
            let (buffer, group) = self.slot(device, used);
            queue.write_buffer(buffer, 0, &bytes);
            let group = group.clone();
            let pipeline = self.pipeline(device, scene, site.format).unwrap().clone();
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..6, 0..1);
            used += 1;
            animating |= scene.animate;
        });
        self.animating = animating;
    }

    fn animating(&self) -> bool {
        self.animating
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_mod_shaders_compile() {
        let mods = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mods");
        let mut seen = 0;
        for entry in std::fs::read_dir(&mods).unwrap().flatten() {
            for file in std::fs::read_dir(entry.path()).into_iter().flatten().flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e == "wgsl") {
                    super::source(&path).unwrap_or_else(|e| panic!("{e}"));
                    seen += 1;
                }
            }
        }
        assert!(seen > 0, "no shaders under {}", mods.display());
    }
}
