use std::{
    ffi::{CStr, CString, c_void},
    mem::offset_of,
    net::{Ipv4Addr, Ipv6Addr},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr::NonNull,
    sync::OnceLock,
};

use objc2::{
    msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyClass, AnyObject},
};
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFNumber, CFRetained, CFString, CFType, kCFAllocatorDefault,
};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayCopyDisplayMode, CGDisplayIsBuiltin, CGDisplayIsMain,
    CGDisplayMode, CGDisplayModelNumber, CGError, CGGetOnlineDisplayList,
};
use objc2_io_kit::{
    IOIteratorNext, IOObjectConformsTo, IOObjectRelease, IORegistryEntryCreateCFProperty,
    IORegistryEntryFromPath, IORegistryEntryGetParentEntry, IOServiceGetMatchingService,
    IOServiceGetMatchingServices, IOServiceMatching, IOServiceNameMatching, io_object_t,
    kIOReturnSuccess, kIOServicePlane,
};

use crate::{cli::Cli, common::*, sysinfo::*};

/// Upper bound on listed displays, far more than any Mac can drive (display-info uses 16 too)
const MAX_DISPLAYS: usize = 16;

/// The name CoreDisplay gives every built-in panel, whatever kind it is
const BUILTIN_PANEL_NAME: &str = "Color LCD";
/// Product IDs (CGDisplayModelNumber) of the built-in Liquid Retina XDR panels, the same list
/// system_profiler checks before reporting "spdisplays_built-in-liquid-retina-xdr". No IOKit or
/// CoreDisplay property names the panel type, so newer panels have to be added here
const LIQUID_RETINA_XDR_MODELS: [u32; 14] = [
    0xa04e, 0xa04f, 0xa050, 0xa051, 0xa057, 0xa058, 0xa059, 0xa05a, 0xa05b, 0xa05c, 0xa05d, 0xa05e,
    0xa05f, 0xa061,
];
/// Product IDs of the built-in plain Liquid Retina panels (MacBook Air), system_profiler's other
/// list. Kept out of the 120 Hz fallback so one of them never shows up as an XDR
const LIQUID_RETINA_MODELS: [u32; 10] =
    [0xa052, 0xa053, 0xa054, 0xa055, 0xa062, 0xa063, 0xa064, 0xa065, 0xa066, 0xa067];
/// ProMotion's top refresh rate, only the Liquid Retina XDR MacBook Pros have it
const PROMOTION_REFRESH_RATE: u64 = 120;

/// CoreDisplay_DisplayCreateInfoDictionary is private API (fastfetch uses it too), it returns a +1
/// retained dictionary describing the display, or null
type CreateInfoDictionaryFn =
    unsafe extern "C" fn(CGDirectDisplayID) -> Option<NonNull<CFDictionary>>;

/// Mach-O magic numbers, fat headers are stored big endian and thin 64-bit headers little endian
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SEGMENT_64: u32 = 0x19;
/// Enough for a Mach-O header with all its load commands (/bin/zsh needs 1.8KB)
const MACHO_HEADER_READ_SIZE: usize = 32 * 1024;
/// proc_pidinfo flavor returning a proc_bsdshortinfo, which the libc crate does not bind
const PROC_PIDT_SHORTBSDINFO: libc::c_int = 13;

// Same as before
pub fn get_power_draw() -> i32 {
    0
}

pub fn get_disk_usage() -> (u64, u64, u64) {
    // MacOS root directory
    get_directory_usage("/")
}

/// Gets the (total, available) bytes of the filesystem holding `path` with one statfs(2) call,
/// available being what a user can still write. statfs rather than statvfs since Darwin's statvfs
/// counts blocks in 32 bits
pub fn get_filesystem_space(path: &CStr) -> Option<(u64, u64)> {
    // SAFETY: statfs is a plain C struct of integers and char arrays, all zeroes is a valid value
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: path is NUL terminated and stats is a valid out pointer the kernel fills in
    if unsafe { libc::statfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }

    let block_size = u64::from(stats.f_bsize);
    Some((stats.f_blocks.checked_mul(block_size)?, stats.f_bavail.checked_mul(block_size)?))
}

pub fn format_os_name() -> String {
    let Some(version) = get_os_version() else {
        return String::from("macOS");
    };

    match get_macos_codename(&version) {
        Some(codename) => format!("macOS {} {}", codename, version),
        None => format!("macOS {}", version),
    }
}

/// Maps a macOS product version to its marketing name, Apple doesn't expose it through any API so
/// new releases have to be added here
pub fn get_macos_codename(version: &str) -> Option<&'static str> {
    let mut parts = version.split('.');
    let major = parts.next()?;
    let minor = parts.next().unwrap_or("0");

    let codename = match (major, minor) {
        ("27", _) => "Golden Gate",
        ("26", _) => "Tahoe",
        ("15", _) => "Sequoia",
        ("14", _) => "Sonoma",
        ("13", _) => "Ventura",
        ("12", _) => "Monterey",
        // Big Sur identifies itself as 10.16 to binaries built with older SDKs
        ("11", _) | ("10", "16") => "Big Sur",
        ("10", "15") => "Catalina",
        ("10", "14") => "Mojave",
        ("10", "13") => "High Sierra",
        _ => return None,
    };
    Some(codename)
}

pub fn format_kernel_version() -> String {
    // The kernel is Darwin, its version (e.g. 25.6.0) is not the macOS version
    format!("Darwin {}", get_kernel_version())
}

pub fn get_distro_id() -> String {
    String::from("macos")
}

/// Gets the core count of each performance level, fastest first (e.g. [6, 12] for 6 Super and 12
/// Performance cores on an M5 Pro). Returns None on Intel Macs which only have one level
pub fn get_cpu_core_types() -> Option<Vec<usize>> {
    let levels = get_sysctl_u32(c"hw.nperflevels")?;
    if levels < 2 {
        return None;
    }

    (0 .. levels)
        .map(|level| {
            let name = CString::new(format!("hw.perflevel{}.physicalcpu", level)).ok()?;
            get_sysctl_u32(&name).map(|count| count as usize)
        })
        .collect()
}

/// Reads a 32-bit integer sysctl by name, returns None if this machine does not have it
fn get_sysctl_u32(name: &CStr) -> Option<u32> {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>();
    // SAFETY: name is NUL-terminated, value and size describe a writable u32 and newp is null so
    // nothing is written to the kernel
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut value).cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0 && size == size_of::<u32>()).then_some(value)
}

/// Gets every online display through CoreGraphics. Width and height are the mode's backing pixels
/// (3456x2234 for a 16" MacBook Pro running "looks like 1728x1117"), the same values fastfetch
/// prints. Names come from CoreDisplay since NSScreen would load AppKit for ~13 ms
pub fn get_active_screens() -> Option<Vec<ScreenInfo>> {
    let mut ids: [CGDirectDisplayID; MAX_DISPLAYS] = [0; MAX_DISPLAYS];
    let mut count: u32 = 0;
    // The online list, like fastfetch, also has sleeping displays and hardware mirrors that the
    // active list leaves out, e.g. when run over SSH while the screens are asleep
    // SAFETY: ids has room for MAX_DISPLAYS entries and count is a valid out pointer
    let error =
        unsafe { CGGetOnlineDisplayList(MAX_DISPLAYS as u32, ids.as_mut_ptr(), &mut count) };
    if error != CGError::Success {
        return None;
    }

    // Names are only shown to tell several displays apart (see format_screen_key), and the first
    // CoreDisplay call costs ~1.5 ms of setup, so a lone display skips it
    let create_info = if count > 1 {
        get_create_info_dictionary_fn()
    } else {
        None
    };
    let screens = ids
        .iter()
        .take(count as usize)
        .filter_map(|&id| {
            let mode = CGDisplayCopyDisplayMode(id)?;
            let refresh_rate = CGDisplayMode::refresh_rate(Some(&mode));
            // Round so 59.94 Hz shows as 60Hz instead of 59Hz
            let refresh_rate = (refresh_rate > 0.0).then(|| refresh_rate.round() as u64);
            let name = create_info
                .and_then(|f| get_display_name(f, id))
                .map(|name| get_panel_name(name, id, refresh_rate));

            Some(ScreenInfo {
                name: name.unwrap_or_default(),
                width: CGDisplayMode::pixel_width(Some(&mode)) as u64,
                height: CGDisplayMode::pixel_height(Some(&mode)) as u64,
                refresh_rate,
                is_primary: CGDisplayIsMain(id),
            })
        })
        .collect();

    Some(screens)
}

/// Looks CoreDisplay_DisplayCreateInfoDictionary up at runtime instead of linking it, so rustfetch
/// still launches (just without display names) if Apple ever removes it
fn get_create_info_dictionary_fn() -> Option<CreateInfoDictionaryFn> {
    // SAFETY: the name is NUL-terminated and RTLD_DEFAULT searches images that are already loaded,
    // CoreGraphics loads CoreDisplay at launch so no new library is opened
    let symbol = unsafe {
        libc::dlsym(libc::RTLD_DEFAULT, c"CoreDisplay_DisplayCreateInfoDictionary".as_ptr())
    };
    if symbol.is_null() {
        return None;
    }

    // SAFETY: the symbol is a C function taking a display id and returning a CFDictionaryRef, the
    // same signature fastfetch declares for it
    Some(unsafe { std::mem::transmute::<*mut c_void, CreateInfoDictionaryFn>(symbol) })
}

/// Gets a display's product name (e.g. "Color LCD" for the built-in panel or "S34CG50")
fn get_display_name(create_info: CreateInfoDictionaryFn, id: CGDirectDisplayID) -> Option<String> {
    // SAFETY: id comes from CGGetOnlineDisplayList and the function follows the Create rule, so we
    // own the returned dictionary and CFRetained releases it
    let info = unsafe { CFRetained::from_raw(create_info(id)?) };
    // SAFETY: the info dictionary is keyed by CFStrings, values are type-checked by downcast
    let info = unsafe { info.cast_unchecked::<CFString, CFType>() };

    let names = info
        .get(&CFString::from_static_str("DisplayProductName"))?
        .downcast::<CFDictionary>()
        .ok()?;
    // SAFETY: DisplayProductName maps locale identifiers (CFStrings) to the localized name
    let names = unsafe { names.cast_unchecked::<CFString, CFType>() };

    // Every locale has held the same name so far, prefer en_US and fall back to any of them
    let name = names
        .get(&CFString::from_static_str("en_US"))
        .or_else(|| names.to_vecs().1.into_iter().next())?;
    Some(name.downcast::<CFString>().ok()?.to_string())
}

/// Swaps CoreDisplay's generic "Color LCD" for "Liquid Retina XDR" on the MacBook Pros that have
/// one, like System Settings and system_profiler do. Every other display keeps its name
fn get_panel_name(name: String, id: CGDirectDisplayID, refresh_rate: Option<u64>) -> String {
    if name == BUILTIN_PANEL_NAME
        && CGDisplayIsBuiltin(id)
        && is_liquid_retina_xdr(CGDisplayModelNumber(id), refresh_rate)
    {
        String::from("Liquid Retina XDR")
    } else {
        name
    }
}

/// Checks if a built-in panel is a Liquid Retina XDR from its product ID, a panel newer than both
/// known lists still counts when it runs at ProMotion's 120 Hz. The current mode is used since
/// listing every mode to find the top rate costs ~3.5 ms
pub fn is_liquid_retina_xdr(model: u32, refresh_rate: Option<u64>) -> bool {
    LIQUID_RETINA_XDR_MODELS.contains(&model)
        || (!LIQUID_RETINA_MODELS.contains(&model)
            && refresh_rate.is_some_and(|rate| rate >= PROMOTION_REFRESH_RATE))
}

/// PCI vendor IDs as reported by the "vendor-id" property of a GPU
const VENDOR_APPLE: u32 = 0x106b;
const VENDOR_AMD: u32 = 0x1002;
const VENDOR_NVIDIA: u32 = 0x10de;

/// Owns an IOKit object handle and releases it when dropped, shared by the GPU and battery lookups
struct IoObject(io_object_t);

impl IoObject {
    /// Wraps a handle returned by IOKit, 0 (IO_OBJECT_NULL) means IOKit had nothing to return
    fn new(handle: io_object_t) -> Option<Self> {
        (handle != 0).then_some(Self(handle))
    }

    /// Copies one of the object's registry properties, None if it doesn't have it
    fn property(&self, key: &'static str) -> Option<CFRetained<CFType>> {
        let key = CFString::from_static_str(key);
        // SAFETY: self.0 is a live handle and key is a valid CFString. The property follows the
        // CF Create rule (+1 retain count), CFRetained takes ownership and releases it on drop
        unsafe { IORegistryEntryCreateCFProperty(self.0, Some(&key), kCFAllocatorDefault, 0) }
    }

    /// Reads a property as raw bytes
    fn data_property(&self, key: &'static str) -> Option<Vec<u8>> {
        self.property(key)?.downcast::<CFData>().ok().map(|data| data.to_vec())
    }

    /// Reads a text property, IOKit stores these as a CFString or as NUL terminated CFData
    fn string_property(&self, key: &'static str) -> Option<String> {
        let text = match self.property(key)?.downcast::<CFString>() {
            Ok(string) => string.to_string(),
            Err(property) => {
                let bytes = property.downcast::<CFData>().ok()?.to_vec();
                let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
                String::from_utf8_lossy(&bytes[.. end]).into_owned()
            },
        };
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// Reads an integer property, IOKit stores these as a CFNumber or as little endian CFData
    fn u32_property(&self, key: &'static str) -> Option<u32> {
        match self.property(key)?.downcast::<CFNumber>() {
            Ok(number) => number.as_i64().and_then(|value| u32::try_from(value).ok()),
            Err(property) => {
                let bytes = property.downcast::<CFData>().ok()?.to_vec();
                Some(u32::from_le_bytes(bytes.get(.. 4)?.try_into().ok()?))
            },
        }
    }

    /// Reads a signed integer property stored as a CFNumber
    fn i64_property(&self, key: &'static str) -> Option<i64> {
        self.property(key)?.downcast_ref::<CFNumber>()?.as_i64()
    }

    /// Reads a boolean property stored as a CFBoolean
    fn bool_property(&self, key: &'static str) -> Option<bool> {
        self.property(key)?.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool)
    }

    /// Checks if the object is an instance of the given IOKit class or one of its subclasses
    fn conforms_to(&self, class: &CStr) -> bool {
        // SAFETY: self.0 is a live handle and class is NUL terminated, IOKit only reads it
        unsafe { IOObjectConformsTo(self.0, class.as_ptr().cast_mut().cast()) }
    }

    /// Gets the object's provider in the IOService plane
    fn parent(&self) -> Option<IoObject> {
        let mut parent: io_object_t = 0;
        // SAFETY: self.0 is a live handle, the plane name is a NUL terminated constant and parent
        // is a valid out pointer. The parent comes back retained and IoObject releases it
        let result = unsafe {
            IORegistryEntryGetParentEntry(
                self.0,
                kIOServicePlane.as_ptr().cast_mut().cast(),
                &mut parent,
            )
        };
        if result != kIOReturnSuccess {
            return None;
        }
        IoObject::new(parent)
    }
}

impl Drop for IoObject {
    /// Releases the handle, IOKit objects are reference counted like CF objects
    fn drop(&mut self) {
        // nothing can be done if releasing fails, so the return code is ignored
        IOObjectRelease(self.0);
    }
}

/// Gets every GPU from the IORegistry (no wgpu, no system_profiler) and returns them as a single
/// line, e.g. "Apple M5 Pro (20 cores) @ 1.62 GHz"
pub fn get_gpu_name(_cli: &Cli) -> Option<String> {
    // IOKit takes well under 1ms, so unlike Linux there's no need to go through the cache
    let gpus: Vec<String> = get_accelerators()?.iter().filter_map(get_gpu_info).collect();
    (!gpus.is_empty()).then(|| gpus.join(", "))
}

/// Gets every IOAccelerator service, macOS registers one for each GPU that has a driver loaded
fn get_accelerators() -> Option<Vec<IoObject>> {
    // SAFETY: the class name is a NUL terminated constant
    let matching = unsafe { IOServiceMatching(c"IOAccelerator".as_ptr()) }?;
    // Same filter as fastfetch, only match the GPU's own accelerator service
    let category_key = CFString::from_static_str("IOMatchCategory");
    let category = CFString::from_static_str("IOAccelerator");
    // SAFETY: IOServiceMatching creates the dictionary with CFType callbacks and string keys, so
    // viewing it as a CFString -> CFString dictionary is valid. The dictionary retains both
    unsafe { matching.cast_unchecked::<CFString, CFString>() }.set(&category_key, &category);

    let mut iterator: io_object_t = 0;
    // SAFETY: the matching dictionary is valid and iterator is a valid out pointer. The function
    // consumes the reference passed to it, CFRetained::from retains first so `matching` can still
    // be released normally when it goes out of scope
    let result = unsafe {
        IOServiceGetMatchingServices(
            0, // MACH_PORT_NULL selects the default main port
            Some(CFRetained::<CFDictionary>::from(&*matching)),
            &mut iterator,
        )
    };
    if result != kIOReturnSuccess {
        return None;
    }
    let iterator = IoObject::new(iterator)?;

    Some(std::iter::from_fn(|| IoObject::new(IOIteratorNext(iterator.0))).collect())
}

/// Reads name, core count, max frequency and type of the GPU behind an IOAccelerator service
fn get_gpu_info(accelerator: &IoObject) -> Option<String> {
    let core_count = accelerator.u32_property("gpu-core-count");

    // Apple Silicon keeps "model" and "vendor-id" on the accelerator itself, Intel and AMD GPUs
    // keep them on the PCI device the accelerator is attached to
    let (name, vendor_id) = match accelerator.string_property("model") {
        Some(name) => (name, accelerator.u32_property("vendor-id")),
        None => {
            let pci_device = accelerator.parent().filter(|p| p.conforms_to(c"IOPCIDevice"))?;
            (pci_device.string_property("model")?, pci_device.u32_property("vendor-id"))
        },
    };

    let frequency_mhz = match vendor_id {
        Some(VENDOR_APPLE) => get_apple_gpu_max_frequency(),
        _ => None,
    };

    Some(format_gpu(&name, core_count, frequency_mhz, vendor_id.and_then(get_gpu_type)))
}

/// Gets the max GPU frequency in MHz of Apple Silicon from the power manager's voltage states,
/// same source as fastfetch and psutil
fn get_apple_gpu_max_frequency() -> Option<u32> {
    // The device tree path is ~4x faster than matching by name (~15µs vs ~65µs), name matching
    // stays as a fallback in case a future chip moves pmgr somewhere else
    // SAFETY: the path is a NUL terminated constant that IOKit only reads, the returned handle
    // (or 0) is retained and IoObject releases it
    let pmgr = IoObject::new(unsafe {
        IORegistryEntryFromPath(0, c"IODeviceTree:/arm-io/pmgr".as_ptr().cast_mut().cast())
    })
    .or_else(find_pmgr_by_name)?;
    // something else named "pmgr" wouldn't have the GPU voltage states
    if !pmgr.conforms_to(c"AppleARMIODevice") {
        return None;
    }
    parse_gpu_max_frequency(&pmgr.data_property("voltage-states9-sram")?)
}

/// Looks up the Apple Silicon power manager service by name
fn find_pmgr_by_name() -> Option<IoObject> {
    // SAFETY: the service name is a NUL terminated constant
    let matching = unsafe { IOServiceNameMatching(c"pmgr".as_ptr()) }?;
    // SAFETY: the matching dictionary is valid, the function consumes the reference passed to it
    // and returns a retained handle (or 0) that IoObject releases
    IoObject::new(unsafe {
        IOServiceGetMatchingService(0, Some(CFRetained::<CFDictionary>::from(&*matching)))
    })
}

/// Parses "voltage-states9-sram", a list of little endian u32 (frequency, voltage) pairs, and
/// returns the highest frequency in MHz
pub fn parse_gpu_max_frequency(voltage_states: &[u8]) -> Option<u32> {
    if voltage_states.is_empty() || !voltage_states.len().is_multiple_of(8) {
        return None;
    }

    let max = voltage_states
        .chunks_exact(8)
        .filter_map(|pair| pair.get(.. 4)?.try_into().ok().map(u32::from_le_bytes))
        .max()?;

    // Older chips store Hz and newer ones may store kHz, no GPU runs below 100 MHz so anything
    // above that has to be Hz (same heuristic as fastfetch)
    let mhz = if max > 100_000_000 {
        max / 1_000_000
    } else {
        max / 1_000
    };
    (mhz > 0).then_some(mhz)
}

/// Maps a PCI vendor ID to the tag shown after its GPU. Only discrete GPUs get one: integrated is
/// all Apple Silicon has, while Intel Macs with two GPUs still need them told apart
pub fn get_gpu_type(vendor_id: u32) -> Option<&'static str> {
    match vendor_id {
        VENDOR_AMD | VENDOR_NVIDIA => Some("Discrete"),
        _ => None,
    }
}

/// Formats a GPU, e.g. "Apple M5 Pro (20 cores) @ 1.62 GHz" or "AMD Radeon Pro 5500M
/// [Discrete]", missing parts are left out
pub fn format_gpu(
    name: &str,
    core_count: Option<u32>,
    frequency_mhz: Option<u32>,
    gpu_type: Option<&str>,
) -> String {
    let mut gpu = name.to_string();
    if let Some(cores) = core_count.filter(|&cores| cores > 0) {
        gpu.push_str(&format!(" ({} cores)", cores));
    }
    if let Some(mhz) = frequency_mhz.filter(|&mhz| mhz > 0) {
        if mhz >= 1000 {
            gpu.push_str(&format!(" @ {:.2} GHz", mhz as f64 / 1000.0));
        } else {
            gpu.push_str(&format!(" @ {} MHz", mhz));
        }
    }
    if let Some(gpu_type) = gpu_type {
        gpu.push_str(&format!(" [{}]", gpu_type));
    }
    gpu
}

const BATTERY_SERVICE: &CStr = c"AppleSmartBattery";

/// Gets battery status as a tuple (Capacity, Status) if available, both are "Unavailable" on
/// Macs without a battery so the line gets hidden
pub fn get_battery() -> (String, String) {
    get_smart_battery()
        .unwrap_or_else(|| (String::from("Unavailable"), String::from("Unavailable")))
}

/// Looks the AppleSmartBattery IOKit service up once, the battery status and the power adapter
/// shown in the Battery line both read it. None on desktops (Mac mini, Studio, Pro, iMac), which
/// don't publish the service at all
fn get_battery_service() -> Option<&'static IoObject> {
    // Kept for the whole run, the handle is released when the process exits
    static SERVICE: OnceLock<Option<IoObject>> = OnceLock::new();
    SERVICE
        .get_or_init(|| {
            // SAFETY: the class name is a NUL terminated constant
            let matching = unsafe { IOServiceMatching(BATTERY_SERVICE.as_ptr()) }?;
            // SAFETY: the matching dictionary is valid, the function consumes the reference
            // passed to it and returns a retained handle (or 0). It stays in the static for the
            // rest of the process and is never released, on purpose
            IoObject::new(unsafe {
                IOServiceGetMatchingService(0, Some(CFRetained::<CFDictionary>::from(&*matching)))
            })
        })
        .as_ref()
}

/// Reads capacity and status from the AppleSmartBattery IOKit service one key at a time, which is
/// ~7x cheaper than copying its whole property table (it holds large BatteryData dictionaries)
fn get_smart_battery() -> Option<(String, String)> {
    let battery = get_battery_service()?;

    // A MacBook running with its battery removed still publishes the service
    if battery.bool_property("BatteryInstalled") == Some(false) {
        return None;
    }

    let current = battery.i64_property("CurrentCapacity")?;
    let max = battery.i64_property("MaxCapacity")?;
    let capacity = battery_percentage(current, max)?;

    let status = battery_status(
        battery.bool_property("IsCharging").unwrap_or(false),
        battery.bool_property("ExternalConnected").unwrap_or(false),
        battery.bool_property("FullyCharged").unwrap_or(false),
    );

    Some((capacity.to_string(), String::from(status)))
}

/// Turns CurrentCapacity / MaxCapacity into a whole percentage. Apple Silicon already reports
/// both as a percentage (MaxCapacity is 100) while Intel reports them in mAh
pub fn battery_percentage(current: i64, max: i64) -> Option<u64> {
    if max <= 0 || current < 0 {
        return None;
    }

    // Rounded like fastfetch, and capped since Intel gauges can briefly report a charge above the
    // learned maximum. Checked since the values come straight from IOKit
    let percentage = current.checked_mul(100)?.checked_add(max / 2)? / max;
    u64::try_from(percentage.min(100)).ok()
}

/// Maps the AppleSmartBattery flags to a short status word like the Linux power_supply ones
pub fn battery_status(
    is_charging: bool,
    external_connected: bool,
    fully_charged: bool,
) -> &'static str {
    if is_charging {
        "Charging"
    } else if !external_connected {
        "Discharging"
    } else if fully_charged {
        "Full"
    } else {
        // Plugged in but held below 100% by the charge limit or Optimized Battery Charging
        "AC Connected"
    }
}

/// Gets the connected power adapter from AppleSmartBattery's AdapterDetails, e.g. "140W USB-C
/// Power Adapter", shown inside the Battery line. None on battery power and on Macs without a
/// battery
pub fn get_ac_adapter() -> Option<String> {
    let battery = get_battery_service()?;
    // AdapterDetails can outlive the adapter being unplugged
    if battery.bool_property("ExternalConnected") != Some(true) {
        return None;
    }

    let details = battery.property("AdapterDetails")?.downcast::<CFDictionary>().ok()?;
    // SAFETY: AdapterDetails is keyed by CFStrings, values are type-checked by downcast
    let details = unsafe { details.cast_unchecked::<CFString, CFType>() };
    let watts = details
        .get(&CFString::from_static_str("Watts"))?
        .downcast_ref::<CFNumber>()?
        .as_i64()?;
    let name = details
        .get(&CFString::from_static_str("Name"))
        .and_then(|name| name.downcast::<CFString>().ok())
        .map(|name| name.to_string());

    format_ac_adapter(watts, name.as_deref())
}

/// Formats a power adapter from its wattage and name, the name is left out of the brackets when
/// it already starts with the wattage. None when no wattage is reported
///
/// For example: (140, "140W USB-C Power Adapter") returns "140W USB-C Power Adapter" and
/// (96, "pd charger") returns "96W (pd charger)"
pub fn format_ac_adapter(watts: i64, name: Option<&str>) -> Option<String> {
    if watts <= 0 {
        return None;
    }

    let wattage = format!("{}W", watts);
    // The name comes from the adapter's own firmware, control characters could be escape codes
    let name: String = name.unwrap_or_default().chars().filter(|c| !c.is_control()).collect();
    let name = name.trim();

    Some(if name.is_empty() {
        wattage
    } else if name.starts_with(&wattage) {
        name.to_string()
    } else {
        format!("{} ({})", wattage, name)
    })
}

/// struct proc_bsdshortinfo from <sys/proc_info.h>, which the libc crate does not bind
#[repr(C)]
struct ProcBsdShortInfo {
    pbsi_pid: u32,
    pbsi_ppid: u32,
    pbsi_pgid: u32,
    pbsi_status: u32,
    pbsi_comm: [libc::c_char; libc::MAXCOMLEN],
    pbsi_flags: u32,
    pbsi_uid: libc::uid_t,
    pbsi_gid: libc::gid_t,
    pbsi_ruid: libc::uid_t,
    pbsi_rgid: libc::gid_t,
    pbsi_svuid: libc::uid_t,
    pbsi_svgid: libc::gid_t,
    pbsi_rfu: u32,
}

/// Gets a process' parent pid and name as (ppid, name) through proc_pidinfo, the name is the
/// executable's file name (not argv[0]) so it never carries a login shell's leading '-'. Uses the
/// short BSD info since the full one is refused (EPERM) for processes of other users, which would
/// stop the shell lookup at sudo, su or login
pub fn get_process_info(pid: u32) -> Option<(u32, String)> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let size = libc::c_int::try_from(size_of::<ProcBsdShortInfo>()).ok()?;

    // SAFETY: ProcBsdShortInfo is a plain C struct made of integers and a char array, all zeroes
    // is a valid value for it
    let mut info: ProcBsdShortInfo = unsafe { std::mem::zeroed() };

    // SAFETY: info is a live ProcBsdShortInfo and size is its exact size, the kernel writes at
    // most size bytes into it and returns how many it wrote (0 or less on error)
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDT_SHORTBSDINFO,
            0,
            (&raw mut info).cast::<libc::c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }

    // pbsi_comm is cut at 16 characters, which every shell name fits in
    Some((info.pbsi_ppid, c_chars_to_string(&info.pbsi_comm)?))
}

/// Gets the absolute path of a process' executable through proc_pidpath, symlinks are already
/// resolved (e.g. /opt/homebrew/bin/fish becomes /opt/homebrew/Cellar/fish/4.9.3/bin/fish)
pub fn get_process_path(pid: u32) -> Option<String> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];

    // SAFETY: buffer is valid for writes of buffer.len() bytes, which is the size we pass, and
    // proc_pidpath returns the number of bytes it wrote (0 or less on error)
    let length = unsafe {
        libc::proc_pidpath(pid, buffer.as_mut_ptr().cast::<libc::c_void>(), buffer.len() as u32)
    };
    let length = usize::try_from(length).ok().filter(|&length| length > 0)?;

    String::from_utf8(buffer.get(.. length)?.to_vec()).ok()
}

/// Gets the name a process was started as (argv[0]) through sysctl KERN_PROCARGS2. macOS' /bin/sh
/// re-executes /bin/bash so the process name says "bash" and only argv[0] still says "sh"
pub fn get_process_argv0(pid: u32) -> Option<String> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROCARGS2,
        libc::c_int::try_from(pid).ok()?,
    ];
    let mut size: libc::size_t = 0;

    // SAFETY: mib holds 3 valid ints, a null buffer asks the kernel only for the needed size
    let status = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0)
    };
    if status != 0 || size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size];
    // SAFETY: buffer is valid for writes of size bytes and the kernel updates size to how many it
    // actually wrote, which is never more than what we passed
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast::<libc::c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    buffer.truncate(size);
    parse_procargs_argv0(&buffer)
}

/// Parses KERN_PROCARGS2's layout: argc (4 bytes), the executable path, NUL padding, then the
/// NUL separated arguments of which we take the first
pub fn parse_procargs_argv0(procargs: &[u8]) -> Option<String> {
    let after_argc = procargs.get(4 ..)?;
    let path_end = after_argc.iter().position(|&b| b == 0)?;
    let rest = after_argc.get(path_end ..)?;
    let argv0_start = rest.iter().position(|&b| b != 0)?;
    let argv0: Vec<u8> =
        rest.get(argv0_start ..)?.iter().take_while(|&&b| b != 0).copied().collect();
    String::from_utf8(argv0).ok()
}

/// Finds the section of an executable that holds its C string literals as (file offset, size),
/// on macOS the Mach-O __TEXT,__cstring section. For /bin/zsh that is 25KB instead of the whole
/// 1.3MB universal binary
pub fn find_string_section(read_at: impl Fn(u64, usize) -> Option<Vec<u8>>) -> Option<(u64, u64)> {
    find_macho_cstring_section(read_at)
}

/// Finds the __TEXT,__cstring section of a 64-bit Mach-O binary as (file offset, size), reading
/// the file through read_at(offset, length). Universal binaries use their first architecture since
/// every slice carries the same strings
pub fn find_macho_cstring_section(
    read_at: impl Fn(u64, usize) -> Option<Vec<u8>>,
) -> Option<(u64, u64)> {
    let header = read_at(0, MACHO_HEADER_READ_SIZE)?;

    // fat_arch and fat_arch_64 both keep the slice offset 8 bytes in, right after the fat header
    let slice_offset = match u32::from_be_bytes(read_array(&header, 0)?) {
        FAT_MAGIC => u64::from(u32::from_be_bytes(read_array(&header, 16)?)),
        FAT_MAGIC_64 => u64::from_be_bytes(read_array(&header, 16)?),
        _ => 0,
    };
    let image = if slice_offset == 0 {
        header
    } else {
        read_at(slice_offset, MACHO_HEADER_READ_SIZE)?
    };

    let read_u32 = |offset: usize| read_array(&image, offset).map(u32::from_le_bytes);
    if read_u32(0)? != MH_MAGIC_64 {
        return None;
    }

    // mach_header_64 is 32 bytes with ncmds at 16, the load commands follow it
    let mut command_offset = 32;
    for _ in 0 .. read_u32(16)? {
        let command_size = usize::try_from(read_u32(command_offset + 4)?).ok()?;

        // segment_command_64 is 72 bytes with segname at 8 and nsects at 64, its section_64
        // entries (80 bytes each, sectname at 0, size at 40, file offset at 48) follow it
        if read_u32(command_offset)? == LC_SEGMENT_64
            && read_array::<16>(&image, command_offset + 8)? == name16(b"__TEXT")
        {
            for index in 0 .. usize::try_from(read_u32(command_offset + 64)?).ok()? {
                let section = command_offset + 72 + index * 80;
                if read_array::<16>(&image, section)? == name16(b"__cstring") {
                    let size = u64::from_le_bytes(read_array(&image, section + 40)?);
                    // section offsets are relative to the start of the architecture's slice
                    return Some((slice_offset + u64::from(read_u32(section + 48)?), size));
                }
            }
        }

        // a zero size command would loop forever on a corrupted file
        if command_size == 0 {
            return None;
        }
        command_offset += command_size;
    }
    None
}

/// Pads a Mach-O segment or section name to its fixed 16 byte field
fn name16(name: &[u8]) -> [u8; 16] {
    let mut field = [0u8; 16];
    for (slot, &byte) in field.iter_mut().zip(name) {
        *slot = byte;
    }
    field
}

/// Converts a NUL terminated C char array into a String, None if it is empty
fn c_chars_to_string(chars: &[libc::c_char]) -> Option<String> {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// ioctl that reads the IN6_IFF_* flags of one IPv6 address: _IOWR('i', 73, struct in6_ifreq) from
/// <netinet6/in6_var.h>, which the libc crate does not bind
pub const SIOCGIFAFLAG_IN6: libc::c_ulong = 0xc000_0000
    | ((size_of::<libc::in6_ifreq>() as libc::c_ulong & 0x1fff) << 16)
    | ((b'i' as libc::c_ulong) << 8)
    | 73;

// IN6_IFF_* address flags from <netinet6/in6_var.h>
const IN6_IFF_TENTATIVE: i32 = 0x0002;
const IN6_IFF_DUPLICATED: i32 = 0x0004;
const IN6_IFF_DETACHED: i32 = 0x0008;
const IN6_IFF_DEPRECATED: i32 = 0x0010;
const IN6_IFF_TEMPORARY: i32 = 0x0080;

/// Sequence number tagging our RTM_GET request so its reply can be told apart from other routing
/// messages the socket receives
const ROUTE_REQUEST_SEQ: i32 = 1;

/// Gets the name of the interface carrying the IPv4 default route (e.g. "en0")
pub fn get_default_interface_ipv4() -> Option<String> {
    let destination = libc::sockaddr_in {
        sin_len: size_of::<libc::sockaddr_in>() as u8,
        sin_family: libc::AF_INET as u8,
        sin_port: 0,
        sin_addr: libc::in_addr { s_addr: 0 },
        sin_zero: [0; 8],
    };
    get_route_interface(libc::AF_INET, destination)
}

/// Gets the name of the interface carrying the IPv6 default route (e.g. "en0")
pub fn get_default_interface_ipv6() -> Option<String> {
    let destination = libc::sockaddr_in6 {
        sin6_len: size_of::<libc::sockaddr_in6>() as u8,
        sin6_family: libc::AF_INET6 as u8,
        sin6_port: 0,
        sin6_flowinfo: 0,
        sin6_addr: libc::in6_addr { s6_addr: [0; 16] },
        sin6_scope_id: 0,
    };
    get_route_interface(libc::AF_INET6, destination)
}

/// Asks the routing table which interface a packet to `destination` (0.0.0.0 or ::) leaves
/// through, the same RTM_GET lookup `route -n get default` does over a PF_ROUTE socket. Only a
/// route through a gateway counts, so None means there is no default route for that family
fn get_route_interface<T: Copy>(family: libc::c_int, destination: T) -> Option<String> {
    let header_len = size_of::<libc::rt_msghdr>();
    let request_len = header_len + size_of::<T>();
    // Big enough for the header plus the RTAX_MAX sockaddrs a reply can carry
    let mut buffer = [0u8; 1024];

    // SAFETY: plain socket(2) call, the descriptor is handed to OwnedFd right away so it is closed
    // on every return path
    let raw_fd = unsafe { libc::socket(libc::PF_ROUTE, libc::SOCK_RAW, family) };
    if raw_fd < 0 {
        return None;
    }
    // SAFETY: raw_fd is a freshly opened descriptor nothing else owns
    let socket = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    // The kernel queues the reply before send() returns, the timeout only guards against a reply
    // that never arrives
    let timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: 100_000,
    };
    // SAFETY: SO_RCVTIMEO reads a timeval, the pointer and length describe `timeout`
    unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const timeout).cast(),
            size_of::<libc::timeval>() as libc::socklen_t,
        );
    }

    // SAFETY: getpid(2) cannot fail
    let pid = unsafe { libc::getpid() };
    // SAFETY: rt_msghdr is a plain C struct, all zeroes is a valid value
    let mut header: libc::rt_msghdr = unsafe { std::mem::zeroed() };
    header.rtm_msglen = request_len as u16;
    header.rtm_version = libc::RTM_VERSION as u8;
    header.rtm_type = libc::RTM_GET as u8;
    header.rtm_flags = libc::RTF_UP | libc::RTF_GATEWAY;
    // RTA_IFP asks the kernel to append the outgoing interface (a sockaddr_dl) to the reply
    header.rtm_addrs = libc::RTA_DST | libc::RTA_IFP;
    header.rtm_pid = pid;
    header.rtm_seq = ROUTE_REQUEST_SEQ;

    // SAFETY: the buffer is far larger than header_len + size_of::<T>(), and write_unaligned has
    // no alignment requirement on the u8 buffer
    unsafe {
        std::ptr::write_unaligned(buffer.as_mut_ptr().cast::<libc::rt_msghdr>(), header);
        std::ptr::write_unaligned(buffer.as_mut_ptr().add(header_len).cast::<T>(), destination);
    }

    // SAFETY: sends the first request_len bytes of the buffer, which are initialized above
    let sent = unsafe { libc::send(socket.as_raw_fd(), buffer.as_ptr().cast(), request_len, 0) };
    // The write itself fails (ESRCH) when there is no route at all for this family
    if sent != request_len as isize {
        return None;
    }

    // The socket also receives routing messages from other processes, skip anything that is not
    // the reply to our request
    for _ in 0 .. 16 {
        // SAFETY: recv writes at most buffer.len() bytes into the buffer
        let received =
            unsafe { libc::recv(socket.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        // Errors (including the timeout) are negative, a short read can't hold a header
        if received < header_len as isize {
            return None;
        }

        // SAFETY: at least header_len bytes were received, read_unaligned has no alignment needs
        let reply = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<libc::rt_msghdr>()) };
        if reply.rtm_pid != pid || reply.rtm_seq != ROUTE_REQUEST_SEQ {
            continue;
        }

        let gateway_route = libc::RTF_UP | libc::RTF_GATEWAY;
        if reply.rtm_errno != 0 || reply.rtm_flags & gateway_route != gateway_route {
            return None;
        }

        let end = (reply.rtm_msglen as usize).min(received as usize);
        let addresses = buffer.get(header_len .. end)?;
        let interface = find_route_sockaddr(reply.rtm_addrs, addresses, libc::RTA_IFP)?;
        return interface_name_from_sockaddr_dl(interface);
    }
    None
}

/// Finds the sockaddr selected by `wanted` (one RTA_* bit) in the address area that follows a
/// routing message header. There is one sockaddr for every bit set in `addrs`, in bit order, each
/// padded to a multiple of 4 bytes (an empty one still takes 4)
pub fn find_route_sockaddr(addrs: i32, data: &[u8], wanted: i32) -> Option<&[u8]> {
    let mut offset = 0;
    for bit in 0 .. libc::RTAX_MAX {
        let flag = 1 << bit;
        if addrs & flag == 0 {
            continue;
        }

        let len = *data.get(offset)? as usize;
        if flag == wanted {
            return data.get(offset .. offset + len);
        }
        // Same rounding as the ROUNDUP macro in Apple's network_cmds
        offset += if len == 0 { 4 } else { len.next_multiple_of(4) };
    }
    None
}

/// Reads the interface name out of a sockaddr_dl (link-level address), like the RTA_IFP entry of
/// a route reply. The name sits at the start of sdl_data and is not NUL terminated
pub fn interface_name_from_sockaddr_dl(sockaddr: &[u8]) -> Option<String> {
    let family = *sockaddr.get(offset_of!(libc::sockaddr_dl, sdl_family))?;
    let name_len = *sockaddr.get(offset_of!(libc::sockaddr_dl, sdl_nlen))? as usize;
    if family != libc::AF_LINK as u8 || name_len == 0 || name_len > libc::IFNAMSIZ {
        return None;
    }

    let name_start = offset_of!(libc::sockaddr_dl, sdl_data);
    let name = sockaddr.get(name_start .. name_start + name_len)?;
    Some(String::from_utf8_lossy(name).into_owned())
}

/// Gets how usable an IPv6 address of `interface` is from its kernel flags, so temporary (privacy)
/// and deprecated addresses can be skipped. Assumes a stable address if the flags can't be read
pub fn get_ipv6_address_state(interface: &str, address: &Ipv6Addr) -> Ipv6AddressState {
    get_ipv6_address_flags(interface, address)
        .map(ipv6_state_from_darwin_flags)
        .unwrap_or(Ipv6AddressState::Stable)
}

/// Maps the IN6_IFF_* flags of an IPv6 address to an address state
pub fn ipv6_state_from_darwin_flags(flags: i32) -> Ipv6AddressState {
    let unusable = IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED | IN6_IFF_DETACHED | IN6_IFF_DEPRECATED;
    if flags & unusable != 0 {
        Ipv6AddressState::Unusable
    } else if flags & IN6_IFF_TEMPORARY != 0 {
        Ipv6AddressState::Temporary
    } else {
        Ipv6AddressState::Stable
    }
}

/// Reads the IN6_IFF_* flags of one IPv6 address of `interface` with the SIOCGIFAFLAG_IN6 ioctl,
/// the same flags `ifconfig` prints as "temporary", "deprecated", "secured"...
fn get_ipv6_address_flags(interface: &str, address: &Ipv6Addr) -> Option<i32> {
    let name = interface.as_bytes();
    // The name must fit in ifr_name with its NUL terminator
    if name.len() >= libc::IFNAMSIZ {
        return None;
    }

    // SAFETY: plain socket(2) call, the descriptor is handed to OwnedFd right away so it is closed
    // on every return path
    let raw_fd = unsafe { libc::socket(libc::AF_INET6, libc::SOCK_DGRAM, 0) };
    if raw_fd < 0 {
        return None;
    }
    // SAFETY: raw_fd is a freshly opened descriptor nothing else owns
    let socket = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    // SAFETY: in6_ifreq is a plain C struct (a name and a union of plain fields), all zeroes is a
    // valid value and leaves ifr_name NUL terminated
    let mut request: libc::in6_ifreq = unsafe { std::mem::zeroed() };
    for (dst, src) in request.ifr_name.iter_mut().zip(name) {
        *dst = *src as libc::c_char;
    }
    request.ifr_ifru.ifru_addr = libc::sockaddr_in6 {
        sin6_len: size_of::<libc::sockaddr_in6>() as u8,
        sin6_family: libc::AF_INET6 as u8,
        sin6_port: 0,
        sin6_flowinfo: 0,
        sin6_addr: libc::in6_addr {
            s6_addr: address.octets(),
        },
        sin6_scope_id: 0,
    };

    // SAFETY: SIOCGIFAFLAG_IN6 reads and writes exactly one in6_ifreq, which `request` is
    if unsafe { libc::ioctl(socket.as_raw_fd(), SIOCGIFAFLAG_IN6, &raw mut request) } != 0 {
        return None;
    }
    // SAFETY: on success the kernel stores the address flags in the ifru_flags6 member
    Some(unsafe { request.ifr_ifru.ifru_flags6 })
}

/// ioctl that reads an interface's media type and link status: _IOWR('i', 56, struct ifmediareq)
/// from <sys/sockio.h>, which the libc crate does not bind
pub const SIOCGIFMEDIA: libc::c_ulong = 0xc000_0000
    | ((size_of::<IfMediaRequest>() as libc::c_ulong & 0x1fff) << 16)
    | ((b'i' as libc::c_ulong) << 8)
    | 56;

// <net/if_media.h>: the network type bits of a media word, 802.11's type and the link status bits
const IFM_NMASK: libc::c_int = 0xe0;
const IFM_IEEE80211: libc::c_int = 0x80;
const IFM_AVALID: libc::c_int = 0x1;
const IFM_ACTIVE: libc::c_int = 0x2;

/// struct ifmediareq from <net/if.h>, which packs it to 4 bytes (44, not 48)
#[repr(C, packed(4))]
struct IfMediaRequest {
    ifm_name: [libc::c_char; libc::IFNAMSIZ],
    ifm_current: libc::c_int,
    ifm_mask: libc::c_int,
    ifm_status: libc::c_int,
    ifm_active: libc::c_int,
    ifm_count: libc::c_int,
    ifm_ulist: *mut libc::c_int,
}

/// CoreWLAN, opened only when configd's scan record has no name: linking it would make every run
/// pay for loading it
const COREWLAN_PATH: &CStr = c"/System/Library/Frameworks/CoreWLAN.framework/CoreWLAN";
/// kCWInterfaceModeStation from <CoreWLAN/CoreWLANTypes.h>, a client joined to an access point
const CW_INTERFACE_MODE_STATION: isize = 1;

// SystemConfiguration is already loaded at launch (CoreGraphics depends on it), so linking it adds
// nothing to the startup time
#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    /// Copies one value of configd's dynamic store (what `scutil` shows), a null store opens a
    /// temporary session. Follows the CF Copy rule
    fn SCDynamicStoreCopyValue(store: *const c_void, key: &CFString) -> Option<NonNull<CFType>>;
}

/// Gets what identifies the network `interface` is on (see WifiLink), None when it isn't a Wi-Fi
/// interface associated with one
pub fn get_wifi_link(interface: &str) -> Option<WifiLink> {
    if !is_wifi_associated(interface) {
        return None;
    }

    let index = get_interface_index(interface);
    Some(WifiLink {
        interface: interface.to_string(),
        mac: index.and_then(get_interface_mac),
        router: index.and_then(get_interface_router),
    })
}

/// Checks if `interface` is a Wi-Fi interface associated with a network, from its media status
/// (what `ifconfig` prints as "status: active"). One ioctl, where CoreWLAN takes milliseconds
pub fn is_wifi_associated(interface: &str) -> bool {
    get_interface_media(interface)
        .is_some_and(|(active, status)| is_associated_wifi_media(active, status))
}

/// Checks SIOCGIFMEDIA's active media word and status for an 802.11 link that is up. AWDL and the
/// low latency WLAN interface are 802.11 too, but have no IPv4 address so are never asked about
pub fn is_associated_wifi_media(active: i32, status: i32) -> bool {
    let link_up = IFM_AVALID | IFM_ACTIVE;
    active & IFM_NMASK == IFM_IEEE80211 && status & link_up == link_up
}

/// Reads an interface's (active media word, status) with the SIOCGIFMEDIA ioctl
fn get_interface_media(interface: &str) -> Option<(i32, i32)> {
    let name = interface.as_bytes();
    // The name must fit in ifm_name with its NUL terminator
    if name.len() >= libc::IFNAMSIZ {
        return None;
    }

    // SAFETY: plain socket(2) call, the descriptor is handed to OwnedFd right away so it is closed
    // on every return path
    let raw_fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
    if raw_fd < 0 {
        return None;
    }
    // SAFETY: raw_fd is a freshly opened descriptor nothing else owns
    let socket = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    // SAFETY: IfMediaRequest is a plain C struct of integers, a char array and a pointer, all
    // zeroes is a valid value. A null ifm_ulist with ifm_count 0 asks for no media list
    let mut request: IfMediaRequest = unsafe { std::mem::zeroed() };
    for (dst, src) in request.ifm_name.iter_mut().zip(name) {
        *dst = *src as libc::c_char;
    }

    // SAFETY: SIOCGIFMEDIA reads and writes exactly one ifmediareq, which `request` is
    if unsafe { libc::ioctl(socket.as_raw_fd(), SIOCGIFMEDIA, &raw mut request) } != 0 {
        return None;
    }
    Some((request.ifm_active, request.ifm_status))
}

/// Gets an interface's index, the one routing messages refer to it by
fn get_interface_index(interface: &str) -> Option<u16> {
    let name = CString::new(interface).ok()?;
    // SAFETY: name is NUL terminated, if_nametoindex returns 0 for an unknown interface
    let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
    u16::try_from(index).ok().filter(|&index| index != 0)
}

/// Gets an interface's MAC address from the RTM_IFINFO message NET_RT_IFLIST returns for it, its
/// sockaddr_dl follows the if_msghdr
fn get_interface_mac(index: u16) -> Option<[u8; 6]> {
    let mib = [
        libc::CTL_NET,
        libc::PF_ROUTE,
        0,
        libc::AF_LINK,
        libc::NET_RT_IFLIST,
        libc::c_int::from(index),
    ];
    let list = read_route_sysctl(mib)?;

    let header_len = size_of::<libc::if_msghdr>();
    if list.len() < header_len {
        return None;
    }
    // SAFETY: the list holds at least header_len bytes, read_unaligned has no alignment needs
    let header = unsafe { std::ptr::read_unaligned(list.as_ptr().cast::<libc::if_msghdr>()) };
    if libc::c_int::from(header.ifm_type) != libc::RTM_IFINFO
        || header.ifm_addrs & libc::RTA_IFP == 0
    {
        return None;
    }
    mac_from_sockaddr_dl(list.get(header_len .. usize::from(header.ifm_msglen))?)
}

/// Gets the router of the interface with this index as (IPv4 address, MAC address): the gateway
/// of its default route, then that gateway's ARP entry. NET_RT_FLAGS lists only the routes with
/// the flag asked for (what `arp -a` reads), so a VPN pushing thousands of routes costs nothing
fn get_interface_router(index: u16) -> Option<(Ipv4Addr, [u8; 6])> {
    let routes_with = |flag| {
        let mib = [
            libc::CTL_NET,
            libc::PF_ROUTE,
            0,
            libc::AF_INET,
            libc::NET_RT_FLAGS,
            flag,
        ];
        read_route_sysctl(mib)
    };
    let router = find_default_gateway(&routes_with(libc::RTF_GATEWAY)?, index)?;
    let mac = find_arp_entry(&routes_with(libc::RTF_LLINFO)?, index, router)?;
    Some((router, mac))
}

/// Reads one of the routing table sysctls (NET_RT_FLAGS, NET_RT_IFLIST) into a buffer, asking for
/// its size first. The table can grow in between, so a buffer that ends up too small (ENOMEM) is
/// tried again
fn read_route_sysctl(mut mib: [libc::c_int; 6]) -> Option<Vec<u8>> {
    for _ in 0 .. 3 {
        let mut size: libc::size_t = 0;
        // SAFETY: mib holds 6 valid ints, a null buffer asks the kernel only for the needed size
        let status = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != 0 {
            return None;
        }

        // Room for a few routes (an ARP entry...) added before the second call
        size += 1024;
        let mut buffer = vec![0u8; size];
        // SAFETY: buffer is valid for writes of size bytes and the kernel updates size to how many
        // it actually wrote, which is never more than what we passed
        let status = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if status == 0 {
            buffer.truncate(size);
            return Some(buffer);
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return None;
        }
    }
    None
}

/// Lists the IPv4 routes through the interface with this index in a routing table dump as (flags,
/// destination, netmask, gateway)
fn interface_routes(
    dump: &[u8],
    index: u16,
) -> impl Iterator<Item = (libc::c_int, Ipv4Addr, &[u8], &[u8])> {
    route_messages(dump)
        .filter(move |(header, _)| header.rtm_index == index)
        .filter_map(|(header, addresses)| {
            let sockaddr = |wanted| find_route_sockaddr(header.rtm_addrs, addresses, wanted);
            let destination = sockaddr_in_address(sockaddr(libc::RTA_DST)?)?;
            // A missing or empty netmask is /0
            let netmask = sockaddr(libc::RTA_NETMASK).unwrap_or_default();
            Some((header.rtm_flags, destination, netmask, sockaddr(libc::RTA_GATEWAY)?))
        })
}

/// Finds the gateway of an interface's default route in a dump of the IPv4 routes (the RTF_GATEWAY
/// ones or all of them). macOS keeps a default route scoped to every interface that has a router,
/// so this also works for Wi-Fi while Ethernet carries the main default route
pub fn find_default_gateway(dump: &[u8], index: u16) -> Option<Ipv4Addr> {
    interface_routes(dump, index).find_map(|(flags, destination, netmask, gateway)| {
        // VPNs' 0.0.0.0/1 halves start at 0.0.0.0 too, but have a netmask
        let is_default = flags & libc::RTF_GATEWAY != 0
            && destination.is_unspecified()
            && netmask.iter().skip(4).all(|&byte| byte == 0);
        is_default.then(|| sockaddr_in_address(gateway)).flatten()
    })
}

/// Finds the MAC address of `address` in a dump of an interface's ARP (RTF_LLINFO) entries, None
/// while it isn't resolved
pub fn find_arp_entry(dump: &[u8], index: u16, address: Ipv4Addr) -> Option<[u8; 6]> {
    interface_routes(dump, index).find_map(|(flags, destination, _, gateway)| {
        let is_entry = flags & libc::RTF_LLINFO != 0 && destination == address;
        is_entry.then(|| mac_from_sockaddr_dl(gateway)).flatten()
    })
}

/// Splits a routing table dump into its messages as (header, the sockaddrs that follow it)
fn route_messages(dump: &[u8]) -> impl Iterator<Item = (libc::rt_msghdr, &[u8])> {
    let header_len = size_of::<libc::rt_msghdr>();
    let mut offset = 0;
    std::iter::from_fn(move || {
        let message = dump.get(offset ..).filter(|message| message.len() >= header_len)?;
        // SAFETY: at least header_len bytes are left, read_unaligned has no alignment needs
        let header =
            unsafe { std::ptr::read_unaligned(message.as_ptr().cast::<libc::rt_msghdr>()) };
        let length = usize::from(header.rtm_msglen);
        // A length shorter than the header would loop forever on a corrupted dump
        if length < header_len {
            return None;
        }
        offset += length;
        Some((header, message.get(header_len .. length)?))
    })
}

/// Reads the address out of a sockaddr_in, None for any other family
fn sockaddr_in_address(sockaddr: &[u8]) -> Option<Ipv4Addr> {
    let family = *sockaddr.get(offset_of!(libc::sockaddr_in, sin_family))?;
    if family != libc::AF_INET as u8 {
        return None;
    }
    let start = offset_of!(libc::sockaddr_in, sin_addr);
    let octets: [u8; 4] = sockaddr.get(start .. start + 4)?.try_into().ok()?;
    Some(Ipv4Addr::from(octets))
}

/// Reads the MAC address out of a sockaddr_dl, where it follows the interface name in sdl_data.
/// None unless it is a 6 byte address (an ARP entry still being resolved has none)
pub fn mac_from_sockaddr_dl(sockaddr: &[u8]) -> Option<[u8; 6]> {
    let family = *sockaddr.get(offset_of!(libc::sockaddr_dl, sdl_family))?;
    let name_len = *sockaddr.get(offset_of!(libc::sockaddr_dl, sdl_nlen))? as usize;
    let address_len = *sockaddr.get(offset_of!(libc::sockaddr_dl, sdl_alen))? as usize;
    if family != libc::AF_LINK as u8 || address_len != 6 {
        return None;
    }

    let start = offset_of!(libc::sockaddr_dl, sdl_data) + name_len;
    let mac: [u8; 6] = sockaddr.get(start .. start + 6)?.try_into().ok()?;
    (mac != [0; 6]).then_some(mac)
}

/// Gets the name of the network a Wi-Fi interface is associated with. macOS hides it from apps
/// without Location Services (CoreWLAN's ssid, scutil's SSID_STR, ipconfig all say nothing), the
/// scan record configd keeps of the joined network still has it, with CoreWLAN as a fallback. A
/// blanked name counts as none, so the next source is still asked
pub fn read_wifi_ssid(interface: &str) -> Option<WifiSsid> {
    exact_ssid_or(read_scan_record_ssid(interface), || read_corewlan_ssid(interface))
}

/// Gets the name from State:/Network/Interface/<interface>/AirPort in configd's dynamic store. Its
/// SSID_STR is redacted but its CachedScanRecord, an NSKeyedArchiver archive of the scan result of
/// the joined network, is not. ~1 ms, nearly all of it opening the configd session
pub fn read_scan_record_ssid(interface: &str) -> Option<String> {
    let key = CFString::from_str(&format!("State:/Network/Interface/{}/AirPort", interface));
    // SAFETY: key is a valid CFString and a null store asks for a temporary session. The value
    // follows the Copy rule, CFRetained takes ownership and releases it
    let state = unsafe { CFRetained::from_raw(SCDynamicStoreCopyValue(std::ptr::null(), &key)?) };
    let state = state.downcast::<CFDictionary>().ok()?;
    // SAFETY: the AirPort state is keyed by CFStrings, values are type-checked by downcast
    let state = unsafe { state.cast_unchecked::<CFString, CFType>() };
    let record = state
        .get(&CFString::from_static_str("CachedScanRecord"))?
        .downcast::<CFData>()
        .ok()?;
    unarchive_scan_record_ssid(&record)
}

/// Decodes a CachedScanRecord with NSKeyedUnarchiver (Foundation is already loaded through
/// CoreGraphics) and reads its SSID_STR. Secure decoding lets only the property list classes a
/// scan record is made of through
fn unarchive_scan_record_ssid(record: &CFData) -> Option<String> {
    let class_names = [
        c"NSDictionary",
        c"NSArray",
        c"NSString",
        c"NSData",
        c"NSNumber",
    ];
    let allowed: Vec<*const AnyObject> = class_names
        .into_iter()
        .map(|name| AnyClass::get(name).map(|class| (class as *const AnyClass).cast()))
        .collect::<Option<_>>()?;
    let set_class = AnyClass::get(c"NSSet")?;
    let unarchiver = AnyClass::get(c"NSKeyedUnarchiver")?;
    let dictionary_class = AnyClass::get(c"NSDictionary")?;

    autoreleasepool(|_| {
        // SAFETY: setWithObjects:count: copies `count` object pointers out of the array, classes
        // are objects too
        let allowed: Option<Retained<AnyObject>> =
            unsafe { msg_send![set_class, setWithObjects: allowed.as_ptr(), count: allowed.len()] };
        let data: &AnyObject = (**record).as_ref();
        let no_error: *mut *mut AnyObject = std::ptr::null_mut();
        // SAFETY: CFData is toll-free bridged to NSData and the error pointer may be null. A record
        // that doesn't decode, or holds a class that isn't allowed, returns nil
        let decoded: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                unarchiver,
                unarchivedObjectOfClasses: &*allowed?,
                fromData: data,
                error: no_error
            ]
        };
        let decoded = decoded?;

        // SAFETY: isKindOfClass: is NSObject's and takes a class
        let is_dictionary: bool = unsafe { msg_send![&*decoded, isKindOfClass: dictionary_class] };
        if !is_dictionary {
            return None;
        }
        let key = CFString::from_static_str("SSID_STR");
        let key: &AnyObject = (**key).as_ref();
        // SAFETY: decoded is an NSDictionary, objectForKey: takes any object and may return nil
        let ssid: Option<Retained<AnyObject>> = unsafe { msg_send![&*decoded, objectForKey: key] };
        ssid.as_deref().and_then(ns_string_to_string)
    })
}

/// Gets the name through CoreWLAN, opened with dlopen. Its ssid is only set when the terminal
/// running rustfetch has Location Services access, otherwise the first known network is taken like
/// fastfetch does, as a guess: CoreWLAN sorts them by when they were last connected to, which can
/// lag behind an automatic join, so right after moving between two known networks it can name the
/// previous one (wifi.rs reads a guess again after GUESSED_SSID_TTL). ~18 ms, mostly loading the
/// configuration with every known network
pub fn read_corewlan_ssid(interface: &str) -> Option<WifiSsid> {
    // SAFETY: the path is a NUL terminated constant. The handle is never closed, the classes it
    // registers have to stay loaded for the rest of the process anyway
    let handle = unsafe { libc::dlopen(COREWLAN_PATH.as_ptr(), libc::RTLD_LAZY) };
    if handle.is_null() {
        return None;
    }
    let client_class = AnyClass::get(c"CWWiFiClient")?;

    autoreleasepool(|_| {
        let name = CFString::from_str(interface);
        let name: &AnyObject = (**name).as_ref();
        // SAFETY: sharedWiFiClient and interfaceWithName: (which takes an NSString, CFString is
        // toll-free bridged to it) are public CoreWLAN API, both may return nil
        let client: Option<Retained<AnyObject>> =
            unsafe { msg_send![client_class, sharedWiFiClient] };
        let wlan: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*client?, interfaceWithName: name] };
        let wlan = wlan?;

        // SAFETY: CWInterface's ssid returns an NSString or nil
        let ssid: Option<Retained<AnyObject>> = unsafe { msg_send![&*wlan, ssid] };
        exact_ssid_or(ssid.as_deref().and_then(ns_string_to_string), || {
            // The known networks are the same for every interface (awdl0 would get them too), so
            // they only stand for the joined one on an interface in station (client) mode
            // SAFETY: interfaceMode returns a CWInterfaceMode, an NSInteger
            let mode: isize = unsafe { msg_send![&*wlan, interfaceMode] };
            if mode != CW_INTERFACE_MODE_STATION {
                return None;
            }
            // SAFETY: configuration, networkProfiles (an NSOrderedSet), firstObject and the
            // profile's ssid are public CoreWLAN and Foundation API, each may return nil
            let configuration: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*wlan, configuration] };
            let profiles: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*configuration?, networkProfiles] };
            let profile: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*profiles?, firstObject] };
            let ssid: Option<Retained<AnyObject>> = unsafe { msg_send![&*profile?, ssid] };
            let name = sanitize_ssid(&ns_string_to_string(&*ssid?)?)?;
            Some(WifiSsid {
                name,
                guessed: true,
            })
        })
    })
}

/// Copies an NSString into a String, None if the object is something else
fn ns_string_to_string(object: &AnyObject) -> Option<String> {
    let string_class = AnyClass::get(c"NSString")?;
    // SAFETY: isKindOfClass: is NSObject's and takes a class
    let is_string: bool = unsafe { msg_send![object, isKindOfClass: string_class] };
    // SAFETY: NSString is toll-free bridged to CFString, so an NSString is a valid CFString
    is_string.then(|| unsafe { &*(object as *const AnyObject).cast::<CFString>() }.to_string())
}
