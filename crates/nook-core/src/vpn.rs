//! Live VPN status: event-driven utun/ipsec/ppp classification.
//!
//! macOS parks a CFRunLoop on `SCDynamicStore` and classifies with
//! `getifaddrs` only when the kernel publishes an address/interface change.
//! No TCC, no Network Extension entitlement. Off macOS the first snapshot
//! is taken once and the watch channel stays quiet.

use crate::settings;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, SystemTime};
use tokio::sync::watch;

/// One classified tunnel plus the island-facing summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnSnapshot {
    pub connected: bool,
    pub service_name: String,
    pub interface: String,
    pub since: Option<SystemTime>,
    /// `true` when `since` is first-seen (VPN already up at launch), not a
    /// real connect timestamp from scutil / a watched down→up edge.
    pub since_estimated: bool,
    pub tunnel_count: usize,
}

impl Default for VpnSnapshot {
    fn default() -> Self {
        Self {
            connected: false,
            service_name: String::new(),
            interface: String::new(),
            since: None,
            since_estimated: false,
            tunnel_count: 0,
        }
    }
}

impl VpnSnapshot {
    pub fn display_name(&self) -> String {
        display_name(&self.service_name, &self.interface)
    }

    pub fn compact_right(&self, show_timer: bool, now: SystemTime) -> String {
        let name = self.display_name();
        if !self.connected {
            return if name.is_empty() {
                "Disconnected".into()
            } else {
                format!("{name} · off")
            };
        }
        compact_right_text(&name, elapsed_parts(self, now), show_timer)
    }

    pub fn elapsed_label(&self, now: SystemTime) -> Option<String> {
        let (secs, estimated) = elapsed_parts(self, now)?;
        let clock = format_elapsed(secs);
        Some(if estimated {
            format!("≥ {clock}")
        } else {
            clock
        })
    }
}

/// Address row used by the classifier. Tests feed this directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfAddr {
    pub name: String,
    pub up: bool,
    pub running: bool,
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
}

fn channel() -> &'static watch::Sender<VpnSnapshot> {
    static TX: OnceLock<watch::Sender<VpnSnapshot>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, _rx) = watch::channel(VpnSnapshot::default());
        tx
    })
}

struct MonitorState {
    snap: VpnSnapshot,
    primed: bool,
}

fn state() -> &'static Mutex<MonitorState> {
    static STATE: OnceLock<Mutex<MonitorState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(MonitorState {
            snap: VpnSnapshot::default(),
            primed: false,
        })
    })
}

/// Latest snapshot. Cheap; the island tick should not poll this.
pub fn current() -> VpnSnapshot {
    channel().borrow().clone()
}

pub fn subscribe() -> watch::Receiver<VpnSnapshot> {
    channel().subscribe()
}

/// Re-classify now (startup, settings ignore-list edits, watchdog).
pub fn refresh() {
    refresh_and_publish();
}

/// Register the SCDynamicStore run loop. Idempotent; no-op off macOS
/// besides a one-shot `getifaddrs` snapshot.
pub fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        refresh_and_publish();
        #[cfg(target_os = "macos")]
        {
            let handle = match std::thread::Builder::new()
                .name("nook-vpn".into())
                .spawn(macos::run_loop)
            {
                Ok(handle) => handle,
                Err(err) => {
                    log::warn!("vpn monitor thread: {err}");
                    return;
                }
            };
            let _ = std::thread::Builder::new()
                .name("nook-vpn-watchdog".into())
                .spawn(move || {
                    let _ = handle.join();
                    log::warn!("vpn monitor ended; falling back to 60s snapshots");
                    loop {
                        std::thread::sleep(Duration::from_secs(60));
                        refresh_and_publish();
                    }
                });
        }
    });
}

fn refresh_and_publish() {
    let ignore = settings::get_app_settings().vpn_ignore_interfaces;
    let interfaces = collect_ifaddrs();
    let tunnels = classify_vpn_interfaces(&interfaces, &ignore);
    let now = SystemTime::now();
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let transition = !guard.primed || guard.snap.connected != !tunnels.is_empty();
    let names = if transition && !tunnels.is_empty() {
        lookup_service_names(&tunnels)
    } else {
        Vec::new()
    };
    let connect_time = if transition && !tunnels.is_empty() {
        names
            .first()
            .and_then(|name| lookup_connect_time(name))
            .flatten()
    } else {
        None
    };
    let next = build_snapshot(&tunnels, &names, connect_time, &guard, now);
    let changed = !same_identity(&guard.snap, &next) || !guard.primed;
    guard.snap = next.clone();
    guard.primed = true;
    drop(guard);
    if changed {
        let _ = channel().send(next);
    }
}

fn same_identity(a: &VpnSnapshot, b: &VpnSnapshot) -> bool {
    a.connected == b.connected
        && a.service_name == b.service_name
        && a.interface == b.interface
        && a.tunnel_count == b.tunnel_count
        && a.since_estimated == b.since_estimated
}

pub fn is_vpn_interface_name(name: &str) -> bool {
    let stem = name
        .split_once(':')
        .map(|(head, _)| head)
        .unwrap_or(name)
        .to_ascii_lowercase();
    let body = stem
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches(|c: char| !c.is_ascii_alphabetic());
    matches!(body, "utun" | "ipsec" | "ppp")
}

pub fn is_ignored_interface(name: &str, ignore: &[String]) -> bool {
    ignore.iter().any(|pat| {
        let pat = pat.trim();
        !pat.is_empty() && name.eq_ignore_ascii_case(pat)
    })
}

pub fn is_routable_ipv6(addr: Ipv6Addr) -> bool {
    !addr.is_unspecified() && !addr.is_loopback() && !addr.is_unicast_link_local()
}

pub fn has_routable_address(ipv4: Option<Ipv4Addr>, ipv6: &[Ipv6Addr]) -> bool {
    let v4 = ipv4.is_some_and(|addr| {
        !addr.is_unspecified() && !addr.is_loopback() && !addr.is_link_local()
    });
    v4 || ipv6.iter().copied().any(is_routable_ipv6)
}

pub fn is_active_vpn(iface: &IfAddr, ignore: &[String]) -> bool {
    iface.up
        && iface.running
        && is_vpn_interface_name(&iface.name)
        && has_routable_address(iface.ipv4, &iface.ipv6)
        && !is_ignored_interface(&iface.name, ignore)
}

pub fn classify_vpn_interfaces<'a>(ifaces: &'a [IfAddr], ignore: &[String]) -> Vec<&'a IfAddr> {
    ifaces
        .iter()
        .filter(|iface| is_active_vpn(iface, ignore))
        .collect()
}

pub fn parse_ignore_list(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn format_ignore_list(names: &[String]) -> String {
    names.join(", ")
}

pub fn display_name(service_name: &str, interface: &str) -> String {
    let service = service_name.trim();
    if !service.is_empty() {
        return service.to_string();
    }
    let iface = interface.trim();
    if iface.is_empty() {
        "VPN".into()
    } else {
        format!("VPN · {iface}")
    }
}

pub fn format_elapsed(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h}:{m:02}:{s:02}")
}

pub fn compact_right_text(
    name: &str,
    elapsed: Option<(u64, bool)>,
    show_timer: bool,
) -> String {
    match (show_timer, elapsed) {
        (true, Some((secs, true))) => format!("{name} · ≥ {}", format_elapsed(secs)),
        (true, Some((secs, false))) => format!("{name} · {}", format_elapsed(secs)),
        _ => name.to_string(),
    }
}

pub fn parse_scutil_nc_list(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| line.contains("(Connected)"))
        .filter_map(|line| {
            let start = line.find('"')?;
            let rest = &line[start + 1..];
            let end = rest.find('"')?;
            let name = rest[..end].trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// `scutil --nc status` ConnectTime, usually a UNIX seconds value.
pub fn parse_scutil_connect_time(stdout: &str) -> Option<SystemTime> {
    for line in stdout.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("connecttime") && !lower.contains("connect time") {
            continue;
        }
        let value = line.split_once(':')?.1.trim();
        let token = value
            .split_whitespace()
            .next()
            .unwrap_or(value)
            .trim_matches(|c: char| !c.is_ascii_digit());
        if let Ok(secs) = token.parse::<u64>() {
            if secs > 1_000_000 {
                return Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
            }
        }
    }
    None
}

fn elapsed_parts(snap: &VpnSnapshot, now: SystemTime) -> Option<(u64, bool)> {
    let since = snap.since?;
    let secs = now.duration_since(since).ok()?.as_secs();
    Some((secs, snap.since_estimated))
}

fn build_snapshot(
    tunnels: &[&IfAddr],
    names: &[String],
    connect_time: Option<SystemTime>,
    state: &MonitorState,
    now: SystemTime,
) -> VpnSnapshot {
    if tunnels.is_empty() {
        return VpnSnapshot {
            connected: false,
            service_name: state.snap.service_name.clone(),
            interface: state.snap.interface.clone(),
            since: None,
            since_estimated: false,
            tunnel_count: 0,
        };
    }

    let primary = pick_primary(tunnels, &state.snap.interface);
    let interface = primary.name.clone();
    let service_name = names
        .first()
        .cloned()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            state
                .snap
                .connected
                .then(|| state.snap.service_name.clone())
                .filter(|s| !s.is_empty() && state.snap.interface == interface)
        })
        .unwrap_or_default();

    let same_iface = state.snap.connected && state.snap.interface == interface;
    let (since, since_estimated) = if same_iface {
        (state.snap.since.or(Some(now)), state.snap.since_estimated)
    } else if let Some(real) = connect_time {
        (Some(real), false)
    } else if !state.primed {
        (Some(now), true)
    } else {
        (Some(now), false)
    };

    VpnSnapshot {
        connected: true,
        service_name,
        interface,
        since,
        since_estimated,
        tunnel_count: tunnels.len(),
    }
}

fn pick_primary<'a>(tunnels: &'a [&'a IfAddr], previous: &str) -> &'a IfAddr {
    tunnels
        .iter()
        .copied()
        .find(|iface| iface.name == previous)
        .or_else(|| {
            tunnels
                .iter()
                .copied()
                .find(|iface| previous.is_empty() || iface.name != previous)
        })
        .unwrap_or(tunnels[0])
}

fn lookup_service_names(tunnels: &[&IfAddr]) -> Vec<String> {
    let _ = tunnels;
    #[cfg(target_os = "macos")]
    {
        macos::scutil_connected_names()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

fn lookup_connect_time(service_name: &str) -> Option<Option<SystemTime>> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::scutil_connect_time(service_name))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = service_name;
        None
    }
}

#[cfg(unix)]
fn collect_ifaddrs() -> Vec<IfAddr> {
    use std::collections::BTreeMap;
    use std::ffi::CStr;

    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return Vec::new();
        }
        let mut by_name: BTreeMap<String, IfAddr> = BTreeMap::new();
        let mut ptr = ifap;
        while !ptr.is_null() {
            let entry = &*ptr;
            if entry.ifa_name.is_null() {
                ptr = entry.ifa_next;
                continue;
            }
            let name = CStr::from_ptr(entry.ifa_name)
                .to_string_lossy()
                .into_owned();
            let flags = entry.ifa_flags;
            let rec = by_name.entry(name.clone()).or_insert_with(|| IfAddr {
                name,
                up: false,
                running: false,
                ipv4: None,
                ipv6: Vec::new(),
            });
            rec.up |= flags & libc::IFF_UP as libc::c_uint != 0;
            rec.running |= flags & libc::IFF_RUNNING as libc::c_uint != 0;
            if !entry.ifa_addr.is_null() {
                let family = (*entry.ifa_addr).sa_family as i32;
                if family == libc::AF_INET {
                    let sin = &*(entry.ifa_addr as *const libc::sockaddr_in);
                    rec.ipv4 = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
                } else if family == libc::AF_INET6 {
                    let sin6 = &*(entry.ifa_addr as *const libc::sockaddr_in6);
                    rec.ipv6.push(Ipv6Addr::from(sin6.sin6_addr.s6_addr));
                }
            }
            ptr = entry.ifa_next;
        }
        libc::freeifaddrs(ifap);
        by_name.into_values().collect()
    }
}

#[cfg(not(unix))]
fn collect_ifaddrs() -> Vec<IfAddr> {
    Vec::new()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{parse_scutil_connect_time, parse_scutil_nc_list};
    use std::process::Command;
    use std::time::SystemTime;
    use system_configuration::core_foundation::array::CFArray;
    use system_configuration::core_foundation::runloop::{
        kCFRunLoopCommonModes, CFRunLoop, CFRunLoopRun,
    };
    use system_configuration::core_foundation::string::CFString;
    use system_configuration::dynamic_store::{
        SCDynamicStore, SCDynamicStoreBuilder, SCDynamicStoreCallBackContext,
    };

    const PATTERNS: [&str; 5] = [
        "State:/Network/Interface/utun[0-9]+/IPv4",
        "State:/Network/Interface/utun[0-9]+/IPv6",
        "State:/Network/Service/.*/PPP",
        "State:/Network/Service/.*/IPSec",
        "State:/Network/Global/IPv4",
    ];

    pub(super) fn run_loop() {
        let context = SCDynamicStoreCallBackContext {
            callout: on_change,
            info: (),
        };
        let store = SCDynamicStoreBuilder::new("openNook-vpn")
            .callback_context(context)
            .build();
        let patterns = CFArray::from_CFTypes(
            &PATTERNS
                .iter()
                .map(|p| CFString::new(p))
                .collect::<Vec<_>>(),
        );
        let keys: CFArray<CFString> = CFArray::from_CFTypes(&[]);
        if !store.set_notification_keys(&keys, &patterns) {
            log::warn!("SCDynamicStoreSetNotificationKeys failed");
            return;
        }
        let source = store.create_run_loop_source();
        let rl = CFRunLoop::get_current();
        rl.add_source(&source, unsafe { kCFRunLoopCommonModes });
        unsafe { CFRunLoopRun() };
    }

    fn on_change(_store: SCDynamicStore, _changed: CFArray<CFString>, _info: &mut ()) {
        super::refresh_and_publish();
    }

    pub(super) fn scutil_connected_names() -> Vec<String> {
        let output = Command::new("/usr/sbin/scutil")
            .args(["--nc", "list"])
            .output()
            .ok();
        output
            .filter(|o| o.status.success())
            .map(|o| parse_scutil_nc_list(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }

    pub(super) fn scutil_connect_time(service: &str) -> Option<SystemTime> {
        let output = Command::new("/usr/sbin/scutil")
            .args(["--nc", "status", service])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_scutil_connect_time(&String::from_utf8_lossy(&output.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn utun(name: &str, v4: Option<Ipv4Addr>, v6: &[&str], up: bool, running: bool) -> IfAddr {
        IfAddr {
            name: name.into(),
            up,
            running,
            ipv4: v4,
            ipv6: v6.iter().map(|s| s.parse().unwrap()).collect(),
        }
    }

    #[test]
    fn vpn_names_match_utun_ipsec_ppp_only() {
        assert!(is_vpn_interface_name("utun4"));
        assert!(is_vpn_interface_name("utun0"));
        assert!(is_vpn_interface_name("ipsec0"));
        assert!(is_vpn_interface_name("ppp0"));
        assert!(is_vpn_interface_name("UTUN12"));
        assert!(!is_vpn_interface_name("en0"));
        assert!(!is_vpn_interface_name("lo0"));
        assert!(!is_vpn_interface_name("bridge0"));
        assert!(!is_vpn_interface_name("ap1"));
        assert!(!is_vpn_interface_name("awdl0"));
        assert!(!is_vpn_interface_name("llw0"));
        assert!(!is_vpn_interface_name("anpi0"));
    }

    #[test]
    fn link_local_only_utun_is_not_a_vpn() {
        let system = utun(
            "utun0",
            None,
            &["fe80::1"],
            true,
            true,
        );
        assert!(!has_routable_address(system.ipv4, &system.ipv6));
        assert!(classify_vpn_interfaces(&[system], &[]).is_empty());
    }

    #[test]
    fn routable_v4_or_global_v6_counts() {
        let tailscale = utun("utun4", Some(Ipv4Addr::new(100, 64, 0, 2)), &[], true, true);
        let wg = utun(
            "utun5",
            None,
            &["fd7a:115c:a1e0::1"],
            true,
            true,
        );
        assert!(is_active_vpn(&tailscale, &[]));
        assert!(is_active_vpn(&wg, &[]));
        assert_eq!(classify_vpn_interfaces(&[tailscale, wg], &[]).len(), 2);
    }

    #[test]
    fn down_or_not_running_is_ignored() {
        let down = utun("utun4", Some(Ipv4Addr::new(10, 8, 0, 2)), &[], false, true);
        let stalled = utun("utun4", Some(Ipv4Addr::new(10, 8, 0, 2)), &[], true, false);
        assert!(!is_active_vpn(&down, &[]));
        assert!(!is_active_vpn(&stalled, &[]));
    }

    #[test]
    fn ignore_list_filters_false_positives() {
        let helper = utun("utun3", Some(Ipv4Addr::new(192, 168, 64, 1)), &[], true, true);
        assert!(is_active_vpn(&helper, &[]));
        assert!(!is_active_vpn(&helper, &["utun3".into()]));
        assert!(!is_active_vpn(&helper, &["UTUN3".into()]));
        assert_eq!(parse_ignore_list("utun3, ipsec0  ppp1"), vec!["utun3", "ipsec0", "ppp1"]);
        assert_eq!(format_ignore_list(&["utun3".into()]), "utun3");
    }

    #[test]
    fn loopback_and_unspecified_are_not_routable() {
        assert!(!has_routable_address(Some(Ipv4Addr::UNSPECIFIED), &[]));
        assert!(!has_routable_address(Some(Ipv4Addr::LOCALHOST), &[]));
        assert!(!has_routable_address(Some(Ipv4Addr::new(169, 254, 1, 1)), &[]));
        assert!(!is_routable_ipv6(Ipv6Addr::LOCALHOST));
        assert!(!is_routable_ipv6("fe80::abcd".parse().unwrap()));
        assert!(is_routable_ipv6("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn display_name_falls_back_to_interface() {
        assert_eq!(display_name("Tailscale", "utun4"), "Tailscale");
        assert_eq!(display_name("", "utun7"), "VPN · utun7");
        assert_eq!(display_name("  ", ""), "VPN");
    }

    #[test]
    fn compact_timer_marks_estimated_sessions() {
        assert_eq!(
            compact_right_text("Tailscale", Some((3661, false)), true),
            "Tailscale · 1:01:01"
        );
        assert_eq!(
            compact_right_text("VPN · utun4", Some((5, true)), true),
            "VPN · utun4 · ≥ 0:00:05"
        );
        assert_eq!(
            compact_right_text("Tailscale", Some((5, false)), false),
            "Tailscale"
        );
        assert_eq!(format_elapsed(0), "0:00:00");
        assert_eq!(format_elapsed(59), "0:00:59");
    }

    #[test]
    fn scutil_parsers_read_connected_names_and_unix_time() {
        let list = r#"
Available network connection services in the current set (* = enabled):
* (Connected)    0A1B2C3D-0000-0000-0000-000000000001 "Tailscale" [VPN:com.apple...]
* (Disconnected) 0A1B2C3D-0000-0000-0000-000000000002 "Work" [VPN:IKEv2]
* (Connected)    0A1B2C3D-0000-0000-0000-000000000003 "Home WG" [VPN:IKEv2]
"#;
        assert_eq!(
            parse_scutil_nc_list(list),
            vec!["Tailscale".to_string(), "Home WG".to_string()]
        );
        let status = r#"
<dictionary> {
  Status : Connected
  ExtendedStatus : <dictionary> {
    ConnectTime : 1700000000
  }
}
"#;
        assert_eq!(
            parse_scutil_connect_time(status),
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        );
        assert_eq!(parse_scutil_connect_time("Status : Connected"), None);
    }

    #[test]
    fn first_observation_is_estimated_until_a_real_edge() {
        let iface = utun("utun4", Some(Ipv4Addr::new(10, 8, 0, 2)), &[], true, true);
        let cold = MonitorState {
            snap: VpnSnapshot::default(),
            primed: false,
        };
        let first = build_snapshot(&[&iface], &["Tailscale".into()], None, &cold, SystemTime::UNIX_EPOCH);
        assert!(first.connected);
        assert!(first.since_estimated);
        assert_eq!(first.service_name, "Tailscale");
        assert_eq!(first.tunnel_count, 1);

        let live = MonitorState {
            snap: VpnSnapshot::default(),
            primed: true,
        };
        let edge = build_snapshot(&[&iface], &[], None, &live, SystemTime::UNIX_EPOCH);
        assert!(edge.connected);
        assert!(!edge.since_estimated);

        let down = build_snapshot(&[], &[], None, &MonitorState { snap: first.clone(), primed: true }, SystemTime::UNIX_EPOCH);
        assert!(!down.connected);
        assert_eq!(down.service_name, "Tailscale");
        assert!(down.since.is_none());
    }

    #[test]
    fn snapshot_compact_right_hides_timer_when_asked() {
        let snap = VpnSnapshot {
            connected: true,
            service_name: "Tailscale".into(),
            interface: "utun4".into(),
            since: Some(SystemTime::UNIX_EPOCH),
            since_estimated: true,
            tunnel_count: 1,
        };
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(12);
        assert_eq!(
            snap.compact_right(true, now),
            "Tailscale · ≥ 0:00:12"
        );
        assert_eq!(snap.compact_right(false, now), "Tailscale");
        assert_eq!(snap.elapsed_label(now).as_deref(), Some("≥ 0:00:12"));
    }
}
