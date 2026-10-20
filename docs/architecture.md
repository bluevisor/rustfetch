# Understanding rustfetch's architecture

This file is reserved to **understanding the core structure of rustfetch** in a digestible and opinionated way, every change to the architecture must be documented.
<br>This file also contains the **reasons** this projects does things the way it does and its philosophy.

This project started as my **first Rust project**, so it has had many architecture iterations, but the current one is by far the **most idiomatic and structured**.

Lets break it down file by file:

<pre>
src/
├── <a href="#mainrs">main.rs</a>         # Entry point
├── <a href="#configrs">config.rs</a>       # TOML config
├── <a href="#clirs">cli.rs</a>          # CLI argument parsing
├── <a href="#cachers">cache.rs</a>        # GPU name cache (Linux only)
├── common/         # Common functions across OSes and files
│   ├── <a href="#displayrs">display.rs</a>  # Display formatting functions
│   ├── <a href="#logors">logo.rs</a>     # Logo loading, coloring and printing
│   ├── <a href="#paletters">palette.rs</a>  # Accent colors and truecolor detection
│   └── <a href="#utilsrs">utils.rs</a>    # Shared utility functions
├── platform/       # OS-specific implementations
│   ├── <a href="#modrs-platform">mod.rs</a>      # Exposes modules based on OS
│   ├── <a href="#linuxrs">linux.rs</a>    # /proc, /sys, ELF, display-info
│   └── <a href="#macosrs">macos.rs</a>    # IOKit, CoreGraphics, sysctl, PF_ROUTE, Mach-O
└── sysinfo/        # Cross-platform system info
    ├── <a href="#sharedrs">shared.rs</a>   # Generic sysinfo functions
    ├── <a href="#shellrs">shell.rs</a>    # Shell detection and its version cache
    ├── <a href="#networkrs">network.rs</a>  # Local IPv4/IPv6 and Tailscale addresses
    └── <a href="#tailscalers">tailscale.rs</a> # Tailscale detection and MagicDNS lookup
tests/              # Tests for "cargo test"
├── <a href="#utils_testsrs">utils_tests.rs</a>  # Tests specific to utils
├── config_tests.rs  # Old config files still parse
├── logo_tests.rs    # [logo] color parsing
├── palette_tests.rs # Palette, usage bar track color, truecolor detection, logo shape
├── shell_tests.rs   # Shell name/version parsing and the version cache
├── network_tests.rs # Interface and address selection, /proc/net parsers
├── tailscale_tests.rs # Tailscale checks, PTR query, DNS reply parsing and machine names
├── macos_tests.rs   # macOS-only platform functions
├── linux_tests.rs   # Linux-only platform functions
└── cache_test.rs    # GPU cache (Linux only)
scripts/
└── <a href="runtimesh">runtime.sh</a>      # Measure program's runtime

<a href="#installsh">install.sh</a>          # Bash installation script
</pre>

> Note: most mod.rs (and lib.rs) files have been omitted from this file tree as they all share the same purpose: exposing modules

## main.rs
Entry point for the program, **handles showing or not showing modules** based on current config settings and **skips arguments** if they are OS-bound and the target OS is different from the supported one:

- **Gets command line arguments** through [clap](https://docs.rs/clap/latest/clap/)
- **Creates a System variable** used for sysinfo functions
-  If the "--all" or "-a" flag is given, **enables all modules** and only reads the `[logo]` table and `usage_bars` of the config file (how lines look, not which ones show), without creating it (This does not skip unsupported [platform specific modules](#platform)).
<br>Else, **gets config options** from [config.rs](#configrs) and shows them based on their boolean value.
- Gets the distro id from the dedicated function inside the **platform** crate
- Creates a **String vector** and adds the return values of the [display functions](#displayrs) to it if the config boolean for it is true and if the function returns Some(), if it returns None or the config for it is false it **just skips it**.
- Prints the info_lines vector alongside the logo's lines and **adds padding** to make all the lines be horizontally aligned. As the line get printed, they are colored according to the distro_id

## config.rs
Main file for configuration handling, **creates and parses a config file** or enables all features:

- Creates a **DisplayConfig** struct that includes all modules as boolean values
- **Creates two implementations** for DisplayConfig: **Default** for initial file creation and **All** where all modules are set to true
- **load_config()** -> returns a DisplayConfig struct from parsing the **config.toml**. If the file does not exist it will create it in the [default config directory](https://docs.rs/dirs/latest/dirs/fn.config_dir.html) and print a message.
- **load_all_config()** -> turns every module on and keeps the `[logo]` settings and `usage_bars`, read without creating the file or printing anything.
- Lines added after the config format was released (shell, usage_bars, ipv4, ipv6, tailscale_ip, tailscale_name, ac_adapter) and the whole `[logo]` table are **defaulted**, so older config files still parse.
- Renamed keys keep reading their **old name**: `tailscale_name` was `tailscale_host`, so a config that still says `tailscale_host = false` keeps the line hidden. The old key is its own `Option` field rather than a serde alias, since an alias fails with "duplicate field" (and resets every setting) on a file that has both; `show_tailscale_name()` lets the new key win.

## cli.rs
Uses [clap](https://docs.rs/clap/latest/clap/) to **parse command line arguments** and creates a public Cli struct with all the possible flags in it. This file also decides which description every flag should have when running "rustfetch --help".

Example of a standard flag:
![Example](./images/cli_argument_description.avif)

## display.rs
Contains all functions related to showing the values returned from other files as formatted text.

Also contains internal private functions such as **color_percentage()** and **format_usage()**, specific to display features. The RAM and disk lines end in a 10 cell **usage bar** (`██████▍░░░ 64%`): the bar itself is drawn by the pure `render_bar()` in [utils.rs](#utilsrs), filled with the percentage's palette color over a dim gray track. `usage_bars = false` brings back the plain `(64%)`. Without colors (`NO_COLOR`, a pipe) the bar glyphs are still printed, uncolored.

## utils.rs
Contains **general purpose functions** shared across multiple files, including the pure formatting helpers of the display lines (`format_screen()`, `render_bar()`...). These must undergo the highest level of [testing](#utils_testsrs) as they're used everywhere.

## logo.rs
Loads the ASCII logos from `ascii/`, colors them (the distro's own color, the rainbow Apple stripes or a solid `[logo]` color) and prints them next to the info lines. Widths are counted in **terminal columns** (`chars().count()`), not bytes, since block elements such as `█` are 3 bytes but one column.

## palette.rs
The six accent colors used by the rainbow logo and the usage percentages and bars, in **one place**: a muted 24-bit palette when `COLORTERM` is `truecolor` or `24bit` (the same rule the colored crate uses) and the plain ANSI colors everywhere else. Detection is read once per process. The empty track of the usage bars is a dim gray (`#4a4a4a`, bright black on ANSI terminals).

## cache.rs
Caches the GPU's pretty name, which on **Linux** comes from wgpu and takes ~45ms. Compiled only on Linux: macOS reads its GPU from IOKit in well under a millisecond, so wgpu isn't even a dependency there.

## mod.rs (Platform)
Exposes modules based on operating system, for example:

- If the target OS is **Linux**, the "platform" module will use functions inside linux.rs
- If the target OS is **MacOS**, the "platform" module will use functions inside macos.rs

Note that **every function that's called from shared code must be written in both files** (As usual, the Rust compiler being strict). Linux-only callers such as cache.rs are compiled only on Linux instead of getting macOS stubs.

## linux.rs
Reads `/proc`, `/sys` and `/etc/os-release`, finds the `.rodata` section of ELF binaries (where shells keep their version string), lists displays through display-info and asks `/proc/net` for the default routes and IPv6 address flags.

## macos.rs
Talks to the system directly instead of spawning tools (`system_profiler`, `ioreg` and `ifconfig` take tens of milliseconds each): **IOKit** for the GPU, battery and power adapter, **CoreGraphics** for displays (AppKit would cost ~13ms just to load), `sysctl` for CPU core types and process arguments, a **PF_ROUTE** socket for the default route, an `ioctl` for IPv6 address flags and the Mach-O `__cstring` section for shell versions.

- **Display names** come from CoreDisplay and are only read with two or more displays. CoreDisplay calls every built-in panel "Color LCD", so a built-in panel whose `CGDisplayModelNumber` is in the list of product IDs system_profiler's SPDisplaysReporter checks for `spdisplays_built-in-liquid-retina-xdr` is shown as **Liquid Retina XDR** (no IOKit or CoreDisplay property names the panel type). A panel newer than that list, and not on its plain Liquid Retina (MacBook Air) list, still counts when it runs at ProMotion's 120 Hz, read from the current mode since listing every mode costs ~3.5ms.
- The **AppleSmartBattery** service is looked up once and shared by the Battery and AC Adapter lines. The adapter comes from its `AdapterDetails` (`Watts`, `Name`) and is hidden when `ExternalConnected` is false or no wattage is reported.
- **GPUs** are read from the IORegistry. Only integrated GPUs go untagged, discrete ones keep `[Discrete]` so dual-GPU Intel Macs can tell them apart.

## shared.rs
Contains all functions which run **regardless of OS**, the [sysinfo crate](https://docs.rs/sysinfo/latest/sysinfo/) is most used here.

Every function that uses sysinfo functions requires a **reference to the main System variable** that it will use internally.

Functions inside this file should never handle displaying the values, as that is almost always exclusively handled in [display.rs](#displayrs).

## shell.rs
Finds the shell rustfetch was started from by walking up the parent processes (falling back to `$SHELL`) and gets its version from the **cheapest source first**: an environment variable, the install path (Homebrew Cellar, Nix store), the version string compiled into the binary, and only then `<shell> --version`. That last one costs 2-6ms, so its answer is kept in `shell.toml` in the cache directory until the shell's executable changes. It lives here rather than in platform/ because only reading process info and binary sections differs between OSes, the logic is the same.

## network.rs
Reads every interface address once with `getifaddrs` and picks what the Local IPv4, Local IPv6 and Tailscale IP lines show: the interface carrying the default route, falling back to the first physical one when the route goes through a tunnel without a usable address (a Tailscale exit node). The selection functions are pure so they can be [tested](#tests) on every OS, the same goes for the Linux `/proc/net` parsers kept here.

## tailscale.rs
Tells Tailscale's tunnel apart from other VPNs (on macOS every VPN is a `utun`, so it also needs Tailscale's fixed `fd7a:115c:a1e0::/48` address) and gets the MagicDNS name with one hand-built PTR query to `100.100.100.100`, which takes ~0.5ms where `tailscale status` takes ~30ms. The Tailscale Name line shows only the **machine name**, the first label of that MagicDNS name (`laptop` for `laptop.tailnet.ts.net`).

## utils_tests.rs
<a id="tests"></a>Testing file for utils functions, the other test files follow the same rules. Files named after an OS only build on that OS, everything else is pure and runs everywhere. The testing requirements are usually:
- **Correct input** -> Should return the expected value
- **Edge cases** -> Used to **enhance error handling** and **understand how the function behaves** deeply. These should be **technically correct though very unlikely** inputs (E.g: rounding an f64::Infinity) and checking the output is still Infinity
- **Incorrect input** -> Testing the function with an **input thats incoherent with what the function is asking**, **assessing the function is returning an error** and, if present, checking if the **error message** is correct

## install.sh
Bash automated installation script for the program that can work through curl, its jobs are the following:
- Checks if the OS is supported, if it is not, it refuses to install
- Gets architecture and OS name and normalizes them to **match the release syntax**
- **Detects shell** (zsh, bash exc) to parse its config file, if it is not found it skips automatic shell integration
- Parses the config file to find any system info tools from a static list of the most famous CLIs similar to this (neofetch, fastfetch exc). If any is detected, it uses **regex** to assert the line is a standalone command and not nested in some complicated logic we shouldn't touch
- If any command is found and it is a **standalone**, it asks the user if they want to replace it with rustfetch. If there is no command at all it asks to append rustfetch. Else, it skips shell integration.
- **Fetches the latest version** from the normalized names we got at the beginning and installs it 

> Note: The file is not modified on-the-fly but its lines are assigned into an array and modified there, it then checks if the script modified any line and overwrites the whole file

## runtime.sh
Bash script that automatically calculates the minimum, average and maximum runtime in milliseconds of any given command. It is used as follows:
```bash
scripts/runtime.sh [--runs N] [--warmup N] -- <command> [args...]
```
This script is mostly used to assess runtime differences with new commits. This commands are used to reliably get rustfetch's runtime speed after new features, as well as the comparison with neofetch inside the <a href="../README.md">README</a>:
```bash
cargo build --release
scripts/runtime.sh --runs 30 --warmup 5 -- ./target/release/rustfetch
```