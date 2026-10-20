use std::{ffi::CStr, fs, net::Ipv6Addr, path::Path};

use display_info::DisplayInfo;

use crate::{
    cache::{create_cache, get_cache},
    cli::Cli,
    common::*,
    sysinfo::*,
};

const BATTERY_CAPACITY_DIR: &str = "/sys/class/power_supply/BAT0/capacity";
const BATTERY_STATUS_DIR: &str = "/sys/class/power_supply/BAT0/status";
const BATTERY_POWER_DRAW_DIR: &str = "/sys/class/power_supply/BAT0/power_now";
const ROOT_DIR: &str = "/";
const ROUTE_IPV4_DIR: &str = "/proc/net/route";
const ROUTE_IPV6_DIR: &str = "/proc/net/ipv6_route";
const IF_INET6_DIR: &str = "/proc/net/if_inet6";

pub fn get_distro_id() -> String {
    fs::read_to_string(Path::new("/etc/os-release"))
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("ID="))
                .and_then(|line| line.split('=').nth(1))
                .map(|id| id.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Gets battery status as a tuple (Capacity, Status) if available
pub fn get_battery() -> (String, String) {
    let capacity =
        get_trimmed(Path::new(BATTERY_CAPACITY_DIR)).unwrap_or_else(|_| "Unavailable".to_string());
    let status =
        get_trimmed(Path::new(BATTERY_STATUS_DIR)).unwrap_or_else(|_| "Unavailable".to_string());
    (capacity, status)
}

/// The power_supply class doesn't name the adapter, so the Battery line only shows the status
pub fn get_ac_adapter() -> Option<String> {
    None
}

/// Gets current power draw and returns it as Watts - Only available on battery-powered devices
pub fn get_power_draw() -> u32 {
    match get_trimmed(Path::new(BATTERY_POWER_DRAW_DIR)) {
        // power_now contains the value in microwatts, we transform it in watts
        Ok(content) => content.parse::<u32>().unwrap_or(0) / 1_000_000,
        Err(_) => 0,
    }
}

pub fn get_disk_usage() -> (u64, u64, u64) {
    // Linux root directory
    get_directory_usage(ROOT_DIR)
}

/// Gets the (total, available) bytes of the filesystem holding `path` with one statvfs(3) call,
/// available being what a user can still write (the root reserve is left out)
// The fields are u64 on 64-bit targets but u32 on some 32-bit ones (a Pi's armv7), so u64::from is
// only a no-op on the former
#[allow(clippy::useless_conversion)]
pub fn get_filesystem_space(path: &CStr) -> Option<(u64, u64)> {
    // SAFETY: statvfs is a plain C struct of integers, all zeroes is a valid value
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: path is NUL terminated and stats is a valid out pointer libc fills in
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }

    // f_frsize is the unit f_blocks and f_bavail are counted in
    let block_size = u64::from(stats.f_frsize);
    Some((
        u64::from(stats.f_blocks).checked_mul(block_size)?,
        u64::from(stats.f_bavail).checked_mul(block_size)?,
    ))
}

pub fn format_os_name() -> String {
    get_os_name()
}

pub fn format_kernel_version() -> String {
    format!("Linux {}", get_kernel_version())
}

/// Gets gpu vendor and device ids and returns them as a tuple: (vendor, device)
pub fn get_gpu_ids() -> Option<(String, String)> {
    // On some systems the indexes might start at 1, so instead of iterating through every single
    // possible "card*" we try the first two which are the most likely
    let gpu_path = Path::new("/sys/class/drm/card0/device");
    let fallback_gpu_path = Path::new("/sys/class/drm/card1/device");

    let vendor = std::fs::read_to_string(gpu_path.join("vendor"))
        .or_else(|_| std::fs::read_to_string(fallback_gpu_path.join("vendor")))
        .ok()?
        .trim()
        .to_string();

    let device = std::fs::read_to_string(gpu_path.join("device"))
        .or_else(|_| std::fs::read_to_string(fallback_gpu_path.join("device")))
        .ok()?
        .trim()
        .to_string();

    Some((format_hex(&vendor), format_hex(&device)))
}

/// Gets subsystem IDs for the GPU, which are used to narrow down the possible names of the GPU
fn get_gpu_subsystem_ids() -> Option<(String, String)> {
    let gpu_path = Path::new("/sys/class/drm/card0/device");
    let fallback_gpu_path = Path::new("/sys/class/drm/card1/device");

    let subsystem_vendor = std::fs::read_to_string(gpu_path.join("subsystem_vendor"))
        .or_else(|_| std::fs::read_to_string(fallback_gpu_path.join("subsystem_vendor")))
        .ok()
        .map(|value| value.trim().to_string());

    let subsystem_device = std::fs::read_to_string(gpu_path.join("subsystem_device"))
        .or_else(|_| std::fs::read_to_string(fallback_gpu_path.join("subsystem_device")))
        .ok()
        .map(|value| value.trim().to_string());

    match (subsystem_vendor, subsystem_device) {
        (Some(subvendor), Some(subdevice)) => Some((subvendor, subdevice)),
        _ => None,
    }
}

/// Gets GPU family and possible names, returns them as string
pub fn get_gpu_name(cli: &Cli) -> Option<String> {
    // TODO: This function is pretty long and, while being significantly faster than WGPU
    // (45ms vs 3ms) it is also less accurate. Shorten it and add accuracy
    let (vendor_id, device_id) = get_gpu_ids()?;
    let subsystem_ids = get_gpu_subsystem_ids();

    let subsystem_ids = subsystem_ids
        .map(|(subvendor, subdevice)| (format_hex(&subvendor), format_hex(&subdevice)));

    if let Ok(cache) = get_cache(cli) {
        // again, this shouldn't be collapsed
        if cache.gpu_device_id == device_id && cache.gpu_vendor_id == vendor_id {
            return Some(cache.gpu_name_pretty);
        } else {
            let _ = create_cache();
        }
    }

    let pci_ids = std::fs::read_to_string("/usr/share/hwdata/pci.ids")
        .or_else(|_| std::fs::read_to_string("/usr/share/misc/pci.ids"))
        .ok()?;

    let mut current_vendor = None;
    let mut current_device = None;

    for line in pci_ids.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }

        // vendor lines start with no leading space
        if !line.starts_with('\t') && !line.starts_with(' ') {
            current_device = None;

            if let Some(vendor) = line.split_whitespace().next() {
                if vendor.eq_ignore_ascii_case(&vendor_id) {
                    current_vendor = Some(line.split_once("  ")?.1.trim());
                } else {
                    current_vendor = None;
                }
            }
        } else if current_vendor.is_some() && line.starts_with('\t') && !line.starts_with("\t\t") {
            if let Some(device) = line.split_whitespace().next() {
                if device.eq_ignore_ascii_case(&device_id) {
                    let name = line.split_once("  ")?.1.trim();
                    current_device = Some(name);
                } else {
                    current_device = None;
                }
            }
        } else if current_vendor.is_some() && current_device.is_some() && line.starts_with("\t\t") {
            // Even though clippy is complaining about it, this already long if statement should not
            // be collapsed. This comment prevents that
            if let Some((target_subvendor, target_subdevice)) = subsystem_ids.as_ref() {
                let trimmed = line.trim();
                let mut parts = trimmed.split_whitespace();
                let subvendor = parts.next()?;
                let subdevice = parts.next()?;

                if subvendor.eq_ignore_ascii_case(target_subvendor)
                    && subdevice.eq_ignore_ascii_case(target_subdevice)
                {
                    let mut split = trimmed.splitn(3, char::is_whitespace);
                    split.next()?;
                    split.next()?;
                    let subsystem_name = split.next()?.trim();
                    if !subsystem_name.is_empty() {
                        return Some(subsystem_name.to_string());
                    }
                }
            }
        }
    }

    Some(format!("{} {}", current_vendor?, current_device?))
}

/// Gets a process' parent pid and name as (ppid, name) from /proc/<pid>/stat, the name is the
/// kernel's comm (the executable's file name, cut at 15 characters)
pub fn get_process_info(pid: u32) -> Option<(u32, String)> {
    let stat = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    parse_proc_stat(&stat)
}

/// Gets the absolute path of a process' executable from /proc/<pid>/exe
pub fn get_process_path(pid: u32) -> Option<String> {
    let exe_link = format!("/proc/{}/exe", pid);
    let path = fs::read_link(&exe_link).ok()?.to_string_lossy().into_owned();

    // A package upgrade while the shell is running deletes the file the link points to, the link
    // itself still opens and runs the old binary so we use it instead
    if path.ends_with(" (deleted)") {
        return Some(exe_link);
    }
    Some(path)
}

/// Gets the name a process was started as (argv[0]) from /proc/<pid>/cmdline
pub fn get_process_argv0(pid: u32) -> Option<String> {
    let cmdline = fs::read(format!("/proc/{}/cmdline", pid)).ok()?;
    let argv0: Vec<u8> = cmdline.into_iter().take_while(|&b| b != 0).collect();
    String::from_utf8(argv0).ok().filter(|argv0| !argv0.is_empty())
}

/// Finds the section of an executable that holds its C string literals as (file offset, size), on
/// Linux the ELF .rodata section. For bash that is ~100KB instead of the whole ~900KB file
pub fn find_string_section(read_at: impl Fn(u64, usize) -> Option<Vec<u8>>) -> Option<(u64, u64)> {
    find_elf_rodata_section(read_at)
}

/// Finds the .rodata section of a little endian ELF binary (32 or 64-bit) as (file offset, size),
/// reading the file through read_at(offset, length)
pub fn find_elf_rodata_section(
    read_at: impl Fn(u64, usize) -> Option<Vec<u8>>,
) -> Option<(u64, u64)> {
    let header = read_at(0, 64)?;
    // Every architecture rustfetch runs on is little endian (EI_DATA = 1)
    if header.get(.. 4)? != b"\x7fELF" || *header.get(5)? != 1 {
        return None;
    }
    let is_64 = match *header.get(4)? {
        1 => false,
        2 => true,
        _ => return None,
    };

    let read_u16 = |data: &[u8], offset| read_array(data, offset).map(u16::from_le_bytes);
    let read_u32 = |data: &[u8], offset| read_array(data, offset).map(u32::from_le_bytes);
    let read_u64 = |data: &[u8], offset| read_array(data, offset).map(u64::from_le_bytes);

    // e_shoff, e_shentsize, e_shnum and e_shstrndx sit at different offsets in ELF32 and ELF64
    let (table_offset, fields) = if is_64 {
        (read_u64(&header, 0x28)?, 0x3a)
    } else {
        (u64::from(read_u32(&header, 0x20)?), 0x2e)
    };
    let entry_size = usize::from(read_u16(&header, fields)?);
    let entry_count = usize::from(read_u16(&header, fields + 2)?);
    let names_index = usize::from(read_u16(&header, fields + 4)?);
    let table = read_at(table_offset, entry_size.checked_mul(entry_count)?)?;

    // Gets a section header's (name offset, file offset, size)
    let section = |index: usize| -> Option<(usize, u64, u64)> {
        let start = index.checked_mul(entry_size)?;
        let name = usize::try_from(read_u32(&table, start)?).ok()?;
        if is_64 {
            Some((name, read_u64(&table, start + 24)?, read_u64(&table, start + 32)?))
        } else {
            let offset = u64::from(read_u32(&table, start + 16)?);
            Some((name, offset, u64::from(read_u32(&table, start + 20)?)))
        }
    };

    let (_, names_offset, names_size) = section(names_index)?;
    let names = read_at(names_offset, usize::try_from(names_size).ok()?)?;

    (0 .. entry_count)
        .filter_map(section)
        .find(|&(name, _, _)| names.get(name ..).is_some_and(|n| n.starts_with(b".rodata\0")))
        .map(|(_, offset, size)| (offset, size))
}

/// Linux has no cheap per-core-type count, so the CPU line falls back to the physical core count
pub fn get_cpu_core_types() -> Option<Vec<usize>> {
    None
}

/// Gets every display through display-info (X11 RandR or Wayland). Names are connector names such
/// as "eDP-1" or "HDMI-A-1", sizes are left as display-info reports them
pub fn get_active_screens() -> Option<Vec<ScreenInfo>> {
    let displays = DisplayInfo::all().ok()?;

    Some(
        displays
            .into_iter()
            .map(|display| ScreenInfo {
                name: display.friendly_name,
                width: display.width as u64,
                height: display.height as u64,
                // Round so 59.94 Hz shows as 60Hz instead of 59Hz
                refresh_rate: (display.frequency > 0.0).then(|| display.frequency.round() as u64),
                is_primary: display.is_primary,
            })
            .collect(),
    )
}

/// Gets the name of the interface carrying the IPv4 default route (e.g. "eth0")
pub fn get_default_interface_ipv4() -> Option<String> {
    parse_default_route_ipv4(&fs::read_to_string(ROUTE_IPV4_DIR).ok()?)
}

/// Gets the name of the interface carrying the IPv6 default route (e.g. "eth0")
pub fn get_default_interface_ipv6() -> Option<String> {
    parse_default_route_ipv6(&fs::read_to_string(ROUTE_IPV6_DIR).ok()?)
}

/// Gets how usable an IPv6 address of `interface` is from its kernel flags, so temporary (privacy)
/// and deprecated addresses can be skipped. Assumes a stable address if the flags can't be read
pub fn get_ipv6_address_state(interface: &str, address: &Ipv6Addr) -> Ipv6AddressState {
    fs::read_to_string(IF_INET6_DIR)
        .ok()
        .and_then(|content| parse_if_inet6_flags(&content, interface, address))
        .map(ipv6_state_from_linux_flags)
        .unwrap_or(Ipv6AddressState::Stable)
}

/// Wi-Fi is only read on macOS for now, so no interface counts as associated and the Wi-Fi line
/// stays hidden
pub fn get_wifi_link(_interface: &str) -> Option<WifiLink> {
    None
}

/// See get_wifi_link, never called on Linux
pub fn read_wifi_ssid(_interface: &str) -> Option<WifiSsid> {
    None
}
