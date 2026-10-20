// Only Linux caches its GPU name, macOS reads it from IOKit on every run
#[cfg(target_os = "linux")]
pub mod cache;
pub mod cli;
pub mod common;
pub mod config;
pub mod platform;
pub mod sysinfo;

use clap::Parser;

use crate::{
    cli::Cli,
    common::{get_logo_lines, parse_logo_color, print_logo},
    config::{load_all_config, load_config},
};

// TODO:
// Add CPU, GPU: temps, usage

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = if cli.all {
        load_all_config(&cli)
    } else {
        load_config(&cli)
    };
    let sys = sysinfo::create_system(&config);
    // The GPU cache (Linux) is rebuilt where it is read, the shell version cache is just dropped
    if cli.clear_cache {
        sysinfo::clear_shell_version_cache();
    }

    let distro_id = platform::get_distro_id();

    // Same order as fastfetch: system, shell, displays, hardware, network, then power
    let info_lines: Vec<String> = config
        .display
        .identifier
        .then(common::display_identifier)
        .into_iter()
        .flatten()
        .chain(
            vec![
                config.display.os.then(common::display_os),
                config.display.kernel.then(common::display_kernel),
                config.display.uptime.then(common::display_uptime),
                config.display.shell.then(common::display_shell).flatten(),
            ]
            .into_iter()
            .flatten(),
        )
        // One line per display, so it can't live in the Option lists
        .chain(
            config.display.screen.then(|| common::display_screens(&config)).into_iter().flatten(),
        )
        .chain(
            vec![
                config.display.cpu.then(|| common::display_cpu(&sys, &config)),
                config.display.gpu.then(|| common::display_gpu_name(&cli)).flatten(),
                config.display.ram.then(|| common::display_ram_usage(&sys)),
                config.display.swap.then(|| common::display_swap_usage(&sys)),
                config.display.disk.then(common::display_disk_usage),
                config.display.ipv4.then(common::display_local_ipv4).flatten(),
                config.display.ipv6.then(common::display_local_ipv6).flatten(),
                config.display.tailscale_ip.then(common::display_tailscale_ip).flatten(),
                config.display.tailscale_host.then(common::display_tailscale_host).flatten(),
                config.display.battery.then(common::display_battery).flatten(),
                #[cfg(target_os = "linux")]
                config.display.power_draw.then(common::display_power_draw).flatten(),
            ]
            .into_iter()
            .flatten(),
        )
        .collect();

    let logo_lines = get_logo_lines(&distro_id);

    let logo_color = parse_logo_color(&config.logo.color);

    print_logo(logo_lines, info_lines, &distro_id, logo_color, &cli)?;

    Ok(())
}
