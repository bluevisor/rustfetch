//! Test that config files written before new display toggles existed still parse

use rustfetch::config::Config;

/// The [display] section rustfetch 0.3.0 wrote, before the shell and network lines existed
const OLD_DISPLAY_SECTION: &str = "[display]
identifier = true
os = true
kernel = true
uptime = true
cpu = true
cpu_frequency = false
gpu = true
screen = true
resolution = true
refresh_rate = true
ram = true
swap = true
disk = true
battery = true
power_draw = false
";

#[test]
fn test_old_config_gets_new_lines() {
    let config: Option<Config> = toml::from_str(OLD_DISPLAY_SECTION).ok();
    let display = config.map(|config| config.display);

    assert!(display.as_ref().is_some_and(|display| display.shell
        && display.ipv4
        && display.ipv6
        && display.tailscale_ip
        && display.tailscale_host));
}

#[test]
fn test_old_config_keeps_logo_color() {
    // A missing key must not make the whole file fall back to the defaults, which would also reset
    // the logo color
    let content = format!("{}\n[logo]\ncolor = \"rainbow\"\n", OLD_DISPLAY_SECTION);
    let config: Option<Config> = toml::from_str(&content).ok();
    assert_eq!(config.map(|config| config.logo.color), Some(String::from("rainbow")));
}

#[test]
fn test_partial_logo_table_keeps_display() {
    // An empty [logo] table, or a misspelled key in it, only resets the logo color: the [display]
    // toggles still apply instead of the whole file falling back to the defaults
    let without_kernel = OLD_DISPLAY_SECTION.replace("kernel = true", "kernel = false");
    for logo in ["[logo]\n", "[logo]\ncolour = \"red\"\n"] {
        let content = format!("{}\n{}", without_kernel, logo);
        let config: Option<Config> = toml::from_str(&content).ok();
        assert!(config.as_ref().is_some_and(|config| !config.display.kernel), "{}", logo);
        assert_eq!(config.map(|config| config.logo.color), Some(String::from("default")));
    }
}

#[test]
fn test_new_toggles_can_be_turned_off() {
    let new_keys = "shell = false\nipv4 = false\nipv6 = false\ntailscale_ip = false\n";
    let content = format!("{}{}tailscale_host = false\n", OLD_DISPLAY_SECTION, new_keys);
    let config: Option<Config> = toml::from_str(&content).ok();
    let display = config.map(|config| config.display);

    assert!(display.as_ref().is_some_and(|display| !display.shell
        && !display.ipv4
        && !display.ipv6
        && !display.tailscale_ip
        && !display.tailscale_host));
}
