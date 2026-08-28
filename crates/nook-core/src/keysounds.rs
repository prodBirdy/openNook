//! Mechey: mechanical keyboard sounds per keystroke.
//!
//! Builtin packs are synthesized PCM (CC0). User packs follow the Mechvibes
//! `config.json` layout and live in `~/Library/Application Support/openNook-gpui/soundpacks`.
//! The cpal output stream opens on the first key and closes after ~20 s idle.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "macos")]
use std::time::Duration;
use std::time::Instant;

const SAMPLE_RATE: u32 = 48_000;
#[cfg(target_os = "macos")]
const IDLE_CLOSE: Duration = Duration::from_secs(20);
const PITCH_JITTER: f32 = 0.04;
const GAIN_JITTER: f32 = 0.08;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    Key,
    Space,
    Enter,
    Backspace,
    Modifier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundPackInfo {
    pub id: String,
    pub name: String,
    pub builtin: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MechvibesPack {
    pub name: String,
    pub key_define_type: String,
    pub sound: Option<String>,
    pub defines: HashMap<u16, MechvibesDefine>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MechvibesDefine {
    /// Sprite sheet: `[offset_ms, duration_ms]`.
    Sprite { offset_ms: f64, duration_ms: f64 },
    /// Separate file per key.
    File(String),
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub samples: Arc<Vec<f32>>,
    pub pos: f32,
    pub step: f32,
    pub gain: f32,
}

impl Voice {
    pub fn done(&self) -> bool {
        self.pos >= self.samples.len() as f32
    }
}

/// Mix active voices into an interleaved buffer. Pure; used by the cpal
/// callback and unit tests.
pub fn mix_into(output: &mut [f32], channels: usize, voices: &mut Vec<Voice>) {
    let channels = channels.max(1);
    for frame in output.chunks_mut(channels) {
        let mut sample = 0.0f32;
        for voice in voices.iter_mut() {
            let idx = voice.pos as usize;
            if idx < voice.samples.len() {
                sample += voice.samples[idx] * voice.gain;
                voice.pos += voice.step;
            }
        }
        let sample = sample.clamp(-1.0, 1.0);
        for ch in frame.iter_mut() {
            *ch = sample;
        }
    }
    voices.retain(|voice| !voice.done());
}

/// Tiny synthesized switch: noise burst + damped sine. Distinct enough for
/// space / enter / backspace / modifiers without bundling recordings.
pub fn synthesize_click(kind: ClickKind, sample_rate: u32, down: bool) -> Vec<f32> {
    let (ms, freq, noise, gain) = match (kind, down) {
        (ClickKind::Space, true) => (14.0, 180.0, 0.35, 0.85),
        (ClickKind::Space, false) => (9.0, 160.0, 0.2, 0.45),
        (ClickKind::Enter, true) => (12.0, 420.0, 0.4, 0.8),
        (ClickKind::Enter, false) => (8.0, 380.0, 0.22, 0.4),
        (ClickKind::Backspace, true) => (10.0, 720.0, 0.45, 0.7),
        (ClickKind::Backspace, false) => (7.0, 640.0, 0.25, 0.35),
        (ClickKind::Modifier, true) => (8.0, 260.0, 0.2, 0.5),
        (ClickKind::Modifier, false) => (6.0, 240.0, 0.12, 0.25),
        (ClickKind::Key, true) => (8.0, 980.0, 0.55, 0.65),
        (ClickKind::Key, false) => (5.5, 860.0, 0.3, 0.32),
    };
    let n = ((sample_rate as f32) * ms / 1000.0) as usize;
    let sr = sample_rate as f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let env = (-t * 90.0).exp();
            let sine = (2.0 * std::f32::consts::PI * freq * t).sin();
            let nse = crate_hash(i as u64) * 2.0 - 1.0;
            (sine * (1.0 - noise) + nse * noise) * env * gain
        })
        .collect()
}

fn crate_hash(i: u64) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 32;
    ((x & 0xFFFF_FFFF) as f32) / (u32::MAX as f32)
}

pub fn click_kind_for_cg(keycode: u16) -> ClickKind {
    match keycode {
        49 => ClickKind::Space,
        36 | 76 => ClickKind::Enter,
        51 | 117 => ClickKind::Backspace,
        54 | 55 | 56 | 57 | 58 | 59 | 60 | 61 | 62 | 63 => ClickKind::Modifier,
        _ => ClickKind::Key,
    }
}

/// ANSI CGKeyCode → JS/DOM keyCode used by Mechvibes `defines`.
pub fn js_keycode_for_cg(cg: u16) -> Option<u16> {
    Some(match cg {
        0 => 65,   // A
        1 => 83,   // S
        2 => 68,   // D
        3 => 70,   // F
        4 => 72,   // H
        5 => 71,   // G
        6 => 90,   // Z
        7 => 88,   // X
        8 => 67,   // C
        9 => 86,   // V
        11 => 66,  // B
        12 => 81,  // Q
        13 => 87,  // W
        14 => 69,  // E
        15 => 82,  // R
        16 => 89,  // Y
        17 => 84,  // T
        18 => 49,  // 1
        19 => 50,  // 2
        20 => 51,  // 3
        21 => 52,  // 4
        22 => 54,  // 6
        23 => 53,  // 5
        24 => 187, // =
        25 => 57,  // 9
        26 => 55,  // 7
        27 => 189, // -
        28 => 56,  // 8
        29 => 48,  // 0
        30 => 221, // ]
        31 => 79,  // O
        32 => 85,  // U
        33 => 219, // [
        34 => 73,  // I
        35 => 80,  // P
        36 => 13,  // Return
        37 => 76,  // L
        38 => 74,  // J
        39 => 222, // '
        40 => 75,  // K
        41 => 186, // ;
        42 => 220, // \
        43 => 188, // ,
        44 => 191, // /
        45 => 78,  // N
        46 => 77,  // M
        47 => 190, // .
        48 => 9,   // Tab
        49 => 32,  // Space
        50 => 192, // `
        51 => 8,   // Delete
        53 => 27,  // Escape
        54 => 93,  // RCommand
        55 => 91,  // LCommand
        56 => 16,  // Shift
        57 => 20,  // Caps
        58 => 18,  // Option
        59 => 17,  // Control
        60 => 16,  // RShift
        61 => 18,  // ROption
        62 => 17,  // RControl
        63 => 93,  // Fn-ish
        64 => 121, // F17
        65 => 110, // keypad .
        67 => 106, // keypad *
        69 => 107, // keypad +
        71 => 12,  // keypad clear
        75 => 111, // keypad /
        76 => 13,  // keypad enter
        78 => 109, // keypad -
        82 => 96,  // keypad 0
        83 => 97,
        84 => 98,
        85 => 99,
        86 => 100,
        87 => 101,
        88 => 102,
        89 => 103,
        91 => 104,
        92 => 105,
        96 => 112, // F5
        97 => 113, // F6
        98 => 114, // F7
        99 => 115, // F3
        100 => 116,
        101 => 117,
        103 => 119,
        109 => 120,
        111 => 122,
        118 => 114, // F4
        120 => 112, // F2
        122 => 112, // F1 (JS 112)
        123 => 37,  // Left
        124 => 39,  // Right
        125 => 40,  // Down
        126 => 38,  // Up
        _ => return None,
    })
}

#[derive(Deserialize)]
struct RawMechvibes {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    key_define_type: Option<String>,
    #[serde(default)]
    sound: Option<String>,
    #[serde(default)]
    defines: HashMap<String, RawDefine>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDefine {
    Sprite(Vec<f64>),
    File(String),
    Null,
}

/// Parse a Mechvibes `config.json`. Keys are JS keyCodes.
pub fn parse_mechvibes(json: &str) -> Result<MechvibesPack, String> {
    let raw: RawMechvibes =
        serde_json::from_str(json).map_err(|err| format!("mechvibes config: {err}"))?;
    let mut defines = HashMap::new();
    for (key, value) in raw.defines {
        let Ok(code) = key.parse::<u16>() else {
            continue;
        };
        match value {
            RawDefine::Sprite(pair) if pair.len() >= 2 => {
                defines.insert(
                    code,
                    MechvibesDefine::Sprite {
                        offset_ms: pair[0],
                        duration_ms: pair[1],
                    },
                );
            }
            RawDefine::File(path) if !path.is_empty() => {
                defines.insert(code, MechvibesDefine::File(path));
            }
            _ => {}
        }
    }
    Ok(MechvibesPack {
        name: raw.name.unwrap_or_else(|| "Untitled".into()),
        key_define_type: raw
            .key_define_type
            .unwrap_or_else(|| "single".into())
            .to_ascii_lowercase(),
        sound: raw.sound,
        defines,
    })
}

pub fn soundpacks_dir() -> PathBuf {
    crate::app_data_dir().join("soundpacks")
}

pub fn list_packs() -> Vec<SoundPackInfo> {
    let mut packs = vec![
        SoundPackInfo {
            id: "nook-click".into(),
            name: "Nook Click".into(),
            builtin: true,
        },
        SoundPackInfo {
            id: "nook-thock".into(),
            name: "Nook Thock".into(),
            builtin: true,
        },
    ];
    if let Ok(entries) = std::fs::read_dir(soundpacks_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if packs.iter().any(|p| p.id == id) {
                continue;
            }
            let name = std::fs::read_to_string(path.join("config.json"))
                .ok()
                .and_then(|json| parse_mechvibes(&json).ok())
                .map(|pack| pack.name)
                .unwrap_or_else(|| id.clone());
            packs.push(SoundPackInfo {
                id,
                name,
                builtin: false,
            });
        }
    }
    packs
}

struct SampleBank {
    id: String,
    down: HashMap<u16, Arc<Vec<f32>>>,
    up: HashMap<u16, Arc<Vec<f32>>>,
    fallback_down: Arc<Vec<f32>>,
    fallback_up: Arc<Vec<f32>>,
}

impl SampleBank {
    fn builtin(id: &str) -> Self {
        let thock = id == "nook-thock";
        let kinds = [
            ClickKind::Key,
            ClickKind::Space,
            ClickKind::Enter,
            ClickKind::Backspace,
            ClickKind::Modifier,
        ];
        let mut down = HashMap::new();
        let mut up = HashMap::new();
        for kind in kinds {
            let mut d = synthesize_click(kind, SAMPLE_RATE, true);
            let mut u = synthesize_click(kind, SAMPLE_RATE, false);
            if thock {
                for s in d.iter_mut().chain(u.iter_mut()) {
                    *s *= 1.15;
                }
            }
            let d = Arc::new(d);
            let u = Arc::new(u);
            for code in codes_for_kind(kind) {
                down.insert(code, d.clone());
                up.insert(code, u.clone());
            }
        }
        Self {
            id: id.to_string(),
            fallback_down: down.get(&0).cloned().unwrap_or_else(|| Arc::new(vec![])),
            fallback_up: up.get(&0).cloned().unwrap_or_else(|| Arc::new(vec![])),
            down,
            up,
        }
    }

    fn sample(&self, keycode: u16, down: bool) -> Arc<Vec<f32>> {
        let map = if down { &self.down } else { &self.up };
        map.get(&keycode)
            .cloned()
            .unwrap_or_else(|| {
                if down {
                    self.fallback_down.clone()
                } else {
                    self.fallback_up.clone()
                }
            })
    }
}

fn codes_for_kind(kind: ClickKind) -> Vec<u16> {
    match kind {
        ClickKind::Space => vec![49],
        ClickKind::Enter => vec![36, 76],
        ClickKind::Backspace => vec![51, 117],
        ClickKind::Modifier => vec![54, 55, 56, 57, 58, 59, 60, 61, 62, 63],
        ClickKind::Key => vec![0],
    }
}

struct Mixer {
    bank: SampleBank,
    voices: Vec<Voice>,
    last_play: Instant,
}

fn mixer() -> &'static Mutex<Mixer> {
    static MIXER: OnceLock<Mutex<Mixer>> = OnceLock::new();
    MIXER.get_or_init(|| {
        Mutex::new(Mixer {
            bank: SampleBank::builtin("nook-click"),
            voices: Vec::new(),
            last_play: Instant::now(),
        })
    })
}

/// Reload the bank when the pack setting changes. Cheap for builtins.
pub fn sync() {
    let settings = crate::settings::get_app_settings();
    if let Ok(mut mix) = mixer().lock() {
        if mix.bank.id != settings.keysound_pack {
            mix.bank = load_bank(&settings.keysound_pack);
        }
    }
    if !settings.keysounds_enabled {
        stop_stream();
    }
}

fn load_bank(id: &str) -> SampleBank {
    if id == "nook-thock" || id == "nook-click" || id.is_empty() {
        return SampleBank::builtin(if id.is_empty() { "nook-click" } else { id });
    }
    match load_user_bank(id) {
        Ok(bank) => bank,
        Err(err) => {
            log::warn!("keysound pack '{id}' failed ({err}); using builtin");
            SampleBank::builtin("nook-click")
        }
    }
}

fn load_user_bank(id: &str) -> Result<SampleBank, String> {
    let dir = soundpacks_dir().join(id);
    let json = std::fs::read_to_string(dir.join("config.json"))
        .map_err(|err| format!("read config: {err}"))?;
    let pack = parse_mechvibes(&json)?;
    let mut down = HashMap::new();
    let fallback = Arc::new(synthesize_click(ClickKind::Key, SAMPLE_RATE, true));
    let fallback_up = Arc::new(synthesize_click(ClickKind::Key, SAMPLE_RATE, false));

    if pack.key_define_type == "single" {
        if let Some(sound) = &pack.sound {
            let pcm = decode_audio(&dir.join(sound))?;
            for (js, define) in &pack.defines {
                if let MechvibesDefine::Sprite {
                    offset_ms,
                    duration_ms,
                } = define
                {
                    let slice = sprite_slice(&pcm, SAMPLE_RATE, *offset_ms, *duration_ms);
                    if let Some(cg) = cg_for_js(*js) {
                        down.insert(cg, Arc::new(slice));
                    }
                }
            }
        }
    } else {
        for (js, define) in &pack.defines {
            if let MechvibesDefine::File(name) = define {
                if let Ok(pcm) = decode_audio(&dir.join(name)) {
                    if let Some(cg) = cg_for_js(*js) {
                        down.insert(cg, Arc::new(pcm));
                    }
                }
            }
        }
    }

    Ok(SampleBank {
        id: id.to_string(),
        fallback_down: fallback,
        fallback_up: fallback_up.clone(),
        down,
        up: HashMap::new(),
    })
}

fn sprite_slice(pcm: &[f32], rate: u32, offset_ms: f64, duration_ms: f64) -> Vec<f32> {
    let start = ((offset_ms / 1000.0) * rate as f64).round() as usize;
    let len = ((duration_ms / 1000.0) * rate as f64).round() as usize;
    let end = (start + len).min(pcm.len());
    if start >= pcm.len() {
        return Vec::new();
    }
    pcm[start..end].to_vec()
}

fn cg_for_js(js: u16) -> Option<u16> {
    (0u16..=126).find(|cg| js_keycode_for_cg(*cg) == Some(js))
}

fn decode_audio(path: &Path) -> Result<Vec<f32>, String> {
    #[cfg(target_os = "macos")]
    {
        decode_audio_macos(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("audio decode is macOS-only".into())
    }
}

#[cfg(target_os = "macos")]
fn decode_audio_macos(path: &Path) -> Result<Vec<f32>, String> {
    use std::fs::File;
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = File::open(path).map_err(|err| err.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|err| err.to_string())?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "no audio track".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    let mut sample_buf = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        if sample_buf.is_none() {
            let spec = *decoded.spec();
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        if let Some(buf) = sample_buf.as_mut() {
            buf.copy_interleaved_ref(decoded);
            let spec_channels = buf.spec().channels.count().max(1);
            let samples = buf.samples();
            for frame in samples.chunks(spec_channels) {
                let mono = frame.iter().sum::<f32>() / spec_channels as f32;
                out.push(mono);
            }
        }
    }
    Ok(out)
}

/// Tap callback: enqueue a one-shot. Skips autorepeat and secure input.
pub fn handle_key(keycode: u16, down: bool, autorepeat: bool) {
    if autorepeat || !crate::settings::get_app_settings().keysounds_enabled {
        return;
    }
    if crate::eventtap::is_secure_input() {
        return;
    }
    play_keycode(keycode, down);
}

pub fn play_test() {
    play_keycode(0, true);
}

fn play_keycode(keycode: u16, down: bool) {
    let settings = crate::settings::get_app_settings();
    let volume = settings.keysound_volume.clamp(0.0, 1.0);
    let mut mix = mixer().lock().unwrap_or_else(|e| e.into_inner());
    if mix.bank.id != settings.keysound_pack {
        mix.bank = load_bank(&settings.keysound_pack);
    }
    let samples = mix.bank.sample(keycode, down);
    if samples.is_empty() {
        return;
    }
    let pitch = 1.0 + jitter(PITCH_JITTER);
    let gain = volume * (1.0 + jitter(GAIN_JITTER));
    mix.voices.push(Voice {
        samples,
        pos: 0.0,
        step: pitch,
        gain,
    });
    mix.last_play = Instant::now();
    drop(mix);
    ensure_stream();
}

fn jitter(span: f32) -> f32 {
    let now = now_bits();
    let unit = ((now.wrapping_mul(0x9E37_79B9) >> 16) as f32) / (u16::MAX as f32);
    (unit * 2.0 - 1.0) * span
}

fn now_bits() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1)
}

static STREAM_ON: AtomicBool = AtomicBool::new(false);
static LAST_MIX_MS: AtomicU64 = AtomicU64::new(0);

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn ensure_stream() {
    LAST_MIX_MS.store(unix_ms(), Ordering::Relaxed);
    if STREAM_ON
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        std::thread::Builder::new()
            .name("nook-keysounds".into())
            .spawn(stream_thread)
            .expect("keysound stream");
    }
    #[cfg(not(target_os = "macos"))]
    {
        STREAM_ON.store(false, Ordering::SeqCst);
    }
}

fn stop_stream() {
    STREAM_ON.store(false, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
fn stream_thread() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let Ok(device) = host.default_output_device().ok_or(()) else {
        STREAM_ON.store(false, Ordering::SeqCst);
        return;
    };
    let Ok(config) = device.default_output_config() else {
        STREAM_ON.store(false, Ordering::SeqCst);
        return;
    };
    let channels = config.channels() as usize;
    let err_fn = |err| log::debug!("keysound stream: {err}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                let mut mix = mixer().lock().unwrap_or_else(|e| e.into_inner());
                mix_into(data, channels, &mut mix.voices);
                if !mix.voices.is_empty() {
                    LAST_MIX_MS.store(unix_ms(), Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        ),
        _ => {
            STREAM_ON.store(false, Ordering::SeqCst);
            return;
        }
    };
    let Ok(stream) = stream else {
        STREAM_ON.store(false, Ordering::SeqCst);
        return;
    };
    if stream.play().is_err() {
        STREAM_ON.store(false, Ordering::SeqCst);
        return;
    }
    while STREAM_ON.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(500));
        let idle = unix_ms().saturating_sub(LAST_MIX_MS.load(Ordering::Relaxed));
        if idle > IDLE_CLOSE.as_millis() as u64 {
            break;
        }
        if !crate::settings::get_app_settings().keysounds_enabled {
            break;
        }
    }
    drop(stream);
    STREAM_ON.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechvibes_sprite_pack_parses() {
        let json = r#"{
            "name": "Cream",
            "key_define_type": "single",
            "sound": "sound.ogg",
            "defines": {
                "65": [0, 80],
                "32": [120, 90],
                "13": "enter.ogg",
                "nope": [1, 2]
            }
        }"#;
        let pack = parse_mechvibes(json).unwrap();
        assert_eq!(pack.name, "Cream");
        assert_eq!(pack.key_define_type, "single");
        assert_eq!(pack.sound.as_deref(), Some("sound.ogg"));
        assert_eq!(
            pack.defines.get(&65),
            Some(&MechvibesDefine::Sprite {
                offset_ms: 0.0,
                duration_ms: 80.0
            })
        );
        assert_eq!(
            pack.defines.get(&13),
            Some(&MechvibesDefine::File("enter.ogg".into()))
        );
        assert!(!pack.defines.contains_key(&0));
    }

    #[test]
    fn cg_to_js_covers_letters_and_specials() {
        assert_eq!(js_keycode_for_cg(0), Some(65)); // A
        assert_eq!(js_keycode_for_cg(1), Some(83)); // S
        assert_eq!(js_keycode_for_cg(49), Some(32)); // Space
        assert_eq!(js_keycode_for_cg(36), Some(13)); // Return
        assert_eq!(js_keycode_for_cg(51), Some(8)); // Delete
        assert_eq!(js_keycode_for_cg(53), Some(27)); // Escape
        assert_eq!(js_keycode_for_cg(200), None);
    }

    #[test]
    fn click_kinds_match_special_keys() {
        assert_eq!(click_kind_for_cg(49), ClickKind::Space);
        assert_eq!(click_kind_for_cg(36), ClickKind::Enter);
        assert_eq!(click_kind_for_cg(51), ClickKind::Backspace);
        assert_eq!(click_kind_for_cg(55), ClickKind::Modifier);
        assert_eq!(click_kind_for_cg(0), ClickKind::Key);
    }

    #[test]
    fn synthesize_and_mix_is_finite() {
        let click = Arc::new(synthesize_click(ClickKind::Key, 8_000, true));
        assert!(click.len() > 10);
        assert!(click.iter().all(|s| s.is_finite()));
        let mut voices = vec![Voice {
            samples: click,
            pos: 0.0,
            step: 1.0,
            gain: 0.5,
        }];
        let mut out = vec![0.0f32; 64];
        mix_into(&mut out, 2, &mut voices);
        assert!(out.iter().any(|s| *s != 0.0));
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn mix_retains_only_live_voices() {
        let samples = Arc::new(vec![0.5f32, 0.25]);
        let mut voices = vec![Voice {
            samples,
            pos: 0.0,
            step: 1.0,
            gain: 1.0,
        }];
        let mut out = vec![0.0f32; 8];
        mix_into(&mut out, 1, &mut voices);
        assert!(voices.is_empty());
    }

    #[test]
    fn builtin_pack_list_is_stable() {
        let packs = list_packs();
        assert!(packs.iter().any(|p| p.id == "nook-click" && p.builtin));
        assert!(packs.iter().any(|p| p.id == "nook-thock" && p.builtin));
    }

    #[test]
    fn sprite_slice_clamps() {
        let pcm = vec![1.0, 2.0, 3.0, 4.0];
        let slice = sprite_slice(&pcm, 1000, 1.0, 10.0);
        assert_eq!(slice, vec![2.0, 3.0, 4.0]);
        assert!(sprite_slice(&pcm, 1000, 50.0, 10.0).is_empty());
    }
}
