//! Test the color palette and truecolor detection in src/common/palette.rs, and the logo width

use colored::Color;
use rustfetch::common::*;

// is_truecolor_value tests

#[test]
fn test_truecolor_values() {
    // The same two values the colored crate accepts, Warp and iTerm2 set "truecolor"
    assert!(is_truecolor_value(Some("truecolor")));
    assert!(is_truecolor_value(Some("24bit")));
}

#[test]
fn test_not_truecolor_values() {
    assert!(!is_truecolor_value(None));
    assert!(!is_truecolor_value(Some("")));
    assert!(!is_truecolor_value(Some("256color")));
    assert!(!is_truecolor_value(Some("yes")));
    // colored compares exactly, so a value it would downgrade must not get the muted palette
    assert!(!is_truecolor_value(Some("TrueColor")));
    assert!(!is_truecolor_value(Some(" truecolor")));
}

// PaletteColor tests

/// Builds a 24-bit color
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::TrueColor { r, g, b }
}

#[test]
fn test_palette_truecolor_is_muted() {
    assert_eq!(PaletteColor::Green.resolve(true), rgb(0x8f, 0xbf, 0x7f));
    assert_eq!(PaletteColor::Yellow.resolve(true), rgb(0xe3, 0xc4, 0x7a));
    assert_eq!(PaletteColor::Orange.resolve(true), rgb(0xe0, 0xa0, 0x6f));
    assert_eq!(PaletteColor::Red.resolve(true), rgb(0xd4, 0x84, 0x7f));
    assert_eq!(PaletteColor::Magenta.resolve(true), rgb(0xae, 0x8a, 0xb8));
    assert_eq!(PaletteColor::Blue.resolve(true), rgb(0x77, 0xaf, 0xd4));
}

#[test]
fn test_palette_vivid_is_apple_1977() {
    assert_eq!(PaletteColor::Green.vivid(), rgb(0x61, 0xbb, 0x46));
    assert_eq!(PaletteColor::Yellow.vivid(), rgb(0xfd, 0xb8, 0x27));
    assert_eq!(PaletteColor::Orange.vivid(), rgb(0xf5, 0x82, 0x1f));
    assert_eq!(PaletteColor::Red.vivid(), rgb(0xe0, 0x3a, 0x3e));
    assert_eq!(PaletteColor::Magenta.vivid(), rgb(0x96, 0x3d, 0x97));
    assert_eq!(PaletteColor::Blue.vivid(), rgb(0x00, 0x9d, 0xdc));
}

#[test]
fn test_palette_fallback_is_ansi() {
    assert_eq!(PaletteColor::Green.resolve(false), Color::Green);
    assert_eq!(PaletteColor::Yellow.resolve(false), Color::Yellow);
    // ANSI has no orange
    assert_eq!(PaletteColor::Orange.resolve(false), Color::BrightRed);
    assert_eq!(PaletteColor::Red.resolve(false), Color::Red);
    assert_eq!(PaletteColor::Magenta.resolve(false), Color::Magenta);
    assert_eq!(PaletteColor::Blue.resolve(false), Color::Blue);
}

#[test]
fn test_usage_bar_track_color() {
    assert_eq!(track_color(true), rgb(0x4a, 0x4a, 0x4a));
    assert_eq!(track_color(false), Color::BrightBlack);
}

// get_display_width and logo tests

#[test]
fn test_display_width_counts_columns_not_bytes() {
    assert_eq!(get_display_width(""), 0);
    assert_eq!(get_display_width("  .;loddo:."), 11);
    // block elements are 3 bytes each but take one column
    assert_eq!(get_display_width("▄█▌ ▐▀▓▒░"), 9);
    assert_eq!("▄█▌".len(), 9);
}

#[test]
fn test_macos_logo_shape() {
    let lines = get_logo_lines("macos");
    // colorize_macos_logo_line's rainbow bands are laid out for exactly 19 lines
    assert_eq!(lines.len(), 19);
    assert_eq!(lines.iter().map(|line| get_display_width(line)).max(), Some(30));
    // no trailing spaces, print_logo pads every line to the widest one itself
    assert!(lines.iter().all(|line| line.trim_end() == line));
}
