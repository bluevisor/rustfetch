//! Tests for the Haiku-only helpers
#![cfg(target_os = "haiku")]

use rustfetch::platform::*;

#[test]
fn test_arm64_part_name_known_cores() {
    assert_eq!(arm64_part_name(0x41, 0xd08), Some("Cortex-A72"));
    assert_eq!(arm64_part_name(0x41, 0xd0b), Some("Cortex-A76"));
    assert_eq!(arm64_part_name(0x61, 0x023), Some("M1"));
}

#[test]
fn test_arm64_part_name_unknown_core() {
    // Apple's hypervisor reports MIDR 0x610f0000, part 0
    assert_eq!(arm64_part_name(0x61, 0x000), None);
    assert_eq!(arm64_part_name(0x00, 0xd08), None);
}

#[test]
fn test_live_readings() {
    let (total_ram, used_ram, _, _) = read_memory();
    assert!(total_ram > 0 && used_ram <= total_ram);
    assert!(get_hrev().is_some_and(|hrev| hrev.starts_with("hrev")));
    assert!(format_os_name().starts_with("Haiku"));
    assert_eq!(get_cpu_core_types().map(|cores| cores.len()), Some(1));
}
