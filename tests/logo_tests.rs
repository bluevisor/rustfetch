//! Test the [logo] name and color and key_color parsing from the config file, the auto key color,
//! how a key is drawn and the Seen Health logo

use colored::{Color, Colorize};
use rustfetch::common::{
    KeyColor, LogoColor, LogoName, PaletteColor, SEEN_LOGO_ID, SeenLeaf, auto_key_color,
    colorize_logo_line, get_display_width, get_logo_lines, get_seen_leaf_lines,
    group_cells_by_leaf, parse_key_color, parse_logo_color, parse_logo_name, render_key,
};

/// Builds a 24-bit color
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::TrueColor { r, g, b }
}

#[test]
fn test_logo_color_default() {
    assert_eq!(parse_logo_color("default"), LogoColor::Default);
    assert_eq!(parse_logo_color(" Default "), LogoColor::Default);
}

#[test]
fn test_logo_color_rainbow() {
    assert_eq!(parse_logo_color("rainbow"), LogoColor::Rainbow);
    assert_eq!(parse_logo_color("Rainbow"), LogoColor::Rainbow);
}

#[test]
fn test_logo_color_names() {
    assert_eq!(parse_logo_color("white"), LogoColor::Solid(Color::White));
    assert_eq!(parse_logo_color("bright white"), LogoColor::Solid(Color::BrightWhite));
    assert_eq!(parse_logo_color("Red"), LogoColor::Solid(Color::Red));
}

#[test]
fn test_logo_color_hex() {
    assert_eq!(
        parse_logo_color("#ff8800"),
        LogoColor::Solid(Color::TrueColor { r: 255, g: 136, b: 0 })
    );
    assert_eq!(
        parse_logo_color("#FFFFFF"),
        LogoColor::Solid(Color::TrueColor { r: 255, g: 255, b: 255 })
    );
}

#[test]
fn test_logo_color_invalid() {
    // Invalid values fall back to the distro's own colors instead of failing
    assert_eq!(parse_logo_color(""), LogoColor::Default);
    assert_eq!(parse_logo_color("not a color"), LogoColor::Default);
    assert_eq!(parse_logo_color("#12345"), LogoColor::Default);
    assert_eq!(parse_logo_color("#zzzzzz"), LogoColor::Default);
    // Multi-byte characters would make colored's hex parser panic on a char boundary
    assert_eq!(parse_logo_color("#1é234"), LogoColor::Default);
}

// parse_logo_name tests

#[test]
fn test_logo_name_parsing() {
    assert_eq!(parse_logo_name("auto"), LogoName::Auto);
    assert_eq!(parse_logo_name("seen"), LogoName::Seen);
    assert_eq!(parse_logo_name(" Seen "), LogoName::Seen);
    assert_eq!(parse_logo_name("SEEN"), LogoName::Seen);
}

#[test]
fn test_logo_name_unknown_is_auto() {
    // Unknown names fall back to the distro's own logo instead of drawing nothing
    assert_eq!(parse_logo_name(""), LogoName::Auto);
    assert_eq!(parse_logo_name("arch"), LogoName::Auto);
    assert_eq!(parse_logo_name("seen health"), LogoName::Auto);
    assert_eq!(parse_logo_name("séen"), LogoName::Auto);
}

#[test]
fn test_logo_name_picks_logo_id() {
    assert_eq!(LogoName::Auto.logo_id("arch"), "arch");
    assert_eq!(LogoName::Auto.logo_id("macos"), "macos");
    assert_eq!(LogoName::Seen.logo_id("arch"), "seen");
}

// key_color tests

#[test]
fn test_key_color_auto_and_none() {
    assert_eq!(parse_key_color("auto"), KeyColor::Auto);
    assert_eq!(parse_key_color(" Auto "), KeyColor::Auto);
    assert_eq!(parse_key_color("none"), KeyColor::Plain);
    assert_eq!(parse_key_color("NONE"), KeyColor::Plain);
}

#[test]
fn test_key_color_names() {
    assert_eq!(parse_key_color("yellow"), KeyColor::Solid(Color::Yellow));
    assert_eq!(parse_key_color("Bright Blue"), KeyColor::Solid(Color::BrightBlue));
}

#[test]
fn test_key_color_hex() {
    assert_eq!(parse_key_color("#fdb827"), KeyColor::Solid(rgb(0xfd, 0xb8, 0x27)));
    assert_eq!(parse_key_color(" #FF8800 "), KeyColor::Solid(rgb(0xff, 0x88, 0x00)));
}

#[test]
fn test_key_color_invalid_falls_back_to_auto() {
    assert_eq!(parse_key_color(""), KeyColor::Auto);
    assert_eq!(parse_key_color("not a color"), KeyColor::Auto);
    // Only the Apple logo has stripes, keys have no rainbow
    assert_eq!(parse_key_color("rainbow"), KeyColor::Auto);
    assert_eq!(parse_key_color("#12345"), KeyColor::Auto);
    assert_eq!(parse_key_color("#zzzzzz"), KeyColor::Auto);
}

#[test]
fn test_key_color_non_ascii_falls_back_to_auto() {
    // 6 bytes after the "#", so without the ASCII check colored would slice inside the é and panic
    assert_eq!(parse_key_color("#1é234"), KeyColor::Auto);
    assert_eq!(parse_key_color("jaune é"), KeyColor::Auto);
}

#[test]
fn test_key_color_resolve_ignores_logo_unless_auto() {
    assert_eq!(KeyColor::Plain.resolve("macos", LogoColor::Rainbow), None);
    assert_eq!(KeyColor::Plain.resolve("arch", LogoColor::Default), None);
    let cyan = KeyColor::Solid(Color::Cyan);
    assert_eq!(cyan.resolve("macos", LogoColor::Solid(Color::Red)), Some(Color::Cyan));
}

// auto_key_color tests

#[test]
fn test_auto_key_color_macos_is_yellow() {
    // fastfetch's macOS key color: Apple's yellow stripe on truecolor terminals, ANSI elsewhere
    let vivid_yellow = Some(PaletteColor::Yellow.vivid());
    assert_eq!(vivid_yellow, Some(rgb(0xfd, 0xb8, 0x27)));
    assert_eq!(auto_key_color("macos", LogoColor::Default, true), vivid_yellow);
    assert_eq!(auto_key_color("macos", LogoColor::Default, false), Some(Color::Yellow));
}

#[test]
fn test_auto_key_color_rainbow_macos_is_yellow() {
    // The stripes don't change it, yellow is already one of them
    let vivid_yellow = Some(PaletteColor::Yellow.vivid());
    assert_eq!(auto_key_color("macos", LogoColor::Rainbow, true), vivid_yellow);
    assert_eq!(auto_key_color("macos", LogoColor::Rainbow, false), Some(Color::Yellow));
}

#[test]
fn test_auto_key_color_follows_distro_logo() {
    assert_eq!(auto_key_color("arch", LogoColor::Default, true), Some(rgb(23, 147, 209)));
    assert_eq!(auto_key_color("debian", LogoColor::Default, false), Some(Color::Red));
    // Rainbow only stripes the Apple logo, other distros keep their own color for the keys too
    assert_eq!(auto_key_color("ubuntu", LogoColor::Rainbow, true), Some(rgb(255, 156, 0)));
    // The same color the logo lines are drawn in
    for distro in ["fedora", "linuxmint", "nixos", "elementary"] {
        let logo = colorize_logo_line(distro, "x").fgcolor;
        assert_eq!(auto_key_color(distro, LogoColor::Default, true), logo, "{}", distro);
    }
}

#[test]
fn test_auto_key_color_uses_solid_logo_color() {
    // A solid [logo] color wins over the distro's color, on macOS too
    let orange = rgb(0xff, 0x88, 0x00);
    assert_eq!(auto_key_color("macos", LogoColor::Solid(orange), true), Some(orange));
    let white = LogoColor::Solid(Color::White);
    assert_eq!(auto_key_color("arch", white, false), Some(Color::White));
}

#[test]
fn test_auto_key_color_seen_is_teal() {
    // The brand teal of the Seen logo's right leaf on truecolor terminals, ANSI cyan elsewhere
    let teal = Some(rgb(0x5d, 0xc6, 0xb2));
    assert_eq!(teal, Some(SeenLeaf::Right.resolve(true)));
    assert_eq!(auto_key_color(SEEN_LOGO_ID, LogoColor::Default, true), teal);
    assert_eq!(auto_key_color(SEEN_LOGO_ID, LogoColor::Default, false), Some(Color::Cyan));
    // Rainbow keeps the leaf colors, so the keys stay teal
    assert_eq!(auto_key_color(SEEN_LOGO_ID, LogoColor::Rainbow, true), teal);
    assert_eq!(auto_key_color(SEEN_LOGO_ID, LogoColor::Rainbow, false), Some(Color::Cyan));
    // On a Mac the Seen logo replaces the Apple one, so its keys aren't yellow
    let on_macos = LogoName::Seen.logo_id("macos");
    assert_eq!(auto_key_color(on_macos, LogoColor::Rainbow, true), teal);
    // A solid [logo] color still wins, as for every logo
    let orange = rgb(0xff, 0x88, 0x00);
    assert_eq!(auto_key_color(SEEN_LOGO_ID, LogoColor::Solid(orange), true), Some(orange));
    let red = LogoColor::Solid(Color::Red);
    assert_eq!(auto_key_color(SEEN_LOGO_ID, red, false), Some(Color::Red));
}

#[test]
fn test_auto_key_color_without_logo() {
    // A distro without a logo prints only the info lines, so there is nothing to follow
    assert_eq!(auto_key_color("unknown", LogoColor::Default, true), None);
    assert_eq!(auto_key_color("", LogoColor::Rainbow, false), None);
    // Unless a solid [logo] color was picked, which the keys take all the same
    assert_eq!(auto_key_color("", LogoColor::Solid(Color::Red), true), Some(Color::Red));
}

// render_key tests

#[test]
fn test_render_key() {
    // One test, the color override is shared by every thread of the test binary
    colored::control::set_override(true);
    // Only the label takes the color, the colon stays bold in the default color
    let yellow = render_key("OS", Some(Color::Yellow));
    assert_eq!(yellow, "\x1b[1;33mOS\x1b[0m\x1b[1m:\x1b[0m");
    let vivid = render_key("Disk (/)", Some(rgb(0xfd, 0xb8, 0x27)));
    assert_eq!(vivid, "\x1b[1;38;2;253;184;39mDisk (/)\x1b[0m\x1b[1m:\x1b[0m");
    // The teal keys next to the Seen logo
    let teal = render_key("OS", auto_key_color(SEEN_LOGO_ID, LogoColor::Rainbow, true));
    assert_eq!(teal, "\x1b[1;38;2;93;198;178mOS\x1b[0m\x1b[1m:\x1b[0m");
    // key_color = "none" is byte for byte the key from before key colors existed
    assert_eq!(render_key("OS", None), "OS:".bold().to_string());
    assert_eq!(render_key("OS", None), "\x1b[1mOS:\x1b[0m");

    // NO_COLOR or a pipe: no escape codes at all
    colored::control::set_override(false);
    assert_eq!(render_key("OS", Some(Color::Yellow)), "OS:");
    assert_eq!(render_key("OS", None), "OS:");
    colored::control::unset_override();
}

// Seen Health logo tests

#[test]
fn test_seen_logo_matches_leaf_map() {
    let art = get_logo_lines("seen");
    let leaves = get_seen_leaf_lines();
    assert_eq!(art.len(), 16);
    assert_eq!(leaves.len(), art.len());

    for (row, (line, leaf_line)) in art.iter().zip(&leaves).enumerate() {
        // no trailing spaces, print_logo pads every line to the widest one itself
        assert_eq!(line.trim_end(), line, "line {}", row + 1);
        // every drawn cell has a leaf and every leaf has a drawn cell, cell for cell
        let drawn: Vec<bool> = line.chars().map(|cell| cell != ' ').collect();
        let mapped: Vec<bool> = leaf_line.chars().map(|cell| cell != ' ').collect();
        assert_eq!(drawn, mapped, "line {}", row + 1);
        let known_leaves = leaf_line.chars().all(|cell| matches!(cell, ' ' | '1' ..= '3'));
        assert!(known_leaves, "line {}", row + 1);
    }
    assert_eq!(art.iter().map(|line| get_display_width(line)).max(), Some(31));
}

#[test]
fn test_seen_leaf_colors() {
    assert_eq!(SeenLeaf::TopLeft.resolve(true), Color::TrueColor { r: 0xa7, g: 0xe2, b: 0xc0 });
    assert_eq!(SeenLeaf::Right.resolve(true), Color::TrueColor { r: 0x5d, g: 0xc6, b: 0xb2 });
    assert_eq!(SeenLeaf::Bottom.resolve(true), Color::TrueColor { r: 0x00, g: 0xa7, b: 0xa2 });
    assert_eq!(SeenLeaf::TopLeft.resolve(false), Color::BrightGreen);
    assert_eq!(SeenLeaf::Right.resolve(false), Color::BrightCyan);
    assert_eq!(SeenLeaf::Bottom.resolve(false), Color::Cyan);
}

#[test]
fn test_leaf_map_cells() {
    assert_eq!(SeenLeaf::from_map_cell('1'), Some(SeenLeaf::TopLeft));
    assert_eq!(SeenLeaf::from_map_cell('2'), Some(SeenLeaf::Right));
    assert_eq!(SeenLeaf::from_map_cell('3'), Some(SeenLeaf::Bottom));
    assert_eq!(SeenLeaf::from_map_cell(' '), None);
    assert_eq!(SeenLeaf::from_map_cell('4'), None);
}

// group_cells_by_leaf tests

const TOP: Option<SeenLeaf> = Some(SeenLeaf::TopLeft);
const RIGHT: Option<SeenLeaf> = Some(SeenLeaf::Right);
const BOTTOM: Option<SeenLeaf> = Some(SeenLeaf::Bottom);

#[test]
fn test_group_cells_same_leaf_is_one_run() {
    assert_eq!(group_cells_by_leaf("▄▓██▓▄░", "1111111"), vec![("▄▓██▓▄░", TOP)]);
}

#[test]
fn test_group_cells_splits_on_leaf_change() {
    // Leading spaces stay uncolored, the spaces between two leaves join the run before them
    assert_eq!(
        group_cells_by_leaf("  ▄██   ░██▌", "  111   2222"),
        vec![("  ", None), ("▄██   ", TOP), ("░██▌", RIGHT)]
    );
}

#[test]
fn test_group_cells_spaces_dont_break_a_run() {
    // Two blobs of the same leaf share one escape, the gap draws nothing either way
    assert_eq!(group_cells_by_leaf("▀▀  ███", "33  333"), vec![("▀▀  ███", BOTTOM)]);
}

#[test]
fn test_group_cells_edge_cases() {
    assert_eq!(group_cells_by_leaf("", ""), vec![]);
    assert_eq!(group_cells_by_leaf("   ", "   "), vec![("   ", None)]);
    // Multi-byte block elements are sliced on char boundaries, not bytes
    assert_eq!(group_cells_by_leaf("█▓", "12"), vec![("█", TOP), ("▓", RIGHT)]);
}

#[test]
fn test_group_cells_short_or_bad_map() {
    // Cells past the end of the map or with an unknown leaf are drawn uncolored
    assert_eq!(group_cells_by_leaf("███", "1"), vec![("█", TOP), ("██", None)]);
    assert_eq!(group_cells_by_leaf("██", "x3"), vec![("█", None), ("█", BOTTOM)]);
}

#[test]
fn test_group_cells_keeps_every_cell() {
    // Joining the runs gives the line back, so colors never change what the logo draws
    let art = get_logo_lines("seen");
    for (line, leaves) in art.iter().zip(get_seen_leaf_lines()) {
        let joined: String = group_cells_by_leaf(line, leaves).iter().map(|run| run.0).collect();
        assert_eq!(&joined, line);
    }
}
