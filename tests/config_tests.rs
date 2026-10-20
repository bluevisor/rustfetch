//! Test that config files written before new display toggles existed still parse

use clap::Parser;
use rustfetch::{
    cli::Cli,
    config::{Config, load_all_config},
};

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
        && display.show_tailscale_name()
        && display.usage_bars
        && display.ac_adapter
        && display.wifi
        && display.key_color == "auto"));
}

#[test]
fn test_old_tailscale_host_key_still_applies() {
    // tailscale_name was called tailscale_host, configs written with the old key keep their choice
    for (value, expected) in [("false", false), ("true", true)] {
        let content = format!("{}tailscale_host = {}\n", OLD_DISPLAY_SECTION, value);
        let config: Option<Config> = toml::from_str(&content).ok();
        assert_eq!(config.map(|config| config.display.show_tailscale_name()), Some(expected));
    }
}

#[test]
fn test_tailscale_name_wins_over_old_key() {
    // A file that still has the old key next to the new one parses (a serde alias would fail with
    // "duplicate field" and reset every setting), and the new key decides
    for (old, new, expected) in [("true", "false", false), ("false", "true", true)] {
        let keys = format!("tailscale_host = {}\ntailscale_name = {}\n", old, new);
        let content = format!("{}{}\n[logo]\ncolor = \"rainbow\"\n", OLD_DISPLAY_SECTION, keys);
        let config: Option<Config> = toml::from_str(&content).ok();
        let shown = config.as_ref().map(|config| config.display.show_tailscale_name());
        assert_eq!(shown, Some(expected));
        assert_eq!(config.map(|config| config.logo.color), Some(String::from("rainbow")));
    }
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
fn test_old_config_gets_auto_key_color() {
    // A file written before key_color existed follows the logo and keeps its [logo] color
    for logo in ["rainbow", "#ff8800"] {
        let content = format!("{}\n[logo]\ncolor = \"{}\"\n", OLD_DISPLAY_SECTION, logo);
        let config: Option<Config> = toml::from_str(&content).ok();
        let key_color = config.as_ref().map(|config| config.display.key_color.as_str());
        assert_eq!(key_color, Some("auto"), "{}", logo);
        assert_eq!(config.map(|config| config.logo.color), Some(String::from(logo)));
    }
}

#[test]
fn test_key_color_is_read() {
    let keys = "key_color = \"none\"\n\n[logo]\ncolor = \"red\"\n";
    let content = format!("{}{}", OLD_DISPLAY_SECTION, keys);
    let config: Option<Config> = toml::from_str(&content).ok();
    let key_color = config.as_ref().map(|config| config.display.key_color.as_str());
    assert_eq!(key_color, Some("none"));
    assert_eq!(config.map(|config| config.logo.color), Some(String::from("red")));
}

#[test]
fn test_key_color_that_isnt_text_keeps_the_rest() -> Result<(), Box<dyn std::error::Error>> {
    // key_color is the only text key in [display], a bool or a number there mustn't reset every
    // toggle and the [logo] color: false turns the colors off, anything else follows the logo
    let without_swap = OLD_DISPLAY_SECTION.replace("swap = true", "swap = false");
    for (value, expected) in [("false", "none"), ("true", "auto"), ("5", "auto")] {
        let style = format!("usage_bars = false\nkey_color = {}\n", value);
        let content = format!("{}{}\n[logo]\ncolor = \"rainbow\"\n", without_swap, style);
        let config: Option<Config> = toml::from_str(&content).ok();
        assert!(config.as_ref().is_some_and(|config| !config.display.swap), "{}", value);
        let key_color = config.as_ref().map(|config| config.display.key_color.as_str());
        assert_eq!(key_color, Some(expected), "{}", value);
        assert_eq!(config.map(|config| config.logo.color), Some(String::from("rainbow")));

        // --all reads the same keys through its own struct
        let name = format!("rustfetch-key-{}-{}.toml", value, std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, &content)?;
        let cli = Cli {
            all: true,
            reset_config: false,
            padding: 1,
            config_file: Some(path.clone()),
            clear_cache: false,
        };
        let config = load_all_config(&cli);
        std::fs::remove_file(&path)?;
        assert_eq!(config.display.key_color, expected, "{}", value);
        assert!(!config.display.usage_bars, "{}", value);
        assert_eq!(config.logo.color, "rainbow");
    }
    Ok(())
}

#[test]
fn test_old_logo_table_gets_auto_name() {
    // A [logo] table written before `name` existed still parses, keeps its color and draws the
    // distro's own logo
    let content = format!("{}\n[logo]\ncolor = \"#ff8800\"\n", OLD_DISPLAY_SECTION);
    let config: Option<Config> = toml::from_str(&content).ok();
    let logo = config.map(|config| (config.logo.name, config.logo.color));
    assert_eq!(logo, Some((String::from("auto"), String::from("#ff8800"))));
}

#[test]
fn test_logo_name_is_read() {
    let content = format!("{}\n[logo]\nname = \"seen\"\ncolor = \"red\"\n", OLD_DISPLAY_SECTION);
    let config: Option<Config> = toml::from_str(&content).ok();
    let logo = config.map(|config| (config.logo.name, config.logo.color));
    assert_eq!(logo, Some((String::from("seen"), String::from("red"))));
}

#[test]
fn test_partial_logo_table_keeps_display() {
    // An empty [logo] table, or a misspelled key in it, only resets the logo name and color: the
    // [display] toggles still apply instead of the whole file falling back to the defaults
    let without_kernel = OLD_DISPLAY_SECTION.replace("kernel = true", "kernel = false");
    for logo in ["[logo]\n", "[logo]\ncolour = \"red\"\n"] {
        let content = format!("{}\n{}", without_kernel, logo);
        let config: Option<Config> = toml::from_str(&content).ok();
        assert!(config.as_ref().is_some_and(|config| !config.display.kernel), "{}", logo);
        let logo_settings = config.map(|config| (config.logo.name, config.logo.color));
        assert_eq!(logo_settings, Some((String::from("auto"), String::from("default"))));
    }
}

#[test]
fn test_wrong_logo_types_keep_the_rest() {
    // A [logo] value that isn't a string falls back to its default on its own, the other [logo] key
    // and the [display] toggles still apply
    let without_kernel = OLD_DISPLAY_SECTION.replace("kernel = true", "kernel = false");
    let cases = [
        ("name = true\ncolor = \"red\"", "auto", "red"),
        ("name = 5\ncolor = \"red\"", "auto", "red"),
        ("name = \"seen\"\ncolor = 5", "seen", "default"),
        ("name = [\"seen\"]\ncolor = { r = 1 }", "auto", "default"),
    ];
    for (logo, name, color) in cases {
        let content = format!("{}\n[logo]\n{}\n", without_kernel, logo);
        let config: Option<Config> = toml::from_str(&content).ok();
        assert!(config.as_ref().is_some_and(|config| !config.display.kernel), "{}", logo);
        let logo_settings = config.map(|config| (config.logo.name, config.logo.color));
        assert_eq!(logo_settings, Some((String::from(name), String::from(color))), "{}", logo);
    }
}

#[test]
fn test_all_flag_keeps_logo_settings() {
    // -a turns every module on but still draws the logo [logo] picks
    let path = std::env::temp_dir().join(format!("rustfetch_all_{}.toml", std::process::id()));
    let content = format!("{}\n[logo]\nname = \"seen\"\ncolor = \"red\"\n", OLD_DISPLAY_SECTION);
    assert!(std::fs::write(&path, content).is_ok());

    let path_text = path.to_string_lossy().into_owned();
    let cli = Cli::parse_from(["rustfetch", "-a", "-c", path_text.as_str()]);
    let config = load_all_config(&cli);
    let _ = std::fs::remove_file(&path);

    assert!(config.display.cpu_frequency && config.display.power_draw);
    assert_eq!(config.logo.name, "seen");
    assert_eq!(config.logo.color, "red");
}

#[test]
fn test_new_toggles_can_be_turned_off() {
    let new_keys = "shell = false\nipv4 = false\nipv6 = false\ntailscale_ip = false\n";
    let newer_keys = "tailscale_name = false\nusage_bars = false\nac_adapter = false\n";
    let content = format!("{}{}{}wifi = false\n", OLD_DISPLAY_SECTION, new_keys, newer_keys);
    let config: Option<Config> = toml::from_str(&content).ok();
    let display = config.map(|config| config.display);

    assert!(display.as_ref().is_some_and(|display| !display.shell
        && !display.ipv4
        && !display.ipv6
        && !display.tailscale_ip
        && !display.show_tailscale_name()
        && !display.usage_bars
        && !display.ac_adapter
        && !display.wifi));
}

#[test]
fn test_all_keeps_usage_bar_style() -> Result<(), Box<dyn std::error::Error>> {
    // --all turns every line on but keeps how lines look: usage_bars and key_color, like the [logo]
    // color
    let path = std::env::temp_dir().join(format!("rustfetch-all-{}.toml", std::process::id()));
    let style = "usage_bars = false\nkey_color = \"#00ff00\"\n";
    let content = format!("{}{}\n[logo]\ncolor = \"red\"\n", OLD_DISPLAY_SECTION, style);
    std::fs::write(&path, content)?;
    let cli = Cli {
        all: true,
        reset_config: false,
        padding: 1,
        config_file: Some(path.clone()),
        clear_cache: false,
    };
    let config = load_all_config(&cli);
    std::fs::remove_file(&path)?;

    assert!(!config.display.usage_bars && config.display.kernel && config.display.ac_adapter);
    assert!(config.display.wifi);
    assert_eq!(config.display.key_color, "#00ff00");
    assert_eq!(config.logo.color, "red");
    Ok(())
}

#[test]
fn test_all_defaults_usage_bars_without_file() {
    let cli = Cli {
        all: true,
        reset_config: false,
        padding: 1,
        config_file: Some(std::env::temp_dir().join("rustfetch-no-such-config.toml")),
        clear_cache: false,
    };
    let config = load_all_config(&cli);
    assert!(config.display.usage_bars && config.display.show_tailscale_name());
    assert_eq!(config.display.key_color, "auto");
}
