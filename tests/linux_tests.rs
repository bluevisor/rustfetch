//! Test Linux-only platform functions

#![cfg(target_os = "linux")]

use std::ffi::CStr;

use rustfetch::{platform::*, sysinfo::read_string_literals};

/// Builds a minimal little endian ELF file with a .rodata and a .shstrtab section, returns it
/// alongside the .rodata (offset, size)
fn build_elf(is_64: bool, rodata: &[u8]) -> (Vec<u8>, (u64, u64)) {
    let names = b"\0.rodata\0.shstrtab\0";
    let header_size = if is_64 { 64 } else { 52 };
    let entry_size: usize = if is_64 { 64 } else { 40 };

    let rodata_offset = header_size;
    let names_offset = rodata_offset + rodata.len();
    let table_offset = names_offset + names.len();

    let mut elf = vec![0u8; header_size];
    elf[.. 4].copy_from_slice(b"\x7fELF");
    elf[4] = if is_64 { 2 } else { 1 };
    elf[5] = 1;
    // e_shoff, then e_shentsize, e_shnum (null, .rodata, .shstrtab) and e_shstrndx
    let fields = if is_64 {
        elf[0x28 .. 0x30].copy_from_slice(&(table_offset as u64).to_le_bytes());
        0x3a
    } else {
        elf[0x20 .. 0x24].copy_from_slice(&(table_offset as u32).to_le_bytes());
        0x2e
    };
    elf[fields .. fields + 2].copy_from_slice(&(entry_size as u16).to_le_bytes());
    elf[fields + 2 .. fields + 4].copy_from_slice(&3u16.to_le_bytes());
    elf[fields + 4 .. fields + 6].copy_from_slice(&2u16.to_le_bytes());

    elf.extend_from_slice(rodata);
    elf.extend_from_slice(names);

    for (name, offset, size) in [
        (0, 0, 0),
        (1, rodata_offset, rodata.len()),
        (9, names_offset, names.len()),
    ] {
        let mut entry = vec![0u8; entry_size];
        entry[.. 4].copy_from_slice(&(name as u32).to_le_bytes());
        if is_64 {
            entry[24 .. 32].copy_from_slice(&(offset as u64).to_le_bytes());
            entry[32 .. 40].copy_from_slice(&(size as u64).to_le_bytes());
        } else {
            entry[16 .. 20].copy_from_slice(&(offset as u32).to_le_bytes());
            entry[20 .. 24].copy_from_slice(&(size as u32).to_le_bytes());
        }
        elf.extend_from_slice(&entry);
    }

    (elf, (rodata_offset as u64, rodata.len() as u64))
}

/// Reads from an in-memory file like read_file_at does from a real one
fn memory_reader(data: &[u8]) -> impl Fn(u64, usize) -> Option<Vec<u8>> + '_ {
    move |offset, length| {
        let start = usize::try_from(offset).ok()?;
        data.get(start .. data.len().min(start + length)).map(<[u8]>::to_vec)
    }
}

#[test]
fn test_find_elf_rodata_section_64_and_32_bit() {
    let rodata = b"\0@(#)Bash version 5.2.15(1) release GNU\0";
    for is_64 in [true, false] {
        let (elf, expected) = build_elf(is_64, rodata);
        assert_eq!(find_elf_rodata_section(memory_reader(&elf)), Some(expected));
    }
}

#[test]
fn test_find_elf_rodata_section_not_elf() {
    assert_eq!(find_elf_rodata_section(memory_reader(b"#!/bin/sh\necho hi\n")), None);
    assert_eq!(find_elf_rodata_section(memory_reader(b"")), None);

    // big endian files are not supported
    let (mut elf, _) = build_elf(true, b"\0zsh-5.9\0");
    elf[5] = 2;
    assert_eq!(find_elf_rodata_section(memory_reader(&elf)), None);
}

#[test]
fn test_read_string_literals_on_self() {
    // string literals of this test binary end up in its own .rodata
    let marker = std::hint::black_box("rustfetch-rodata-test-marker");
    let literals = read_string_literals("/proc/self/exe").unwrap_or_default();
    assert!(literals.windows(marker.len()).any(|window| window == marker.as_bytes()));
}

#[test]
fn test_process_functions_on_self() {
    let pid = std::process::id();

    let (ppid, name) = get_process_info(pid).unwrap_or_default();
    assert_eq!(ppid, std::os::unix::process::parent_id());
    assert!(!name.is_empty());

    let path = get_process_path(pid).unwrap_or_default();
    let current_exe = std::env::current_exe().unwrap_or_default();
    assert_eq!(std::path::Path::new(&path), current_exe);

    assert!(get_process_argv0(pid).is_some_and(|argv0| !argv0.is_empty()));
}

#[test]
fn test_filesystem_space_of_root() {
    let (total, available) = get_filesystem_space(c"/").unwrap_or_default();
    assert!(total > 0 && available <= total);
    let missing: &CStr = c"/no/such/directory/for/rustfetch";
    assert_eq!(get_filesystem_space(missing), None);
}

#[test]
fn test_process_functions_missing_pid() {
    assert_eq!(get_process_info(999_999_999), None);
    assert_eq!(get_process_path(999_999_999), None);
    assert_eq!(get_process_argv0(999_999_999), None);
}
