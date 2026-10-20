//! Test the [logo] color parsing from the config file

use colored::Color;
use rustfetch::common::{LogoColor, parse_logo_color};

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
