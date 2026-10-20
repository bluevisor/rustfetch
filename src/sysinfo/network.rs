//! Local and Tailscale IP addresses through getifaddrs(3), shared by macOS and Linux, and the pure
//! helpers that pick which interface and address to show

use std::{
    ffi::CStr,
    mem::offset_of,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::OnceLock,
};

use super::{is_tailscale_ipv4, is_tailscale_tunnel};
use crate::platform;

/// One address of a network interface, copied out of the getifaddrs list
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceAddress {
    pub interface: String,
    pub address: IpAddr,
    pub prefix_len: u32,
}

impl InterfaceAddress {
    /// Returns the (interface, "address/prefix") pair the display functions print
    pub fn to_pair(&self) -> (String, String) {
        (self.interface.clone(), format!("{}/{}", self.address, self.prefix_len))
    }

    /// Checks if this is a global unicast IPv6 address (2000::/3)
    fn is_global_ipv6(&self) -> bool {
        match self.address {
            IpAddr::V6(address) => is_global_ipv6(&address),
            IpAddr::V4(_) => false,
        }
    }
}

/// Gets the IPv4 address of the primary interface as (interface, "address/prefix"), for example
/// ("en0", "192.168.1.20/24"), see select_local_ipv4
pub fn get_local_ipv4() -> Option<(String, String)> {
    select_local_ipv4(get_interface_addresses(), get_default_route_ipv4())
        .map(InterfaceAddress::to_pair)
}

/// Picks the address the Local IPv4 line shows: the one on the interface carrying the default
/// route, or when there is none (or it only has Tailscale's address, as with an exit node) the
/// first physical interface that has an IPv4 address
pub fn select_local_ipv4<'a>(
    addresses: &'a [InterfaceAddress],
    route_interface: Option<&str>,
) -> Option<&'a InterfaceAddress> {
    // Tailscale has its own line
    let is_local_ipv4 = |entry: &&InterfaceAddress| {
        entry.address.is_ipv4() && !is_tailscale_entry(addresses, entry)
    };

    addresses
        .iter()
        .filter(is_local_ipv4)
        .find(|entry| route_interface == Some(entry.interface.as_str()))
        .or_else(|| {
            addresses
                .iter()
                .filter(is_local_ipv4)
                .find(|entry| !is_virtual_interface(&entry.interface))
        })
}

/// Gets this machine's Tailscale IPv4 address, None when Tailscale isn't connected. Comes from the
/// same getifaddrs snapshot as the local addresses
pub fn get_tailscale_ipv4() -> Option<Ipv4Addr> {
    select_tailscale_ipv4(get_interface_addresses())
}

/// Picks Tailscale's IPv4 address out of `addresses`, see is_tailscale_entry
pub fn select_tailscale_ipv4(addresses: &[InterfaceAddress]) -> Option<Ipv4Addr> {
    addresses
        .iter()
        .filter(|entry| is_tailscale_entry(addresses, entry))
        .find_map(|entry| match entry.address {
            IpAddr::V4(address) => Some(address),
            IpAddr::V6(_) => None,
        })
}

/// Checks if an entry is Tailscale's IPv4 address: in 100.64.0.0/10 on Tailscale's tunnel, which is
/// recognized from the interface's other addresses (see is_tailscale_tunnel)
fn is_tailscale_entry(addresses: &[InterfaceAddress], entry: &InterfaceAddress) -> bool {
    let IpAddr::V4(address) = entry.address else {
        return false;
    };
    let interface_addresses = addresses
        .iter()
        .filter(|other| other.interface == entry.interface)
        .map(|other| &other.address);
    is_tailscale_ipv4(&address) && is_tailscale_tunnel(&entry.interface, interface_addresses)
}

/// Gets the global IPv6 address of the primary interface as (interface, "address/prefix"), see
/// select_local_ipv6. None on IPv4-only networks
pub fn get_local_ipv6() -> Option<(String, String)> {
    let addresses = get_interface_addresses();
    // IPv4-only networks have nothing to show, so don't bother asking the routing table
    if !addresses.iter().any(InterfaceAddress::is_global_ipv6) {
        return None;
    }

    let route_interfaces = [get_default_route_ipv6(), get_default_route_ipv4()];
    select_local_ipv6(addresses, route_interfaces, platform::get_ipv6_address_state)
        .map(InterfaceAddress::to_pair)
}

/// Picks the address the Local IPv6 line shows: a global address of the interface carrying the
/// IPv6 default route, else the IPv4 one, else the first physical interface with one.
/// `address_state` gives the kernel's flags of an address (see pick_ipv6_address)
///
/// An interface without a usable global address is skipped rather than ending the search: a tunnel
/// carrying the default route (a Tailscale exit node, a full VPN) often only has unique local
/// addresses while the physical interface still has a global one, the same case the IPv4 line
/// falls back for
pub fn select_local_ipv6<'a>(
    addresses: &'a [InterfaceAddress],
    route_interfaces: [Option<&str>; 2],
    address_state: impl Fn(&str, &Ipv6Addr) -> Ipv6AddressState,
) -> Option<&'a InterfaceAddress> {
    let physical_interfaces = addresses
        .iter()
        .filter(|entry| entry.is_global_ipv6() && !is_virtual_interface(&entry.interface))
        .map(|entry| entry.interface.as_str());

    route_interfaces
        .into_iter()
        .flatten()
        .chain(physical_interfaces)
        .find_map(|interface| pick_interface_ipv6(addresses, interface, &address_state))
}

/// Picks which global IPv6 address of `interface` to show (see pick_ipv6_address), None when it
/// has no usable one or isn't up
fn pick_interface_ipv6<'a>(
    addresses: &'a [InterfaceAddress],
    interface: &str,
    address_state: &impl Fn(&str, &Ipv6Addr) -> Ipv6AddressState,
) -> Option<&'a InterfaceAddress> {
    // The iterator is lazy, so the kernel flags are only looked up until a stable address is found
    let candidates = addresses.iter().filter(|entry| entry.interface == interface).filter_map(
        |entry| match entry.address {
            IpAddr::V6(address) if is_global_ipv6(&address) => {
                Some((entry, address_state(interface, &address)))
            },
            _ => None,
        },
    );
    pick_ipv6_address(candidates)
}

/// Gets the interface carrying the IPv4 default route, looked up once per process since the IPv4
/// and IPv6 lines both need it
fn get_default_route_ipv4() -> Option<&'static str> {
    static INTERFACE: OnceLock<Option<String>> = OnceLock::new();
    INTERFACE.get_or_init(platform::get_default_interface_ipv4).as_deref()
}

/// Gets the interface carrying the IPv6 default route, looked up once per process
fn get_default_route_ipv6() -> Option<&'static str> {
    static INTERFACE: OnceLock<Option<String>> = OnceLock::new();
    INTERFACE.get_or_init(platform::get_default_interface_ipv6).as_deref()
}

/// Gets the addresses of every interface that is up and running, read once per process since
/// getifaddrs is the expensive part (~150 us with musl) and the IPv4, IPv6 and Tailscale lines
/// all need it
fn get_interface_addresses() -> &'static [InterfaceAddress] {
    static ADDRESSES: OnceLock<Vec<InterfaceAddress>> = OnceLock::new();
    ADDRESSES.get_or_init(read_interface_addresses)
}

/// Lists the IPv4 and IPv6 addresses of every interface that is up and running, loopback
/// excluded, in the order the kernel reports them
fn read_interface_addresses() -> Vec<InterfaceAddress> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs only writes the list head, the list is released below with freeifaddrs
    if unsafe { libc::getifaddrs(&raw mut list) } != 0 {
        return Vec::new();
    }

    let wanted_flags = (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_uint;
    let mut addresses = Vec::new();
    let mut current = list;
    while !current.is_null() {
        // SAFETY: current is a non-null node of the list getifaddrs returned, which stays valid
        // until freeifaddrs
        let entry = unsafe { &*current };
        current = entry.ifa_next;

        let is_loopback = entry.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint != 0;
        if entry.ifa_flags & wanted_flags != wanted_flags || is_loopback {
            continue;
        }
        if entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
            continue;
        }

        // SAFETY: ifa_addr is non-null and every sockaddr starts with its family
        let family = libc::c_int::from(unsafe { (*entry.ifa_addr).sa_family });
        let address = match family {
            libc::AF_INET => {
                // SAFETY: AF_INET entries point to a full sockaddr_in, read_unaligned copies it
                // out without alignment requirements
                let sockaddr =
                    unsafe { entry.ifa_addr.cast::<libc::sockaddr_in>().read_unaligned() };
                IpAddr::V4(Ipv4Addr::from(u32::from_be(sockaddr.sin_addr.s_addr)))
            },
            libc::AF_INET6 => {
                // SAFETY: AF_INET6 entries point to a full sockaddr_in6, read_unaligned copies it
                // out without alignment requirements
                let sockaddr =
                    unsafe { entry.ifa_addr.cast::<libc::sockaddr_in6>().read_unaligned() };
                IpAddr::V6(Ipv6Addr::from(sockaddr.sin6_addr.s6_addr))
            },
            _ => continue,
        };

        // SAFETY: ifa_name is a non-null NUL terminated string owned by the list
        let interface = unsafe { CStr::from_ptr(entry.ifa_name) }.to_string_lossy().into_owned();
        let prefix_len = get_netmask_prefix_len(entry.ifa_netmask, family);
        addresses.push(InterfaceAddress {
            interface,
            address,
            prefix_len,
        });
    }

    // SAFETY: list came from a successful getifaddrs and nothing borrowed from it outlives this
    unsafe { libc::freeifaddrs(list) };
    addresses
}

/// Gets the prefix length of a netmask sockaddr from getifaddrs. macOS trims IPv4 netmasks to
/// sa_len bytes (a /24 has sa_len 7, so the last mask byte isn't there at all), so only that many
/// bytes are read and the rest count as zero
fn get_netmask_prefix_len(netmask: *const libc::sockaddr, family: libc::c_int) -> u32 {
    if netmask.is_null() {
        return 0;
    }

    let (start, len) = if family == libc::AF_INET {
        (offset_of!(libc::sockaddr_in, sin_addr), 4)
    } else {
        (offset_of!(libc::sockaddr_in6, sin6_addr), 16)
    };

    // SAFETY: netmask is non-null, sa_len is the first byte of every BSD sockaddr
    #[cfg(target_os = "macos")]
    let available = usize::from(unsafe { (*netmask).sa_len });
    // Linux has no sa_len, its netmasks are always the full sockaddr of the address family
    #[cfg(not(target_os = "macos"))]
    let available = start + len;

    let end = (start + len).min(available);
    if end <= start {
        return 0;
    }

    let mut bytes = [0u8; 16];
    // SAFETY: the sockaddr is at least `end` bytes long and end - start <= 16 fits in `bytes`
    unsafe {
        std::ptr::copy_nonoverlapping(
            netmask.cast::<u8>().add(start),
            bytes.as_mut_ptr(),
            end - start,
        );
    }
    prefix_len_from_netmask(&bytes[.. len])
}

// Route flags shared by Linux's /proc/net/route and /proc/net/ipv6_route
const RTF_UP: u32 = 0x0001;
const RTF_REJECT: u32 = 0x0200;

// Linux IFA_F_* address flags as printed in /proc/net/if_inet6
const IFA_F_TEMPORARY: u32 = 0x01;
const IFA_F_DADFAILED: u32 = 0x08;
const IFA_F_DEPRECATED: u32 = 0x20;
const IFA_F_TENTATIVE: u32 = 0x40;

/// How suitable an IPv6 address is to be displayed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ipv6AddressState {
    /// A stable address: SLAAC (EUI-64 or "secured"), DHCPv6 or static
    Stable,
    /// A temporary privacy address (RFC 8981), only shown when there is no stable one
    Temporary,
    /// Deprecated, tentative or duplicated, never shown
    Unusable,
}

/// Counts the prefix length of a netmask given as raw bytes, for example [255, 255, 255, 0] is 24.
/// Works for both IPv4 (4 bytes) and IPv6 (16 bytes) masks
pub fn prefix_len_from_netmask(netmask: &[u8]) -> u32 {
    // Same as fastfetch and ifconfig: count the set bits, real netmasks are contiguous anyway
    netmask.iter().map(|byte| byte.count_ones()).sum()
}

/// Checks if an IPv6 address is a global unicast address (2000::/3), which rules out link-local
/// (fe80::/10), unique local (fc00::/7), loopback and multicast addresses
pub fn is_global_ipv6(address: &Ipv6Addr) -> bool {
    address.segments()[0] & 0xe000 == 0x2000
}

/// Picks which IPv6 address of an interface to show: the first stable one, else the first
/// temporary one, as privacy addresses rotate daily. Unusable addresses are never picked
pub fn pick_ipv6_address<T>(
    candidates: impl IntoIterator<Item = (T, Ipv6AddressState)>,
) -> Option<T> {
    let mut temporary = None;
    for (candidate, state) in candidates {
        match state {
            Ipv6AddressState::Stable => return Some(candidate),
            Ipv6AddressState::Temporary => {
                temporary.get_or_insert(candidate);
            },
            Ipv6AddressState::Unusable => {},
        }
    }
    temporary
}

/// Checks if an interface is a virtual or tunnel interface (VPN, AirDrop, containers, bridges...)
/// that should not be picked as the primary interface when there is no default route
pub fn is_virtual_interface(name: &str) -> bool {
    const VIRTUAL_PREFIXES: [&str; 18] = [
        // macOS: VPN tunnels (Tailscale, iCloud Private Relay...), AirDrop, low latency WLAN,
        // Thunderbolt bridge, Apple internal NICs, Personal Hotspot and legacy 6to4/gif tunnels
        "utun",
        "awdl",
        "llw",
        "bridge",
        "anpi",
        "ap",
        "gif",
        "stf",
        // Linux: containers, VMs and VPNs
        "docker",
        "veth",
        "br-",
        "virbr",
        "tailscale",
        "tun",
        "tap",
        "wg",
        "zt",
        "lxc",
    ];
    VIRTUAL_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// Maps Linux IFA_F_* address flags (from /proc/net/if_inet6) to an address state
pub fn ipv6_state_from_linux_flags(flags: u32) -> Ipv6AddressState {
    if flags & (IFA_F_DEPRECATED | IFA_F_TENTATIVE | IFA_F_DADFAILED) != 0 {
        Ipv6AddressState::Unusable
    } else if flags & IFA_F_TEMPORARY != 0 {
        Ipv6AddressState::Temporary
    } else {
        Ipv6AddressState::Stable
    }
}

/// Finds the interface carrying the IPv4 default route in the contents of /proc/net/route,
/// picking the lowest metric when there are several
pub fn parse_default_route_ipv4(content: &str) -> Option<String> {
    // Columns: Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT, addresses
    // and flags are hex while the metric is decimal
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let interface = fields.next()?;
            let destination = fields.next()?;
            let flags = u32::from_str_radix(fields.nth(1)?, 16).ok()?;
            let metric = fields.nth(2)?.parse::<u32>().ok()?;
            let mask = fields.next()?;

            let is_default = destination == "00000000" && mask == "00000000";
            (is_default && flags & RTF_UP != 0).then_some((interface, metric))
        })
        .min_by_key(|(_, metric)| *metric)
        .map(|(interface, _)| interface.to_string())
}

/// Finds the interface carrying the IPv6 default route in the contents of /proc/net/ipv6_route,
/// picking the lowest metric when there are several
pub fn parse_default_route_ipv6(content: &str) -> Option<String> {
    // Columns: destination, destination prefix length, source, source prefix length, next hop,
    // metric, refcount, use, flags, interface. Everything but the interface is hex
    content
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let destination = fields.next()?;
            let prefix_len = fields.next()?;
            let metric = u32::from_str_radix(fields.nth(3)?, 16).ok()?;
            let flags = u32::from_str_radix(fields.nth(2)?, 16).ok()?;
            let interface = fields.next()?;

            // The kernel keeps an "unreachable" default route on lo with RTF_REJECT, skip it
            let is_default = prefix_len == "00" && destination.bytes().all(|byte| byte == b'0');
            let is_usable = flags & RTF_UP != 0 && flags & RTF_REJECT == 0 && interface != "lo";
            (is_default && is_usable).then_some((interface, metric))
        })
        .min_by_key(|(_, metric)| *metric)
        .map(|(interface, _)| interface.to_string())
}

/// Finds the IFA_F_* flags of one address of `interface` in the contents of /proc/net/if_inet6
pub fn parse_if_inet6_flags(content: &str, interface: &str, address: &Ipv6Addr) -> Option<u32> {
    // Columns: address (32 hex digits), interface index, prefix length, scope, flags, interface
    let wanted = u128::from(*address);
    content.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let line_address = u128::from_str_radix(fields.next()?, 16).ok()?;
        let flags = u32::from_str_radix(fields.nth(3)?, 16).ok()?;
        let line_interface = fields.next()?;

        (line_address == wanted && line_interface == interface).then_some(flags)
    })
}
