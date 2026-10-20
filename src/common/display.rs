use colored::*;
use sysinfo::System;

use crate::{
    cli::Cli,
    common::{
        PaletteColor, extract_numeric_value, format_battery_status, format_cpu_cores,
        format_frequency, format_screen, format_screen_key, paint, paint_track, render_bar,
    },
    config::Config,
    platform::{self, get_power_draw},
    sysinfo::*,
};

/// Cells in the RAM and disk usage bars
const USAGE_BAR_WIDTH: usize = 10;

/// Picks the color of a usage percentage (RAM, swap, disk), low is green and high is red
fn usage_color(percentage: u64) -> PaletteColor {
    if percentage < 40 {
        PaletteColor::Green
    } else if (40 .. 80).contains(&percentage) {
        PaletteColor::Yellow
    } else {
        PaletteColor::Red
    }
}

/// Colors a usage percentage (RAM, swap, disk), low is green and high is red
fn color_percentage(percentage: u64) -> ColoredString {
    paint(&format!("{}%", percentage), usage_color(percentage))
}

/// Formats the usage after RAM and disk figures, "██████▍░░░ 64%" with usage_bars on and "(64%)"
/// with it off. The filled part takes the percentage's color and the empty track is dim gray
fn format_usage(percentage: u64, config: &Config) -> String {
    if !config.display.usage_bars {
        return format!("({})", color_percentage(percentage));
    }

    let (filled, empty) = render_bar(percentage, USAGE_BAR_WIDTH);
    // colored still wraps an empty string in escape codes, so a 0% or 100% bar skips that part
    let filled = if filled.is_empty() {
        filled
    } else {
        paint(&filled, usage_color(percentage)).to_string()
    };
    let empty = if empty.is_empty() {
        empty
    } else {
        paint_track(&empty).to_string()
    };
    format!("{}{} {}", filled, empty, color_percentage(percentage))
}

/// Colors a charge percentage (battery), low is red and high is green
fn color_percentage_inverse(percentage: f64) -> ColoredString {
    let text = format!("{}%", percentage);
    if percentage < 30.0 {
        paint(&text, PaletteColor::Red)
    } else if (30.0 .. 70.0).contains(&percentage) {
        paint(&text, PaletteColor::Yellow)
    } else {
        paint(&text, PaletteColor::Green)
    }
}

pub fn display_os() -> String {
    format!(
        "{} {} ({})",
        "OS:".bold(),
        platform::format_os_name(),
        std::env::consts::ARCH // CPU Architecture the program was compiled for
    )
}

pub fn display_kernel() -> String {
    format!("{} {}", "Kernel:".bold(), platform::format_kernel_version())
}

/// Formats the shell line, e.g. "Shell: fish 4.9.3", the version is left out when it can't be found
pub fn display_shell() -> Option<String> {
    let (name, version) = get_shell()?;
    match version {
        Some(version) => Some(format!("{} {} {}", "Shell:".bold(), name, version)),
        None => Some(format!("{} {}", "Shell:".bold(), name)),
    }
}

/// Formats the CPU line, e.g. "CPU: Apple M5 Pro (6+12) @ 4.61 GHz", the frequency is only shown
/// when cpu_frequency is on
pub fn display_cpu(sys: &System, config: &Config) -> String {
    let mut line = format!("{} {}", "CPU:".bold(), get_cpu_name(sys));

    if let Some(cores) = get_cpu_core_counts().and_then(|counts| format_cpu_cores(&counts)) {
        line.push_str(&format!(" ({})", cores));
    }

    if config.display.cpu_frequency
        && let Some(frequency) = format_frequency(get_cpu_frequency(sys))
    {
        line.push_str(&format!(" @ {}", frequency));
    }

    line
}

/// Formats the RAM line, e.g. "RAM: 41.41 GB / 64 GB ██████▍░░░ 64%"
pub fn display_ram_usage(sys: &System, config: &Config) -> String {
    let (total, used, percentage) = get_ram_usage(sys);
    format!("{} {} / {} {}", "RAM:".bold(), used, total, format_usage(percentage, config))
}

pub fn display_swap_usage(sys: &System) -> String {
    let (total, used, percentage) = get_swap_usage(sys);
    if extract_numeric_value(&total).is_ok_and(|v| v == 0.0) {
        format!("{} Disabled", "Swap:".bold())
    } else {
        format!("{} {} / {} ({})", "Swap:".bold(), used, total, color_percentage(percentage))
    }
}

pub fn display_uptime() -> String {
    format!("{} {}", "Uptime:".bold(), get_uptime())
}

/// Formats the battery line, e.g. "Battery: 80% (Discharging)". While plugged in the brackets name
/// the adapter when ac_adapter is on (macOS), e.g. "Battery: 64% (Charging, 140W USB-C Power
/// Adapter)", see format_battery_status
pub fn display_battery(config: &Config) -> Option<String> {
    let (capacity, status) = platform::get_battery();
    if capacity == "Unavailable" || status == "Unavailable" {
        return None;
    }

    let adapter = config.display.ac_adapter.then(platform::get_ac_adapter).flatten();
    Some(format!(
        "{} {} ({})",
        "Battery:".bold(),
        color_percentage_inverse(capacity.parse::<f64>().unwrap_or(0.0)),
        format_battery_status(&status, adapter.as_deref())
    ))
}

pub fn display_power_draw() -> Option<String> {
    let power_draw = get_power_draw();
    if power_draw != 0 {
        Some(format!("{} {}W", "Power Draw:".bold(), power_draw))
    } else {
        None
    }
}

/// Formats the local IPv4 line, e.g. "Local IPv4 (en0): 192.168.1.20/24"
pub fn display_local_ipv4() -> Option<String> {
    get_local_ipv4().map(|(interface, address)| {
        format!("{} {}", format!("Local IPv4 ({}):", interface).bold(), address)
    })
}

/// Formats the local IPv6 line, e.g. "Local IPv6 (en0): 2001:db8::1/64", None when the primary
/// interface has no global IPv6 address
pub fn display_local_ipv6() -> Option<String> {
    get_local_ipv6().map(|(interface, address)| {
        format!("{} {}", format!("Local IPv6 ({}):", interface).bold(), address)
    })
}

/// Formats the Wi-Fi line, e.g. "Wi-Fi: SeenHealth IT", None when not connected to Wi-Fi (and on
/// Linux, where it isn't read yet)
pub fn display_wifi() -> Option<String> {
    get_wifi_ssid().map(|ssid| format!("{} {}", "Wi-Fi:".bold(), ssid))
}

/// Formats the Tailscale IP line, e.g. "Tailscale IP: 100.81.138.51", None when Tailscale is not
/// connected
pub fn display_tailscale_ip() -> Option<String> {
    get_tailscale_ipv4().map(|address| format!("{} {}", "Tailscale IP:".bold(), address))
}

/// Formats the Tailscale name line, e.g. "Tailscale Name: laptop" for the MagicDNS name
/// laptop.tailnet.ts.net, None when Tailscale is not connected or MagicDNS doesn't answer
pub fn display_tailscale_name() -> Option<String> {
    let host = get_tailscale_host(get_tailscale_ipv4()?)?;
    let name = get_tailscale_machine_name(&host)?;
    Some(format!("{} {}", "Tailscale Name:".bold(), name))
}

/// Formats the disk line, e.g. "Disk (/): 900GB / 994GB █████████░ 90%"
pub fn display_disk_usage(config: &Config) -> String {
    let (total, used, percentage) = platform::get_disk_usage();
    format!(
        "{} {}GB / {}GB {}",
        "Disk (/):".bold(), // FIXME: Shows "/" dir statically
        used,
        total,
        format_usage(percentage, config)
    )
}

pub fn display_gpu_name(cli: &Cli) -> Option<String> {
    platform::get_gpu_name(cli).map(|gpu_name| format!("{} {}", "GPU:".bold(), gpu_name))
}

/// Formats one Screen line per display, e.g. "Screen (Color LCD): 3456x2234 @ 120Hz". With a
/// single display the name is left out so the line stays "Screen: 3456x2234 @ 120Hz"
pub fn display_screens(config: &Config) -> Vec<String> {
    if !config.display.resolution && !config.display.refresh_rate {
        return Vec::new();
    }

    let screens = get_screens();
    let count = screens.len();

    screens
        .iter()
        .enumerate()
        .filter_map(|(index, screen)| {
            let resolution = config.display.resolution.then_some((screen.width, screen.height));
            let refresh_rate = screen.refresh_rate.filter(|_| config.display.refresh_rate);
            let value = format_screen(resolution, refresh_rate)?;
            let key = format!("{}:", format_screen_key(&screen.name, index, count));

            Some(format!("{} {}", key.bold(), value))
        })
        .collect()
}

pub fn display_identifier() -> Vec<String> {
    let mut host_name = String::from("");
    if let Some(host) = get_host_name() {
        // macOS reports the Bonjour name (Johns-MacBook-Pro.local), the .local suffix is just noise
        host_name = host.strip_suffix(".local").map(str::to_string).unwrap_or(host);
    }
    let username = get_username();

    let mut identifier = format!("{}@{}", username, host_name);
    let underline = "-".repeat(identifier.chars().count());

    // this may add overhead and i will try to find a better way,
    // though bolding everything up messes with the underline
    identifier = format!("{}", identifier.bold());

    vec![identifier, underline]
}
