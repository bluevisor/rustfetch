//! Test the shell detection helpers in src/sysinfo/shell.rs

use rustfetch::sysinfo::*;

// parse_shell_version tests

#[test]
fn test_parse_shell_version_common_shells() {
    let bash =
        "GNU bash, version 5.3.20(1)-release (aarch64-apple-darwin25.6.0)\nCopyright (C) 2025";
    assert_eq!(parse_shell_version(bash), Some(String::from("5.3.20")));

    let apple_bash = "GNU bash, version 3.2.57(1)-release (arm64-apple-darwin25)";
    assert_eq!(parse_shell_version(apple_bash), Some(String::from("3.2.57")));

    let zsh = "zsh 5.9 (arm64-apple-darwin25.0)";
    assert_eq!(parse_shell_version(zsh), Some(String::from("5.9")));

    assert_eq!(parse_shell_version("fish, version 4.9.3"), Some(String::from("4.9.3")));
}

#[test]
fn test_parse_shell_version_other_shells() {
    let tcsh = "tcsh 6.24.07 (Astron) 2022-12-21 (aarch64-apple-darwin) options wide,nls,dl";
    assert_eq!(parse_shell_version(tcsh), Some(String::from("6.24.07")));

    // ksh93 prints the release after a slash on stderr
    let ksh = "  version         sh (AT&T Research) 93u+m/1.0.8 2024-01-01";
    assert_eq!(parse_shell_version(ksh), Some(String::from("1.0.8")));

    assert_eq!(parse_shell_version("xonsh/0.14.1"), Some(String::from("0.14.1")));
    assert_eq!(parse_shell_version("0.21.0"), Some(String::from("0.21.0")));
    assert_eq!(parse_shell_version("PowerShell 7.4.1"), Some(String::from("7.4.1")));
    assert_eq!(
        parse_shell_version("Yet another shell, version 2.55"),
        Some(String::from("2.55"))
    );
    assert_eq!(
        parse_shell_version("Oils 0.18.0\t\thttps://www.oilshell.org/"),
        Some(String::from("0.18.0"))
    );
    // a leading 'v' is allowed
    assert_eq!(
        parse_shell_version("BusyBox v1.36.1 (2023-11-07 18:53:09 UTC) multi-call binary."),
        Some(String::from("1.36.1"))
    );
}

#[test]
fn test_parse_shell_version_no_version() {
    // dash has no version flag and complains instead
    assert_eq!(parse_shell_version("/bin/dash: 0: Illegal option --"), None);
    // mksh's KSH_VERSION has no dotted number
    assert_eq!(parse_shell_version("@(#)MIRBSD KSH R59 2020/10/31"), None);
    // numbers glued to a word are a target triple, not a version
    assert_eq!(parse_shell_version("arm64-apple-darwin25.0"), None);
    assert_eq!(parse_shell_version("version 1..2"), None);
    assert_eq!(parse_shell_version(""), None);
}

// parse_ksh_version and parse_version_output tests

/// What macOS' /bin/ksh (ksh93u+) prints on stderr for --version
const APPLE_KSH_BANNER: &str = "  version         sh (AT&T Research) 93u+ 2012-08-01\n";

#[test]
fn test_parse_ksh_version() {
    // Same as fastfetch: the word after "(AT&T Research)"
    assert_eq!(parse_ksh_version(APPLE_KSH_BANNER), Some(String::from("93u+")));
    let ksh93u_m = "  version         sh (AT&T Research) 93u+m/1.0.8 2024-01-01";
    assert_eq!(parse_ksh_version(ksh93u_m), Some(String::from("93u+m/1.0.8")));

    assert_eq!(parse_ksh_version("mksh: --version: unknown option"), None);
    assert_eq!(parse_ksh_version("sh (AT&T Research) "), None);
    assert_eq!(parse_ksh_version(""), None);
}

#[test]
fn test_parse_version_output() {
    // the generic parser finds no dotted number in ksh's banner, the ksh one does
    assert_eq!(parse_shell_version(APPLE_KSH_BANNER), None);
    assert_eq!(parse_version_output("ksh", APPLE_KSH_BANNER), Some(String::from("93u+")));
    // ksh can also be another Korn shell that does print a dotted number
    assert_eq!(parse_version_output("ksh", "ksh 2020.0.0"), Some(String::from("2020.0.0")));

    let tcsh = "tcsh 6.21.00 (Astron) 2019-05-08 (unknown-apple-darwin) options wide,nls,dl";
    assert_eq!(parse_version_output("tcsh", tcsh), Some(String::from("6.21.00")));
    // a run that was killed for taking too long printed nothing
    assert_eq!(parse_version_output("fish", ""), None);
}

// normalize_shell_name and is_known_shell tests

#[test]
fn test_normalize_shell_name() {
    assert_eq!(normalize_shell_name("fish"), "fish");
    assert_eq!(normalize_shell_name("/opt/homebrew/bin/fish"), "fish");
    // login shells get a leading '-'
    assert_eq!(normalize_shell_name("-zsh"), "zsh");
    assert_eq!(normalize_shell_name("-/bin/bash"), "bash");
    // Nix wraps some programs as ".name-wrapped"
    assert_eq!(normalize_shell_name("/nix/store/abc-fish-4.0/bin/.fish-wrapped"), "fish");
    assert_eq!(normalize_shell_name(""), "");
}

#[test]
fn test_is_known_shell() {
    for shell in [
        "fish", "zsh", "bash", "sh", "dash", "ksh", "tcsh", "nu", "pwsh", "yash",
    ] {
        assert!(is_known_shell(shell), "{} should be a shell", shell);
    }
    for not_shell in [
        "python3",
        "Python",
        "login",
        "sudo",
        "tmux",
        "rustfetch",
        "",
    ] {
        assert!(!is_known_shell(not_shell), "{} should not be a shell", not_shell);
    }
}

#[test]
fn test_pretty_shell_name() {
    assert_eq!(pretty_shell_name("nu"), "nushell");
    assert_eq!(pretty_shell_name("pwsh"), "PowerShell");
    assert_eq!(pretty_shell_name("fish"), "fish");
}

// parse_package_path_version tests

#[test]
fn test_parse_package_path_version_homebrew() {
    assert_eq!(
        parse_package_path_version("/opt/homebrew/Cellar/fish/4.9.3/bin/fish"),
        Some(String::from("4.9.3"))
    );
    // the "_1" revision suffix is Homebrew's, not the shell's
    assert_eq!(
        parse_package_path_version("/opt/homebrew/Cellar/bash/5.3.20_1/bin/bash"),
        Some(String::from("5.3.20"))
    );
    assert_eq!(
        parse_package_path_version("/home/linuxbrew/.linuxbrew/Cellar/zsh/5.9/bin/zsh"),
        Some(String::from("5.9"))
    );
    assert_eq!(
        parse_package_path_version("/opt/homebrew/Cellar/nushell/0.98.0/bin/nu"),
        Some(String::from("0.98.0"))
    );
}

#[test]
fn test_parse_package_path_version_nix() {
    let hash = "0c3gd9h0bk5zvjx8mbn5zbnj6n0n6a8x";
    assert_eq!(
        parse_package_path_version(&format!("/nix/store/{}-fish-3.7.1/bin/fish", hash)),
        Some(String::from("3.7.1"))
    );
    // wrapped programs live in the same package
    assert_eq!(
        parse_package_path_version(&format!("/nix/store/{}-zsh-5.9/bin/.zsh-wrapped", hash)),
        Some(String::from("5.9"))
    );
    // the version starts at the first '-' followed by a digit, so dashes in the name are kept out
    assert_eq!(
        parse_package_path_version(&format!("/nix/store/{}-oils-for-unix-0.22.0/bin/osh", hash)),
        Some(String::from("0.22.0"))
    );
}

#[test]
fn test_parse_package_path_version_nix_unusable() {
    let hash = "0c3gd9h0bk5zvjx8mbn5zbnj6n0n6a8x";
    // Nix's bash is "5.2p37", which the binary's own version string gives as 5.2.37 instead
    let bash = format!("/nix/store/{}-bash-interactive-5.2p37/bin/bash", hash);
    assert_eq!(parse_package_path_version(&bash), None);
    assert_eq!(parse_package_path_version(&format!("/nix/store/{}-fish/bin/fish", hash)), None);
    assert_eq!(parse_package_path_version("/nix/store/"), None);
    assert_eq!(parse_package_path_version("/nix/store/fish-3.7.1"), None);
}

#[test]
fn test_parse_package_path_version_other_layouts() {
    assert_eq!(
        parse_package_path_version("/opt/homebrew/Cellar/fish/HEAD-1a2b3c4/bin/fish"),
        None
    );
    assert_eq!(parse_package_path_version("/opt/homebrew/Cellar/fish"), None);
    assert_eq!(parse_package_path_version("/usr/bin/fish"), None);
    assert_eq!(parse_package_path_version("/bin/zsh"), None);
    assert_eq!(parse_package_path_version(""), None);
}

// find_version_after_marker tests

#[test]
fn test_find_version_after_marker_bash() {
    let data = b"\x7fELF\0junk\0@(#)Bash version 5.3.20(1) release GNU\0more\0";
    assert_eq!(
        find_version_after_marker(data, b"@(#)Bash version "),
        Some(String::from("5.3.20"))
    );
}

#[test]
fn test_find_version_after_marker_skips_non_versions() {
    // zsh contains other strings starting with "zsh-" before its patch level
    let data = b"\0zsh-newuser-install\0zsh-5.9-0-g73d3173\0";
    assert_eq!(find_version_after_marker(data, b"zsh-"), Some(String::from("5.9")));

    // a match that isn't at the start of a string is ignored
    assert_eq!(find_version_after_marker(b"\0xzsh-5.9\0", b"zsh-"), None);
    // the start of the data counts as the start of a string
    assert_eq!(find_version_after_marker(b"zsh-5.8.1\0", b"zsh-"), Some(String::from("5.8.1")));
}

#[test]
fn test_find_version_after_marker_edge_cases() {
    assert_eq!(find_version_after_marker(b"", b"zsh-"), None);
    assert_eq!(find_version_after_marker(b"\0zsh-5.9\0", b""), None);
    // the marker at the very end of the data has nothing after it
    assert_eq!(find_version_after_marker(b"\0zsh-", b"zsh-"), None);
}

// parse_proc_stat tests

#[test]
fn test_parse_proc_stat() {
    let stat = "1234 (fish) S 987 1234 1234 34816 5678 4194304 1450 0 0 0 4 2 0 0 20 0 1 0";
    assert_eq!(parse_proc_stat(stat), Some((987, String::from("fish"))));

    // comm can contain spaces and parentheses, the last ')' ends it
    assert_eq!(
        parse_proc_stat("42 (tmux: server) S 1 42"),
        Some((1, String::from("tmux: server")))
    );
    assert_eq!(parse_proc_stat("42 (a) b) R 7 42"), Some((7, String::from("a) b"))));
}

#[test]
fn test_parse_proc_stat_invalid() {
    assert_eq!(parse_proc_stat(""), None);
    assert_eq!(parse_proc_stat("42 fish S 1"), None);
    assert_eq!(parse_proc_stat("42 (fish) S"), None);
    assert_eq!(parse_proc_stat("42 (fish) S notapid"), None);
}

// version source tables

#[test]
fn test_version_sources() {
    assert!(get_binary_version_marker("bash").is_some());
    assert!(get_binary_version_marker("zsh").is_some());
    assert!(get_binary_version_marker("fish").is_none());

    // shells without a version flag are never spawned
    assert_eq!(get_version_args("dash"), None);
    assert_eq!(get_version_args("sh"), None);
    assert_eq!(get_version_args("fish"), Some(&["--version"][..]));
    assert_eq!(get_version_args("ksh"), Some(&["--version"][..]));
}

// ShellVersionCache tests

const BASH_STAMP: (u64, u64) = (1_290_128, 1_790_302_016_000_000_000);

#[test]
fn test_shell_version_cache_lookup() {
    let mut cache = ShellVersionCache::default();
    assert_eq!(cache.lookup("/bin/bash", "bash", BASH_STAMP), None);

    cache.insert("/bin/bash", "bash", BASH_STAMP, Some(String::from("3.2.57")));
    assert_eq!(
        cache.lookup("/bin/bash", "bash", BASH_STAMP),
        Some(Some(String::from("3.2.57")))
    );

    // an upgraded or rebuilt executable has another size or modification time
    assert_eq!(cache.lookup("/bin/bash", "bash", (BASH_STAMP.0 + 1, BASH_STAMP.1)), None);
    assert_eq!(cache.lookup("/bin/bash", "bash", (BASH_STAMP.0, BASH_STAMP.1 + 1)), None);
    // same path run as another shell (xonsh's path is python's)
    assert_eq!(cache.lookup("/bin/bash", "xonsh", BASH_STAMP), None);
    assert_eq!(cache.lookup("/bin/zsh", "bash", BASH_STAMP), None);
}

#[test]
fn test_shell_version_cache_remembers_no_version() {
    // a shell that printed no version is cached too, so it isn't run again for nothing
    let mut cache = ShellVersionCache::default();
    cache.insert("/usr/bin/fish", "fish", (10, 20), None);
    assert_eq!(cache.lookup("/usr/bin/fish", "fish", (10, 20)), Some(None));

    // a new entry for the same path replaces the old one
    cache.insert("/usr/bin/fish", "fish", (11, 21), Some(String::from("4.0.2")));
    assert_eq!(cache.lookup("/usr/bin/fish", "fish", (10, 20)), None);
    assert_eq!(
        cache.lookup("/usr/bin/fish", "fish", (11, 21)),
        Some(Some(String::from("4.0.2")))
    );
}

#[test]
fn test_shell_version_cache_toml_round_trip() {
    let mut cache = ShellVersionCache::default();
    cache.insert("/bin/bash", "bash", BASH_STAMP, Some(String::from("3.2.57")));
    cache.insert("/bin/tcsh", "tcsh", (5, 6), None);

    let content = toml::to_string(&cache).unwrap_or_default();
    let parsed: ShellVersionCache = toml::from_str(&content).unwrap_or_default();
    assert_eq!(
        parsed.lookup("/bin/bash", "bash", BASH_STAMP),
        Some(Some(String::from("3.2.57")))
    );
    assert_eq!(parsed.lookup("/bin/tcsh", "tcsh", (5, 6)), Some(None));

    // a corrupted file is an empty cache, not an error
    assert!(toml::from_str::<ShellVersionCache>("shells = 3").is_err());
    let empty: ShellVersionCache = toml::from_str("").unwrap_or_default();
    assert_eq!(empty.lookup("/bin/bash", "bash", BASH_STAMP), None);
}

#[test]
fn test_sh_has_no_version() {
    // sh is whatever the system links it to, its version would be the linked shell's
    assert_eq!(get_shell_version("sh", Some("/bin/bash")), None);
    assert_eq!(get_shell_version("sh", None), None);
}

#[test]
fn test_get_shell_does_not_panic() {
    // The result depends on how the tests are launched, it only has to not crash
    if let Some((name, version)) = get_shell() {
        assert!(!name.is_empty());
        assert!(version.is_none_or(|v| !v.is_empty()));
    }
}
