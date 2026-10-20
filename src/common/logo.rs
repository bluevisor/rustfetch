//! This file handles everything related to displaying the distro logo

use std::io::{BufWriter, Write};

use colored::*;

use crate::{
    cli::Cli,
    common::{PaletteColor, paint_vivid, supports_truecolor},
};

/// Gets a distro's ASCII logo, empty for distros without one
fn get_logo(distro_id: &str) -> &'static str {
    // include_str!() works even in compiled binaries
    match distro_id {
        "arch" => include_str!("../../ascii/arch.txt"),
        "ubuntu" => include_str!("../../ascii/ubuntu.txt"),
        "fedora" => include_str!("../../ascii/fedora.txt"),
        "manjaro" => include_str!("../../ascii/manjaro.txt"),
        "debian" => include_str!("../../ascii/debian.txt"),
        "opensuse" => include_str!("../../ascii/opensuse.txt"),
        "alpine" => include_str!("../../ascii/alpine.txt"),
        "gentoo" => include_str!("../../ascii/gentoo.txt"),
        "endeavouros" => include_str!("../../ascii/endeavouros.txt"),
        "popos" => include_str!("../../ascii/popos.txt"),
        "cachyos" => include_str!("../../ascii/cachyos.txt"),
        "garuda" => include_str!("../../ascii/garuda.txt"),
        "linuxmint" => include_str!("../../ascii/linuxmint.txt"),
        "kali" => include_str!("../../ascii/kali.txt"),
        "macos" => include_str!("../../ascii/macos.txt"),
        "zorin" => include_str!("../../ascii/zorin.txt"),
        "elementary" => include_str!("../../ascii/elementary.txt"),
        "nixos" => include_str!("../../ascii/nixos.txt"),
        _ => "",
    }
}

/// Gets the lines logos in a vector and returns them
pub fn get_logo_lines(distro_id: &str) -> Vec<String> {
    get_logo(distro_id).lines().map(|l| l.to_string()).collect()
}

/// Gets the color a distro's logo is drawn in by default, shared by its logo lines and its "auto"
/// key color
fn distro_logo_color(distro_id: &str) -> Color {
    let rgb = |r, g, b| Color::TrueColor { r, g, b };
    match distro_id {
        // The exact colors should be tested on your distro and eventually changed it they do not
        // match the color of the distro's logo excessively
        "arch" => rgb(23, 147, 209),
        "ubuntu" => rgb(255, 156, 0),
        "cachyos" => rgb(0, 184, 148),
        "fedora" => rgb(11, 87, 164),
        "garuda" => rgb(138, 43, 226),
        "gentoo" => rgb(84, 73, 149),
        "endeavouros" => rgb(122, 58, 237),
        "kali" => rgb(38, 139, 210),
        "linuxmint" | "manjaro" => Color::Green,
        "debian" => Color::Red,
        "alpine" => Color::Cyan,
        "popos" => rgb(72, 149, 239),
        "opensuse" => rgb(115, 186, 37),
        "nixos" => rgb(125, 176, 221),
        "zorin" => rgb(17, 162, 236),
        "elementary" => Color::White,
        _ => Color::White, // this includes macos
    }
}

/// Getting a reference to an element of the vector of strings we're printing and matching it with
/// the distro id, we get return its colorized version.
/// Example: if distro_id = ubuntu, then we return the line in orange
pub fn colorize_logo_line(distro_id: &str, line: &str) -> ColoredString {
    line.color(distro_logo_color(distro_id))
}

/// The classic rainbow Apple stripes over the 19 line macOS logo. Green covers the leaf and the
/// top of the apple, every other stripe is two rows so the stripes across the body look even (the
/// last blue row is only the half-height bottom edge)
fn colorize_macos_logo_line(line: &str, line_index: usize) -> ColoredString {
    let color = match line_index {
        0 ..= 7 => PaletteColor::Green, // leaf and top of the apple
        8 | 9 => PaletteColor::Yellow,
        10 | 11 => PaletteColor::Orange,
        12 | 13 => PaletteColor::Red,
        14 | 15 => PaletteColor::Magenta,
        _ => PaletteColor::Blue,
    };
    paint_vivid(line, color)
}

/// Counts the terminal columns a logo line takes. Logos only use single-width characters (ASCII
/// and block elements) so every char is one column, while len() would count a block element's 3
/// UTF-8 bytes and push the info column out of line
pub fn get_display_width(line: &str) -> usize {
    line.chars().count()
}

/// How the logo is colored, set by `color` in the [logo] section of the config file
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogoColor {
    /// The distro's own colors
    Default,
    /// The classic rainbow Apple stripes (macOS only, other distros keep their own colors)
    Rainbow,
    /// The whole logo in one color
    Solid(Color),
}

/// Parses a color name ("white", "bright white") or a hex code ("#ff8800"), None for anything else
fn parse_color(value: &str) -> Option<Color> {
    // Non-ASCII is never a valid color, and colored's hex parser slices by byte index (would panic)
    if !value.is_ascii() {
        return None;
    }
    value.parse().ok()
}

/// Parses the [logo] color from the config file: "default", "rainbow", a color name ("white",
/// "bright white") or a hex code ("#ff8800"). Anything else falls back to the default colors
pub fn parse_logo_color(value: &str) -> LogoColor {
    let value = value.trim();
    if value.eq_ignore_ascii_case("rainbow") {
        return LogoColor::Rainbow;
    }
    parse_color(value).map(LogoColor::Solid).unwrap_or(LogoColor::Default)
}

/// How the text before each info line's colon is colored, set by key_color in the [display]
/// section of the config file
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyColor {
    /// Follows the logo being drawn, see auto_key_color
    Auto,
    /// Plain bold keys ("none")
    Plain,
    /// Every key in one color
    Solid(Color),
}

impl KeyColor {
    /// Gets the color the keys are drawn in next to the logo with this id (the one print_logo
    /// draws), None for plain bold keys
    pub fn resolve(self, logo_id: &str, logo_color: LogoColor) -> Option<Color> {
        match self {
            KeyColor::Auto => auto_key_color(logo_id, logo_color, supports_truecolor()),
            KeyColor::Plain => None,
            KeyColor::Solid(color) => Some(color),
        }
    }
}

/// Parses key_color from the config file: "auto", "none", a color name ("yellow", "bright blue")
/// or a hex code ("#fdb827"). Anything else falls back to auto
pub fn parse_key_color(value: &str) -> KeyColor {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return KeyColor::Plain;
    }
    // "auto" isn't a color name either, so it lands on the fallback
    parse_color(value).map(KeyColor::Solid).unwrap_or(KeyColor::Auto)
}

/// Picks the "auto" key color from the id of the logo being drawn (the distro's own logo for
/// now): a solid [logo] color when one is set, yellow next to the Apple logo (fastfetch's macOS
/// key color) and the logo's own color everywhere else. None when there's neither a logo nor a
/// solid color to follow. A new logo whose keys shouldn't take distro_logo_color (white for any
/// id it doesn't know) gets one more arm here
pub fn auto_key_color(logo_id: &str, logo_color: LogoColor, truecolor: bool) -> Option<Color> {
    match (logo_id, logo_color) {
        // The color picked for the logo, even when there's no logo to draw it on
        (_, LogoColor::Solid(color)) => Some(color),
        // White or rainbow, the Apple logo's keys take its yellow stripe (ANSI yellow without
        // 24-bit color)
        ("macos", _) => Some(PaletteColor::Yellow.resolve_vivid(truecolor)),
        _ if get_logo(logo_id).is_empty() => None,
        _ => Some(distro_logo_color(logo_id)),
    }
}

pub fn print_logo(
    logo_lines: Vec<String>,
    info_lines: Vec<String>,
    distro_id: &str,
    logo_color: LogoColor,
    cli: &Cli,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut handle = BufWriter::new(stdout.lock());

    if logo_lines.is_empty() {
        // If the logo does not match any inside ../ascii/LOGO.txt, just print the info
        for line in info_lines {
            let _ = writeln!(handle, "{}", line);
        }
    } else {
        let max_lines = logo_lines.len().max(info_lines.len());
        // We get the maximum length from the logo using .max()
        let logo_column_width = logo_lines.iter().map(|l| get_display_width(l)).max().unwrap_or(0);

        for i in 0 .. max_lines {
            if i < logo_lines.len() {
                let colored_line = match logo_color {
                    LogoColor::Solid(color) => logo_lines[i].color(color),
                    LogoColor::Rainbow if distro_id == "macos" => {
                        colorize_macos_logo_line(&logo_lines[i], i)
                    },
                    _ => colorize_logo_line(distro_id, &logo_lines[i]),
                };
                write!(handle, "{}", colored_line)?;
                let padding = logo_column_width.saturating_sub(get_display_width(&logo_lines[i]))
                    + cli.padding as usize;
                write!(handle, "{:width$}", "", width = padding)?;
            } else {
                // when past logo lines, print spaces that are logo_column_width + padding
                let total_width = logo_column_width + cli.padding as usize;
                write!(handle, "{:width$}", "", width = total_width)?;
            }

            if i < info_lines.len() {
                writeln!(handle, "  {}", info_lines[i])?;
            } else {
                writeln!(handle)?;
            }
        }
    }

    handle.flush()?;
    Ok(())
}
