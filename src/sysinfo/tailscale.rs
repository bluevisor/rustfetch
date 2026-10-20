//! Tailscale address detection and the MagicDNS reverse lookup of this machine's tailnet name

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, UdpSocket},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::common::read_array;

/// Tailscale's MagicDNS resolver, reachable from every node of the tailnet. Asking it directly
/// takes ~0.5 ms where `tailscale status --json` takes ~30 ms, and its LocalAPI needs a token
const MAGIC_DNS: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(100, 100, 100, 100), 53);
/// MagicDNS answers from tailscaled on this machine in ~0.2-0.5 ms, so 20 ms is plenty of headroom
/// and bounds the wait when it isn't serving
const MAGIC_DNS_TIMEOUT: Duration = Duration::from_millis(20);
/// A plain UDP DNS reply (no EDNS) is at most 512 bytes
const DNS_UDP_SIZE: usize = 512;
/// DNS header: ID, flags, then the question, answer, authority and additional record counts
const DNS_HEADER_SIZE: usize = 12;
/// Standard query with the RD (recursion desired) bit set
const DNS_FLAGS_QUERY: u16 = 0x0100;
/// QR bit, set on responses
const DNS_FLAG_RESPONSE: u16 = 0x8000;
/// RCODE bits, 0 means no error
const DNS_RCODE_MASK: u16 = 0x000f;
const DNS_TYPE_PTR: u16 = 12;
const DNS_CLASS_IN: u16 = 1;
/// Compression pointers followed in one name before giving up, real names need one or two and a
/// malformed reply could otherwise point in a loop
const MAX_POINTER_HOPS: usize = 16;
/// Longest a domain name can be on the wire (RFC 1035)
const MAX_NAME_LENGTH: usize = 255;

/// Checks if an IPv4 address is in 100.64.0.0/10, the CGNAT range Tailscale hands out
pub fn is_tailscale_ipv4(address: &Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 100 && second & 0xc0 == 64
}

/// Checks if an IPv6 address is in fd7a:115c:a1e0::/48, the unique local range Tailscale gives
/// every node next to its 100.x address
pub fn is_tailscale_ipv6(address: &Ipv6Addr) -> bool {
    let [first, second, third, ..] = address.segments();
    (first, second, third) == (0xfd7a, 0x115c, 0xa1e0)
}

/// Checks if an interface is Tailscale's tunnel, given the interface's addresses. Linux names it
/// tailscale0, macOS gives it a numbered utun like every other VPN, and some of those (Cloudflare
/// WARP, NordVPN Meshnet) also use 100.64.0.0/10, so a utun must also carry Tailscale's IPv6 range
pub fn is_tailscale_tunnel<'a>(
    interface: &str,
    mut addresses: impl Iterator<Item = &'a IpAddr>,
) -> bool {
    let has_tailscale_ipv6 = |address: &IpAddr| match address {
        IpAddr::V6(address) => is_tailscale_ipv6(address),
        IpAddr::V4(_) => false,
    };
    interface.starts_with("tailscale")
        || (interface.starts_with("utun") && addresses.any(has_tailscale_ipv6))
}

/// Gets this machine's MagicDNS name (e.g. "laptop.tailnet.ts.net") by asking MagicDNS for the
/// PTR record of its Tailscale address. Returns None on any failure, including a timeout
pub fn get_tailscale_host(address: Ipv4Addr) -> Option<String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.set_read_timeout(Some(MAGIC_DNS_TIMEOUT)).ok()?;
    // A connected UDP socket only receives datagrams sent back by MagicDNS
    socket.connect(MAGIC_DNS).ok()?;

    let id = get_query_id();
    socket.send(&build_ptr_query(id, address)).ok()?;

    let mut buffer = [0u8; DNS_UDP_SIZE];
    let length = socket.recv(&mut buffer).ok()?;
    parse_ptr_response(buffer.get(.. length)?, id)
}

/// Picks a DNS query ID from the pid and the clock, so a late reply to an earlier run is ignored
fn get_query_id() -> u16 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.subsec_nanos())
        .unwrap_or(0);
    // only the low 16 bits fit in the ID field
    (std::process::id() ^ nanos) as u16
}

/// Builds the reverse lookup name of an IPv4 address, e.g. 100.81.138.51 gives
/// "51.138.81.100.in-addr.arpa"
pub fn get_ptr_query_name(address: Ipv4Addr) -> String {
    let [a, b, c, d] = address.octets();
    format!("{}.{}.{}.{}.in-addr.arpa", d, c, b, a)
}

/// Builds a DNS query packet asking for the PTR record of an IPv4 address
pub fn build_ptr_query(id: u16, address: Ipv4Addr) -> Vec<u8> {
    let mut packet = Vec::with_capacity(48);
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&DNS_FLAGS_QUERY.to_be_bytes());
    // one question, no answer, authority or additional records
    packet.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);

    // The name is a list of length-prefixed labels ending with an empty one, every label here is
    // a number or "in-addr"/"arpa" so they are all well under the 63 byte limit
    for label in get_ptr_query_name(address).split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);

    packet.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
    packet.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
    packet
}

/// Parses the reply to the query built by build_ptr_query and returns the name of its first PTR
/// answer, without the trailing dot. Returns None for a reply to another query, an error reply,
/// a reply without a PTR answer or a malformed packet
pub fn parse_ptr_response(message: &[u8], id: u16) -> Option<String> {
    let read_u16 = |offset: usize| read_array(message, offset).map(u16::from_be_bytes);

    let flags = read_u16(2)?;
    if read_u16(0)? != id || flags & DNS_FLAG_RESPONSE == 0 || flags & DNS_RCODE_MASK != 0 {
        return None;
    }
    let question_count = read_u16(4)?;
    let answer_count = read_u16(6)?;

    // Each question is a name followed by its type and class
    let mut offset = DNS_HEADER_SIZE;
    for _ in 0 .. question_count {
        offset = read_dns_name(message, offset)?.1.checked_add(4)?;
    }

    // Each answer is a name, type, class, TTL (4 bytes), data length and the data itself
    for _ in 0 .. answer_count {
        offset = read_dns_name(message, offset)?.1;
        let record_type = read_u16(offset)?;
        let class = read_u16(offset.checked_add(2)?)?;
        let data_length = usize::from(read_u16(offset.checked_add(8)?)?);
        let data_start = offset.checked_add(10)?;
        let data_end = data_start.checked_add(data_length)?;
        if data_end > message.len() {
            return None;
        }

        if record_type == DNS_TYPE_PTR && class == DNS_CLASS_IN {
            let (name, _) = read_dns_name(message, data_start)?;
            return (!name.is_empty()).then_some(name);
        }
        offset = data_end;
    }
    None
}

/// Reads a domain name at offset, following compression pointers (a 0b11 prefixed offset to a
/// name earlier in the message). Returns the dotted name and where the name ends at offset, the
/// end is right after the first pointer when there is one
///
/// Only hostname characters (letters, digits, '-' and '_') are accepted, which also keeps terminal
/// escape sequences in a forged reply out of the output
pub fn read_dns_name(message: &[u8], offset: usize) -> Option<(String, usize)> {
    let mut labels: Vec<&str> = Vec::new();
    let mut name_length = 0;
    let mut position = offset;
    let mut end = None;
    let mut hops = 0;

    loop {
        let length_byte = *message.get(position)?;
        match length_byte & 0xc0 {
            // the empty label ends the name
            0x00 if length_byte == 0 => break,
            0x00 => {
                let start = position + 1;
                let label = message.get(start .. start + usize::from(length_byte))?;
                if !label.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                    return None;
                }

                name_length += label.len() + 1;
                if name_length > MAX_NAME_LENGTH {
                    return None;
                }
                labels.push(std::str::from_utf8(label).ok()?);
                position = start + label.len();
            },
            0xc0 => {
                hops += 1;
                if hops > MAX_POINTER_HOPS {
                    return None;
                }
                let low_byte = *message.get(position + 1)?;
                end.get_or_insert(position + 2);
                position = usize::from(length_byte & 0x3f) << 8 | usize::from(low_byte);
            },
            // 0b01 and 0b10 prefixes are reserved label types
            _ => return None,
        }
    }

    Some((labels.join("."), end.unwrap_or(position + 1)))
}
