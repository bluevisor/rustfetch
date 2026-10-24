use std::{
    ffi::{CStr, c_char, c_int, c_void},
    mem::{MaybeUninit, offset_of, size_of},
    net::Ipv6Addr,
    sync::OnceLock,
};

use super::find_elf_rodata_section;
use crate::{cli::Cli, sysinfo::*};

// <sys/sockio.h> and <net/route.h>, the libc crate binds neither
const SIOCGRTSIZE: libc::c_ulong = 8926;
const SIOCGRTTABLE: libc::c_ulong = 8927;
const RTF_DEFAULT: u32 = 0x80;
const IF_NAMESIZE: usize = 32;

// <OS.h> topology_level_type
const B_TOPOLOGY_ROOT: u32 = 1;
const B_TOPOLOGY_CORE: u32 = 3;
const B_TOPOLOGY_PACKAGE: u32 = 4;
// <OS.h> cpu_platform
const B_CPU_ARM_64: u32 = 7;

/// <OS.h> cpu_vendor names in enum order, the same strings Haiku's cpu_type.h prints. libc's
/// cpu_vendor enum stops at Fujitsu, so the topology is read with plain integers instead
const CPU_VENDORS: [&str; 30] = [
    "Unknown",
    "AMD",
    "Cyrix",
    "IDT",
    "Intel",
    "National Semiconductor",
    "Rise",
    "Transmeta",
    "VIA",
    "IBM",
    "Motorola",
    "NEC",
    "Hygon",
    "Sun",
    "Fujitsu",
    "ARM",
    "Broadcom",
    "Cavium",
    "DEC",
    "HiSilicon",
    "Infineon",
    "Freescale",
    "NVIDIA",
    "Applied Micro",
    "Qualcomm",
    "Marvell",
    "Apple",
    "Ampere",
    "Microsoft",
    "",
];

/// <OS.h> cpu_topology_node_info, with the enums as u32 so an unknown vendor is not UB
#[repr(C)]
#[derive(Clone, Copy)]
struct TopologyNode {
    id: u32,
    level_type: u32,
    level: u32,
    data: TopologyData,
}

#[repr(C)]
#[derive(Clone, Copy)]
union TopologyData {
    /// cpu_topology_root_info: the cpu_platform
    platform: u32,
    /// cpu_topology_package_info: (cpu_vendor, cache_line_size)
    package: [u32; 2],
    core: TopologyCore,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TopologyCore {
    /// MIDR_EL1 on arm64, the cpuid family/model on x86
    model: u32,
    /// In Hz, 0 when the firmware doesn't say (QEMU and Apple's hypervisor)
    default_frequency: u64,
}

unsafe extern "C" {
    fn get_cpu_topology_info(nodes: *mut TopologyNode, count: *mut u32) -> libc::status_t;
}

// haiku_graphics.cpp, built by build.rs, see Graphics
#[link(name = "rustfetch_haiku_graphics", kind = "static")]
#[link(name = "be")]
#[link(name = "stdc++")]
unsafe extern "C" {
    fn rustfetch_haiku_graphics(
        gpu: *mut c_char,
        gpu_size: usize,
        screen_name: *mut c_char,
        screen_name_size: usize,
        screen: *mut u32,
    ) -> c_int;
}

/// What the graphics shim read from app_server: the GPU (accelerant) name and the main screen
struct Graphics {
    gpu: Option<String>,
    screen: Option<ScreenInfo>,
}

/// Asks app_server once for both the GPU and the screen, since connecting (a BApplication) costs
/// ~2 ms and the screen query ~4 ms. Haiku drives a single screen
fn get_graphics() -> &'static Graphics {
    static GRAPHICS: OnceLock<Graphics> = OnceLock::new();
    GRAPHICS.get_or_init(|| {
        let mut gpu = [0 as c_char; 256];
        let mut screen_name = [0 as c_char; 256];
        // width, height, refresh rate in mHz
        let mut screen = [0u32; 3];
        // SAFETY: every buffer is valid for the size passed with it, the shim NUL terminates
        let found = unsafe {
            rustfetch_haiku_graphics(
                gpu.as_mut_ptr(),
                gpu.len(),
                screen_name.as_mut_ptr(),
                screen_name.len(),
                screen.as_mut_ptr(),
            )
        };
        // SAFETY: both buffers are NUL terminated, by the zero fill or the shim
        let gpu_name = unsafe { CStr::from_ptr(gpu.as_ptr()) }.to_string_lossy().into_owned();
        let screen_name =
            unsafe { CStr::from_ptr(screen_name.as_ptr()) }.to_string_lossy().into_owned();

        let [width, height, refresh_millihertz] = screen;
        Graphics {
            gpu: (found & 1 != 0 && !gpu_name.is_empty()).then_some(gpu_name),
            screen: (found & 2 != 0).then(|| ScreenInfo {
                name: screen_name,
                width: u64::from(width),
                height: u64::from(height),
                // Round so 59.94 Hz shows as 60Hz instead of 59Hz
                refresh_rate: (refresh_millihertz > 0)
                    .then(|| (u64::from(refresh_millihertz) + 500) / 1000),
                is_primary: true,
            }),
        }
    })
}

fn get_system_info() -> Option<libc::system_info> {
    // SAFETY: system_info is a plain C struct of integers and char arrays
    let mut info: libc::system_info = unsafe { std::mem::zeroed() };
    // SAFETY: info is a valid out pointer get_system_info fills in
    (unsafe { libc::get_system_info(&mut info) } == 0).then_some(info)
}

/// Gets uname(3)'s fields as (sysname, release, version), e.g. ("Haiku", "R1~beta6+development",
/// "hrev60290 Oct 22 2026 09:12:39")
fn get_uname() -> Option<(String, String, String)> {
    // SAFETY: utsname is a plain C struct of char arrays
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: name is a valid out pointer uname fills in
    if unsafe { libc::uname(&mut name) } != 0 {
        return None;
    }
    let field = |chars: &[c_char]| {
        // SAFETY: uname NUL terminates every field
        unsafe { CStr::from_ptr(chars.as_ptr()) }.to_string_lossy().into_owned()
    };
    Some((field(&name.sysname), field(&name.release), field(&name.version)))
}

pub fn get_distro_id() -> String {
    String::from("haiku")
}

/// Haiku's ACPI battery driver is not read yet, so the Battery line stays hidden
pub fn get_battery() -> (String, String) {
    (String::from("Unavailable"), String::from("Unavailable"))
}

pub fn get_ac_adapter() -> Option<String> {
    None
}

pub fn get_power_draw() -> u32 {
    0
}

/// The directory whose filesystem the Disk line shows: Haiku keeps the system and the user's files
/// on the boot volume, "/" is a small virtual root
pub const DISK_DIR: &str = "/boot";

pub fn get_disk_usage() -> (u64, u64, u64) {
    get_directory_usage(DISK_DIR)
}

/// Gets the (total, available) bytes of the filesystem holding `path` with one statvfs(3) call
pub fn get_filesystem_space(path: &CStr) -> Option<(u64, u64)> {
    // SAFETY: statvfs is a plain C struct of integers, all zeroes is a valid value
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: path is NUL terminated and stats is a valid out pointer libc fills in
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }

    // f_frsize is the unit f_blocks and f_bavail are counted in, Haiku's block counts are signed
    let block_size = u64::from(stats.f_frsize);
    Some((
        u64::try_from(stats.f_blocks).ok()?.checked_mul(block_size)?,
        u64::try_from(stats.f_bavail).ok()?.checked_mul(block_size)?,
    ))
}

/// Formats the release the way Haiku writes it, e.g. "Haiku R1/beta6+development" (uname's release
/// is the package version, where '~' stands for '/')
pub fn format_os_name() -> String {
    match get_uname() {
        Some((sysname, release, _)) => format!("{} {}", sysname, release.replace('~', "/")),
        None => String::from("Haiku"),
    }
}

/// The kernel has no version of its own, its build is the hrev (e.g. "Haiku hrev60290")
pub fn format_kernel_version() -> String {
    get_hrev().map_or_else(|| String::from("Haiku"), |hrev| format!("Haiku {}", hrev))
}

/// Gets the source revision the system was built from, e.g. "hrev60290"
pub fn get_hrev() -> Option<String> {
    let (_, _, version) = get_uname()?;
    version.split_whitespace().next().map(str::to_string)
}

pub fn read_host_name() -> Option<String> {
    let mut name = [0 as c_char; 256];
    // SAFETY: name is valid for its length, the last byte stays 0 so it is always NUL terminated
    if unsafe { libc::gethostname(name.as_mut_ptr(), name.len() - 1) } != 0 {
        return None;
    }
    // SAFETY: NUL terminated above
    let name = unsafe { CStr::from_ptr(name.as_ptr()) }.to_string_lossy().into_owned();
    (!name.is_empty()).then_some(name)
}

/// Seconds since boot, system_time() counts microseconds
pub fn get_uptime_seconds() -> u64 {
    // SAFETY: system_time has no preconditions
    u64::try_from(unsafe { libc::system_time() }).unwrap_or(0) / 1_000_000
}

/// Gets memory as (total RAM, used RAM, total swap, used swap) in bytes, used RAM counting the
/// same pages AboutSystem does
pub fn read_memory() -> (u64, u64, u64, u64) {
    let Some(info) = get_system_info() else {
        return (0, 0, 0, 0);
    };
    let page = libc::B_PAGE_SIZE as u64;
    (
        info.max_pages * page,
        info.used_pages * page,
        info.max_swap_pages * page,
        info.max_swap_pages.saturating_sub(info.free_swap_pages) * page,
    )
}

fn get_cpu_topology() -> Vec<TopologyNode> {
    let mut count = 0u32;
    // SAFETY: a null buffer asks for the node count only
    if unsafe { get_cpu_topology_info(std::ptr::null_mut(), &mut count) } != 0 || count == 0 {
        return Vec::new();
    }
    // SAFETY: TopologyNode is plain integers, all zeroes is a valid value
    let mut nodes = vec![unsafe { std::mem::zeroed::<TopologyNode>() }; count as usize];
    // SAFETY: nodes has room for count entries, count is updated to how many were written
    if unsafe { get_cpu_topology_info(nodes.as_mut_ptr(), &mut count) } != 0 {
        return Vec::new();
    }
    nodes.truncate(count as usize);
    nodes
}

/// Gets the CPU's name: the brand string on x86, the vendor and core (from MIDR_EL1) on arm64.
/// Apple's hypervisor hides the part number, so a VM on a Mac reads "Apple CPU"
pub fn read_cpu_name() -> Option<String> {
    #[cfg(target_arch = "x86_64")]
    if let Some(brand) = get_x86_brand_string() {
        return Some(brand);
    }

    let nodes = get_cpu_topology();
    let level = |level_type| nodes.iter().find(|node| node.level_type == level_type);
    // SAFETY: the node type says which union field the kernel filled in
    let platform = level(B_TOPOLOGY_ROOT).map(|node| unsafe { node.data.platform });
    let vendor = level(B_TOPOLOGY_PACKAGE).map(|node| unsafe { node.data.package[0] })?;
    let model = level(B_TOPOLOGY_CORE).map(|node| unsafe { node.data.core.model });

    let vendor_name = CPU_VENDORS.get(vendor as usize).copied().filter(|name| !name.is_empty())?;
    let part = model
        .filter(|_| platform == Some(B_CPU_ARM_64))
        .and_then(|midr| arm64_part_name(midr >> 24, (midr >> 4) & 0xfff));
    Some(match part {
        Some(part) => format!("{} {}", vendor_name, part),
        None => format!("{} CPU", vendor_name),
    })
}

#[cfg(target_arch = "x86_64")]
fn get_x86_brand_string() -> Option<String> {
    use std::arch::x86_64::__cpuid;

    // SAFETY: cpuid exists on every x86_64 CPU, leaf 0x80000000 gives the highest extended leaf
    if unsafe { __cpuid(0x8000_0000) }.eax < 0x8000_0004 {
        return None;
    }
    let mut bytes = Vec::with_capacity(48);
    for leaf in 0x8000_0002 ..= 0x8000_0004 {
        // SAFETY: the leaf is supported, checked above
        let regs = unsafe { __cpuid(leaf) };
        for reg in [regs.eax, regs.ebx, regs.ecx, regs.edx] {
            bytes.extend_from_slice(&reg.to_le_bytes());
        }
    }
    let brand = String::from_utf8_lossy(&bytes).trim_matches(char::from(0)).trim().to_string();
    (!brand.is_empty()).then_some(brand)
}

/// Names an arm64 core from its MIDR implementer and part number, the common ones from Haiku's
/// arch_cpu_type.h (itself from FreeBSD) plus Apple's M-series
pub fn arm64_part_name(implementer: u32, part: u32) -> Option<&'static str> {
    let name = match (implementer, part) {
        (0x41, 0xd03) => "Cortex-A53",
        (0x41, 0xd04) => "Cortex-A35",
        (0x41, 0xd05) => "Cortex-A55",
        (0x41, 0xd07) => "Cortex-A57",
        (0x41, 0xd08) => "Cortex-A72",
        (0x41, 0xd09) => "Cortex-A73",
        (0x41, 0xd0a) => "Cortex-A75",
        (0x41, 0xd0b) => "Cortex-A76",
        (0x41, 0xd0c) => "Neoverse N1",
        (0x41, 0xd0d) => "Cortex-A77",
        (0x41, 0xd40) => "Neoverse V1",
        (0x41, 0xd41) => "Cortex-A78",
        (0x41, 0xd44) => "Cortex-X1",
        (0x41, 0xd46) => "Cortex-A510",
        (0x41, 0xd47) => "Cortex-A710",
        (0x41, 0xd48) => "Cortex-X2",
        (0x41, 0xd49) => "Neoverse N2",
        (0x41, 0xd4f) => "Neoverse V2",
        (0x41, 0xd80) => "Cortex-A520",
        (0x41, 0xd81) => "Cortex-A720",
        (0x41, 0xd82) => "Cortex-X4",
        (0x61, 0x022 | 0x023) => "M1",
        (0x61, 0x024 | 0x025) => "M1 Pro",
        (0x61, 0x028 | 0x029) => "M1 Max",
        (0x61, 0x032 | 0x033) => "M2",
        (0x61, 0x034 | 0x035) => "M2 Pro",
        (0x61, 0x038 | 0x039) => "M2 Max",
        _ => return None,
    };
    Some(name)
}

/// Gets the CPU frequency in MHz, the current one when the kernel tracks it, otherwise the core's
/// default
pub fn read_cpu_frequency() -> u64 {
    // SAFETY: cpu_info is a plain C struct, all zeroes is a valid value
    let mut info: libc::cpu_info = unsafe { std::mem::zeroed() };
    // SAFETY: info has room for the one CPU asked for
    if unsafe { libc::get_cpu_info(0, 1, &mut info) } == 0 && info.current_frequency > 0 {
        return info.current_frequency / 1_000_000;
    }
    get_cpu_topology()
        .iter()
        .find(|node| node.level_type == B_TOPOLOGY_CORE)
        // SAFETY: a core node holds the core union field
        .map(|node| unsafe { node.data.core.default_frequency } / 1_000_000)
        .unwrap_or(0)
}

/// Haiku has no per-core-type count, the CPU line shows the number of cores (SMT threads not
/// counted), or of logical CPUs when the topology can't be read
pub fn get_cpu_core_types() -> Option<Vec<usize>> {
    let cores = get_cpu_topology()
        .iter()
        .filter(|node| node.level_type == B_TOPOLOGY_CORE)
        .count();
    let cores = match cores {
        0 => get_system_info()?.cpu_count as usize,
        cores => cores,
    };
    Some(vec![cores])
}

/// Gets the GPU as its accelerant names its chipset (e.g. "Virtio" or "Radeon HD 7850")
pub fn get_gpu_name(_cli: &Cli) -> Option<String> {
    get_graphics().gpu.clone()
}

/// Gets the main screen from app_server, the only one Haiku drives
pub fn get_active_screens() -> Option<Vec<ScreenInfo>> {
    get_graphics().screen.clone().map(|screen| vec![screen])
}

fn get_team_info(pid: u32) -> Option<libc::team_info> {
    // SAFETY: team_info is a plain C struct of integers and a char array
    let mut info: libc::team_info = unsafe { std::mem::zeroed() };
    // SAFETY: info is a valid out pointer get_team_info fills in
    (unsafe { libc::get_team_info(pid as libc::team_id, &mut info) } == 0).then_some(info)
}

/// Gets a process' parent pid and name as (ppid, name). Haiku's public team_info has no parent, so
/// it is only known for rustfetch itself, every other process reports 0 and ends the walk up
pub fn get_process_info(pid: u32) -> Option<(u32, String)> {
    let path = get_process_path(pid).or_else(|| get_process_argv0(pid))?;
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    let ppid = if pid == std::process::id() {
        std::os::unix::process::parent_id()
    } else {
        0
    };
    Some((ppid, name))
}

/// Gets the path of a process' executable, the image its team was started from
pub fn get_process_path(pid: u32) -> Option<String> {
    let mut cookie = 0i32;
    // image_info holds two function pointers, which can't be zeroed
    let mut image = MaybeUninit::<libc::image_info>::uninit();
    loop {
        // SAFETY: cookie and image are valid out pointers for the size passed
        let status = unsafe {
            libc::_get_next_image_info(
                pid as libc::team_id,
                &mut cookie,
                image.as_mut_ptr(),
                size_of::<libc::image_info>(),
            )
        };
        if status != 0 {
            return None;
        }
        // SAFETY: get_next_image_info filled the whole struct in
        let image = unsafe { image.assume_init_ref() };
        if image.image_type == libc::B_APP_IMAGE as c_int {
            // SAFETY: the kernel NUL terminates name
            let path = unsafe { CStr::from_ptr(image.name.as_ptr()) };
            return Some(path.to_string_lossy().into_owned());
        }
    }
}

/// Gets the name a process was started as (argv[0]), the first word of team_info's args, which
/// the kernel cuts at 64 bytes
pub fn get_process_argv0(pid: u32) -> Option<String> {
    let info = get_team_info(pid)?;
    // SAFETY: the kernel NUL terminates args
    let args = unsafe { CStr::from_ptr(info.args.as_ptr()) }.to_string_lossy().into_owned();
    args.split_whitespace().next().map(str::to_string)
}

/// Finds the section of an executable that holds its C string literals, Haiku binaries are ELF
pub fn find_string_section(read_at: impl Fn(u64, usize) -> Option<Vec<u8>>) -> Option<(u64, u64)> {
    find_elf_rodata_section(read_at)
}

/// <sys/sockio.h> ifconf, its union is read as the buffer pointer or the route table size
#[repr(C)]
union IfConfValue {
    buffer: *mut c_void,
    value: c_int,
}

#[repr(C)]
struct IfConf {
    length: c_int,
    value: IfConfValue,
}

/// <net/route.h> route_entry. The addresses point into the same buffer, right after the entry
#[repr(C)]
struct RouteEntry {
    destination: *const libc::sockaddr,
    mask: *const libc::sockaddr,
    gateway: *const libc::sockaddr,
    source: *const libc::sockaddr,
    flags: u32,
    mtu: u32,
}

/// Gets the name of the interface carrying the default route of an address family (e.g.
/// "/dev/net/ipro1000/0"), from the stack's route table like route(8) lists it
fn get_default_interface(family: c_int) -> Option<String> {
    // SAFETY: plain socket(2) call, the descriptor is closed below
    let socket = unsafe { libc::socket(family, libc::SOCK_DGRAM, 0) };
    if socket < 0 {
        return None;
    }
    let interface = read_route_table(socket).and_then(|table| find_default_route(&table));
    // SAFETY: socket is open and only closed here
    unsafe { libc::close(socket) };
    interface
}

fn read_route_table(socket: c_int) -> Option<Vec<u8>> {
    let mut config = IfConf {
        length: size_of::<c_int>() as c_int,
        value: IfConfValue { value: 0 },
    };
    // SAFETY: config is a valid ifconf, Haiku's ioctl takes the argument size after it
    if unsafe { libc::ioctl(socket, SIOCGRTSIZE, &mut config, size_of::<IfConf>()) } != 0 {
        return None;
    }
    // SAFETY: SIOCGRTSIZE returns the table size in the union's int
    let size = usize::try_from(unsafe { config.value.value }).ok().filter(|&size| size > 0)?;

    let mut table = vec![0u8; size];
    config.length = c_int::try_from(size).ok()?;
    config.value.buffer = table.as_mut_ptr().cast();
    // SAFETY: the buffer has room for the size given in length
    if unsafe { libc::ioctl(socket, SIOCGRTTABLE, &mut config, size_of::<IfConf>()) } != 0 {
        return None;
    }
    table.truncate(usize::try_from(config.length).ok()?.min(size));
    Some(table)
}

/// Walks a SIOCGRTTABLE buffer: each entry is an interface name, a route_entry and then the
/// destination, mask and gateway sockaddrs it points to
fn find_default_route(table: &[u8]) -> Option<String> {
    let range = table.as_ptr_range();
    // Gets the sa_len of an address the entry points to, after checking it lies in the table
    let address_size = |address: *const libc::sockaddr| -> Option<usize> {
        if address.is_null() {
            return Some(0);
        }
        let address = address.cast::<u8>();
        if !range.contains(&address) {
            return None;
        }
        // SAFETY: address points inside table, sa_len is its first byte
        Some(usize::from(unsafe { *address }))
    };

    let entry_size = IF_NAMESIZE + size_of::<RouteEntry>();
    let mut offset = 0;
    while offset + entry_size <= table.len() {
        let name = &table[offset .. offset + IF_NAMESIZE];
        // SAFETY: the entry lies inside table (checked by the loop condition)
        let route = unsafe {
            table.as_ptr().add(offset + IF_NAMESIZE).cast::<RouteEntry>().read_unaligned()
        };

        if route.flags & RTF_DEFAULT != 0 {
            let name = CStr::from_bytes_until_nul(name).ok()?.to_string_lossy().into_owned();
            return (!name.is_empty()).then_some(name);
        }

        offset += entry_size
            + address_size(route.destination)?
            + address_size(route.mask)?
            + address_size(route.gateway)?;
    }
    None
}

// Keep the layout assumptions the parser relies on in sight
const _: () = assert!(offset_of!(RouteEntry, flags) == 4 * size_of::<*const c_void>());

pub fn get_default_interface_ipv4() -> Option<String> {
    get_default_interface(libc::AF_INET)
}

pub fn get_default_interface_ipv6() -> Option<String> {
    get_default_interface(libc::AF_INET6)
}

/// Haiku's stack has no privacy addresses, every IPv6 address counts as stable
pub fn get_ipv6_address_state(_interface: &str, _address: &Ipv6Addr) -> Ipv6AddressState {
    Ipv6AddressState::Stable
}

/// Wi-Fi is only read on macOS for now, so the Wi-Fi line stays hidden
pub fn get_wifi_link(_interface: &str) -> Option<WifiLink> {
    None
}

/// See get_wifi_link, never called on Haiku
pub fn read_wifi_ssid(_interface: &str) -> Option<WifiSsid> {
    None
}
