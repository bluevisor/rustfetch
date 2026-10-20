//! Test the [logo] color and key_color parsing from the config file, the auto key color and how a
//! key is drawn

use colored::{Color, Colorize};
use rustfetch::common::{
    KeyColor, LogoColor, PaletteColor, auto_key_color, colorize_logo_line, parse_key_color,
    parse_logo_color, render_key,
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
    // key_color = "none" is byte for byte the key from before key colors existed
    assert_eq!(render_key("OS", None), "OS:".bold().to_string());
    assert_eq!(render_key("OS", None), "\x1b[1mOS:\x1b[0m");

    // NO_COLOR or a pipe: no escape codes at all
    colored::control::set_override(false);
    assert_eq!(render_key("OS", Some(Color::Yellow)), "OS:");
    assert_eq!(render_key("OS", None), "OS:");
    colored::control::unset_override();
}
