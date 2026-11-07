//! Live loudness of what the Mac is playing, in five bands (bass to treble), for mods' shaders
//! (`LEVEL(i)`). On macOS 14.2+ the sound comes from a Core Audio process tap of every app but
//! openNook, mixed to stereo on its way to the speakers; nothing is recorded or kept, only each
//! band's level. The tap exists only while something reads the levels ([`levels`]) and is torn
//! down a moment after the last read. Its first start asks for "System Audio Recording"; when
//! that's denied the tap hears silence and every level stays 0.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

pub const BANDS: usize = 5;

static LEVELS: [AtomicU32; BANDS] = [const { AtomicU32::new(0) }; BANDS];
/// Milliseconds since `EPOCH` of the last [`levels`] call.
static LAST_READ: AtomicU64 = AtomicU64::new(0);
static START: Once = Once::new();

fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

/// The bands' levels now, 0–1 (bass, low mids, mids, high mids, treble), and a request to
/// keep listening: the tap starts on the first call and stops ~1.5 s after the last.
pub fn levels() -> [f32; BANDS] {
    LAST_READ.store(now_ms().max(1), Ordering::Relaxed);
    START.call_once(|| {
        std::thread::Builder::new().name("nook-levels".into()).spawn(control).ok();
    });
    std::array::from_fn(|i| f32::from_bits(LEVELS[i].load(Ordering::Relaxed)))
}

fn publish(levels: &[f32; BANDS]) {
    for (slot, v) in LEVELS.iter().zip(levels) {
        slot.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Starts the tap while levels are wanted, stops it when they aren't, follows the default
/// output device, and backs off after a failure.
fn control() {
    const IDLE_MS: u64 = 1500;
    let mut tap: Option<macos::Tap> = None;
    let mut retry_at = 0u64;
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let now = now_ms();
        let wanted = now.saturating_sub(LAST_READ.load(Ordering::Relaxed)) < IDLE_MS;
        if !wanted {
            if tap.take().is_some() {
                publish(&[0.0; BANDS]);
            }
            continue;
        }
        if tap.as_ref().is_some_and(|t| !t.current()) {
            tap = None;
        }
        if tap.is_none() && now >= retry_at {
            match macos::Tap::start() {
                Ok(t) => tap = Some(t),
                Err(e) => {
                    log::warn!("audio levels: {e}");
                    retry_at = now + 30_000;
                }
            }
        }
    }
}

/// Five bands' loudness from mono samples. Each band reads against its own average over the
/// last second or so: steady sound sits at half height and the bars show what moves (a kick,
/// a snare, a swell), whatever the track's loudness or its balance of bass and treble. Against
/// a peak instead, mastered music's steady bass and mids pin their bars at the top. Fades and
/// quiet passages still bring the bars down.
pub struct Analyzer {
    filters: [Biquad; BANDS],
    energy: [f32; BANDS],
    frames: u32,
    /// Each band's running average, dB; None until it first hears something.
    mean_db: [Option<f32>; BANDS],
    out: [f32; BANDS],
    rate: f32,
}

/// The bands: a low-pass for the kick and bass, three band-passes, a high-pass for the air.
const CENTERS: [f32; BANDS] = [120.0, 400.0, 1200.0, 3200.0, 7000.0];
/// dB above (below) a band's average that fills (empties) its bar.
const SPREAD_DB: f32 = 9.0;
/// How long a band's average remembers, seconds.
const MEAN_S: f32 = 1.2;
/// Below this a band is silent (a paused or muted app); bars shrink over the next `FADE_DB`.
const GATE_DB: f32 = -62.0;
const FADE_DB: f32 = 24.0;
const ATTACK_S: f32 = 0.015;
const RELEASE_S: f32 = 0.11;

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        let rate = if rate > 1000.0 { rate } else { 48_000.0 };
        let filters = std::array::from_fn(|i| match i {
            0 => Biquad::low_pass(CENTERS[i], rate),
            4 => Biquad::high_pass(CENTERS[i], rate),
            _ => Biquad::band_pass(CENTERS[i], 0.9, rate),
        });
        Self { filters, energy: [0.0; BANDS], frames: 0, mean_db: [None; BANDS], out: [0.0; BANDS], rate }
    }

    pub fn push(&mut self, sample: f32) {
        for (f, e) in self.filters.iter_mut().zip(self.energy.iter_mut()) {
            let y = f.run(sample);
            *e += y * y;
        }
        self.frames += 1;
    }

    /// Folds the samples pushed since the last call into the levels and returns them.
    pub fn finish(&mut self) -> [f32; BANDS] {
        if self.frames == 0 {
            return self.out;
        }
        let dt = self.frames as f32 / self.rate;
        let attack = (-dt / ATTACK_S).exp();
        let release = (-dt / RELEASE_S).exp();
        let remember = (-dt / MEAN_S).exp();
        for i in 0..BANDS {
            let rms = (self.energy[i] / self.frames as f32).sqrt();
            let db = 20.0 * (rms + 1e-9).log10();
            let target = if db < GATE_DB {
                0.0
            } else {
                let mean = self.mean_db[i].map_or(db, |m| db + (m - db) * remember);
                self.mean_db[i] = Some(mean);
                let moving = (0.5 + 0.5 * (db - mean) / SPREAD_DB).clamp(0.0, 1.0);
                moving * ((db - GATE_DB) / FADE_DB).clamp(0.0, 1.0)
            };
            let keep = if target > self.out[i] { attack } else { release };
            self.out[i] = target + (self.out[i] - target) * keep;
            self.energy[i] = 0.0;
        }
        self.frames = 0;
        self.out
    }
}

/// An RBJ cookbook biquad (transposed direct form II).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(b: [f32; 3], a0: f32, a: [f32; 2]) -> Self {
        Self { b: b.map(|v| v / a0), a: a.map(|v| v / a0), z: [0.0; 2] }
    }

    fn parts(f0: f32, q: f32, rate: f32) -> (f32, f32) {
        let w0 = 2.0 * std::f32::consts::PI * (f0 / rate).min(0.45);
        (w0.cos(), w0.sin() / (2.0 * q))
    }

    fn low_pass(f0: f32, rate: f32) -> Self {
        let (c, alpha) = Self::parts(f0, std::f32::consts::FRAC_1_SQRT_2, rate);
        Self::new([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], 1.0 + alpha, [-2.0 * c, 1.0 - alpha])
    }

    fn high_pass(f0: f32, rate: f32) -> Self {
        let (c, alpha) = Self::parts(f0, std::f32::consts::FRAC_1_SQRT_2, rate);
        Self::new([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], 1.0 + alpha, [-2.0 * c, 1.0 - alpha])
    }

    fn band_pass(f0: f32, q: f32, rate: f32) -> Self {
        let (c, alpha) = Self::parts(f0, q, rate);
        Self::new([alpha, 0.0, -alpha], 1.0 + alpha, [-2.0 * c, 1.0 - alpha])
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        // A NaN from bad input would stick in the state forever; start the filter over.
        if !y.is_finite() {
            self.z = [0.0; 2];
            return 0.0;
        }
        y
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{publish, Analyzer};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_core_audio::{
        kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceIsStackedKey, kAudioAggregateDeviceMainSubDeviceKey,
        kAudioAggregateDeviceNameKey, kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceTapAutoStartKey,
        kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceUID,
        kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyTranslatePIDToProcessObject,
        kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioSubDeviceUIDKey,
        kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey, kAudioTapPropertyFormat, kAudioTapPropertyUID,
        AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop,
        AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
        AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
        CATapDescription, CATapMuteBehavior,
    };
    use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
    use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};
    use std::ffi::{c_void, CStr};
    use std::ptr::NonNull;

    /// IO callbacks so far and the loudest raw sample, for the probe test.
    pub static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub static RAW_PEAK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// A running tap: the process tap, a private aggregate device that reads it, and the IO
    /// callback's analyzer. Dropping it stops and destroys all three.
    pub struct Tap {
        tap: AudioObjectID,
        agg: AudioObjectID,
        proc_id: AudioDeviceIOProcID,
        analyzer: *mut Analyzer,
        output: AudioObjectID,
    }

    // The raw analyzer is the IO thread's while the device runs and is freed only after it stops.
    unsafe impl Send for Tap {}

    impl Tap {
        pub fn start() -> Result<Self, String> {
            let output = read_u32(kAudioObjectSystemObject as AudioObjectID, kAudioHardwarePropertyDefaultOutputDevice).ok_or("no output device")?;
            let output_uid = read_string(output, kAudioDevicePropertyDeviceUID).ok_or("the output device has no uid")?;
            let tap = unsafe { create_tap()? };
            let mut t = Tap { tap, agg: 0, proc_id: None, analyzer: std::ptr::null_mut(), output };
            let tap_uid = read_string(tap, kAudioTapPropertyUID).ok_or("the tap has no uid")?;
            let rate = read_format(tap).map_or(48_000.0, |f| f.mSampleRate as f32);

            let dict = aggregate(&output_uid, &tap_uid);
            let cf: &CFDictionary = unsafe { &*(Retained::as_ptr(&dict) as *const CFDictionary) };
            let status = unsafe { AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut t.agg)) };
            if status != 0 || t.agg == 0 {
                return Err(format!("couldn't make the aggregate device ({status})"));
            }
            t.analyzer = Box::into_raw(Box::new(Analyzer::new(rate)));
            let status = unsafe { AudioDeviceCreateIOProcID(t.agg, Some(io_proc), t.analyzer.cast(), NonNull::from(&mut t.proc_id)) };
            if status != 0 || t.proc_id.is_none() {
                return Err(format!("couldn't add the IO callback ({status})"));
            }
            let status = unsafe { AudioDeviceStart(t.agg, t.proc_id) };
            if status != 0 {
                return Err(format!("couldn't start the tap ({status})"));
            }
            log::info!("audio levels: tapping system audio at {rate} Hz");
            Ok(t)
        }

        /// Still on the default output device (it follows headphones, AirPlay, …).
        pub fn current(&self) -> bool {
            read_u32(kAudioObjectSystemObject as AudioObjectID, kAudioHardwarePropertyDefaultOutputDevice) == Some(self.output)
        }
    }

    impl Drop for Tap {
        fn drop(&mut self) {
            unsafe {
                if self.agg != 0 && self.proc_id.is_some() {
                    AudioDeviceStop(self.agg, self.proc_id);
                    AudioDeviceDestroyIOProcID(self.agg, self.proc_id);
                }
                if self.agg != 0 {
                    AudioHardwareDestroyAggregateDevice(self.agg);
                }
                if self.tap != 0 {
                    AudioHardwareDestroyProcessTap(self.tap);
                }
                if !self.analyzer.is_null() {
                    drop(Box::from_raw(self.analyzer));
                }
            }
        }
    }

    /// Everything playing but openNook, mixed to stereo, still going to the speakers.
    unsafe fn create_tap() -> Result<AudioObjectID, String> {
        let ours = own_process_object();
        let ids: Vec<Retained<NSNumber>> = ours.into_iter().map(NSNumber::new_u32).collect();
        let refs: Vec<&NSNumber> = ids.iter().map(|n| n.as_ref()).collect();
        let exclude = NSArray::from_slice(&refs);
        let desc = CATapDescription::initStereoGlobalTapButExcludeProcesses(CATapDescription::alloc(), &exclude);
        desc.setName(&NSString::from_str("openNook levels"));
        desc.setPrivate(true);
        desc.setMuteBehavior(CATapMuteBehavior::Unmuted);
        let mut tap: AudioObjectID = 0;
        let status = AudioHardwareCreateProcessTap(Some(&desc), &mut tap);
        if status != 0 || tap == 0 {
            return Err(format!("couldn't create the process tap ({status}); macOS 14.2 or later is needed"));
        }
        Ok(tap)
    }

    fn own_process_object() -> Option<AudioObjectID> {
        let pid = std::process::id() as i32;
        let addr = address(kAudioHardwarePropertyTranslatePIDToProcessObject);
        let mut object: AudioObjectID = 0;
        let mut size = std::mem::size_of::<AudioObjectID>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&addr),
                std::mem::size_of::<i32>() as u32,
                (&pid as *const i32).cast(),
                NonNull::from(&mut size),
                NonNull::from(&mut object).cast(),
            )
        };
        (status == 0 && object != 0).then_some(object)
    }

    /// A private aggregate device clocked by the output device, whose only input is the tap.
    fn aggregate(output_uid: &str, tap_uid: &str) -> Retained<NSDictionary<NSString, AnyObject>> {
        let ns = |k: &CStr| NSString::from_str(k.to_str().unwrap_or_default());
        let yes = NSNumber::new_bool(true);
        let no = NSNumber::new_bool(false);
        let output = NSString::from_str(output_uid);
        let sub = NSDictionary::<NSString, AnyObject>::from_slices(&[&*ns(kAudioSubDeviceUIDKey)], &[&*output]);
        let tap = NSDictionary::<NSString, AnyObject>::from_slices(
            &[&*ns(kAudioSubTapUIDKey), &*ns(kAudioSubTapDriftCompensationKey)],
            &[&*NSString::from_str(tap_uid), &*yes],
        );
        let subs = NSArray::from_retained_slice(&[sub]);
        let taps = NSArray::from_retained_slice(&[tap]);
        let name = NSString::from_str("openNook levels");
        let uid = NSUUID::new().UUIDString();
        NSDictionary::<NSString, AnyObject>::from_slices(
            &[
                &*ns(kAudioAggregateDeviceNameKey),
                &*ns(kAudioAggregateDeviceUIDKey),
                &*ns(kAudioAggregateDeviceMainSubDeviceKey),
                &*ns(kAudioAggregateDeviceIsPrivateKey),
                &*ns(kAudioAggregateDeviceIsStackedKey),
                &*ns(kAudioAggregateDeviceTapAutoStartKey),
                &*ns(kAudioAggregateDeviceSubDeviceListKey),
                &*ns(kAudioAggregateDeviceTapListKey),
            ],
            &[&*name, &*uid, &*output, &*yes, &*no, &*yes, &*subs, &*taps],
        )
    }

    unsafe extern "C-unwind" fn io_proc(
        _device: AudioObjectID,
        _now: NonNull<AudioTimeStamp>,
        input: NonNull<AudioBufferList>,
        _input_time: NonNull<AudioTimeStamp>,
        output: NonNull<AudioBufferList>,
        _output_time: NonNull<AudioTimeStamp>,
        client: *mut c_void,
    ) -> i32 {
        let _ = std::panic::catch_unwind(|| analyze(input, output, client.cast::<Analyzer>()));
        0
    }

    /// Mixes every channel of the tap's buffers to mono into the analyzer; plays silence.
    fn analyze(input: NonNull<AudioBufferList>, output: NonNull<AudioBufferList>, analyzer: *mut Analyzer) {
        unsafe {
            let out = output.as_ref();
            for b in std::slice::from_raw_parts_mut(out.mBuffers.as_ptr() as *mut AudioBuffer, out.mNumberBuffers as usize) {
                if !b.mData.is_null() {
                    std::ptr::write_bytes(b.mData.cast::<u8>(), 0, b.mDataByteSize as usize);
                }
            }
            let Some(analyzer) = analyzer.as_mut() else { return };
            let list = input.as_ref();
            let bufs = std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize);
            let channels: u32 = bufs.iter().map(|b| b.mNumberChannels.max(1)).sum();
            let frames = bufs
                .iter()
                .filter(|b| !b.mData.is_null())
                .map(|b| b.mDataByteSize as usize / 4 / b.mNumberChannels.max(1) as usize)
                .min()
                .unwrap_or(0);
            CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if frames == 0 || channels == 0 {
                return;
            }
            for f in 0..frames {
                let mut sum = 0.0;
                for b in bufs {
                    let ch = b.mNumberChannels.max(1) as usize;
                    let data = b.mData as *const f32;
                    for c in 0..ch {
                        sum += *data.add(f * ch + c);
                    }
                }
                let s = sum / channels as f32;
                if s.abs() > f32::from_bits(RAW_PEAK.load(std::sync::atomic::Ordering::Relaxed)) {
                    RAW_PEAK.store(s.abs().to_bits(), std::sync::atomic::Ordering::Relaxed);
                }
                analyzer.push(s);
            }
            publish(&analyzer.finish());
        }
    }

    fn address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress { mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain }
    }

    fn read<T>(object: AudioObjectID, selector: u32, value: &mut T) -> bool {
        let addr = address(selector);
        let mut size = std::mem::size_of::<T>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(object, NonNull::from(&addr), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::from(value).cast())
        };
        status == 0
    }

    fn read_u32(object: AudioObjectID, selector: u32) -> Option<u32> {
        let mut v = 0u32;
        read(object, selector, &mut v).then_some(v).filter(|v| *v != 0)
    }

    fn read_format(object: AudioObjectID) -> Option<AudioStreamBasicDescription> {
        let mut f: AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
        read(object, kAudioTapPropertyFormat, &mut f).then_some(f)
    }

    fn read_string(object: AudioObjectID, selector: u32) -> Option<String> {
        let mut cf: *const CFString = std::ptr::null();
        if !read(object, selector, &mut cf) {
            return None;
        }
        let owned = unsafe { CFRetained::from_raw(NonNull::new(cf as *mut CFString)?) };
        Some(owned.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    pub struct Tap;
    impl Tap {
        pub fn start() -> Result<Self, String> {
            Err("system audio levels need macOS".into())
        }
        pub fn current(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, seconds: f32, amp: f32) -> [f32; BANDS] {
        let rate = 48_000.0;
        let mut a = Analyzer::new(rate);
        let mut out = [0.0; BANDS];
        let block = 512;
        let n = (seconds * rate) as usize;
        for i in 0..n {
            a.push(amp * (2.0 * std::f32::consts::PI * hz * i as f32 / rate).sin());
            if i % block == block - 1 {
                out = a.finish();
            }
        }
        out
    }

    #[test]
    fn a_steady_bass_tone_sits_mid_height_in_the_bass_band() {
        let l = tone(60.0, 2.0, 0.5);
        assert!((0.35..0.65).contains(&l[0]), "{l:?}");
        assert!(l[4] < 0.05, "{l:?}");
    }

    #[test]
    fn a_steady_high_tone_sits_mid_height_in_the_treble_band() {
        let l = tone(9000.0, 2.0, 0.5);
        assert!((0.35..0.65).contains(&l[4]), "{l:?}");
        assert!(l[0] < 0.05, "{l:?}");
    }

    #[test]
    fn silence_reads_zero() {
        assert_eq!(tone(1000.0, 0.3, 0.0), [0.0; BANDS]);
    }

    /// A kick every half second: the bass bar should swing, not sit pinned at the top.
    #[test]
    fn beats_swing_the_bar_instead_of_pinning_it() {
        let rate = 48_000.0;
        let mut a = Analyzer::new(rate);
        let (mut lo, mut hi) = (1.0f32, 0.0f32);
        for i in 0..(rate as usize * 4) {
            let t = i as f32 / rate;
            let env = (-(t % 0.5) * 18.0).exp();
            a.push(0.6 * env * (2.0 * std::f32::consts::PI * 60.0 * t).sin());
            if i % 512 == 511 {
                let l = a.finish()[0];
                if t > 2.0 {
                    lo = lo.min(l);
                    hi = hi.max(l);
                }
            }
        }
        assert!(hi > 0.85 && lo < 0.3, "bass bar swung {lo}..{hi}");
    }

    #[test]
    fn levels_fall_after_the_sound_stops() {
        let rate = 48_000.0;
        let mut a = Analyzer::new(rate);
        for i in 0..24_000 {
            a.push(0.5 * (2.0 * std::f32::consts::PI * 60.0 * i as f32 / rate).sin());
        }
        let loud = a.finish()[0];
        for _ in 0..24_000 {
            a.push(0.0);
        }
        let after = a.finish()[0];
        assert!(loud > 0.3 && after < 0.05, "{loud} → {after}");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod probe {
    /// Taps the Mac's audio for four seconds and prints the loudest each band got. Play
    /// something first. `cargo test -p nook-core live_tap -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_tap() {
        let tap = super::macos::Tap::start();
        println!("tap: {:?}", tap.as_ref().map(|_| "started"));
        let mut max = [0.0f32; super::BANDS];
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let now: [f32; super::BANDS] = std::array::from_fn(|i| f32::from_bits(super::LEVELS[i].load(std::sync::atomic::Ordering::Relaxed)));
            for (m, v) in max.iter_mut().zip(now) {
                *m = m.max(v);
            }
        }
        println!("loudest per band: {max:?}");
        println!("callbacks: {}, raw peak: {}", super::macos::CALLS.load(std::sync::atomic::Ordering::Relaxed),
            f32::from_bits(super::macos::RAW_PEAK.load(std::sync::atomic::Ordering::Relaxed)));
        drop(tap);
    }
}
