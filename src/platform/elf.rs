//! ELF parsing shared by the platforms whose executables are ELF (Linux and Haiku)

use crate::common::read_array;

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
