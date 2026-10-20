//! Test the Tailscale helpers in src/sysinfo/tailscale.rs: address checks, the PTR query builder
//! and the DNS reply parser

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rustfetch::sysinfo::*;

const TAILSCALE_IP: Ipv4Addr = Ipv4Addr::new(100, 81, 138, 51);
const HOST: &str = "johns-m5-macbook-pro.heron-minor.ts.net";

/// Encodes a dotted name as DNS labels ending with the empty label
fn encode_name(name: &str) -> Vec<u8> {
    let mut encoded = Vec::new();
    for label in name.split('.') {
        encoded.push(label.len() as u8);
        encoded.extend_from_slice(label.as_bytes());
    }
    encoded.push(0);
    encoded
}

/// Builds the reply MagicDNS sends to build_ptr_query(id, TAILSCALE_IP): the question copied back
/// and one PTR answer whose owner name points to the question name (offset 12)
fn ptr_reply(id: u16, flags: u16, answer_type: u16, answer_data: &[u8]) -> Vec<u8> {
    let mut reply = build_ptr_query(id, TAILSCALE_IP);
    reply[2 .. 4].copy_from_slice(&flags.to_be_bytes());
    // one answer
    reply[6 .. 8].copy_from_slice(&1u16.to_be_bytes());

    reply.extend_from_slice(&[0xc0, 0x0c]);
    reply.extend_from_slice(&answer_type.to_be_bytes());
    reply.extend_from_slice(&1u16.to_be_bytes());
    reply.extend_from_slice(&600u32.to_be_bytes());
    reply.extend_from_slice(&(answer_data.len() as u16).to_be_bytes());
    reply.extend_from_slice(answer_data);
    reply
}

// is_tailscale_ipv4, is_tailscale_ipv6 and is_tailscale_tunnel tests

#[test]
fn test_tailscale_ipv4_range() {
    // 100.64.0.0/10 is 100.64.0.0 to 100.127.255.255
    for address in [
        TAILSCALE_IP,
        Ipv4Addr::new(100, 64, 0, 1),
        Ipv4Addr::new(100, 127, 255, 254),
    ] {
        assert!(is_tailscale_ipv4(&address), "{} should be a Tailscale address", address);
    }
    for address in [
        Ipv4Addr::new(100, 63, 255, 255),
        Ipv4Addr::new(100, 128, 0, 1),
        Ipv4Addr::new(172, 18, 7, 204),
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::LOCALHOST,
    ] {
        assert!(!is_tailscale_ipv4(&address), "{} should not be a Tailscale address", address);
    }
}

#[test]
fn test_tailscale_ipv6_range() {
    // Tailscale's fixed unique local prefix, fd7a:115c:a1e0::/48
    assert!(is_tailscale_ipv6(&Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e0, 0, 0, 0, 0x33, 0x8a33)));
    assert!(is_tailscale_ipv6(&Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e0, 0xffff, 1, 2, 3, 4)));
    assert!(!is_tailscale_ipv6(&Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e1, 0, 0, 0, 0, 1)));
    assert!(!is_tailscale_ipv6(&Ipv6Addr::new(
        0xfe80, 0, 0, 0, 0x3082, 0x7bc1, 0xf9e4, 0x6aec
    )));
    assert!(!is_tailscale_ipv6(&Ipv6Addr::LOCALHOST));
}

/// The addresses Tailscale puts on its macOS utun: the 100.x address, the fd7a one and link-local
fn tailscale_utun_addresses() -> Vec<IpAddr> {
    vec![
        IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0x3082, 0x7bc1, 0xf9e4, 0x6aec)),
        IpAddr::V4(TAILSCALE_IP),
        IpAddr::V6(Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e0, 0, 0, 0, 0x33, 0x8a33)),
    ]
}

#[test]
fn test_tailscale_tunnel_utun_needs_tailscale_ipv6() {
    assert!(is_tailscale_tunnel("utun4", tailscale_utun_addresses().iter()));

    // Cloudflare WARP and NordVPN Meshnet also put 100.64.0.0/10 addresses on a utun
    let warp = [
        IpAddr::V4(Ipv4Addr::new(100, 96, 0, 7)),
        IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x110, 0, 0, 0, 0, 7)),
    ];
    assert!(!is_tailscale_tunnel("utun5", warp.iter()));
    assert!(!is_tailscale_tunnel("utun3", [IpAddr::V4(Ipv4Addr::new(100, 64, 1, 2))].iter()));
}

#[test]
fn test_tailscale_tunnel_names() {
    // Linux's tailscale0 is recognized by name alone, it may have IPv6 turned off
    assert!(is_tailscale_tunnel("tailscale0", [IpAddr::V4(TAILSCALE_IP)].iter()));
    assert!(is_tailscale_tunnel("tailscale0", [].iter()));
    // Tailscale's addresses on an interface that can't be its tunnel
    for name in ["en0", "eth0", "wg0", ""] {
        assert!(!is_tailscale_tunnel(name, tailscale_utun_addresses().iter()), "{}", name);
    }
}

#[test]
fn test_get_tailscale_ipv4_is_in_range() {
    // Depends on the machine running the tests, it only has to be a valid Tailscale address
    if let Some(address) = get_tailscale_ipv4() {
        assert!(is_tailscale_ipv4(&address));
    }
}

// build_ptr_query tests

#[test]
fn test_ptr_query_name() {
    assert_eq!(get_ptr_query_name(TAILSCALE_IP), "51.138.81.100.in-addr.arpa");
    assert_eq!(get_ptr_query_name(Ipv4Addr::new(100, 64, 0, 1)), "1.0.64.100.in-addr.arpa");
}

#[test]
fn test_build_ptr_query() {
    let query = build_ptr_query(0xbeef, TAILSCALE_IP);

    let mut expected = vec![
        0xbe, 0xef, // ID
        0x01, 0x00, // flags: standard query, recursion desired
        0, 1, // one question
        0, 0, 0, 0, 0, 0, // no answer, authority or additional records
    ];
    expected.extend_from_slice(b"\x0251\x03138\x0281\x03100\x07in-addr\x04arpa\x00");
    expected.extend_from_slice(&[0, 12, 0, 1]); // QTYPE PTR, QCLASS IN

    assert_eq!(query, expected);
}

// parse_ptr_response tests

#[test]
fn test_parse_ptr_response() {
    let reply = ptr_reply(0x1234, 0x8180, 12, &encode_name(HOST));
    assert_eq!(parse_ptr_response(&reply, 0x1234), Some(String::from(HOST)));
}

#[test]
fn test_parse_ptr_response_compressed_answer() {
    // The answer data is "laptop" followed by a pointer to "in-addr.arpa" inside the question,
    // which starts after the 12 byte header and the "51", "138", "81" and "100" labels
    let in_addr_offset = 12 + 3 + 4 + 3 + 4;
    let mut data = b"\x06laptop".to_vec();
    data.extend_from_slice(&[0xc0, in_addr_offset]);

    let reply = ptr_reply(7, 0x8180, 12, &data);
    assert_eq!(parse_ptr_response(&reply, 7), Some(String::from("laptop.in-addr.arpa")));
}

#[test]
fn test_parse_ptr_response_rejects_other_replies() {
    let name = encode_name(HOST);
    // a reply to another query
    assert_eq!(parse_ptr_response(&ptr_reply(1, 0x8180, 12, &name), 2), None);
    // NXDOMAIN (RCODE 3)
    assert_eq!(parse_ptr_response(&ptr_reply(1, 0x8183, 12, &name), 1), None);
    // a query, not a response
    assert_eq!(parse_ptr_response(&ptr_reply(1, 0x0100, 12, &name), 1), None);
    // a CNAME answer instead of a PTR
    assert_eq!(parse_ptr_response(&ptr_reply(1, 0x8180, 5, &name), 1), None);
    // no answer at all, which is our own query sent back
    assert_eq!(parse_ptr_response(&build_ptr_query(1, TAILSCALE_IP), 1), None);
    // an empty name
    assert_eq!(parse_ptr_response(&ptr_reply(1, 0x8180, 12, &[0]), 1), None);
}

#[test]
fn test_parse_ptr_response_truncated() {
    // Cutting the reply anywhere must give None, never a panic or a partial name
    let reply = ptr_reply(9, 0x8180, 12, &encode_name(HOST));
    for length in 0 .. reply.len() {
        assert_eq!(parse_ptr_response(&reply[.. length], 9), None, "cut at {}", length);
    }
}

#[test]
fn test_parse_ptr_response_data_length_past_the_end() {
    let mut reply = ptr_reply(9, 0x8180, 12, &encode_name(HOST));
    let data_length_offset = reply.len() - encode_name(HOST).len() - 2;
    reply[data_length_offset .. data_length_offset + 2].copy_from_slice(&0xffffu16.to_be_bytes());
    assert_eq!(parse_ptr_response(&reply, 9), None);
}

#[test]
fn test_parse_ptr_response_rejects_escape_sequences() {
    // A forged reply must not be able to print terminal escape sequences
    let reply = ptr_reply(3, 0x8180, 12, b"\x05\x1b[31m\x02ts\x00");
    assert_eq!(parse_ptr_response(&reply, 3), None);
}

// read_dns_name tests

#[test]
fn test_read_dns_name() {
    let message = encode_name("host.example.ts.net");
    assert_eq!(read_dns_name(&message, 0), Some((String::from("host.example.ts.net"), 21)));
}

#[test]
fn test_read_dns_name_pointer() {
    // "ts.net" at 0, then "host" followed by a pointer back to it
    let mut message = encode_name("ts.net");
    message.extend_from_slice(b"\x04host\xc0\x00");
    // the name ends right after the pointer, not where the pointer leads
    assert_eq!(read_dns_name(&message, 8), Some((String::from("host.ts.net"), 15)));
}

#[test]
fn test_read_dns_name_pointer_loop() {
    // a pointer to itself, and two pointers pointing at each other
    assert_eq!(read_dns_name(&[0xc0, 0x00], 0), None);
    assert_eq!(read_dns_name(&[0xc0, 0x02, 0xc0, 0x00], 0), None);
    // a label then a pointer back to the start of the name
    assert_eq!(read_dns_name(b"\x01a\xc0\x00", 0), None);
}

#[test]
fn test_read_dns_name_truncated() {
    assert_eq!(read_dns_name(&[], 0), None);
    // a label longer than what is left
    assert_eq!(read_dns_name(b"\x05ab", 0), None);
    // no terminating empty label
    assert_eq!(read_dns_name(b"\x02ts", 0), None);
    // a pointer missing its second byte, and one pointing past the end
    assert_eq!(read_dns_name(&[0xc0], 0), None);
    assert_eq!(read_dns_name(&[0xc0, 0x40], 0), None);
    // an offset past the end
    assert_eq!(read_dns_name(&[0], 5), None);
}

#[test]
fn test_read_dns_name_reserved_label_types() {
    // 0b01 and 0b10 prefixed length bytes are not labels or pointers
    assert_eq!(read_dns_name(&[0x40, 0x00], 0), None);
    assert_eq!(read_dns_name(&[0x80, 0x00], 0), None);
}

#[test]
fn test_read_dns_name_too_long() {
    // five 63 byte labels are over the 255 byte limit of a name
    let label = "a".repeat(63);
    let name = [label.as_str(); 5].join(".");
    assert_eq!(read_dns_name(&encode_name(&name), 0), None);

    let name = [label.as_str(); 3].join(".");
    assert_eq!(read_dns_name(&encode_name(&name), 0).map(|(name, _)| name.len()), Some(191));
}

// get_tailscale_machine_name tests

#[test]
fn test_tailscale_machine_name_from_fqdn() {
    assert_eq!(
        get_tailscale_machine_name("johns-m5-macbook-pro.heron-minor.ts.net"),
        Some("johns-m5-macbook-pro")
    );
}

#[test]
fn test_tailscale_machine_name_trailing_dot() {
    assert_eq!(get_tailscale_machine_name("laptop.tailnet.ts.net."), Some("laptop"));
    assert_eq!(get_tailscale_machine_name("laptop."), Some("laptop"));
}

#[test]
fn test_tailscale_machine_name_single_label() {
    assert_eq!(get_tailscale_machine_name("laptop"), Some("laptop"));
}

#[test]
fn test_tailscale_machine_name_empty() {
    assert_eq!(get_tailscale_machine_name(""), None);
    assert_eq!(get_tailscale_machine_name("."), None);
    assert_eq!(get_tailscale_machine_name(".tailnet.ts.net"), None);
}
