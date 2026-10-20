//! Test all functions within src/utils

use core::f64;

use rustfetch::common::*;

// extract_numeric_value tests
#[test]
fn test_no_numeric_value() {
    let result = extract_numeric_value("");
    assert_eq!(result, Err("No numeric value found in the input".to_string()));

    let result = extract_numeric_value("abcd");
    assert!(result.is_err());
}

#[test]
fn test_numeric_value_correct_syntax() {
    let result = extract_numeric_value("512 GB");
    assert_eq!(result, Ok(512.0));

    let result = extract_numeric_value("412 mb");
    assert_eq!(result, Ok(412.0));

    // The function splits whitespace values, there must be a space between the value and the unit
    // for the syntax to be valid. Thus this is incorrect syntax and should return an error.
    let result = extract_numeric_value("999KiB");
    assert!(result.is_err());
}

// round_to_two_decimals tests

#[test]
fn test_round_to_two_decimals_correct_input() {
    let result = round_to_two_decimal(14.209839);
    assert_eq!(result, 14.20);

    // This test is crucial since its validity is in the doc comments for the function in utils.rs
    let result = round_to_two_decimal(9.989);
    assert_eq!(result, 9.98);

    // 0s after the last decimal number do not matter, as it is in standard math:
    // 9.1 is equal to 9.10, so the function should work
    let result = round_to_two_decimal(9.1);
    assert_eq!(result, 9.10)
}

#[test]
fn test_round_to_two_decimals_edge_cases() {
    // With our current implementation of the function there should be no reason for an input to be
    // negative, but it's better to test it sooner rather than it crashing later
    let result = round_to_two_decimal(-25.11238);
    assert_eq!(result, -25.11);

    // The following 2 should pass through
    let result = round_to_two_decimal(f64::INFINITY);
    assert!(result.is_infinite());

    let result = round_to_two_decimal(f64::NAN);
    assert!(result.is_nan());
}

// get_percentage_from_part tests

#[test]
fn test_get_percentage_from_part_correct_input() {
    let result = get_percentage_from_part(10.0, 100.0);
    assert_eq!(result, Ok(10));

    let result = get_percentage_from_part(200.0, 10.0);
    assert_eq!(result, Ok(2000));
}

#[test]
fn test_get_percentage_from_part_invalid_input() {
    let result = get_percentage_from_part(-3.0, 30.0);
    assert_eq!(result, Err("Part or total cannot be negative".to_string()));
}

// convert_to_bytes tests

#[test]
fn convert_to_bytes_correct_input() {
    // This test is crucial since its validity is in the doc comments for the function in utils.rs
    let result = convert_to_bytes(15_369.0);
    assert_eq!(result, Ok(String::from("15 MB")));

    let result = convert_to_bytes(28.0);
    assert_eq!(result, Ok(String::from("28 KiB")));
}

#[test]
fn convert_to_bytes_invalid_input() {
    let result = convert_to_bytes(-15.0);
    assert_eq!(result, Err("Memory value cannot be negative".to_string()));
}

// format_cpu_cores tests

#[test]
fn test_format_cpu_cores_hybrid() {
    // Apple M5 Pro: 6 Super + 12 Performance cores, fastest level first
    assert_eq!(format_cpu_cores(&[6, 12]), Some(String::from("6+12")));
    assert_eq!(format_cpu_cores(&[4, 4]), Some(String::from("4+4")));
    assert_eq!(format_cpu_cores(&[2, 4, 8]), Some(String::from("2+4+8")));
}

#[test]
fn test_format_cpu_cores_single_type() {
    assert_eq!(format_cpu_cores(&[8]), Some(String::from("8")));
}

#[test]
fn test_format_cpu_cores_unreadable() {
    assert_eq!(format_cpu_cores(&[]), None);
    // A core type that could not be read must not show up as "6+0"
    assert_eq!(format_cpu_cores(&[6, 0]), None);
    assert_eq!(format_cpu_cores(&[0]), None);
}

// format_frequency tests

#[test]
fn test_format_frequency_ghz() {
    // This test is crucial since its validity is in the doc comments for the function in utils.rs
    assert_eq!(format_frequency(4608), Some(String::from("4.61 GHz")));
    assert_eq!(format_frequency(1000), Some(String::from("1.00 GHz")));
    assert_eq!(format_frequency(3504), Some(String::from("3.50 GHz")));
}

#[test]
fn test_format_frequency_mhz() {
    assert_eq!(format_frequency(800), Some(String::from("800 MHz")));
    assert_eq!(format_frequency(999), Some(String::from("999 MHz")));
}

#[test]
fn test_format_frequency_unknown() {
    // sysinfo reports 0 when it can't read the frequency
    assert_eq!(format_frequency(0), None);
}

// format_screen_key tests

#[test]
fn test_format_screen_key_single_display() {
    // One display keeps the plain key, like before multi-display support
    assert_eq!(format_screen_key("Color LCD", 0, 1), "Screen");
    assert_eq!(format_screen_key("", 0, 1), "Screen");
}

#[test]
fn test_format_screen_key_multiple_displays() {
    assert_eq!(format_screen_key("Color LCD", 0, 2), "Screen (Color LCD)");
    assert_eq!(format_screen_key("S34CG50", 1, 2), "Screen (S34CG50)");
    assert_eq!(format_screen_key("HDMI-A-1", 1, 2), "Screen (HDMI-A-1)");
}

#[test]
fn test_format_screen_key_unnamed_display() {
    // Unnamed displays are numbered from 1
    assert_eq!(format_screen_key("", 1, 2), "Screen 2");
}

// format_screen tests

#[test]
fn test_format_screen_full() {
    // This test is crucial since its validity is in the doc comments for the function in utils.rs
    assert_eq!(
        format_screen(Some((3456, 2234)), Some(120)),
        Some(String::from("3456x2234 @ 120Hz"))
    );
}

#[test]
fn test_format_screen_partial() {
    assert_eq!(format_screen(Some((3440, 1440)), None), Some(String::from("3440x1440")));
    assert_eq!(format_screen(None, Some(60)), Some(String::from("60Hz")));
}

#[test]
fn test_format_screen_nothing_to_show() {
    assert_eq!(format_screen(None, None), None);
}
