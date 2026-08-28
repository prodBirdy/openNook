//! Local host sampler: CPU, memory, network throughput, disk capacity.
//!
//! Pure-sync. CPU and network are delta-based, so the sampler keeps the last
//! counters across collapse — the first frame after a re-expand can show a
//! real rate instead of "—". A gap longer than [`RATE_GAP`] is treated as a
//! reset (same idea as observe's `RATE_GAP_MS`). Never persist these samples.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, RefreshKind, System};

/// Discard rate samples older than this so a multi-hour collapse cannot
/// produce a nonsense average.
pub const RATE_GAP: Duration = Duration::from_millis(3 * 60 * 1000);

fn default_true() -> bool {
    true
}

/// Per-stat toggles + NIC filter. The widget itself is gated by
/// [`crate::settings::AppSettings::show_sysstats`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SysStatsSettings {
    #[serde(default = "default_true")]
    pub show_cpu: bool,
    #[serde(default = "default_true")]
    pub show_mem: bool,
    #[serde(default = "default_true")]
    pub show_net: bool,
    #[serde(default = "default_true")]
    pub show_disk: bool,
    /// Sum `en*` / `eth*` / `wl*` only; skip loopback, utun, docker, etc.
    #[serde(default = "default_true")]
    pub physical_nics: bool,
}

impl Default for SysStatsSettings {
    fn default() -> Self {
        Self {
            show_cpu: true,
            show_mem: true,
            show_net: true,
            show_disk: true,
            physical_nics: true,
        }
    }
}

/// One host reading. Rate fields are `None` until a second sample arrives
/// (or after a stale gap / counter reset).
#[derive(Debug, Clone, PartialEq)]
pub struct SysSnapshot {
    pub cpu_pct: Option<f32>,
    pub per_core: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub net_up_bps: Option<f64>,
    pub net_down_bps: Option<f64>,
    pub disk_used: u64,
    pub disk_total: u64,
}

impl Default for SysSnapshot {
    fn default() -> Self {
        Self {
            cpu_pct: None,
            per_core: Vec::new(),
            mem_used: 0,
            mem_total: 0,
            net_up_bps: None,
            net_down_bps: None,
            disk_used: 0,
            disk_total: 0,
        }
    }
}

/// Raw counters from one host scrape. Tests feed this into [`SysSampler::apply`].
#[derive(Debug, Clone, PartialEq)]
pub struct RawSample {
    pub cpu: Option<CpuTicks>,
    pub cores: Vec<CpuTicks>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub disk_used: u64,
    pub disk_total: u64,
}

/// Aggregate (or per-core) tick counters. Busy = user + system + nice
/// (+ irq/softirq/steal on Linux). Total includes idle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTicks {
    pub busy: u64,
    pub total: u64,
}

/// Stateful sampler. Keep one of these for the process lifetime so collapse
/// does not throw away the last counters.
pub struct SysSampler {
    prev_cpu: Option<CpuTicks>,
    prev_cores: Vec<CpuTicks>,
    prev_rx: Option<u64>,
    prev_tx: Option<u64>,
    prev_at: Option<Instant>,
    sys: System,
    nets: Networks,
}

impl Default for SysSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl SysSampler {
    pub fn new() -> Self {
        // No process list — agents.rs already notes a full refresh is heavy.
        let sys = System::new_with_specifics(RefreshKind::nothing());
        Self {
            prev_cpu: None,
            prev_cores: Vec::new(),
            prev_rx: None,
            prev_tx: None,
            prev_at: None,
            sys,
            nets: Networks::new_with_refreshed_list(),
        }
    }

    /// Live host sample. `physical_nics` matches [`SysStatsSettings::physical_nics`].
    pub fn sample(&mut self, physical_nics: bool) -> SysSnapshot {
        let raw = self.read_host(physical_nics);
        self.apply(raw, Instant::now())
    }

    /// Apply a raw scrape. Pure aside from `now` — this is what the tests drive.
    pub fn apply(&mut self, raw: RawSample, now: Instant) -> SysSnapshot {
        let elapsed = self.prev_at.map(|prev| now.saturating_duration_since(prev));
        let fresh = elapsed.is_some_and(|dt| dt > Duration::ZERO && dt <= RATE_GAP);

        let cpu_pct = if fresh {
            match (self.prev_cpu, raw.cpu) {
                (Some(prev), Some(next)) => cpu_pct(prev, next),
                _ => None,
            }
        } else {
            None
        };
        let per_core = if fresh {
            raw.cores
                .iter()
                .enumerate()
                .map(|(i, next)| {
                    self.prev_cores
                        .get(i)
                        .and_then(|prev| cpu_pct(*prev, *next))
                        .unwrap_or(0.0)
                })
                .collect()
        } else {
            Vec::new()
        };

        let (net_down_bps, net_up_bps) = if fresh {
            let dt = elapsed.unwrap();
            let down = self
                .prev_rx
                .and_then(|prev| rate_bps(counter_delta(prev, raw.net_rx), dt));
            let up = self
                .prev_tx
                .and_then(|prev| rate_bps(counter_delta(prev, raw.net_tx), dt));
            (down, up)
        } else {
            (None, None)
        };

        self.prev_cpu = raw.cpu;
        self.prev_cores = raw.cores;
        self.prev_rx = Some(raw.net_rx);
        self.prev_tx = Some(raw.net_tx);
        self.prev_at = Some(now);

        SysSnapshot {
            cpu_pct,
            per_core,
            mem_used: raw.mem_used,
            mem_total: raw.mem_total,
            net_up_bps,
            net_down_bps,
            disk_used: raw.disk_used,
            disk_total: raw.disk_total,
        }
    }

    fn read_host(&mut self, physical_nics: bool) -> RawSample {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.nets.refresh(true);

        let (cpu, cores) = read_cpu_ticks().unwrap_or_else(|| {
            // sysinfo only exposes percentages. Accumulate synthetic ticks
            // so apply() still does the delta / stale-gap math.
            let pct = self.sys.global_cpu_usage();
            let cores: Vec<CpuTicks> = self
                .sys
                .cpus()
                .iter()
                .enumerate()
                .map(|(i, cpu)| accumulate_pct(self.prev_cores.get(i).copied(), cpu.cpu_usage()))
                .collect();
            (Some(accumulate_pct(self.prev_cpu, pct)), cores)
        });

        let (net_rx, net_tx) = sum_net(&self.nets, physical_nics);
        let (disk_used, disk_total) = root_disk();

        RawSample {
            cpu,
            cores,
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            net_rx,
            net_tx,
            disk_used,
            disk_total,
        }
    }
}

fn accumulate_pct(prev: Option<CpuTicks>, pct: f32) -> CpuTicks {
    let add = pct.clamp(0.0, 100.0).round() as u64;
    let prev = prev.unwrap_or_default();
    CpuTicks {
        busy: prev.busy.saturating_add(add),
        total: prev.total.saturating_add(100),
    }
}

/// Busy / total delta as a 0–100 percentage. `None` when the counter did
/// not advance (idle tick, or a reset that produced `total == 0`).
pub fn cpu_pct(prev: CpuTicks, next: CpuTicks) -> Option<f32> {
    let busy = next.busy.saturating_sub(prev.busy);
    let total = next.total.saturating_sub(prev.total);
    if total == 0 {
        return None;
    }
    Some(((busy as f64) * 100.0 / (total as f64)) as f32)
}

/// Wrap-aware unsigned delta.
///
/// A decrease is treated as a single 32-bit wrap when both values fit in
/// `u32` (getifaddrs `ifi_*bytes` counters). A decrease on a 64-bit counter
/// is a reset (interface bounce) and yields `0`.
pub fn counter_delta(prev: u64, next: u64) -> u64 {
    if next >= prev {
        next - prev
    } else if prev <= u32::MAX as u64 && next <= u32::MAX as u64 {
        (u32::MAX as u64 - prev) + next + 1
    } else {
        0
    }
}

/// Bytes-per-second from a byte delta and elapsed time. `None` when the
/// interval is empty or longer than [`RATE_GAP`].
pub fn rate_bps(delta: u64, elapsed: Duration) -> Option<f64> {
    if elapsed.is_zero() || elapsed > RATE_GAP {
        return None;
    }
    Some(delta as f64 / elapsed.as_secs_f64())
}

/// Loopback and virtual links the default filter skips.
pub fn include_iface(name: &str, physical_only: bool) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    if is_loopback(name) {
        return false;
    }
    if !physical_only {
        return !is_virtual(name);
    }
    is_physical(name)
}

fn is_loopback(name: &str) -> bool {
    name == "lo" || name == "lo0" || name.starts_with("lo")
}

fn is_virtual(name: &str) -> bool {
    name.starts_with("utun")
        || name.starts_with("tun")
        || name.starts_with("tap")
        || name.starts_with("awdl")
        || name.starts_with("llw")
        || name.starts_with("bridge")
        || name.starts_with("docker")
        || name.starts_with("veth")
        || name.starts_with("br-")
        || name.starts_with("cni")
        || name.starts_with("flannel")
}

fn is_physical(name: &str) -> bool {
    name.starts_with("en") || name.starts_with("eth") || name.starts_with("wl")
}

fn sum_net(nets: &Networks, physical_nics: bool) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    for (name, data) in nets.iter() {
        if !include_iface(name, physical_nics) {
            continue;
        }
        rx = rx.saturating_add(data.total_received());
        tx = tx.saturating_add(data.total_transmitted());
    }
    (rx, tx)
}

fn root_disk() -> (u64, u64) {
    let disks = Disks::new_with_refreshed_list();
    for disk in disks.list() {
        if disk.mount_point() == std::path::Path::new("/") {
            let total = disk.total_space();
            let avail = disk.available_space();
            return (total.saturating_sub(avail), total);
        }
    }
    let mut used = 0u64;
    let mut total = 0u64;
    for disk in disks.list() {
        let t = disk.total_space();
        total = total.saturating_add(t);
        used = used.saturating_add(t.saturating_sub(disk.available_space()));
    }
    (used, total)
}

fn read_cpu_ticks() -> Option<(Option<CpuTicks>, Vec<CpuTicks>)> {
    #[cfg(target_os = "linux")]
    {
        return parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?);
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Parse Linux `/proc/stat`. First `cpu ` line is the aggregate; `cpuN` lines
/// are per-core. Columns: user nice system idle iowait irq softirq steal …
pub fn parse_proc_stat(text: &str) -> Option<(Option<CpuTicks>, Vec<CpuTicks>)> {
    let mut agg = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(label) = parts.next() else {
            continue;
        };
        if !label.starts_with("cpu") {
            break;
        }
        let nums: Vec<u64> = parts.filter_map(|s| s.parse().ok()).collect();
        if nums.len() < 4 {
            continue;
        }
        let user = nums[0];
        let nice = nums[1];
        let system = nums[2];
        let idle = nums[3];
        let iowait = nums.get(4).copied().unwrap_or(0);
        let irq = nums.get(5).copied().unwrap_or(0);
        let softirq = nums.get(6).copied().unwrap_or(0);
        let steal = nums.get(7).copied().unwrap_or(0);
        let busy = user
            .saturating_add(nice)
            .saturating_add(system)
            .saturating_add(irq)
            .saturating_add(softirq)
            .saturating_add(steal);
        let total = busy.saturating_add(idle).saturating_add(iowait);
        let ticks = CpuTicks { busy, total };
        if label == "cpu" {
            agg = Some(ticks);
        } else {
            cores.push(ticks);
        }
    }
    if agg.is_none() && cores.is_empty() {
        return None;
    }
    Some((agg, cores))
}

static SAMPLER: OnceLock<Mutex<SysSampler>> = OnceLock::new();

fn sampler() -> &'static Mutex<SysSampler> {
    SAMPLER.get_or_init(|| Mutex::new(SysSampler::new()))
}

/// Process-wide sample. The island calls this only while the SysStats card
/// is on screen so idle cost stays zero.
pub fn sample(physical_nics: bool) -> SysSnapshot {
    sampler()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sample(physical_nics)
}

pub fn format_pct(pct: f32) -> String {
    format!("{pct:.0}%")
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let n = bytes as f64;
    if n >= TIB {
        format!("{:.1} TB", n / TIB)
    } else if n >= GIB {
        format!("{:.1} GB", n / GIB)
    } else if n >= MIB {
        format!("{:.0} MB", n / MIB)
    } else if n >= KIB {
        format!("{:.0} KB", n / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_bps(bps: f64) -> String {
    if !bps.is_finite() || bps < 0.0 {
        return "—".into();
    }
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bps >= GIB {
        format!("{:.1} GB/s", bps / GIB)
    } else if bps >= MIB {
        format!("{:.1} MB/s", bps / MIB)
    } else if bps >= KIB {
        format!("{:.0} KB/s", bps / KIB)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(busy: u64, total: u64) -> CpuTicks {
        CpuTicks { busy, total }
    }

    #[test]
    fn cpu_pct_is_busy_over_total() {
        assert_eq!(cpu_pct(ticks(10, 100), ticks(30, 200)), Some(20.0));
        assert_eq!(cpu_pct(ticks(0, 0), ticks(50, 100)), Some(50.0));
        assert_eq!(cpu_pct(ticks(5, 10), ticks(5, 10)), None);
        assert_eq!(cpu_pct(ticks(80, 100), ticks(80, 90)), None);
    }

    #[test]
    fn counter_delta_handles_increase_wrap_and_reset() {
        assert_eq!(counter_delta(100, 250), 150);
        assert_eq!(counter_delta(0, 0), 0);
        // 32-bit wrap: MAX-100 → 50 = 151 bytes.
        assert_eq!(
            counter_delta(u32::MAX as u64 - 100, 50),
            (u32::MAX as u64 - (u32::MAX as u64 - 100)) + 50 + 1
        );
        assert_eq!(counter_delta(u32::MAX as u64, 0), 1);
        // 64-bit decrease is an interface reset, not a wrap.
        assert_eq!(counter_delta(1 << 40, 12), 0);
    }

    #[test]
    fn rate_bps_rejects_empty_and_stale_intervals() {
        assert_eq!(rate_bps(1_024, Duration::from_secs(1)), Some(1_024.0));
        assert_eq!(rate_bps(2_048, Duration::from_millis(500)), Some(4_096.0));
        assert_eq!(rate_bps(100, Duration::ZERO), None);
        assert_eq!(rate_bps(100, RATE_GAP + Duration::from_millis(1)), None);
        assert_eq!(
            rate_bps(100, RATE_GAP),
            Some(100.0 / RATE_GAP.as_secs_f64())
        );
    }

    #[test]
    fn apply_computes_rates_and_discards_stale_gaps() {
        let mut sampler = SysSampler::new();
        let t0 = Instant::now();
        let first = sampler.apply(
            RawSample {
                cpu: Some(ticks(10, 100)),
                cores: vec![ticks(1, 10), ticks(2, 10)],
                mem_used: 4 * 1024 * 1024 * 1024,
                mem_total: 16 * 1024 * 1024 * 1024,
                net_rx: 1_000,
                net_tx: 2_000,
                disk_used: 100,
                disk_total: 200,
            },
            t0,
        );
        assert_eq!(first.cpu_pct, None);
        assert!(first.per_core.is_empty());
        assert_eq!(first.net_up_bps, None);
        assert_eq!(first.net_down_bps, None);
        assert_eq!(first.mem_used, 4 * 1024 * 1024 * 1024);
        assert_eq!(first.disk_total, 200);

        let second = sampler.apply(
            RawSample {
                cpu: Some(ticks(30, 200)),
                cores: vec![ticks(3, 20), ticks(8, 20)],
                mem_used: 5 * 1024 * 1024 * 1024,
                mem_total: 16 * 1024 * 1024 * 1024,
                net_rx: 1_000 + 2_048,
                net_tx: 2_000 + 4_096,
                disk_used: 110,
                disk_total: 200,
            },
            t0 + Duration::from_secs(1),
        );
        assert_eq!(second.cpu_pct, Some(20.0));
        assert_eq!(second.per_core, vec![20.0, 60.0]);
        assert_eq!(second.net_down_bps, Some(2_048.0));
        assert_eq!(second.net_up_bps, Some(4_096.0));

        let stale = sampler.apply(
            RawSample {
                cpu: Some(ticks(80, 400)),
                cores: vec![ticks(10, 40)],
                mem_used: 6,
                mem_total: 16,
                net_rx: 9_000,
                net_tx: 9_000,
                disk_used: 120,
                disk_total: 200,
            },
            t0 + Duration::from_secs(1) + RATE_GAP + Duration::from_secs(1),
        );
        assert_eq!(stale.cpu_pct, None);
        assert!(stale.per_core.is_empty());
        assert_eq!(stale.net_up_bps, None);
        assert_eq!(stale.net_down_bps, None);
        assert_eq!(stale.mem_used, 6);
    }

    #[test]
    fn apply_wraps_32bit_iface_counters() {
        let mut sampler = SysSampler::new();
        let t0 = Instant::now();
        let prev = u32::MAX as u64 - 200;
        sampler.apply(
            RawSample {
                cpu: None,
                cores: Vec::new(),
                mem_used: 0,
                mem_total: 0,
                net_rx: prev,
                net_tx: prev,
                disk_used: 0,
                disk_total: 0,
            },
            t0,
        );
        let next = sampler.apply(
            RawSample {
                cpu: None,
                cores: Vec::new(),
                mem_used: 0,
                mem_total: 0,
                net_rx: 55,
                net_tx: 10,
                disk_used: 0,
                disk_total: 0,
            },
            t0 + Duration::from_secs(1),
        );
        assert_eq!(next.net_down_bps, Some(counter_delta(prev, 55) as f64));
        assert_eq!(next.net_up_bps, Some(counter_delta(prev, 10) as f64));
    }

    #[test]
    fn parse_proc_stat_reads_aggregate_and_cores() {
        let text = "\
cpu  100 20 30 850 0 5 5 0 0 0
cpu0 50 10 15 425 0 2 3 0 0 0
cpu1 50 10 15 425 0 3 2 0 0 0
intr 1
";
        let (agg, cores) = parse_proc_stat(text).unwrap();
        let agg = agg.unwrap();
        // busy = 100+20+30+5+5+0 = 160; total = 160+850+0 = 1010
        assert_eq!(agg.busy, 160);
        assert_eq!(agg.total, 1010);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].busy, 80);
    }

    #[test]
    fn iface_filter_skips_loopback_and_virtual() {
        assert!(!include_iface("lo0", true));
        assert!(!include_iface("lo", false));
        assert!(include_iface("en0", true));
        assert!(include_iface("eth0", true));
        assert!(!include_iface("utun2", true));
        assert!(!include_iface("utun2", false));
        assert!(!include_iface("docker0", false));
        assert!(!include_iface("bridge0", false));
    }

    #[test]
    fn formatters_use_binary_units() {
        assert_eq!(format_pct(12.4), "12%");
        assert_eq!(format_bytes(16 * 1024 * 1024 * 1024), "16.0 GB");
        assert_eq!(format_bps(1_572_864.0), "1.5 MB/s");
        assert_eq!(format_bps(512.0), "512 B/s");
    }

    #[test]
    fn settings_default_shows_every_stat() {
        let parsed: SysStatsSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, SysStatsSettings::default());
        assert!(parsed.show_cpu && parsed.physical_nics);
    }
}
