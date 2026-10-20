//! Test the Wi-Fi name cache, its network fingerprint and SSID cleanup

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rustfetch::{
    common::{read_cache_file, write_cache_file},
    sysinfo::*,
};

const OWN_MAC: [u8; 6] = [0x0a, 0x6f, 0xb5, 0x20, 0xc6, 0x02];
const ROUTER_MAC: [u8; 6] = [0xcc, 0x03, 0xd9, 0x3a, 0xed, 0xd0];
const ROUTER: Ipv4Addr = Ipv4Addr::new(172, 18, 7, 1);
const FINGERPRINT: &str = "en0 0a:6f:b5:20:c6:02 172.18.7.1 cc:03:d9:3a:ed:d0";
/// The time lookups are made at, seconds since the Unix epoch
const NOW: u64 = 1_792_475_989;

/// Builds the link of an associated en0 with its router
fn link(mac: Option<[u8; 6]>, router: Option<(Ipv4Addr, [u8; 6])>) -> WifiLink {
    WifiLink {
        interface: String::from("en0"),
        mac,
        router,
    }
}

// Fingerprint tests

#[test]
fn test_wifi_fingerprint() {
    let full = link(Some(OWN_MAC), Some((ROUTER, ROUTER_MAC)));
    assert_eq!(full.fingerprint(), Some(String::from(FINGERPRINT)));

    // the interface's own address is optional, the router is not
    let without_mac = link(None, Some((ROUTER, ROUTER_MAC)));
    assert_eq!(
        without_mac.fingerprint(),
        Some(String::from("en0 - 172.18.7.1 cc:03:d9:3a:ed:d0"))
    );
    assert_eq!(link(Some(OWN_MAC), None).fingerprint(), None);
}

#[test]
fn test_wifi_fingerprint_tells_networks_apart() {
    // Another network behind the same router gets another private address, and another router
    // has another MAC address even at the same IP
    let home = link(Some(OWN_MAC), Some((ROUTER, ROUTER_MAC)));
    let guest = link(Some([0x0a, 0, 0, 0, 0, 1]), Some((ROUTER, ROUTER_MAC)));
    let elsewhere = link(Some(OWN_MAC), Some((ROUTER, [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc])));
    assert_ne!(home.fingerprint(), guest.fingerprint());
    assert_ne!(home.fingerprint(), elsewhere.fingerprint());
}

#[test]
fn test_format_mac() {
    // single digits are zero padded, unlike arp's "cc:3:d9:3a:ed:d0"
    assert_eq!(format_mac(&ROUTER_MAC), "cc:03:d9:3a:ed:d0");
    assert_eq!(format_mac(&[0; 6]), "00:00:00:00:00:00");
}

// SSID cleanup tests

#[test]
fn test_sanitize_ssid() {
    assert_eq!(sanitize_ssid("SeenHealth IT"), Some(String::from("SeenHealth IT")));
    assert_eq!(sanitize_ssid("Café ☕ 5G"), Some(String::from("Café ☕ 5G")));
    // spaces around the name are dropped, the ones inside it are kept
    assert_eq!(sanitize_ssid("  Home  Network \n"), Some(String::from("Home  Network")));
}

#[test]
fn test_sanitize_ssid_strips_control_characters() {
    // a network can be named anything, an escape code must not reach the terminal
    assert_eq!(sanitize_ssid("\u{1b}[31mEvil\u{7}"), Some(String::from("[31mEvil")));
    assert_eq!(sanitize_ssid("Tab\tbed\u{9b}"), Some(String::from("Tabbed")));
}

#[test]
fn test_sanitize_ssid_empty() {
    assert_eq!(sanitize_ssid(""), None);
    assert_eq!(sanitize_ssid("   "), None);
    assert_eq!(sanitize_ssid("\u{0}\u{1b}\r\n"), None);
}

#[test]
fn test_sanitize_ssid_redacted() {
    // what macOS prints in place of a name it hides is not a name
    assert_eq!(sanitize_ssid("<redacted>"), None);
    assert_eq!(sanitize_ssid(" <redacted>\n"), None);
    assert_eq!(sanitize_ssid("Not <redacted>"), Some(String::from("Not <redacted>")));
}

// exact_ssid_or tests

/// A fallback's answer, a guessed name
fn guess(name: &str) -> Option<WifiSsid> {
    Some(WifiSsid {
        name: name.to_string(),
        guessed: true,
    })
}

#[test]
fn test_exact_ssid_or_takes_exact_name() {
    let ssid = exact_ssid_or(Some(String::from(" SeenHealth IT\u{7}")), || {
        panic!("the fallback must not be read when the exact name is usable")
    });
    let exact = WifiSsid {
        name: String::from("SeenHealth IT"),
        guessed: false,
    };
    assert_eq!(ssid, Some(exact));
}

#[test]
fn test_exact_ssid_or_falls_back_on_blanked_name() {
    // configd blanks SSID_STR rather than leaving it out, a scan record could be blanked the same
    // way, and that must not hide the Wi-Fi line while the fallback still knows the name
    for exact in [
        None,
        Some(""),
        Some("   "),
        Some("\u{0}"),
        Some("<redacted>"),
    ] {
        let ssid = exact_ssid_or(exact.map(String::from), || guess("SeenHealth IT"));
        assert_eq!(ssid, guess("SeenHealth IT"), "{:?}", exact);
    }
    assert_eq!(exact_ssid_or(Some(String::new()), || None), None);
}

// Interface selection tests

/// Builds one getifaddrs entry
fn address(interface: &str, address: IpAddr) -> InterfaceAddress {
    InterfaceAddress {
        interface: interface.to_string(),
        address,
        prefix_len: 24,
    }
}

#[test]
fn test_wifi_candidates() {
    let addresses = [
        address("en0", IpAddr::V6(Ipv6Addr::LOCALHOST)),
        address("utun4", IpAddr::V4(Ipv4Addr::new(100, 81, 138, 51))),
        address("en0", IpAddr::V4(Ipv4Addr::new(172, 18, 7, 204))),
        address("en0", IpAddr::V4(Ipv4Addr::new(172, 18, 7, 205))),
        address("bridge100", IpAddr::V4(Ipv4Addr::new(192, 168, 64, 1))),
        address("en9", IpAddr::V4(Ipv4Addr::new(192, 168, 55, 2))),
        address("awdl0", IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ];
    // tunnels and bridges are never Wi-Fi, and IPv6-only interfaces are skipped
    assert_eq!(wifi_candidates(&addresses), vec!["en0", "en9"]);
    assert!(wifi_candidates(&[]).is_empty());
}

// WifiSsidCache tests

#[test]
fn test_wifi_ssid_cache_lookup() {
    let mut cache = WifiSsidCache::default();
    assert_eq!(cache.lookup(FINGERPRINT, NOW), None);

    cache.insert(FINGERPRINT, "SeenHealth IT", false, 1);
    assert_eq!(cache.lookup(FINGERPRINT, NOW), Some(String::from("SeenHealth IT")));

    // a renamed network is stored again under the same fingerprint
    cache.insert(FINGERPRINT, "SeenHealth Staff", false, 2);
    assert_eq!(cache.lookup(FINGERPRINT, NOW), Some(String::from("SeenHealth Staff")));
}

#[test]
fn test_wifi_ssid_cache_stale_fingerprint() {
    // After moving to another network (another router, or another private address) the cached
    // name of the old one must not show up
    let mut cache = WifiSsidCache::default();
    cache.insert(FINGERPRINT, "SeenHealth IT", false, 1);

    let moved = link(Some(OWN_MAC), Some((Ipv4Addr::new(192, 168, 1, 1), ROUTER_MAC)));
    let other_address = link(Some([0x0a, 0, 0, 0, 0, 1]), Some((ROUTER, ROUTER_MAC)));
    for fingerprint in [moved.fingerprint(), other_address.fingerprint()] {
        assert_eq!(fingerprint.as_deref().and_then(|key| cache.lookup(key, NOW)), None);
    }
}

#[test]
fn test_wifi_ssid_cache_guessed_name_expires() {
    let mut cache = WifiSsidCache::default();
    cache.insert(FINGERPRINT, "Previous Network", true, NOW - 60);
    assert_eq!(cache.lookup(FINGERPRINT, NOW), Some(String::from("Previous Network")));
    // once GUESSED_SSID_TTL old it is read again, so a wrong guess doesn't stick
    assert_eq!(cache.lookup(FINGERPRINT, NOW - 60 + GUESSED_SSID_TTL), None);
    // the clock was set back since it was guessed
    assert_eq!(cache.lookup(FINGERPRINT, NOW - 61), None);

    // an exact name read since replaces it and never expires
    cache.insert(FINGERPRINT, "SeenHealth IT", false, NOW - 60);
    assert_eq!(
        cache.lookup(FINGERPRINT, NOW + 365 * 24 * 3600),
        Some(String::from("SeenHealth IT"))
    );
    assert_eq!(cache.lookup(FINGERPRINT, 0), Some(String::from("SeenHealth IT")));
}

#[test]
fn test_wifi_ssid_cache_drops_oldest_when_full() {
    let mut cache = WifiSsidCache::default();
    for index in 0 .. MAX_CACHED_NETWORKS as u64 {
        cache.insert(&format!("network {}", index), "Old", false, 100 + index);
    }

    // replacing a known network makes no room
    cache.insert("network 0", "Renamed", false, 100);
    assert_eq!(cache.lookup("network 0", NOW), Some(String::from("Renamed")));
    assert_eq!(cache.lookup("network 1", NOW), Some(String::from("Old")));

    // a new one replaces the network added longest ago
    cache.insert("new network", "New", false, 500);
    assert_eq!(cache.lookup("new network", NOW), Some(String::from("New")));
    assert_eq!(cache.lookup("network 0", NOW), None);
    assert_eq!(cache.lookup("network 1", NOW), Some(String::from("Old")));
}

#[test]
fn test_wifi_ssid_cache_file_round_trip() {
    let path = std::env::temp_dir().join(format!("rustfetch-wifi-{}.toml", std::process::id()));
    let mut cache = WifiSsidCache::default();
    cache.insert(FINGERPRINT, "SeenHealth IT", false, 1_792_475_989);
    cache.insert("en0 - 192.168.1.1 12:34:56:78:9a:bc", "Home \"5G\"", false, 1);
    write_cache_file(&path, &cache);

    let parsed: WifiSsidCache = read_cache_file(&path);
    let leftover = path.with_extension(format!("toml.{}", std::process::id()));
    let _ = std::fs::remove_file(&path);

    assert_eq!(parsed.lookup(FINGERPRINT, NOW), Some(String::from("SeenHealth IT")));
    assert_eq!(
        parsed.lookup("en0 - 192.168.1.1 12:34:56:78:9a:bc", NOW),
        Some(String::from("Home \"5G\""))
    );
    // the temporary file was renamed over the cache
    assert!(!leftover.exists());
}

#[test]
fn test_wifi_ssid_cache_file_keeps_guesses_apart() {
    let path = std::env::temp_dir().join(format!("rustfetch-guess-{}.toml", std::process::id()));
    let mut cache = WifiSsidCache::default();
    cache.insert(FINGERPRINT, "SeenHealth IT", false, NOW);
    cache.insert("en0 - 192.168.1.1 12:34:56:78:9a:bc", "Home", true, NOW);
    write_cache_file(&path, &cache);

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let parsed: WifiSsidCache = read_cache_file(&path);
    let _ = std::fs::remove_file(&path);

    // only the guess is marked, exact entries look like they did before guesses were tracked
    assert_eq!(content.matches("guessed = true").count(), 1, "{}", content);
    assert!(!content.contains("guessed = false"), "{}", content);
    let home = "en0 - 192.168.1.1 12:34:56:78:9a:bc";
    assert_eq!(parsed.lookup(home, NOW), Some(String::from("Home")));
    assert_eq!(parsed.lookup(home, NOW + GUESSED_SSID_TTL), None);
    assert_eq!(
        parsed.lookup(FINGERPRINT, NOW + GUESSED_SSID_TTL),
        Some(String::from("SeenHealth IT"))
    );
}

#[test]
fn test_wifi_ssid_cache_malformed_file() {
    // a corrupted or missing file is an empty cache, not an error
    for content in [
        "networks = 3",
        "[networks.\"en0\"]\nssid = 5\n",
        "not toml at all [",
    ] {
        assert!(toml::from_str::<WifiSsidCache>(content).is_err(), "{}", content);
    }
    let empty: WifiSsidCache = toml::from_str("").unwrap_or_default();
    assert_eq!(empty.lookup(FINGERPRINT, NOW), None);

    let missing = std::env::temp_dir().join("rustfetch-no-such-wifi-cache.toml");
    let cache: WifiSsidCache = read_cache_file(&missing);
    assert_eq!(cache.lookup(FINGERPRINT, NOW), None);
}

#[test]
fn test_wifi_ssid_cache_corrupted_file_on_disk() {
    // Bytes that aren't UTF-8, then TOML of the wrong shape: both read as an empty cache, and the
    // next write replaces them
    let path = std::env::temp_dir().join(format!("rustfetch-corrupt-{}.toml", std::process::id()));
    // 0xff never appears in UTF-8
    for content in [vec![0xff; 4096], b"[networks]\nen0 = 5\n".to_vec()] {
        let _ = std::fs::write(&path, content);
        let cache: WifiSsidCache = read_cache_file(&path);
        assert_eq!(cache.lookup(FINGERPRINT, NOW), None);
    }

    let mut cache = WifiSsidCache::default();
    cache.insert(FINGERPRINT, "SeenHealth IT", false, NOW);
    write_cache_file(&path, &cache);
    let parsed: WifiSsidCache = read_cache_file(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(parsed.lookup(FINGERPRINT, NOW), Some(String::from("SeenHealth IT")));
}

#[test]
fn test_wifi_ssid_cache_hand_edited_file() {
    // An entry without its date still parses, and names edited into the file are cleaned up like
    // freshly read ones
    let content = format!(
        "[networks.\"{}\"]\nssid = \"\\u001b[31mEvil\"\n\n[networks.\"blank\"]\nssid = \" \"\n",
        FINGERPRINT
    );
    let cache: WifiSsidCache = toml::from_str(&content).unwrap_or_default();
    assert_eq!(cache.lookup(FINGERPRINT, NOW), Some(String::from("[31mEvil")));
    assert_eq!(cache.lookup("blank", NOW), None);
}
