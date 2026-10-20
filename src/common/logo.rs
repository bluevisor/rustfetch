//! This file handles everything related to displaying the logo, the distro's own or the Seen
//! Health one

use std::io::{BufWriter, Write};

use colored::*;

use crate::{
    cli::Cli,
    common::{PaletteColor, paint_vivid, supports_truecolor},
};

/// The logo id of the Seen Health logo, used in place of a distro id
pub const SEEN_LOGO_ID: &str = "seen";

/// Gets a logo's ASCII art (a distro id, or SEEN_LOGO_ID for the Seen Health logo), empty for
/// distros without one
fn get_logo(logo_id: &str) -> &'static str {
    // include_str!() works even in compiled binaries
    match logo_id {
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
        SEEN_LOGO_ID => include_str!("../../ascii/seen.txt"),
        _ => "",
    }
}

/// Gets the lines of a logo (a distro id, or SEEN_LOGO_ID for the Seen Health logo) in a vector
pub fn get_logo_lines(logo_id: &str) -> Vec<String> {
    get_logo(logo_id).lines().map(|l| l.to_string()).collect()
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

/// Gets the Seen Health logo's leaf map, the same shape as its art with every drawn cell set to the
/// leaf it belongs to: 1 top-left, 2 right, 3 bottom
pub fn get_seen_leaf_lines() -> Vec<&'static str> {
    include_str!("../../ascii/seen_colors.txt").lines().collect()
}

/// The three leaves of the Seen Health ginkgo logo, each drawn in its own brand color
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeenLeaf {
    TopLeft,
    Right,
    Bottom,
}

impl SeenLeaf {
    /// Gets the leaf a leaf map cell stands for, blank or unknown cells belong to no leaf
    pub fn from_map_cell(cell: char) -> Option<SeenLeaf> {
        match cell {
            '1' => Some(SeenLeaf::TopLeft),
            '2' => Some(SeenLeaf::Right),
            '3' => Some(SeenLeaf::Bottom),
            _ => None,
        }
    }

    /// Picks the brand color on truecolor terminals and the closest ANSI color everywhere else
    pub fn resolve(self, truecolor: bool) -> Color {
        let (r, g, b, ansi) = match self {
            SeenLeaf::TopLeft => (0xa7, 0xe2, 0xc0, Color::BrightGreen),
            SeenLeaf::Right => (0x5d, 0xc6, 0xb2, Color::BrightCyan),
            SeenLeaf::Bottom => (0x00, 0xa7, 0xa2, Color::Cyan),
        };
        if truecolor {
            Color::TrueColor { r, g, b }
        } else {
            ansi
        }
    }
}

/// Splits a logo line into runs of cells that share a leaf in its leaf map line, so each run takes
/// one color escape instead of one per cell. Spaces draw nothing so they join the run they sit in,
/// cells the map leaves blank (and leading spaces) get no leaf
pub fn group_cells_by_leaf<'a>(line: &'a str, leaves: &str) -> Vec<(&'a str, Option<SeenLeaf>)> {
    let mut segments = Vec::new();
    let mut map_cells = leaves.chars();
    let (mut start, mut run_leaf) = (0, None);

    for (index, cell) in line.char_indices() {
        // Read the map cell even for spaces, it has to stay in step with the line
        let leaf = map_cells.next().and_then(SeenLeaf::from_map_cell);
        if cell == ' ' || leaf == run_leaf {
            continue;
        }
        if index > start {
            segments.push((&line[start .. index], run_leaf));
        }
        (start, run_leaf) = (index, leaf);
    }
    if start < line.len() {
        segments.push((&line[start ..], run_leaf));
    }
    segments
}

/// Colors one line of the Seen Health logo cell by cell from its leaf map line
fn colorize_seen_logo_line(line: &str, leaves: &str) -> String {
    let truecolor = supports_truecolor();
    group_cells_by_leaf(line, leaves)
        .into_iter()
        .map(|(text, leaf)| match leaf {
            Some(leaf) => text.color(leaf.resolve(truecolor)).to_string(),
            None => text.to_string(),
        })
        .collect()
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
    /// The logo's own colors (the distro's, or the Seen logo's leaf colors)
    Default,
    /// The classic rainbow Apple stripes (macOS only, other logos keep their own colors)
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

/// Picks the "auto" key color from the id of the logo being drawn (the distro's own logo or the
/// Seen one): a solid [logo] color when one is set, yellow next to the Apple logo (fastfetch's
/// macOS key color), the brand teal next to the Seen logo and the logo's own color everywhere
/// else. None when there's neither a logo nor a solid color to follow. A new logo whose keys
/// shouldn't take distro_logo_color (white for any id it doesn't know) gets one more arm here
pub fn auto_key_color(logo_id: &str, logo_color: LogoColor, truecolor: bool) -> Option<Color> {
    match (logo_id, logo_color) {
        // The color picked for the logo, even when there's no logo to draw it on
        (_, LogoColor::Solid(color)) => Some(color),
        // White or rainbow, the Apple logo's keys take its yellow stripe (ANSI yellow without
        // 24-bit color)
        ("macos", _) => Some(PaletteColor::Yellow.resolve_vivid(truecolor)),
        // Default or rainbow (it keeps its leaf colors either way), the Seen logo's keys take the
        // brand teal of its right leaf (#5DC6B2), ANSI cyan without 24-bit color
        (SEEN_LOGO_ID, _) if truecolor => Some(SeenLeaf::Right.resolve(true)),
        (SEEN_LOGO_ID, _) => Some(Color::Cyan),
        _ if get_logo(logo_id).is_empty() => None,
        _ => Some(distro_logo_color(logo_id)),
    }
}

/// Which logo is drawn, set by `name` in the [logo] section of the config file
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogoName {
    /// The distro's own logo
    Auto,
    /// The Seen Health ginkgo logo
    Seen,
}

impl LogoName {
    /// Gets the id get_logo_lines() and print_logo() take, the distro's own one for auto
    pub fn logo_id(self, distro_id: &str) -> &str {
        match self {
            LogoName::Auto => distro_id,
            LogoName::Seen => SEEN_LOGO_ID,
        }
    }
}

/// Parses the [logo] name from the config file: "auto" or "seen". Anything else falls back to auto
pub fn parse_logo_name(value: &str) -> LogoName {
    if value.trim().eq_ignore_ascii_case("seen") {
        LogoName::Seen
    } else {
        LogoName::Auto
    }
}

/// Prints the logo (picked by logo_id) with the info lines next to it
pub fn print_logo(
    logo_lines: Vec<String>,
    info_lines: Vec<String>,
    logo_id: &str,
    logo_color: LogoColor,
    cli: &Cli,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut handle = BufWriter::new(stdout.lock());
    // Only the Seen logo is colored cell by cell
    let seen_leaves = match logo_id {
        SEEN_LOGO_ID => get_seen_leaf_lines(),
        _ => Vec::new(),
    };

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
                let line = &logo_lines[i];
                let colored_line = match logo_color {
                    LogoColor::Solid(color) => line.color(color).to_string(),
                    LogoColor::Rainbow if logo_id == "macos" => {
                        colorize_macos_logo_line(line, i).to_string()
                    },
                    // Default and rainbow both keep the leaf colors, the stripes are Apple's
                    _ if logo_id == SEEN_LOGO_ID => {
                        colorize_seen_logo_line(line, seen_leaves.get(i).copied().unwrap_or(""))
                    },
                    _ => colorize_logo_line(logo_id, line).to_string(),
                };
                write!(handle, "{}", colored_line)?;
                let padding = logo_column_width.saturating_sub(get_display_width(line))
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
