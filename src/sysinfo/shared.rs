use std::ffi::CString;

use sysinfo::*;

use crate::{common::*, config::Config, platform};

const BYTES_TO_GB: u64 = 1_000_000_000;
const KIB_TO_MB: u64 = 1024;
const SECONDS_TO_HOURS: u64 = 3600;
const MINUTES_TO_HOURS: u64 = 60;

/// Creates a System variable once and refreshes features according to what is on in the config file
pub fn create_system(config: &Config) -> System {
    let mut sys = System::new();
    if config.display.ram || config.display.swap {
        sys.refresh_memory();
    }
    if config.display.cpu {
        // The first refresh always reads the CPU name, the frequency costs an extra IOKit lookup
        // (~0.25 ms on Apple Silicon) so only ask for it when it is displayed. CPU usage needs two
        // samples so it is never worth refreshing here
        let refresh_kind = if config.display.cpu_frequency {
            CpuRefreshKind::nothing().with_frequency()
        } else {
            CpuRefreshKind::nothing()
        };
        sys.refresh_cpu_specifics(refresh_kind);
    }
    sys
}

/// Gets RAM usage values and returns them as a formatted String alongside the usage percentage as
/// unsigned int. Returns 0 on all values in case any error occurs
pub fn get_ram_usage(sys: &System) -> (String, String, u64) {
    let total_kib = (sys.total_memory() / KIB_TO_MB) as f64;
    let used_kib = (sys.used_memory() / KIB_TO_MB) as f64;
    let percentage = get_percentage_from_part(used_kib, total_kib).unwrap_or(0);

    (
        convert_to_bytes(total_kib).unwrap_or(String::from("0 KiB")),
        convert_to_bytes(used_kib).unwrap_or(String::from("0 KiB")),
        percentage,
    )
}

/// Gets swap usage values and returns them as a formatted String alongside the usage percentage as
/// unsigned int. Returns 0 on all values in case any error occurs
pub fn get_swap_usage(sys: &System) -> (String, String, u64) {
    let total_kib = (sys.total_swap() / KIB_TO_MB) as f64;
    let used_kib = (sys.used_swap() / KIB_TO_MB) as f64;
    let percentage = get_percentage_from_part(used_kib, total_kib).unwrap_or(0);

    (
        convert_to_bytes(total_kib).unwrap_or(String::from("0 KiB")),
        convert_to_bytes(used_kib).unwrap_or(String::from("0 KiB")),
        percentage,
    )
}

/// Gets system uptime in HHh MMm SSs format
pub fn get_uptime() -> String {
    let uptime_seconds = System::uptime();

    let hours = uptime_seconds / SECONDS_TO_HOURS;
    let minutes = (uptime_seconds % SECONDS_TO_HOURS) / MINUTES_TO_HOURS;
    let seconds = uptime_seconds % MINUTES_TO_HOURS;

    if hours < 1 {
        format!("{:02}m {:02}s", minutes, seconds)
    } else {
        format!("{:02}h {:02}m {:02}s", hours, minutes, seconds)
    }
}

/// Gets the usage of the filesystem holding `directory` as (total GB, used GB, percentage floored).
/// One statfs call takes microseconds where sysinfo's disk list took ~30 ms on macOS, since it asks
/// every mounted volume for its purgeable space. Free space is what a user can still write (no
/// purgeable space or root reserve), the same number `df` and fastfetch show
pub fn get_directory_usage(directory: &str) -> (u64, u64, u64) {
    let space = CString::new(directory)
        .ok()
        .and_then(|path| platform::get_filesystem_space(&path));
    let Some((total, free)) = space else {
        return (0, 0, 0);
    };
    let used = total.saturating_sub(free);

    let percentage = get_percentage_from_part(used as f64, total as f64).unwrap_or(0);

    (total / BYTES_TO_GB, used / BYTES_TO_GB, percentage)
}

/// Gets os name on any given system
pub fn get_os_name() -> String {
    System::name().unwrap_or_else(|| String::from("Unknown"))
}

/// Gets the OS version on any given system, on macOS this is the product version (e.g. 26.7.1)
pub fn get_os_version() -> Option<String> {
    System::os_version()
}

/// Gets kernel version on any given system
pub fn get_kernel_version() -> String {
    System::kernel_version().unwrap_or_else(|| String::from("Unknown"))
}

/// Gets cpu name on any given system, filters out any comments such as x-core processor
pub fn get_cpu_name(sys: &System) -> String {
    sys.cpus()
        .first()
        .map(|cpu| {
            let full_name = cpu.brand();
            let mut end_pos = full_name.len();

            // cpu.brand() returns the full name of the CPU, for example:
            // "Ryzen 5 5600X 6-Core Processor"
            // We just find the first occurrence of any known suffix and get anything before it
            if let Some(pos) = full_name.find(" with ") {
                end_pos = end_pos.min(pos);
            }
            if let Some(pos) = full_name.find(" @ ") {
                end_pos = end_pos.min(pos);
            }

            // looks for "-Core" pattern like "6-Core Processor"
            if let Some(pos) = full_name.find("-Core")
                && let Some(space_pos) = full_name[.. pos].rfind(' ')
            {
                end_pos = end_pos.min(space_pos);
            }

            full_name[.. end_pos].trim().to_string()
        })
        .unwrap_or_else(|| String::from("Unknown CPU"))
}

/// Gets CPU frequency in MHz
pub fn get_cpu_frequency(sys: &System) -> u64 {
    sys.cpus().first().map(|cpu| cpu.frequency()).unwrap_or_else(|| 0)
}

/// Gets the CPU core counts, one entry per core type on hybrid CPUs (e.g. [6, 12] on an M5 Pro)
/// and the physical core count everywhere else
pub fn get_cpu_core_counts() -> Option<Vec<usize>> {
    platform::get_cpu_core_types()
        .or_else(|| System::physical_core_count().map(|count| vec![count]))
}

/// Gets the pretty version of the GPU through wgpu, this function is really slow (~45ms), so its
/// value is stored in the cache and only retrieved at first startup or if the system ids do not
/// match the ones in the cache. Linux only, macOS reads its GPU from IOKit
#[cfg(target_os = "linux")]
pub fn get_gpu_name_pretty() -> Option<String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());

    let adapters: Vec<wgpu::Adapter> =
        pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));

    if let Some(adapter) = adapters.into_iter().next() {
        let gpu_name = adapter.get_info().name;

        // Some AMD and Intel GPUs will return their name as "GPU_NAME (Some unneded stuff)"
        // For example "AMD Ryzen RX 580 Series (RADV POLARIS10)"
        // Here we're just truncating the string to get those parentheses out
        let mut end_pos = gpu_name.len();
        if let Some(parentheses_pos) = gpu_name.find("(") {
            end_pos = end_pos.min(parentheses_pos);
        }

        return Some((gpu_name[.. end_pos]).to_string());
    }

    None
}

/// A connected display, width and height are in physical pixels on macOS and as reported by
/// X11/Wayland elsewhere
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenInfo {
    pub name: String,
    pub width: u64,
    pub height: u64,
    /// None if the display reports 0 Hz, some older panels do
    pub refresh_rate: Option<u64>,
    pub is_primary: bool,
}

/// Gets every connected display with the primary one first, the rest keep the order the OS lists
/// them in. Returns an empty Vec if there is no display server (e.g. a TTY or SSH session)
pub fn get_screens() -> Vec<ScreenInfo> {
    let mut screens = platform::get_active_screens().unwrap_or_default();
    // sort_by_key is stable so only the primary display moves
    screens.sort_by_key(|screen| !screen.is_primary);
    screens
}

pub fn get_host_name() -> Option<String> {
    System::host_name()
}

pub fn get_username() -> String {
    std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
}
