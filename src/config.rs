//! Main file for the configuration setup
//! To regenerate the config file and test new setups just run
//! cargo run -- --reset-config

use std::path::PathBuf;

use serde::{Deserialize, Serialize}; // This transforms toml files into structs and viceversa

use crate::cli::Cli;

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Config {
    pub display: DisplayConfig,
    // Defaulted so config files created before the [logo] section existed still parse
    #[serde(default)]
    pub logo: LogoConfig,
}

// Defaulted field by field too, so an empty or misspelled [logo] table doesn't throw the [display]
// settings away along with it
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct LogoConfig {
    pub color: String,
}

impl Default for LogoConfig {
    fn default() -> Self {
        Self { color: String::from("default") }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DisplayConfig {
    // The names MUST match the names inside config.toml
    pub identifier: bool,
    pub os: bool,
    pub kernel: bool,
    // Lines added after the config format was released are defaulted, so older config files
    // without their keys still parse (a parse error would also throw away the [logo] settings)
    #[serde(default = "default_true")]
    pub shell: bool,
    pub uptime: bool,
    pub cpu: bool,
    pub cpu_frequency: bool,
    pub gpu: bool,
    pub screen: bool,
    pub resolution: bool,
    pub refresh_rate: bool,
    pub ram: bool,
    pub swap: bool,
    pub disk: bool,
    #[serde(default = "default_true")]
    pub ipv4: bool,
    #[serde(default = "default_true")]
    pub ipv6: bool,
    #[serde(default = "default_true")]
    pub tailscale_ip: bool,
    #[serde(default = "default_true")]
    pub tailscale_host: bool,
    pub battery: bool,
    pub power_draw: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            // If the config file is missing or corrupted, default to this
            identifier: true,
            os: true,
            kernel: true,
            shell: true,
            uptime: true,
            cpu: true,
            cpu_frequency: false,
            gpu: true,
            screen: true,
            resolution: true,
            refresh_rate: true,
            ram: true,
            swap: true,
            disk: true,
            ipv4: true,
            ipv6: true,
            tailscale_ip: true,
            tailscale_host: true,
            battery: true,
            power_draw: false,
        }
    }
}

/// serde default for display toggles added after the config file format was released
fn default_true() -> bool {
    true
}

fn get_config_template() -> String {
    r##"# Rustfetch config file

[display]
# Display username and host name
identifier = true
# Display the OS name
os = true
# Display the kernel version
kernel = true
# Display the shell rustfetch was started from and its version
shell = true
# Display system uptime
uptime = true

# CPU INFO
# --------
# Display CPU info
cpu = true
    cpu_frequency = false

# GRAPHICS INFO
# --------
gpu = true
screen = true
    resolution = true
    refresh_rate = true

# MEMORY INFO
# -----------
# Display RAM usage
ram = true
# Display swap usage
swap = true

# DISK INFO
# ---------
# Display disk usage
disk = true

# NETWORK INFO
# ------------
# Display the local IPv4 address of the interface carrying the default route
ipv4 = true
# Display its global IPv6 address, hidden on IPv4-only networks
ipv6 = true
# Display this machine's Tailscale IPv4 address, hidden when Tailscale is not connected
tailscale_ip = true
# Display its Tailscale MagicDNS name, looked up from Tailscale's DNS server (100.100.100.100)
tailscale_host = true

# LAPTOP-RELATED INFO
# If the device is not a laptop, turning
# these on just wont display anything
# --------------------------------------
# Display battery information
battery = true
# Display power draw (Linux only)
power_draw = false

# LOGO
# ----
[logo]
# "default" keeps the distro's own colors, "rainbow" draws the classic striped Apple logo (macOS
# only), anything else draws the whole logo in one solid color: a name (white, bright white, red,
# green, yellow, blue, magenta, cyan, bright red, ...) or a hex code such as "#ff8800"
color = "default"
"##
    .to_string()
}

/// Gets the default config path for rustfetch. The default path is ~/.config/rustfetch/config.toml
/// or ./rustfetch.toml as fallback
pub fn get_default_path() -> PathBuf {
    dirs::config_dir()
        .map(|p| p.join("rustfetch/config.toml"))
        .unwrap_or_else(|| PathBuf::from("rustfetch.toml")) // fallback = current directory
}

/// Creates the config file with default options and comments
fn create_config_file(config_path: &PathBuf) -> Config {
    let default_config = Config::default();

    // If parent directory does not exist, create it
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let toml_string = get_config_template();

    if let Err(e) = std::fs::write(config_path, toml_string) {
        eprintln!("Warning: Could not create config file at {:?}: {}", config_path, e);
        eprintln!("Using default configuration in memory");
    } else {
        println!("Created default config file at:  {:?}", config_path);
    }

    default_config
}

pub fn load_config(cli: &Cli) -> Config {
    let config_path = cli.config_file.as_ref().cloned().unwrap_or_else(get_default_path);

    if cli.reset_config {
        return create_config_file(&config_path);
    }

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        // If parsing fails, use defaults instead
        toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to parse config file: {}", e);
            eprintln!("Using default configuration");
            Config::default()
        })
    } else {
        create_config_file(&config_path)
    }
}

/// Only the [logo] table of a config file, every other table is ignored without being checked
#[derive(Deserialize, Default)]
struct LogoOnlyConfig {
    #[serde(default)]
    logo: LogoConfig,
}

/// Reads the [logo] settings from the config file without creating it or printing anything, which
/// --all never did. A missing or unreadable file gives the default logo
fn load_logo_config(cli: &Cli) -> LogoConfig {
    let config_path = cli.config_file.as_ref().cloned().unwrap_or_else(get_default_path);
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|content| toml::from_str::<LogoOnlyConfig>(&content).ok())
        .map(|config| config.logo)
        .unwrap_or_default()
}

/// Enables every module but keeps the user's [logo] settings from the config file
pub fn load_all_config(cli: &Cli) -> Config {
    Config {
        logo: load_logo_config(cli),
        display: DisplayConfig {
            identifier: true,
            os: true,
            kernel: true,
            shell: true,
            uptime: true,
            cpu: true,
            cpu_frequency: true,
            gpu: true,
            screen: true,
            resolution: true,
            refresh_rate: true,
            ram: true,
            swap: true,
            disk: true,
            ipv4: true,
            ipv6: true,
            tailscale_ip: true,
            tailscale_host: true,
            battery: true,
            power_draw: true,
        },
    }
}
