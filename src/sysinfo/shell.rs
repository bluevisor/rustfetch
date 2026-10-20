//! Detects the shell rustfetch was launched from and its version

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{common::read_file_at, platform};

/// Shells we stop at while walking up the process tree, anything in between (sudo, script, make,
/// python, a terminal multiplexer...) is skipped
const KNOWN_SHELLS: &[&str] = &[
    "fish", "zsh", "bash", "sh", "dash", "ash", "ksh", "mksh", "oksh", "tcsh", "csh", "nu",
    "elvish", "xonsh", "pwsh", "ion", "oil", "oil.ovm", "osh", "ysh", "yash",
];

/// How many ancestors to look at before giving up, the shell is almost always the direct parent but
/// wrappers like sudo, nix-shell or a task runner can sit in between
const MAX_PARENT_DEPTH: usize = 16;

/// Longest `<shell> --version` may run. Shells answer in 2-6 ms and PowerShell (which starts .NET)
/// in a few hundred, anything slower is treated as having no version
const VERSION_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
/// How often a running `<shell> --version` is checked on, short so its answer isn't held up
const VERSION_COMMAND_POLL: Duration = Duration::from_micros(250);

/// A shell's string section is well under 1MB, a bigger size means a corrupted file
const MAX_STRING_SECTION_SIZE: usize = 8 * 1024 * 1024;

/// Gets the shell rustfetch was launched from as (name, version), e.g. ("fish", Some("4.9.3")).
/// Walks up the parent processes until a known shell, falling back to $SHELL when rustfetch was
/// not started from one (a GUI launcher, cron, an IDE task...)
pub fn get_shell() -> Option<(String, Option<String>)> {
    let mut pid = std::os::unix::process::parent_id();

    for _ in 0 .. MAX_PARENT_DEPTH {
        // pid 0 is the kernel. pid 1 is still checked: it is launchd/init on a normal system but in
        // a container (docker run -it alpine sh) it is the shell itself
        if pid == 0 {
            break;
        }
        let Some((ppid, process_name)) = platform::get_process_info(pid) else {
            break;
        };

        let mut name = normalize_shell_name(&process_name);
        if is_known_shell(&name) {
            // argv[0] says how the shell was invoked, which differs from the binary for macOS'
            // /bin/sh (it runs /bin/bash) or "exec -a"
            if let Some(argv0) = platform::get_process_argv0(pid).map(|a| normalize_shell_name(&a))
                && is_known_shell(&argv0)
            {
                name = argv0;
            }
            let exe_path = platform::get_process_path(pid);
            let version = get_shell_version(&name, exe_path.as_deref());
            return Some((pretty_shell_name(&name), version));
        }
        pid = ppid;
    }

    let login_shell = std::env::var("SHELL").ok().filter(|shell| !shell.is_empty())?;
    // The name comes from the path as written (/bin/sh stays "sh"), the version from the resolved
    // one since Homebrew's /opt/homebrew/bin/fish symlink hides the Cellar path that carries it
    let name = normalize_shell_name(&login_shell);
    // $SHELL can name any program, only a shell we know is run to ask for its version
    if !is_known_shell(&name) {
        return Some((name, None));
    }
    let exe_path = std::fs::canonicalize(&login_shell)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or(login_shell);
    let version = get_shell_version(&name, Some(&exe_path));
    Some((pretty_shell_name(&name), version))
}

/// Turns a process name or executable path into a bare shell name, dropping the directories, the
/// leading '-' of login shells and Nix's ".name-wrapped" wrapper naming
pub fn normalize_shell_name(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name).trim_start_matches('-');
    let base = base.strip_prefix('.').and_then(|b| b.strip_suffix("-wrapped")).unwrap_or(base);
    base.to_string()
}

/// Checks if a normalized process name is one of the shells we recognize
pub fn is_known_shell(name: &str) -> bool {
    KNOWN_SHELLS.contains(&name)
}

/// Renames the shells whose binary name is not what people call them, the same ones fastfetch
/// renames (nu is shown as nushell)
pub fn pretty_shell_name(name: &str) -> String {
    match name {
        "nu" => String::from("nushell"),
        "pwsh" => String::from("PowerShell"),
        "oil.ovm" => String::from("oils"),
        _ => name.to_string(),
    }
}

/// Gets a shell's version trying the cheapest source first: the install path, the version string
/// compiled into the binary, the version cache, and only then running "<shell> --version" (2-6 ms,
/// up to VERSION_COMMAND_TIMEOUT), whose answer is cached until the executable changes
pub fn get_shell_version(name: &str, exe_path: Option<&str>) -> Option<String> {
    // sh is whatever the system links it to (bash on macOS, dash on Debian) so a version would be
    // misleading, fastfetch skips it for the same reason
    if name == "sh" {
        return None;
    }

    // Some shells export their own version to child processes
    let version_env = match name {
        "nu" => Some("NU_VERSION"),
        "xonsh" => Some("XONSH_VERSION"),
        "pwsh" => Some("POWERSHELL_VERSION"),
        _ => None,
    };
    if let Some(version) = version_env
        .and_then(|var| std::env::var(var).ok())
        .and_then(|value| parse_shell_version(&value))
    {
        return Some(version);
    }

    let exe_path = exe_path?;
    if let Some(version) = parse_package_path_version(exe_path) {
        return Some(version);
    }

    if let Some(marker) = get_binary_version_marker(name)
        && let Some(version) = read_binary_version(exe_path, marker)
    {
        return Some(version);
    }

    let args = get_version_args(name)?;
    // Apple's /bin/bash, tcsh, ksh and fish outside Homebrew have no cheaper source, so a run's
    // answer is remembered for as long as the executable keeps its size and modification time
    let stamp = get_executable_stamp(exe_path);
    let cache_path = get_shell_cache_path();
    let mut cache = cache_path.as_deref().map(read_shell_cache).unwrap_or_default();
    if let Some(stamp) = stamp
        && let Some(version) = cache.lookup(exe_path, name, stamp)
    {
        return version;
    }

    // xonsh runs inside python, running its executable would give python's version
    let program = if name == "xonsh" { "xonsh" } else { exe_path };
    // A shell that couldn't be started is not cached, the next run tries again
    let version = parse_version_output(name, &run_version_command(program, args)?);

    if let (Some(path), Some(stamp)) = (cache_path, stamp) {
        cache.insert(exe_path, name, stamp, version.clone());
        write_shell_cache(&path, &cache);
    }
    version
}

/// Runs `<program> <args>` and returns everything it printed, stdout then stderr (ksh prints its
/// version on stderr). A run that takes longer than VERSION_COMMAND_TIMEOUT is killed and counts
/// as printing nothing, None means it couldn't be started at all
fn run_version_command(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    // try_wait rather than wait so a shell that hangs can't hang rustfetch. A version banner is far
    // smaller than a pipe buffer, so the shell never blocks on writing it before it exits
    let deadline = Instant::now() + VERSION_COMMAND_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(VERSION_COMMAND_POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Some(String::new());
            },
        }
    }

    let mut output = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut output);
    }
    output.push(b'\n');
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_end(&mut output);
    }
    Some(String::from_utf8_lossy(&output).into_owned())
}

/// Finds a shell's version in what `<shell> --version` printed
pub fn parse_version_output(name: &str, output: &str) -> Option<String> {
    match name {
        // ksh93's release ("93u+") is not a dotted number
        "ksh" => parse_ksh_version(output).or_else(|| parse_shell_version(output)),
        _ => parse_shell_version(output),
    }
}

/// Gets ksh93's release from its banner, the word after "(AT&T Research)" as fastfetch reads it:
/// "  version         sh (AT&T Research) 93u+ 2012-08-01" from macOS' /bin/ksh gives "93u+"
pub fn parse_ksh_version(output: &str) -> Option<String> {
    let (_, after_vendor) = output.split_once("(AT&T Research) ")?;
    after_vendor.split_whitespace().next().map(str::to_string)
}

/// Finds the first dotted version number in a shell's version output, e.g. "5.3.20" in
/// "GNU bash, version 5.3.20(1)-release (aarch64-apple-darwin25.6.0)". Numbers glued to a word
/// such as "darwin25.6.0" are skipped, a leading 'v' is allowed
pub fn parse_shell_version(output: &str) -> Option<String> {
    output
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .find_map(|token| {
            let token = token.strip_prefix('v').unwrap_or(token);
            let end = token.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(token.len());
            let version = token[.. end].trim_end_matches('.');

            // At least two numbers, skips plain numbers like dates or bash's "(1)" build number
            let is_version =
                version.contains('.') && version.split('.').all(|part| !part.is_empty());
            is_version.then(|| version.to_string())
        })
}

/// Gets the version from a package manager's install path, which costs nothing since we already
/// have the path: Homebrew's ".../Cellar/fish/4.9.3/bin/fish" gives "4.9.3" (a "_1" revision suffix
/// is dropped) and Nix's "/nix/store/<hash>-fish-3.7.1/bin/fish" gives "3.7.1". HEAD builds and
/// any other layout return None
pub fn parse_package_path_version(exe_path: &str) -> Option<String> {
    parse_homebrew_path_version(exe_path).or_else(|| parse_nix_path_version(exe_path))
}

/// Gets the version from a Homebrew keg path, see parse_package_path_version
fn parse_homebrew_path_version(exe_path: &str) -> Option<String> {
    let (_, after_cellar) = exe_path.split_once("/Cellar/")?;
    let mut parts = after_cellar.split('/');
    let _formula = parts.next()?;
    let keg = parts.next()?;
    let version = keg.split('_').next()?;

    // the whole keg name must be the version, "5.2p37" or "HEAD-1a2b3c" are not
    (parse_shell_version(version).as_deref() == Some(version)).then(|| version.to_string())
}

/// Gets the version from a Nix store path, see parse_package_path_version. The version starts
/// after the first '-' that is followed by a digit, the same split as Nix's own parseDrvName
fn parse_nix_path_version(exe_path: &str) -> Option<String> {
    let (_, after_store) = exe_path.split_once("/nix/store/")?;
    let package = after_store.split('/').next()?;
    // drop the store hash
    let (_, name_version) = package.split_once('-')?;
    let version = name_version.match_indices('-').find_map(|(index, _)| {
        name_version
            .get(index + 1 ..)
            .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    })?;

    // like a keg, "5.2p37" (Nix's bash) is not a version we can show as is
    (parse_shell_version(version).as_deref() == Some(version)).then(|| version.to_string())
}

/// Gets the string that precedes the version inside a shell's binary, the same ones fastfetch looks
/// for: "@(#)Bash version 5.3.20(1) release GNU", "zsh-5.9-0-g73d3173", "BusyBox v1.36.1 (...)"
pub fn get_binary_version_marker(name: &str) -> Option<&'static [u8]> {
    match name {
        "bash" => Some(b"@(#)Bash version "),
        "zsh" => Some(b"zsh-"),
        "ash" => Some(b"BusyBox "),
        _ => None,
    }
}

/// Gets the shells that print their version with --version, None for those that have no way to
/// print it (dash, busybox ash) or that would print something else
pub fn get_version_args(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "sh" | "dash" | "ash" | "mksh" | "oksh" => None,
        _ => Some(&["--version"]),
    }
}

/// Looks for the version string compiled into a shell's binary
fn read_binary_version(exe_path: &str, marker: &[u8]) -> Option<String> {
    find_version_after_marker(&read_string_literals(exe_path)?, marker)
}

/// Reads the part of an executable that holds its C string literals (see
/// platform::find_string_section), a small slice of the file where version strings live
pub fn read_string_literals(exe_path: &str) -> Option<Vec<u8>> {
    let file = File::open(exe_path).ok()?;
    let read_at = |offset, length| {
        (length <= MAX_STRING_SECTION_SIZE)
            .then(|| read_file_at(&file, offset, length))
            .flatten()
    };
    let (offset, size) = platform::find_string_section(read_at)?;
    read_at(offset, usize::try_from(size).ok()?)
}

/// Finds a C string starting with marker in a binary and parses the version that follows it.
/// Matches that are not at the start of a string, or not followed by a version (zsh also contains
/// "zsh-newuser-install"), are skipped
pub fn find_version_after_marker(data: &[u8], marker: &[u8]) -> Option<String> {
    let mut search_from = 0;

    while let Some(found) = find_bytes(data.get(search_from ..)?, marker) {
        let start = search_from + found;
        search_from = start + 1;

        // String literals sit right after the previous literal's NUL, a match anywhere else is code
        // or data that happens to contain the same bytes
        if start > 0 && data.get(start - 1) != Some(&0) {
            continue;
        }

        // The version is always near the marker, 64 bytes is plenty and keeps the parse short
        let rest = data.get(start + marker.len() ..)?;
        let literal: Vec<u8> = rest.iter().take(64).take_while(|&&b| b != 0).copied().collect();
        if let Some(version) = parse_shell_version(&String::from_utf8_lossy(&literal)) {
            return Some(version);
        }
    }
    None
}

/// Finds the first position of needle in haystack. Jumping between occurrences of its first byte
/// is ~7x faster than comparing every window
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, rest) = needle.split_first()?;
    let mut offset = 0;
    while let Some(found) = haystack.get(offset ..)?.iter().position(|&b| b == first) {
        let start = offset + found;
        if haystack.get(start + 1 ..)?.starts_with(rest) {
            return Some(start);
        }
        offset = start + 1;
    }
    None
}

/// Parses /proc/<pid>/stat's "pid (comm) state ppid ..." as (ppid, comm). comm can contain spaces
/// and parentheses so it is everything between the first '(' and the last ')'
pub fn parse_proc_stat(stat: &str) -> Option<(u32, String)> {
    let comm_start = stat.find('(')? + 1;
    let comm_end = stat.rfind(')')?;
    let comm = stat.get(comm_start .. comm_end)?;

    // after the comm come the state and then the parent pid
    let ppid = stat.get(comm_end + 1 ..)?.split_whitespace().nth(1)?.parse().ok()?;
    Some((ppid, comm.to_string()))
}

/// Versions found by running shells, keyed by executable path. Stored as TOML in the cache
/// directory next to rustfetch's GPU cache
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ShellVersionCache {
    #[serde(default)]
    shells: BTreeMap<String, CachedShellVersion>,
}

/// One shell's cached version, valid while its executable keeps the same size and modification
/// time (an upgrade or rebuild changes both)
#[derive(Debug, Serialize, Deserialize)]
struct CachedShellVersion {
    name: String,
    size: u64,
    /// Nanoseconds since the Unix epoch
    modified: u64,
    /// None when the shell printed no version, so it isn't run again for nothing
    version: Option<String>,
}

impl ShellVersionCache {
    /// Gets the cached version of the shell `name` at `exe_path`, the outer None meaning there is
    /// no entry for that exact executable (stamp is its (size, modification time))
    pub fn lookup(&self, exe_path: &str, name: &str, stamp: (u64, u64)) -> Option<Option<String>> {
        let entry = self.shells.get(exe_path)?;
        // xonsh's path is python's, so the name has to match too
        let is_same = entry.name == name && (entry.size, entry.modified) == stamp;
        is_same.then(|| entry.version.clone())
    }

    /// Remembers a shell's version, replacing what was cached for the same path
    pub fn insert(
        &mut self,
        exe_path: &str,
        name: &str,
        stamp: (u64, u64),
        version: Option<String>,
    ) {
        let entry = CachedShellVersion {
            name: name.to_string(),
            size: stamp.0,
            modified: stamp.1,
            version,
        };
        self.shells.insert(exe_path.to_string(), entry);
    }
}

/// Gets an executable's (size, modification time in nanoseconds), which identify one build of it
fn get_executable_stamp(exe_path: &str) -> Option<(u64, u64)> {
    let metadata = fs::metadata(exe_path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), u64::try_from(modified.as_nanos()).ok()?))
}

/// Gets where the shell version cache lives (~/.cache/rustfetch/shell.toml on Linux,
/// ~/Library/Caches/rustfetch/shell.toml on macOS), None when there is no cache directory
fn get_shell_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|path| path.join("rustfetch/shell.toml"))
}

/// Reads the shell version cache, a missing or unreadable file is an empty cache
fn read_shell_cache(path: &Path) -> ShellVersionCache {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

/// Writes the shell version cache. It is written to a temporary file that then replaces the cache,
/// so a rustfetch started at the same time (several terminal panes opening) never reads half of
/// it. Failures are ignored, the version is just looked up again next time
fn write_shell_cache(path: &Path, cache: &ShellVersionCache) {
    let Ok(content) = toml::to_string(cache) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let temporary = path.with_extension(format!("toml.{}", std::process::id()));
    if fs::write(&temporary, content).is_ok() && fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

/// Deletes the shell version cache, for --clear-cache
pub fn clear_shell_version_cache() {
    if let Some(path) = get_shell_cache_path() {
        let _ = fs::remove_file(path);
    }
}
