//! Test the pure local IP helpers within src/sysinfo/network.rs

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rustfetch::sysinfo::*;

// prefix_len_from_netmask tests
#[test]
fn test_prefix_len_ipv4() {
    assert_eq!(prefix_len_from_netmask(&[255, 255, 255, 0]), 24);
    assert_eq!(prefix_len_from_netmask(&[255, 255, 240, 0]), 20);
    assert_eq!(prefix_len_from_netmask(&[255, 255, 255, 255]), 32);
    assert_eq!(prefix_len_from_netmask(&[0, 0, 0, 0]), 0);
}

#[test]
fn test_prefix_len_ipv6() {
    let mut mask = [0u8; 16];
    mask[.. 8].fill(0xff);
    assert_eq!(prefix_len_from_netmask(&mask), 64);

    mask[8] = 0x80;
    assert_eq!(prefix_len_from_netmask(&mask), 65);
    assert_eq!(prefix_len_from_netmask(&[0xff; 16]), 128);
}

#[test]
fn test_prefix_len_truncated_netmask() {
    // macOS hands out a /24 netmask with only 3 mask bytes, the missing byte counts as 0
    assert_eq!(prefix_len_from_netmask(&[255, 255, 255]), 24);
    assert_eq!(prefix_len_from_netmask(&[]), 0);
}

// is_global_ipv6 tests
#[test]
fn test_global_ipv6_addresses() {
    let global = [
        "2601:646:8f00:1a2b::1",
        "2001:db8::1",
        "3fff::1",
        "2a00:1450:4001:82a::200e",
    ];
    for address in global {
        let address: Ipv6Addr = address.parse().unwrap_or(Ipv6Addr::UNSPECIFIED);
        assert!(is_global_ipv6(&address), "{} should be global", address);
    }
}

#[test]
fn test_non_global_ipv6_addresses() {
    // Link-local, unique local (Tailscale uses fd7a:115c:a1e0::/48), loopback, unspecified,
    // multicast and IPv4-mapped
    let local = [
        "fe80::1c2b:9bcd:603a:22bf",
        "fd7a:115c:a1e0::33:8a33",
        "::1",
        "::",
        "ff02::1",
    ];
    for address in local.into_iter().chain(["::ffff:192.168.1.1", "4000::1"]) {
        let address: Ipv6Addr = address.parse().unwrap_or(Ipv6Addr::UNSPECIFIED);
        assert!(!is_global_ipv6(&address), "{} should not be global", address);
    }
}

// is_virtual_interface tests
#[test]
fn test_virtual_interfaces() {
    for name in [
        "utun4", "awdl0", "llw0", "bridge0", "anpi1", "ap1", "gif0", "stf0",
    ] {
        assert!(is_virtual_interface(name), "{} should be virtual", name);
    }
    for name in [
        "docker0",
        "veth1a2b",
        "br-3f2a",
        "virbr0",
        "tailscale0",
        "tun0",
        "wg0",
    ] {
        assert!(is_virtual_interface(name), "{} should be virtual", name);
    }
}

#[test]
fn test_physical_interfaces() {
    for name in [
        "en0", "en5", "eth0", "eno1", "enp3s0", "wlan0", "wlP1p1s0", "br0", "bond0",
    ] {
        assert!(!is_virtual_interface(name), "{} should be physical", name);
    }
}

// pick_ipv6_address tests
#[test]
fn test_pick_ipv6_prefers_stable() {
    let candidates = [
        ("temporary", Ipv6AddressState::Temporary),
        ("deprecated", Ipv6AddressState::Unusable),
        ("stable", Ipv6AddressState::Stable),
    ];
    assert_eq!(pick_ipv6_address(candidates), Some("stable"));
}

#[test]
fn test_pick_ipv6_falls_back_to_temporary() {
    let candidates = [
        ("deprecated", Ipv6AddressState::Unusable),
        ("first temporary", Ipv6AddressState::Temporary),
        ("second temporary", Ipv6AddressState::Temporary),
    ];
    assert_eq!(pick_ipv6_address(candidates), Some("first temporary"));
}

#[test]
fn test_pick_ipv6_never_picks_unusable() {
    assert_eq!(pick_ipv6_address([("tentative", Ipv6AddressState::Unusable)]), None);
    assert_eq!(pick_ipv6_address(Vec::<(&str, Ipv6AddressState)>::new()), None);
}

// Linux flag and /proc parsing tests, they're pure so they run on every OS
#[test]
fn test_ipv6_state_from_linux_flags() {
    // IFA_F_PERMANENT (0x80) is a static address
    assert_eq!(ipv6_state_from_linux_flags(0x80), Ipv6AddressState::Stable);
    assert_eq!(ipv6_state_from_linux_flags(0x00), Ipv6AddressState::Stable);
    assert_eq!(ipv6_state_from_linux_flags(0x01), Ipv6AddressState::Temporary);
    // Deprecated wins over temporary
    assert_eq!(ipv6_state_from_linux_flags(0x21), Ipv6AddressState::Unusable);
    assert_eq!(ipv6_state_from_linux_flags(0x40), Ipv6AddressState::Unusable);
    assert_eq!(ipv6_state_from_linux_flags(0x08), Ipv6AddressState::Unusable);
}

// Copied from a Jetson AGX Orin with Wi-Fi, a USB gadget bridge whose link is down and Docker
const PROC_NET_ROUTE: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlP1p1s0\t00000000\t010712AC\t0003\t0\t0\t600\t00000000\t0\t0\t0
l4tbr0\t00000000\t6437A8C0\t0003\t0\t0\t20426\t00000000\t0\t0\t0
l4tbr0\t00000000\t6437A8C0\t0003\t0\t0\t32766\t00000000\t0\t0\t0
docker0\t0000FEA9\t00000000\t0001\t0\t0\t1000\t0000FFFF\t0\t0\t0
";

#[test]
fn test_parse_default_route_ipv4_lowest_metric() {
    assert_eq!(parse_default_route_ipv4(PROC_NET_ROUTE), Some(String::from("wlP1p1s0")));
}

#[test]
fn test_parse_default_route_ipv4_without_gateway() {
    // WireGuard and PPP style "default dev wg0" routes have no gateway but are still the default
    let content = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
wg0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\t0\t0\t0
eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
";
    assert_eq!(parse_default_route_ipv4(content), Some(String::from("wg0")));
}

#[test]
fn test_parse_default_route_ipv4_none() {
    let content = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
";
    assert_eq!(parse_default_route_ipv4(content), None);
    assert_eq!(parse_default_route_ipv4(""), None);
}

/// Builds one /proc/net/ipv6_route line for a route to `destination`/`prefix_len` through
/// `next_hop`, the source is always ::/0 and refcount and use are dummies
fn ipv6_route_line(
    destination: &str,
    prefix_len: &str,
    next_hop: &str,
    metric: &str,
    flags: &str,
    interface: &str,
) -> String {
    format!(
        "{} {} {} 00 {} {} 00000001 00000000 {} {:>8}\n",
        destination,
        prefix_len,
        "0".repeat(32),
        next_hop,
        metric,
        flags,
        interface
    )
}

#[test]
fn test_parse_default_route_ipv6() {
    let any = "0".repeat(32);
    let router = "fe80000000000000021122fffe334455";
    let prefix = "20010db8000000000000000000000000";
    // The kernel's unreachable default route on lo (RTF_REJECT, not RTF_UP) must be skipped, the
    // others are default routes learned from router advertisements on two interfaces
    let content = [
        ipv6_route_line(&any, "00", &any, "ffffffff", "00200200", "lo"),
        ipv6_route_line(&any, "00", router, "00000800", "00450003", "wlan0"),
        ipv6_route_line(&any, "00", router, "00000400", "00450003", "eth0"),
        ipv6_route_line(prefix, "40", &any, "00000100", "00000001", "eth0"),
    ]
    .concat();
    assert_eq!(parse_default_route_ipv6(&content), Some(String::from("eth0")));
}

#[test]
fn test_parse_default_route_ipv6_only_unreachable() {
    // What a Jetson AGX Orin on an IPv4-only network has: the unreachable route and fe80::/64
    let any = "0".repeat(32);
    let link_local = "fe800000000000000000000000000000";
    let content = [
        ipv6_route_line(&any, "00", &any, "ffffffff", "00200200", "lo"),
        ipv6_route_line(link_local, "40", &any, "00000100", "00000001", "wlP1p1s0"),
    ]
    .concat();
    assert_eq!(parse_default_route_ipv6(&content), None);
}

#[test]
fn test_parse_if_inet6_flags() {
    let content = "\
00000000000000000000000000000001 01 80 10 80       lo
20010db8000000000000000000000042 02 40 00 00     eth0
20010db800000000a1b2c3d4e5f60718 02 40 00 01     eth0
fe80000000000000f1d98474094418ba 02 40 20 80     eth0
";
    let stable = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x42);
    let temporary = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0xa1b2, 0xc3d4, 0xe5f6, 0x0718);

    assert_eq!(parse_if_inet6_flags(content, "eth0", &stable), Some(0x00));
    assert_eq!(parse_if_inet6_flags(content, "eth0", &temporary), Some(0x01));
    // Same address on another interface, or an address that isn't there
    assert_eq!(parse_if_inet6_flags(content, "wlan0", &stable), None);
    assert_eq!(parse_if_inet6_flags(content, "eth0", &Ipv6Addr::LOCALHOST), None);
}

// select_local_ipv4, select_local_ipv6 and select_tailscale_ipv4 tests

/// Builds one getifaddrs entry
fn entry(interface: &str, address: &str, prefix_len: u32) -> InterfaceAddress {
    InterfaceAddress {
        interface: interface.to_string(),
        address: address.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        prefix_len,
    }
}

/// A dual-stack Mac on Wi-Fi with Tailscale up: en0 with a stable and a temporary global address,
/// Tailscale's utun4 and an iCloud Private Relay style utun0 with only a link-local address
fn dual_stack_with_tailscale() -> Vec<InterfaceAddress> {
    vec![
        entry("en0", "fe80::1c2b:9bcd:603a:22bf", 64),
        entry("en0", "172.18.7.204", 24),
        entry("en0", "2601:646:8f00:1a2b:10ab:cdff:fe01:2345", 64),
        entry("en0", "2601:646:8f00:1a2b:9d3e:1f2a:77c1:aa01", 64),
        entry("utun0", "fe80::ce81:b1c:bd2c:69e", 64),
        entry("utun4", "fe80::3082:7bc1:f9e4:6aec", 64),
        entry("utun4", "100.81.138.51", 32),
        entry("utun4", "fd7a:115c:a1e0::33:8a33", 48),
    ]
}

/// Marks the second en0 global address as a temporary (privacy) address
fn en0_address_state(_interface: &str, address: &Ipv6Addr) -> Ipv6AddressState {
    if address.segments()[4] == 0x9d3e {
        Ipv6AddressState::Temporary
    } else {
        Ipv6AddressState::Stable
    }
}

#[test]
fn test_select_addresses_default_route_on_wifi() {
    let addresses = dual_stack_with_tailscale();
    let ipv4 = select_local_ipv4(&addresses, Some("en0")).map(InterfaceAddress::to_pair);
    assert_eq!(ipv4, Some((String::from("en0"), String::from("172.18.7.204/24"))));

    let routes = [Some("en0"), Some("en0")];
    let ipv6 = select_local_ipv6(&addresses, routes, en0_address_state);
    assert_eq!(
        ipv6.map(InterfaceAddress::to_pair),
        Some((String::from("en0"), String::from("2601:646:8f00:1a2b:10ab:cdff:fe01:2345/64")))
    );
    assert_eq!(select_tailscale_ipv4(&addresses), Some(Ipv4Addr::new(100, 81, 138, 51)));
}

#[test]
fn test_select_addresses_with_tailscale_exit_node() {
    // An exit node moves both default routes to utun4, which only has Tailscale's addresses, so
    // both lines fall back to en0 instead of disappearing
    let addresses = dual_stack_with_tailscale();
    let ipv4 = select_local_ipv4(&addresses, Some("utun4")).map(InterfaceAddress::to_pair);
    assert_eq!(ipv4, Some((String::from("en0"), String::from("172.18.7.204/24"))));

    let ipv6 = select_local_ipv6(&addresses, [Some("utun4"), Some("utun4")], en0_address_state);
    assert_eq!(ipv6.map(|entry| entry.interface.as_str()), Some("en0"));
    assert_eq!(select_tailscale_ipv4(&addresses), Some(Ipv4Addr::new(100, 81, 138, 51)));
}

#[test]
fn test_select_ipv6_route_interface_not_up() {
    // A default route through an interface getifaddrs didn't list (down), then no route at all
    let addresses = dual_stack_with_tailscale();
    let ipv6 = select_local_ipv6(&addresses, [Some("en7"), None], en0_address_state);
    assert_eq!(ipv6.map(|entry| entry.interface.as_str()), Some("en0"));
    let ipv6 = select_local_ipv6(&addresses, [None, None], en0_address_state);
    assert_eq!(ipv6.map(|entry| entry.interface.as_str()), Some("en0"));
}

#[test]
fn test_select_ipv6_skips_unusable_and_falls_back_to_temporary() {
    let addresses = dual_stack_with_tailscale();
    let stable_deprecated = |_: &str, address: &Ipv6Addr| match en0_address_state("en0", address) {
        Ipv6AddressState::Stable => Ipv6AddressState::Unusable,
        state => state,
    };
    let ipv6 = select_local_ipv6(&addresses, [Some("en0"), None], stable_deprecated);
    assert_eq!(
        ipv6.map(|entry| entry.address.to_string()),
        Some(String::from("2601:646:8f00:1a2b:9d3e:1f2a:77c1:aa01"))
    );

    let all_unusable = |_: &str, _: &Ipv6Addr| Ipv6AddressState::Unusable;
    assert_eq!(select_local_ipv6(&addresses, [Some("en0"), None], all_unusable), None);
}

#[test]
fn test_select_addresses_ipv4_only() {
    let addresses = vec![
        entry("en0", "fe80::1", 64),
        entry("en0", "192.168.1.20", 24),
    ];
    assert_eq!(select_local_ipv6(&addresses, [None, Some("en0")], en0_address_state), None);
    assert_eq!(select_tailscale_ipv4(&addresses), None);
    // no default route at all: the first physical interface
    let ipv4 = select_local_ipv4(&addresses, None).map(|entry| entry.address.to_string());
    assert_eq!(ipv4, Some(String::from("192.168.1.20")));
}

#[test]
fn test_select_addresses_with_cgnat_vpn() {
    // Cloudflare WARP as the default route: its 100.96.0.0/12 address is not Tailscale's, so there
    // is no Tailscale line and the Local IPv4 line shows the tunnel carrying the traffic
    let addresses = vec![
        entry("en0", "172.18.7.204", 24),
        entry("utun5", "100.96.0.7", 32),
        entry("utun5", "2606:4700:110:8a2c::7", 128),
    ];
    assert_eq!(select_tailscale_ipv4(&addresses), None);
    let ipv4 = select_local_ipv4(&addresses, Some("utun5")).map(|entry| entry.address.to_string());
    assert_eq!(ipv4, Some(String::from("100.96.0.7")));
    // without the default route it isn't picked, it is a virtual interface
    let ipv4 = select_local_ipv4(&addresses, None).map(|entry| entry.interface.as_str());
    assert_eq!(ipv4, Some("en0"));
}

#[test]
fn test_select_tailscale_on_linux() {
    // tailscale0 is recognized by its name, even with Tailscale's IPv6 turned off
    let addresses = vec![
        entry("wlP1p1s0", "172.18.7.246", 24),
        entry("tailscale0", "100.72.94.55", 32),
    ];
    assert_eq!(select_tailscale_ipv4(&addresses), Some(Ipv4Addr::new(100, 72, 94, 55)));
    let ipv4 = select_local_ipv4(&addresses, Some("tailscale0"));
    assert_eq!(ipv4.map(|entry| entry.interface.as_str()), Some("wlP1p1s0"));
}
