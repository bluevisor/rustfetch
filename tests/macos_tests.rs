//! Test macOS-only platform functions

#![cfg(target_os = "macos")]

use std::ffi::CStr;

use rustfetch::{platform::*, sysinfo::*};

// get_macos_codename tests

#[test]
fn test_macos_codename_known_versions() {
    assert_eq!(get_macos_codename("27.0"), Some("Golden Gate"));
    assert_eq!(get_macos_codename("26.7.1"), Some("Tahoe"));
    assert_eq!(get_macos_codename("15.0"), Some("Sequoia"));
    assert_eq!(get_macos_codename("11.7.10"), Some("Big Sur"));
    assert_eq!(get_macos_codename("10.15.7"), Some("Catalina"));
}

#[test]
fn test_macos_codename_compat_version() {
    // Big Sur reports itself as 10.16 to binaries built with older SDKs
    assert_eq!(get_macos_codename("10.16"), Some("Big Sur"));
}

#[test]
fn test_macos_codename_unknown_versions() {
    assert_eq!(get_macos_codename("28.0"), None);
    assert_eq!(get_macos_codename("10.12.6"), None);
    assert_eq!(get_macos_codename(""), None);
}

// Shell tests

#[test]
fn test_parse_procargs_argv0() {
    // argc, the executable path, NUL padding, then argv
    let mut procargs = 3u32.to_le_bytes().to_vec();
    procargs.extend_from_slice(b"/bin/bash\0\0\0\0sh\0-c\0echo hi\0PATH=/usr/bin\0");
    assert_eq!(parse_procargs_argv0(&procargs), Some(String::from("sh")));

    let mut login = 1u32.to_le_bytes().to_vec();
    login.extend_from_slice(b"/opt/homebrew/bin/fish\0-fish\0");
    assert_eq!(parse_procargs_argv0(&login), Some(String::from("-fish")));
}

#[test]
fn test_parse_procargs_argv0_invalid() {
    assert_eq!(parse_procargs_argv0(b""), None);
    assert_eq!(parse_procargs_argv0(b"\x01\0\0\0/bin/zsh"), None);
    assert_eq!(parse_procargs_argv0(b"\x01\0\0\0/bin/zsh\0\0\0"), None);
}

#[test]
fn test_find_macho_cstring_section_universal_zsh() {
    // /bin/zsh ships on every macOS as a universal (fat) binary
    let data = std::fs::read("/bin/zsh").unwrap_or_default();
    let read_at = |offset: u64, length: usize| {
        let start = usize::try_from(offset).ok()?;
        data.get(start .. data.len().min(start + length)).map(<[u8]>::to_vec)
    };

    let (offset, size) = find_macho_cstring_section(read_at).unwrap_or((0, 0));
    assert!(size > 0 && size < data.len() as u64);

    let section = read_at(offset, size as usize).unwrap_or_default();
    assert!(find_version_after_marker(&section, b"zsh-").is_some());
}

#[test]
fn test_find_macho_cstring_section_not_macho() {
    let script = b"#!/bin/sh\necho not a binary\n".to_vec();
    let read_at = |offset: u64, length: usize| {
        let start = usize::try_from(offset).ok()?;
        script.get(start .. script.len().min(start + length)).map(<[u8]>::to_vec)
    };
    assert_eq!(find_macho_cstring_section(read_at), None);
}

#[test]
fn test_read_string_literals_is_smaller_than_the_binary() {
    let literals = read_string_literals("/bin/zsh").unwrap_or_default();
    let file_size = std::fs::metadata("/bin/zsh").map(|m| m.len()).unwrap_or(0);
    assert!(!literals.is_empty());
    assert!((literals.len() as u64) < file_size / 10);
}

#[test]
fn test_process_functions_on_self() {
    let pid = std::process::id();

    let (ppid, name) = get_process_info(pid).unwrap_or_default();
    assert_eq!(ppid, std::os::unix::process::parent_id());
    assert!(!name.is_empty());

    let path = get_process_path(pid).unwrap_or_default();
    let current_exe = std::env::current_exe().and_then(|p| p.canonicalize()).unwrap_or_default();
    assert_eq!(std::path::Path::new(&path), current_exe);

    assert!(get_process_argv0(pid).is_some_and(|argv0| !argv0.is_empty()));
}

#[test]
fn test_process_info_of_another_user() {
    // launchd belongs to root, the full BSD info call is refused for it (EPERM) while the short one
    // used here works, so the parent walk can go through sudo or su
    assert_eq!(get_process_info(1), Some((0, String::from("launchd"))));
    assert_eq!(get_process_path(1), Some(String::from("/sbin/launchd")));
}

#[test]
fn test_stock_shells_cheap_version_sources() {
    // zsh carries its version string, so it never has to be run (nor cached)
    assert!(get_shell_version("zsh", Some("/bin/zsh")).is_some());
    // Apple's bash 3.2 has no "@(#)Bash version" string, it is run once and then cached
    let bash = read_string_literals("/bin/bash").unwrap_or_default();
    assert!(!bash.is_empty());
    assert_eq!(find_version_after_marker(&bash, b"@(#)Bash version "), None);
}

#[test]
fn test_process_functions_missing_pid() {
    // pids are capped well below this
    assert_eq!(get_process_info(999_999_999), None);
    assert_eq!(get_process_path(999_999_999), None);
}

// GPU tests

/// Builds a pmgr voltage-states table from (frequency, voltage) pairs
fn voltage_states(pairs: &[(u32, u32)]) -> Vec<u8> {
    pairs
        .iter()
        .flat_map(|&(frequency, voltage)| {
            frequency.to_le_bytes().into_iter().chain(voltage.to_le_bytes())
        })
        .collect()
}

#[test]
fn test_gpu_frequency_m5_pro() {
    // "voltage-states9-sram" as read from an M5 Pro (20 core GPU), frequencies are in Hz
    let table = voltage_states(&[
        (0, 1385),
        (338_000_000, 1385),
        (486_000_000, 1385),
        (636_000_000, 1385),
        (796_000_000, 1385),
        (888_000_000, 1385),
        (988_000_000, 1385),
        (1_084_000_000, 1385),
        (1_182_000_000, 1385),
        (1_278_000_000, 1385),
        (1_374_000_000, 1385),
        (1_470_000_000, 1385),
        (1_578_000_000, 1385),
        (1_620_000_000, 1385),
    ]);
    assert_eq!(table.len(), 112);
    assert_eq!(parse_gpu_max_frequency(&table), Some(1620));
}

#[test]
fn test_gpu_frequency_khz_table() {
    // some chips store kHz instead of Hz
    let table = voltage_states(&[(0, 0), (720_000, 800), (1_398_000, 900)]);
    assert_eq!(parse_gpu_max_frequency(&table), Some(1398));
}

#[test]
fn test_gpu_frequency_invalid_tables() {
    assert_eq!(parse_gpu_max_frequency(&[]), None);
    // not a whole number of (frequency, voltage) pairs
    assert_eq!(parse_gpu_max_frequency(&[0; 12]), None);
    assert_eq!(parse_gpu_max_frequency(&voltage_states(&[(0, 0), (0, 900)])), None);
}

#[test]
fn test_gpu_type_from_vendor() {
    assert_eq!(get_gpu_type(0x106b), Some("Integrated")); // Apple
    assert_eq!(get_gpu_type(0x8086), Some("Integrated")); // Intel
    assert_eq!(get_gpu_type(0x1002), Some("Discrete")); // AMD
    assert_eq!(get_gpu_type(0x10de), Some("Discrete")); // NVIDIA
    assert_eq!(get_gpu_type(0x1af4), None); // virtio
}

#[test]
fn test_format_gpu() {
    assert_eq!(
        format_gpu("Apple M5 Pro", Some(20), Some(1620), Some("Integrated")),
        "Apple M5 Pro (20 cores) @ 1.62 GHz [Integrated]"
    );
    assert_eq!(
        format_gpu("AMD Radeon Pro 5500M", None, None, Some("Discrete")),
        "AMD Radeon Pro 5500M [Discrete]"
    );
    assert_eq!(
        format_gpu("Apple M1", Some(8), Some(1278), None),
        "Apple M1 (8 cores) @ 1.28 GHz"
    );
    assert_eq!(format_gpu("Apple M2", None, Some(2000), None), "Apple M2 @ 2.00 GHz");
    assert_eq!(format_gpu("Some GPU", Some(0), Some(800), None), "Some GPU @ 800 MHz");
    assert_eq!(format_gpu("Some GPU", None, Some(0), None), "Some GPU");
}

// Battery tests

#[test]
fn test_battery_percentage_apple_silicon() {
    // Apple Silicon reports CurrentCapacity already as a percent with MaxCapacity = 100
    assert_eq!(battery_percentage(80, 100), Some(80));
    assert_eq!(battery_percentage(100, 100), Some(100));
    assert_eq!(battery_percentage(0, 100), Some(0));
}

#[test]
fn test_battery_percentage_intel_mah() {
    // Intel reports mAh, rounded to the nearest percent and capped at 100
    assert_eq!(battery_percentage(4012, 5103), Some(79));
    assert_eq!(battery_percentage(2551, 5103), Some(50));
    assert_eq!(battery_percentage(5200, 5103), Some(100));
}

#[test]
fn test_battery_percentage_invalid() {
    assert_eq!(battery_percentage(50, 0), None);
    assert_eq!(battery_percentage(50, -1), None);
    assert_eq!(battery_percentage(-1, 100), None);
    // values that would overflow the percentage math
    assert_eq!(battery_percentage(i64::MAX, 100), None);
    assert_eq!(battery_percentage(i64::MAX / 100, i64::MAX), None);
}

#[test]
fn test_battery_status() {
    // (is_charging, external_connected, fully_charged)
    assert_eq!(battery_status(true, true, false), "Charging");
    assert_eq!(battery_status(false, false, false), "Discharging");
    assert_eq!(battery_status(false, true, true), "Full");
    assert_eq!(battery_status(false, true, false), "AC Connected");
}

#[test]
fn test_get_battery_shape() {
    let (capacity, status) = get_battery();
    if capacity == "Unavailable" {
        // Desktop Macs: both halves must be Unavailable so display_battery hides the line
        assert_eq!(status, "Unavailable");
    } else {
        assert!(capacity.parse::<u64>().is_ok_and(|p| p <= 100), "capacity was {}", capacity);
        assert!(["Charging", "Discharging", "Full", "AC Connected"].contains(&status.as_str()));
    }
}

// CPU and screen tests

#[test]
fn test_cpu_core_types() {
    // Apple Silicon lists one count per performance level, Intel Macs have a single level
    if let Some(counts) = get_cpu_core_types() {
        assert!(counts.len() >= 2);
        assert!(counts.iter().all(|&count| count > 0));
    }
}

#[test]
#[cfg(target_arch = "aarch64")]
fn test_cpu_core_types_apple_silicon() {
    // Every Apple Silicon chip has at least two performance levels
    assert!(get_cpu_core_types().is_some_and(|counts| counts.len() >= 2));
}

#[test]
fn test_filesystem_space_of_root() {
    let (total, available) = get_filesystem_space(c"/").unwrap_or_default();
    assert!(total > 0 && available <= total);
    let missing: &CStr = c"/no/such/directory/for/rustfetch";
    assert_eq!(get_filesystem_space(missing), None);
}

#[test]
fn test_active_screens_shape() {
    // A headless Mac (CI, SSH without a session) has no displays, which must not crash
    for screen in get_active_screens().unwrap_or_default() {
        assert!(screen.width > 0 && screen.height > 0);
        assert!(screen.refresh_rate.is_none_or(|rate| rate > 0));
    }
}

// Local IP tests

// RTA_* bits from <net/route.h>
const RTA_DST: i32 = 0x1;
const RTA_GATEWAY: i32 = 0x2;
const RTA_NETMASK: i32 = 0x4;
const RTA_IFP: i32 = 0x10;
const RTA_IFA: i32 = 0x20;

/// Builds a 20-byte sockaddr_dl for interface `name` like the RTA_IFP entry of a route reply
fn sockaddr_dl(name: &str) -> Vec<u8> {
    // sdl_len, sdl_family (AF_LINK), sdl_index, sdl_type (IFT_ETHER), sdl_nlen, sdl_alen, sdl_slen
    let mut sockaddr = vec![20, 18, 15, 0, 6, name.len() as u8, 6, 0];
    sockaddr.extend_from_slice(name.as_bytes());
    sockaddr.extend_from_slice(&[0x0a, 0x6f, 0xb5, 0x20, 0xc6, 0x02]);
    sockaddr.resize(20, 0);
    sockaddr
}

#[test]
fn test_siocgifaflag_in6_matches_header() {
    // Value of SIOCGIFAFLAG_IN6 printed by a C program including <netinet6/in6_var.h>
    assert_eq!(SIOCGIFAFLAG_IN6, 0xc120_6949);
}

#[test]
fn test_find_route_sockaddr() {
    // Reply to RTM_GET default: destination 0.0.0.0, gateway 172.18.7.1, an empty netmask (still 4
    // bytes), then the outgoing interface and its address
    let mut data = vec![16, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    data.extend_from_slice(&[16, 2, 0, 0, 172, 18, 7, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    data.extend_from_slice(&[0, 0, 0, 0]);
    data.extend_from_slice(&sockaddr_dl("en0"));
    data.extend_from_slice(&[16, 2, 0, 0, 172, 18, 7, 204, 0, 0, 0, 0, 0, 0, 0, 0]);
    let addrs = RTA_DST | RTA_GATEWAY | RTA_NETMASK | RTA_IFP | RTA_IFA;

    let interface = find_route_sockaddr(addrs, &data, RTA_IFP);
    assert_eq!(interface, Some(sockaddr_dl("en0").as_slice()));
    assert_eq!(interface.and_then(interface_name_from_sockaddr_dl), Some(String::from("en0")));

    let gateway = find_route_sockaddr(addrs, &data, RTA_GATEWAY);
    assert_eq!(
        gateway.and_then(|sockaddr| sockaddr.get(4 .. 8)),
        Some([172, 18, 7, 1].as_slice())
    );
}

#[test]
fn test_find_route_sockaddr_rounds_odd_lengths() {
    // A 7-byte netmask (a trimmed /24) is padded to 8 bytes before the next sockaddr
    let mut data = vec![7, 2, 0, 0, 255, 255, 255, 0];
    data.extend_from_slice(&sockaddr_dl("en5"));
    let interface = find_route_sockaddr(RTA_NETMASK | RTA_IFP, &data, RTA_IFP);
    assert_eq!(interface.and_then(interface_name_from_sockaddr_dl), Some(String::from("en5")));
}

#[test]
fn test_find_route_sockaddr_missing_or_truncated() {
    let data = vec![16, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    // RTA_IFP was not returned
    assert_eq!(find_route_sockaddr(RTA_DST, &data, RTA_IFP), None);
    // RTA_IFP claimed but the buffer ends early
    assert_eq!(find_route_sockaddr(RTA_DST | RTA_IFP, &data, RTA_IFP), None);
    assert_eq!(find_route_sockaddr(RTA_DST | RTA_IFP, &data[.. 4], RTA_DST), None);
}

#[test]
fn test_interface_name_from_sockaddr_dl_rejects_bad_input() {
    // Not AF_LINK
    let mut sockaddr = sockaddr_dl("en0");
    sockaddr[1] = 2;
    assert_eq!(interface_name_from_sockaddr_dl(&sockaddr), None);
    // No name, or a name running past the end
    assert_eq!(interface_name_from_sockaddr_dl(&sockaddr_dl("")), None);
    assert_eq!(interface_name_from_sockaddr_dl(&sockaddr_dl("en0")[.. 9]), None);
    assert_eq!(interface_name_from_sockaddr_dl(&[]), None);
}

#[test]
fn test_ipv6_state_from_darwin_flags() {
    // IN6_IFF_AUTOCONF | IN6_IFF_SECURED, what `ifconfig` shows as "autoconf secured"
    assert_eq!(ipv6_state_from_darwin_flags(0x0440), Ipv6AddressState::Stable);
    // IN6_IFF_DYNAMIC (DHCPv6) and no flags at all (static)
    assert_eq!(ipv6_state_from_darwin_flags(0x0100), Ipv6AddressState::Stable);
    assert_eq!(ipv6_state_from_darwin_flags(0), Ipv6AddressState::Stable);
    // IN6_IFF_AUTOCONF | IN6_IFF_TEMPORARY, "autoconf temporary"
    assert_eq!(ipv6_state_from_darwin_flags(0x00c0), Ipv6AddressState::Temporary);
    // Deprecated temporary, tentative, duplicated, detached
    assert_eq!(ipv6_state_from_darwin_flags(0x00d0), Ipv6AddressState::Unusable);
    assert_eq!(ipv6_state_from_darwin_flags(0x0002), Ipv6AddressState::Unusable);
    assert_eq!(ipv6_state_from_darwin_flags(0x0004), Ipv6AddressState::Unusable);
    assert_eq!(ipv6_state_from_darwin_flags(0x0008), Ipv6AddressState::Unusable);
}
