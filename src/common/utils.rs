//! Common functions shared across all platforms

use std::{
    fs::{self, File},
    os::unix::fs::FileExt,
    path::Path,
};

const KIB_IN_MB: f64 = 1024.0;
const KIB_IN_GB: f64 = 1024.0 * 1024.0; // We are declaring it as f64 as we'll use it as a float in this file to minimize casting
/// Eighth blocks that end a usage bar's filled part, from 1/8 to 7/8 of a cell
const PARTIAL_BLOCKS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// Gets content from a single line file and trims it
///
/// # Errors
/// Returns an error if the file cannot be read or if the trimmed content is empty
pub fn get_trimmed(path: &Path) -> Result<String, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let trimmed = content.trim().to_string();

    if trimmed.is_empty() {
        Err("File content is empty".to_string())
    } else {
        Ok(trimmed)
    }
}

/// Converts KiB figures into GB, MB or unchanged based on its size.
/// Returns a formatted String based on the conversion that has happened
///
/// For example: 15_369.0 KiB will return "15 MB"
pub fn convert_to_bytes(memory_kib: f64) -> Result<String, String> {
    if memory_kib < 0.0 {
        return Err("Memory value cannot be negative".to_string());
    }

    if memory_kib >= KIB_IN_GB {
        // If memory is more than 1GB, transform it to GB
        // 1 GB = (1024 * 1024) KiB
        let memory_gb = round_to_two_decimal(memory_kib / (KIB_IN_GB));
        Ok(format!("{} GB", memory_gb))
    } else if memory_kib >= KIB_IN_MB {
        // Same as before but with MB
        // 1 MB = 1024 KiB
        let memory_mb = round_to_two_decimal(memory_kib / KIB_IN_MB);
        Ok(format!("{} MB", memory_mb))
    } else {
        // Else, just return it in KiB
        Ok(format!("{} KiB", memory_kib))
    }
}

/// Gets the percentage as a rounded value considering a part and a total
pub fn get_percentage_from_part(part: f64, total: f64) -> Result<u64, String> {
    if part < 0.0 || total < 0.0 {
        return Err("Part or total cannot be negative".to_string());
    }
    if total == 0.0 {
        return Err("Division by zero error avoided: total cannot be 0".to_string());
    }
    Ok((part / total * 100.0).floor() as u64)
}

/// Rounds an f64 variable to two decimal points, note that the conversion is not accurate.
/// For example: 9.989 will return 9.98
pub fn round_to_two_decimal(input: f64) -> f64 {
    // We truncate the number since floor rounds to -infinity and it behaves differently
    // between positive and negative numbers
    (input * 100.0).trunc() / 100.0
}

/// Extracts the first whitespace-separated numeric value from a string.
///
/// For example, "1234567 kB" returns 1234567.0
pub fn extract_numeric_value(input: &str) -> Result<f64, String> {
    input
        .split_whitespace()
        .next()
        .ok_or_else(|| "No numeric value found in the input".to_string())
        .and_then(|num_str| num_str.parse::<f64>().map_err(|e| e.to_string()))
}

/// normalizes hex codes to remove the x, for example "0xf89" becomes "0f89"
pub fn format_hex(id: &str) -> String {
    id.trim_start_matches("0x").trim_start_matches("0X").to_ascii_lowercase()
}

/// Reads N bytes at offset as an array, to decode binary headers with from_le_bytes/from_be_bytes
pub fn read_array<const N: usize>(data: &[u8], offset: usize) -> Option<[u8; N]> {
    data.get(offset .. offset.checked_add(N)?)?.try_into().ok()
}

/// Reads up to length bytes of a file starting at offset, fewer if the file ends first
pub fn read_file_at(file: &File, offset: u64, length: usize) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; length];
    let mut filled = 0;
    while filled < length {
        match file.read_at(buffer.get_mut(filled ..)?, offset + filled as u64) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(_) => return None,
        }
    }
    buffer.truncate(filled);
    Some(buffer)
}

/// Formats CPU core counts as "P+E" for hybrid CPUs or a plain count otherwise. Returns None if
/// there is nothing to show
///
/// For example: [6, 12] returns "6+12" and [8] returns "8"
pub fn format_cpu_cores(core_counts: &[usize]) -> Option<String> {
    // A 0 means that core type could not be read, showing "6+0" would be wrong
    if core_counts.is_empty() || core_counts.contains(&0) {
        return None;
    }

    Some(core_counts.iter().map(|count| count.to_string()).collect::<Vec<_>>().join("+"))
}

/// Formats a frequency given in MHz, GHz is rounded to two decimals like fastfetch does. Returns
/// None for 0 since that means the frequency could not be read
///
/// For example: 4608 returns "4.61 GHz" and 800 returns "800 MHz"
pub fn format_frequency(frequency_mhz: u64) -> Option<String> {
    match frequency_mhz {
        0 => None,
        1 .. 1000 => Some(format!("{} MHz", frequency_mhz)),
        _ => Some(format!("{:.2} GHz", frequency_mhz as f64 / 1000.0)),
    }
}

/// Builds the key of a Screen line, the display name is only added when there is more than one
/// display so single-monitor setups keep the plain "Screen" key
///
/// For example: ("Color LCD", 0, 2) returns "Screen (Color LCD)"
pub fn format_screen_key(name: &str, index: usize, count: usize) -> String {
    if count <= 1 {
        String::from("Screen")
    } else if name.is_empty() {
        // Unnamed displays still need telling apart
        format!("Screen {}", index + 1)
    } else {
        format!("Screen ({})", name)
    }
}

/// Formats a display's resolution and refresh rate, either can be left out. Returns None if both
/// are
///
/// For example: (Some((3456, 2234)), Some(120)) returns "3456x2234 @ 120Hz"
pub fn format_screen(resolution: Option<(u64, u64)>, refresh_rate: Option<u64>) -> Option<String> {
    match (resolution, refresh_rate) {
        (Some((width, height)), Some(rate)) => Some(format!("{}x{} @ {}Hz", width, height, rate)),
        (Some((width, height)), None) => Some(format!("{}x{}", width, height)),
        (None, Some(rate)) => Some(format!("{}Hz", rate)),
        (None, None) => None,
    }
}

/// Draws a usage bar `width` cells wide as its (filled, empty) parts, so each can get its own
/// color. The filled part is rounded to the nearest eighth of a cell, ending in an eighth block,
/// and the empty part is light shade. Percentages above 100 draw a full bar
///
/// For example: (64, 10) returns ("██████▍", "░░░")
pub fn render_bar(percent: u64, width: usize) -> (String, String) {
    let percent = percent.min(100) as usize;
    let eighths = percent.saturating_mul(width).saturating_mul(8).saturating_add(50) / 100;

    let mut filled = "█".repeat(eighths / 8);
    // a remainder of 0 means the bar ends on a whole cell
    if let Some(&block) = (eighths % 8).checked_sub(1).and_then(|index| PARTIAL_BLOCKS.get(index)) {
        filled.push(block);
    }
    let empty = "░".repeat(width.saturating_sub(filled.chars().count()));

    (filled, empty)
}
