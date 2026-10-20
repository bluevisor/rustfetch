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
    // Draws a bar next to the RAM and disk percentages
    #[serde(default = "default_true")]
    pub usage_bars: bool,
    #[serde(default = "default_true")]
    pub ipv4: bool,
    #[serde(default = "default_true")]
    pub ipv6: bool,
    #[serde(default = "default_true")]
    pub tailscale_ip: bool,
    // Was tailscale_host until the line started showing only the machine name. The old key is its
    // own field, a serde alias would fail on a file that has both, see show_tailscale_name()
    #[serde(default)]
    pub tailscale_name: Option<bool>,
    #[serde(default, skip_serializing)]
    pub tailscale_host: Option<bool>,
    pub battery: bool,
    // The connected power adapter (macOS only)
    #[serde(default = "default_true")]
    pub ac_adapter: bool,
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
            usage_bars: true,
            ipv4: true,
            ipv6: true,
            tailscale_ip: true,
            tailscale_name: Some(true),
            tailscale_host: None,
            battery: true,
            ac_adapter: true,
            power_draw: false,
        }
    }
}

impl DisplayConfig {
    /// Checks if the Tailscale Name line is on: tailscale_name wins over the old tailscale_host
    /// key, and with neither the line is shown like every line added after the format's release
    pub fn show_tailscale_name(&self) -> bool {
        self.tailscale_name.or(self.tailscale_host).unwrap_or(true)
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

# USAGE BARS
# ----------
# Draw a bar before the RAM and disk percentages, false shows just "(64%)". --all keeps it too
usage_bars = true

# NETWORK INFO
# ------------
# Display the local IPv4 address of the interface carrying the default route
ipv4 = true
# Display its global IPv6 address, hidden on IPv4-only networks
ipv6 = true
# Display this machine's Tailscale IPv4 address, hidden when Tailscale is not connected
tailscale_ip = true
# Display its Tailscale machine name, the first part of its MagicDNS name (laptop for
# laptop.tailnet.ts.net) looked up from Tailscale's DNS server (100.100.100.100). Replaces the
# old tailscale_host key, which still works and loses to this one when both are set
tailscale_name = true

# LAPTOP-RELATED INFO
# If the device is not a laptop, turning
# these on just wont display anything
# --------------------------------------
# Display battery information
battery = true
# Display the connected power adapter, e.g. "140W USB-C Power Adapter" (macOS only)
ac_adapter = true
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

/// What --all keeps from a config file: the [logo] table and the [display] keys that change how
/// lines look rather than which ones show. Every other key is ignored without being checked
#[derive(Deserialize, Default)]
struct StyleOnlyConfig {
    #[serde(default)]
    logo: LogoConfig,
    #[serde(default)]
    display: DisplayStyleConfig,
}

/// The [display] keys --all keeps, defaulted one by one like [logo]
#[derive(Deserialize)]
#[serde(default)]
struct DisplayStyleConfig {
    usage_bars: bool,
}

impl Default for DisplayStyleConfig {
    fn default() -> Self {
        Self { usage_bars: true }
    }
}

/// Reads the settings --all keeps from the config file without creating it or printing anything,
/// which --all never did. A missing or unreadable file gives the defaults
fn load_style_config(cli: &Cli) -> StyleOnlyConfig {
    let config_path = cli.config_file.as_ref().cloned().unwrap_or_else(get_default_path);
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|content| toml::from_str::<StyleOnlyConfig>(&content).ok())
        .unwrap_or_default()
}

/// Enables every module but keeps the user's [logo] settings and usage bar style from the config
/// file
pub fn load_all_config(cli: &Cli) -> Config {
    let style = load_style_config(cli);
    Config {
        logo: style.logo,
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
            usage_bars: style.display.usage_bars,
            ipv4: true,
            ipv6: true,
            tailscale_ip: true,
            tailscale_name: Some(true),
            tailscale_host: None,
            battery: true,
            ac_adapter: true,
            power_draw: true,
        },
    }
}
