//! The name (SSID) of the Wi-Fi network this machine is on. Telling that a Wi-Fi interface is
//! associated and which network it is on takes a few syscalls, reading the name itself costs
//! milliseconds on macOS, so it is read once per network and kept in wifi.toml in the cache
//! directory

use std::{
    collections::BTreeMap,
    fs,
    net::Ipv4Addr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::{InterfaceAddress, get_interface_addresses, is_virtual_interface};
use crate::{
    common::{read_cache_file, write_cache_file},
    platform,
};

/// Networks remembered in wifi.toml, the ones added longest ago are dropped first
pub const MAX_CACHED_NETWORKS: usize = 32;
/// Seconds a guessed name (see WifiSsid) is trusted before it's read again, so an exact name
/// replaces it as soon as one can be read and a wrong guess doesn't stick until --clear-cache
pub const GUESSED_SSID_TTL: u64 = 15 * 60;

/// A network's name as platform::read_wifi_ssid read it, already cleaned up by sanitize_ssid
#[derive(Debug, Clone, PartialEq)]
pub struct WifiSsid {
    pub name: String,
    /// Only a guess (macOS: CoreWLAN's most recent known network), which can name the previous
    /// network, rather than read from the joined network itself
    pub guessed: bool,
}

/// A Wi-Fi interface associated with a network, and what tells that network apart from others
/// without knowing its name
#[derive(Debug, Clone, PartialEq)]
pub struct WifiLink {
    pub interface: String,
    /// The interface's own MAC address. macOS gives every network its own private address by
    /// default, which tells apart two networks behind the same router
    pub mac: Option<[u8; 6]>,
    /// The router's IPv4 and MAC addresses, None when the interface has no default route
    pub router: Option<(Ipv4Addr, [u8; 6])>,
}

impl WifiLink {
    /// Builds the key the network's name is cached under, e.g.
    /// "en0 0a:6f:b5:20:c6:02 172.18.7.1 cc:03:d9:3a:ed:d0". None without a router, since nothing
    /// would tell one network from another
    pub fn fingerprint(&self) -> Option<String> {
        let (router, router_mac) = self.router?;
        let mac = self.mac.map(|mac| format_mac(&mac)).unwrap_or_else(|| String::from("-"));
        Some(format!("{} {} {} {}", self.interface, mac, router, format_mac(&router_mac)))
    }
}

/// Formats a MAC address the usual way, e.g. "cc:03:d9:3a:ed:d0"
pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter().map(|byte| format!("{:02x}", byte)).collect::<Vec<_>>().join(":")
}

/// Gets the name of the Wi-Fi network this machine is connected to, None when no Wi-Fi interface
/// is associated. A network seen before comes from the cache (a guessed name only for
/// GUESSED_SSID_TTL), a new one is read with platform::read_wifi_ssid and remembered
pub fn get_wifi_ssid() -> Option<String> {
    let link = wifi_candidates(get_interface_addresses())
        .into_iter()
        .find_map(platform::get_wifi_link)?;
    let fingerprint = link.fingerprint();
    let cache_path = get_wifi_cache_path();

    // Without a fingerprint there is nothing to look up or store
    let mut cache: WifiSsidCache = match (&fingerprint, &cache_path) {
        (Some(_), Some(path)) => read_cache_file(path),
        _ => WifiSsidCache::default(),
    };
    let now = unix_time();
    if let Some(ssid) = fingerprint.as_deref().and_then(|key| cache.lookup(key, now)) {
        return Some(ssid);
    }

    // A name that couldn't be read is not cached, the next run tries again
    let ssid = platform::read_wifi_ssid(&link.interface)?;
    if let (Some(fingerprint), Some(path)) = (fingerprint, cache_path) {
        cache.insert(&fingerprint, &ssid.name, ssid.guessed, now);
        write_cache_file(&path, &cache);
    }
    Some(ssid.name)
}

/// Picks the interfaces that could be on Wi-Fi: physical ones with an IPv4 address, each once and
/// in the order the kernel lists them
pub fn wifi_candidates(addresses: &[InterfaceAddress]) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for entry in addresses.iter().filter(|entry| entry.address.is_ipv4()) {
        let name = entry.interface.as_str();
        if !is_virtual_interface(name) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Cleans a network name up for printing: control characters are dropped (a network can be named
/// anything, escape codes included) and so are the spaces around it. None when nothing is left, or
/// for "<redacted>", what macOS prints in place of a name it hides
pub fn sanitize_ssid(ssid: &str) -> Option<String> {
    let ssid: String = ssid.chars().filter(|c| !c.is_control()).collect();
    let ssid = ssid.trim();
    (!ssid.is_empty() && ssid != "<redacted>").then(|| ssid.to_string())
}

/// Takes the name an exact source read when it is still a name once cleaned up, else what
/// `fallback` reads. macOS hides a name by blanking it rather than leaving it out, so an empty or
/// "<redacted>" name has to fall through to the next source instead of hiding the Wi-Fi line
pub fn exact_ssid_or(
    exact: Option<String>,
    fallback: impl FnOnce() -> Option<WifiSsid>,
) -> Option<WifiSsid> {
    match exact.as_deref().and_then(sanitize_ssid) {
        Some(name) => Some(WifiSsid {
            name,
            guessed: false,
        }),
        None => fallback(),
    }
}

/// Names of the Wi-Fi networks seen so far, keyed by WifiLink::fingerprint. Stored as TOML in the
/// cache directory next to the shell version cache
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WifiSsidCache {
    #[serde(default)]
    networks: BTreeMap<String, CachedWifiNetwork>,
}

/// One network's name and when it was cached
#[derive(Debug, Serialize, Deserialize)]
struct CachedWifiNetwork {
    ssid: String,
    /// Seconds since the Unix epoch, the oldest network is dropped first when the cache is full.
    /// Defaulted so an entry written by hand still parses
    #[serde(default)]
    added: u64,
    /// The name is only a guess (see WifiSsid), trusted for GUESSED_SSID_TTL. Left out of the file
    /// for exact names
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    guessed: bool,
}

impl WifiSsidCache {
    /// Gets the name of the network with this fingerprint, None when it hasn't been seen or is a
    /// guess that is GUESSED_SSID_TTL old (or from the future, the clock was set back) at `now`
    pub fn lookup(&self, fingerprint: &str, now: u64) -> Option<String> {
        let network = self.networks.get(fingerprint)?;
        if network.guessed
            && now.checked_sub(network.added).is_none_or(|age| age >= GUESSED_SSID_TTL)
        {
            return None;
        }
        // The file can be edited by hand, so its names are cleaned up like freshly read ones
        sanitize_ssid(&network.ssid)
    }

    /// Remembers a network's name, replacing what was cached for the same fingerprint. Once
    /// MAX_CACHED_NETWORKS are remembered the one added longest ago makes room
    pub fn insert(&mut self, fingerprint: &str, ssid: &str, guessed: bool, added: u64) {
        while !self.networks.contains_key(fingerprint) && self.networks.len() >= MAX_CACHED_NETWORKS
        {
            let oldest = self
                .networks
                .iter()
                .min_by_key(|(_, network)| network.added)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            self.networks.remove(&oldest);
        }

        let network = CachedWifiNetwork {
            ssid: ssid.to_string(),
            added,
            guessed,
        };
        self.networks.insert(fingerprint.to_string(), network);
    }
}

/// Gets where the Wi-Fi name cache lives (~/Library/Caches/rustfetch/wifi.toml on macOS), None
/// when there is no cache directory
fn get_wifi_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|path| path.join("rustfetch/wifi.toml"))
}

/// Gets the current time in seconds since the Unix epoch, 0 if the clock is before it
fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or(0)
}

/// Deletes the Wi-Fi name cache, for --clear-cache (e.g. after a network was renamed)
pub fn clear_wifi_ssid_cache() {
    if let Some(path) = get_wifi_cache_path() {
        let _ = fs::remove_file(path);
    }
}
