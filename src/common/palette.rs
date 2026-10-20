//! The accent colors used for the rainbow logo and the usage percentages: on terminals that support
//! 24-bit color the logo stripes are saturated and the percentages muted, plain ANSI elsewhere

use std::sync::OnceLock;

use colored::*;

/// The accent colors rustfetch draws with
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteColor {
    Green,
    Yellow,
    Orange,
    Red,
    Magenta,
    Blue,
}

impl PaletteColor {
    /// Gets the muted 24-bit version, softer than the saturated ANSI colors most themes ship
    pub fn muted(self) -> Color {
        let (r, g, b) = match self {
            PaletteColor::Green => (0x8f, 0xbf, 0x7f),
            PaletteColor::Yellow => (0xe3, 0xc4, 0x7a),
            PaletteColor::Orange => (0xe0, 0xa0, 0x6f),
            PaletteColor::Red => (0xd4, 0x84, 0x7f),
            PaletteColor::Magenta => (0xae, 0x8a, 0xb8),
            PaletteColor::Blue => (0x77, 0xaf, 0xd4),
        };
        Color::TrueColor { r, g, b }
    }

    /// Gets the saturated 24-bit version, the stripe colors of Apple's 1977 six-color logo. Used
    /// for the rainbow logo, where muted stripes look washed out
    pub fn vivid(self) -> Color {
        let (r, g, b) = match self {
            PaletteColor::Green => (0x61, 0xbb, 0x46),
            PaletteColor::Yellow => (0xfd, 0xb8, 0x27),
            PaletteColor::Orange => (0xf5, 0x82, 0x1f),
            PaletteColor::Red => (0xe0, 0x3a, 0x3e),
            PaletteColor::Magenta => (0x96, 0x3d, 0x97),
            PaletteColor::Blue => (0x00, 0x9d, 0xdc),
        };
        Color::TrueColor { r, g, b }
    }

    /// Gets the ANSI color used when the terminal can't show 24-bit color, ANSI has no orange so
    /// bright red stands in for it
    pub fn ansi(self) -> Color {
        match self {
            PaletteColor::Green => Color::Green,
            PaletteColor::Yellow => Color::Yellow,
            PaletteColor::Orange => Color::BrightRed,
            PaletteColor::Red => Color::Red,
            PaletteColor::Magenta => Color::Magenta,
            PaletteColor::Blue => Color::Blue,
        }
    }

    /// Picks the muted or the ANSI version depending on the terminal
    pub fn resolve(self, truecolor: bool) -> Color {
        if truecolor { self.muted() } else { self.ansi() }
    }
}

/// Checks a COLORTERM value the same way the colored crate does, which turns 24-bit colors into
/// the closest ANSI color unless COLORTERM is exactly "truecolor" or "24bit"
pub fn is_truecolor_value(colorterm: Option<&str>) -> bool {
    matches!(colorterm, Some("truecolor" | "24bit"))
}

/// Checks if the terminal supports 24-bit color, read once since every colored line asks
pub fn supports_truecolor() -> bool {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    *TRUECOLOR.get_or_init(|| is_truecolor_value(std::env::var("COLORTERM").ok().as_deref()))
}

/// Colors text with a palette color, muted on truecolor terminals and plain ANSI elsewhere
pub fn paint(text: &str, color: PaletteColor) -> ColoredString {
    text.color(color.resolve(supports_truecolor()))
}

/// Colors text with a palette color, saturated on truecolor terminals and plain ANSI elsewhere
pub fn paint_vivid(text: &str, color: PaletteColor) -> ColoredString {
    text.color(if supports_truecolor() { color.vivid() } else { color.ansi() })
}
