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
    ├── <a href="#tailscalers">tailscale.rs</a> # Tailscale detection and MagicDNS lookup
    └── <a href="#wifirs">wifi.rs</a>     # Wi-Fi network name and its cache
tests/              # Tests for "cargo test"
├── <a href="#utils_testsrs">utils_tests.rs</a>  # Tests specific to utils
├── config_tests.rs  # Old config files still parse
├── logo_tests.rs    # [logo] and key_color parsing, the auto key color, render_key(), Seen logo leaf map and coloring
├── palette_tests.rs # Palette, usage bar track color, truecolor detection, logo shape
├── shell_tests.rs   # Shell name/version parsing and the version cache
├── network_tests.rs # Interface and address selection, /proc/net parsers
├── tailscale_tests.rs # Tailscale checks, PTR query, DNS reply parsing and machine names
├── wifi_tests.rs    # Wi-Fi fingerprint, name cleanup and the name cache
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
-  If the "--all" or "-a" flag is given, **enables all modules** and only reads the `[logo]` table, `usage_bars` and `key_color` of the config file (how lines look, not which ones show), without creating it (This does not skip unsupported [platform specific modules](#platform)).
<br>Else, **gets config options** from [config.rs](#configrs) and shows them based on their boolean value.
- Gets the distro id from the dedicated function inside the **platform** crate
- Picks the logo once: the distro's own one, or the Seen Health logo when `[logo]` sets `name = "seen"`. The same `logo_id` loads the logo lines, colors them and picks the key color
- Works out the **key color** (the text before each line's colon) once, from `key_color` and the logo about to be drawn (see [logo.rs](#logors)), and hands it to [display.rs](#displayrs) before any line is formatted
- Creates a **String vector** and adds the return values of the [display functions](#displayrs) to it if the config boolean for it is true and if the function returns Some(), if it returns None or the config for it is false it **just skips it**.
- Prints the info_lines vector alongside the logo's lines and **adds padding** to make all the lines be horizontally aligned. As the line get printed, they are colored according to the logo

## config.rs
Main file for configuration handling, **creates and parses a config file** or enables all features:

- Creates a **DisplayConfig** struct that includes all modules as boolean values
- **Creates two implementations** for DisplayConfig: **Default** for initial file creation and **All** where all modules are set to true
- **load_config()** -> returns a DisplayConfig struct from parsing the **config.toml**. If the file does not exist it will create it in the [default config directory](https://docs.rs/dirs/latest/dirs/fn.config_dir.html) and print a message.
- **load_all_config()** -> turns every module on and keeps the `[logo]` settings (`name`, `color`), `usage_bars` and `key_color`, read without creating the file or printing anything.
- Lines added after the config format was released (shell, usage_bars, ipv4, ipv6, wifi, tailscale_ip, tailscale_name, ac_adapter), `key_color` (`"auto"`) and the whole `[logo]` table (`name`, `color`) are **defaulted**, so older config files still parse. A `[logo]` value of the wrong type falls back to its own default instead of resetting the file.
- `ac_adapter` doesn't add a line of its own: it names the power adapter inside the Battery line's brackets (see [display.rs](#displayrs)), so with `battery = false` there is no adapter either.
- Renamed keys keep reading their **old name**: `tailscale_name` was `tailscale_host`, so a config that still says `tailscale_host = false` keeps the line hidden. The old key is its own `Option` field rather than a serde alias, since an alias fails with "duplicate field" (and resets every setting) on a file that has both; `show_tailscale_name()` lets the new key win.

## cli.rs
Uses [clap](https://docs.rs/clap/latest/clap/) to **parse command line arguments** and creates a public Cli struct with all the possible flags in it. This file also decides which description every flag should have when running "rustfetch --help".

Example of a standard flag:
![Example](./images/cli_argument_description.avif)

## display.rs
Contains all functions related to showing the values returned from other files as formatted text.

Also contains internal private functions such as **color_percentage()** and **format_usage()**, specific to display features. Every line's key goes through **format_key()**: bold, with the text before the colon (`OS`, `Disk (/)`, `Screen (Liquid Retina XDR)`) in the key color that main sets once with `set_key_color()` (a `OnceLock`, so it isn't worked out per line), while the colon and the value keep their own colors. The drawing itself is the pure `render_key()`, tested on its own. Without a key color it is the plain bold `OS:` from before. The title line and its dashes are left alone. The logo column is padded from the logo's uncolored width and each info line is written after that padding, so the key's escape codes never affect alignment. The RAM and disk lines end in a 10 cell **usage bar** (`██████▍░░░ 64%`): the bar itself is drawn by the pure `render_bar()` in [utils.rs](#utilsrs), filled with the percentage's palette color over a dim gray track. `usage_bars = false` brings back the plain `(64%)`. Without colors (`NO_COLOR`, a pipe) the bar glyphs are still printed, uncolored.

The Battery line's brackets come from the pure `format_battery_status()`: while an adapter is plugged in it follows the status (`Charging, 140W USB-C Power Adapter`, `Full, 140W ...`), and it replaces the plain `AC Connected`, which only says that one is plugged in. On battery, or when the adapter can't be named (Linux, `ac_adapter = false`), the status word is left as it is.

## utils.rs
Contains **general purpose functions** shared across multiple files, including the pure formatting helpers of the display lines (`format_screen()`, `render_bar()`...). These must undergo the highest level of [testing](#utils_testsrs) as they're used everywhere.

## logo.rs
Loads the ASCII logos from `ascii/`, colors them (the distro's own color, the rainbow Apple stripes or a solid `[logo]` color) and prints them next to the info lines. The distro colors live in `distro_logo_color()`, shared by the logo lines and the **"auto" key color**: `auto_key_color()` maps the id of the logo being drawn (main passes the same `logo_id` to it, `get_logo_lines()` and `print_logo()`: the distro's own logo or the Seen one) to its keys' color in one match: a solid `[logo]` color when one is set, yellow next to the Apple logo whether white or rainbow (fastfetch's macOS key color, Apple's `#fdb827` stripe on truecolor terminals and ANSI yellow elsewhere), the brand teal of the right leaf next to the Seen logo whether default or rainbow (`#5DC6B2` on truecolor terminals, ANSI cyan elsewhere) and the distro's own color otherwise, with plain bold keys when there's neither a logo nor a solid color. Giving another logo its own key color is one more arm there; a logo id `distro_logo_color()` doesn't know gets white. `key_color` and the `[logo]` color are parsed by the same colored parser (color names, `#rrggbb`), behind a check that turns non-ASCII away before colored's hex parser slices it by byte; an invalid `key_color` falls back to auto. In the config file `key_color = false` reads as `"none"` and any other value that isn't a string as `"auto"`, so a wrong type there doesn't reset the whole file. Widths are counted in **terminal columns** (`chars().count()`), not bytes, since block elements such as `█` are 3 bytes but one column.

`[logo] name = "seen"` swaps the distro's logo for the **Seen Health ginkgo** (`ascii/seen.txt`, unknown names fall back to `"auto"`). It is colored **per cell**, not per line: `ascii/seen_colors.txt` has the same shape and marks every drawn cell with its leaf (1 top-left, 2 right, 3 bottom), and runs of same-leaf cells share one escape code. The leaf colors are the brand ones on truecolor terminals (same detection as [palette.rs](#paletters)) and bright green, bright cyan and cyan elsewhere; a solid `[logo]` color still paints the whole logo (and its keys, see above).

## palette.rs
The six accent colors used by the rainbow logo, the macOS key color and the usage percentages and bars, in **one place**: a muted 24-bit palette when `COLORTERM` is `truecolor` or `24bit` (the same rule the colored crate uses) and the plain ANSI colors everywhere else. Detection is read once per process. The empty track of the usage bars is a dim gray (`#4a4a4a`, bright black on ANSI terminals).

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
Talks to the system directly instead of spawning tools (`system_profiler`, `ioreg` and `ifconfig` take tens of milliseconds each): **IOKit** for the GPU, battery and power adapter, **CoreGraphics** for displays (AppKit would cost ~13ms just to load), `sysctl` for CPU core types, process arguments and the routing table, a **PF_ROUTE** socket for the default route, `ioctl`s for IPv6 address flags and Wi-Fi link status, configd's **dynamic store** for the Wi-Fi name and the Mach-O `__cstring` section for shell versions.

- **Display names** come from CoreDisplay and are only read with two or more displays. CoreDisplay calls every built-in panel "Color LCD", so a built-in panel whose `CGDisplayModelNumber` is in the list of product IDs system_profiler's SPDisplaysReporter checks for `spdisplays_built-in-liquid-retina-xdr` is shown as **Liquid Retina XDR** (no IOKit or CoreDisplay property names the panel type). A panel newer than that list, and not on its plain Liquid Retina (MacBook Air) list, still counts when it runs at ProMotion's 120 Hz, read from the current mode since listing every mode costs ~3.5ms.
- The **AppleSmartBattery** service is looked up once and shared by the battery status and the power adapter the Battery line names. The adapter comes from its `AdapterDetails` (`Watts`, `Name`) and is left out when `ExternalConnected` is false or no wattage is reported.
- **Wi-Fi** needs no CoreWLAN until a new network shows up. An interface counts as associated when the `SIOCGIFMEDIA` ioctl reports an 802.11 media type with an active link (`ifconfig`'s "status: active", ~0.03ms). Its network is told apart by the interface's own MAC address (a `NET_RT_IFLIST` sysctl), the gateway of its default route and that gateway's MAC address, from two `NET_RT_FLAGS` sysctls that list only the gateway routes and the ARP entries (what `arp -a` reads), so a VPN pushing thousands of routes doesn't slow it down (~0.1ms all together, no `arp` or `scutil` spawned). See [wifi.rs](#wifirs) for how that fingerprint is cached.
- The **Wi-Fi name** is redacted by macOS for programs without Location Services: CoreWLAN's `ssid` is nil, `scutil`'s `SSID_STR` is empty and `ipconfig getsummary` prints `<redacted>`. The `CachedScanRecord` of configd's `State:/Network/Interface/<if>/AirPort` key, an NSKeyedArchiver archive of the joined network's scan result, still holds `SSID_STR` and is decoded with Foundation's NSKeyedUnarchiver (secure decoding, plist classes only), ~1ms. When it's missing, or blanked (empty, `<redacted>`) the way macOS hides the other sources, CoreWLAN is opened with `dlopen` (linking it would cost every run dyld time) and asked through objc2: `ssid` when Location Services were granted, else the first of the interface's known networks like fastfetch does (~18ms, nearly all loading the configuration). That last one is a **guess**: CoreWLAN sorts known networks by their private `lastConnected` date, which isn't updated on every automatic join, so right after moving between two known networks it can name the previous one. It is only used on an interface in station mode, since `awdl0` lists the same known networks, and [wifi.rs](#wifirs) caches it as a guess.
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

## wifi.rs
Picks the Wi-Fi line's interface (a physical interface with an IPv4 address that [macos.rs](#macosrs) says is associated) and gets the network's name, which costs ~1ms (and ~18ms through CoreWLAN) where everything else about the link costs ~0.1ms. So names are cached in `wifi.toml` in the cache directory, written atomically like `shell.toml` (both go through `read_cache_file()`/`write_cache_file()` in [utils.rs](#utilsrs)), under a **fingerprint** of the network: interface, its own MAC address, router IPv4 and router MAC (`en0 0a:6f:b5:20:c6:02 172.18.7.1 cc:03:d9:3a:ed:d0`). macOS gives each network its own private Wi-Fi address by default, which tells apart two networks behind the same router; with private addresses turned off, two SSIDs bridged onto one router share a fingerprint and the first one seen is shown for both. Without a fingerprint nothing is cached and the name is read every run (~1ms, ~18ms through CoreWLAN): that's without a router (no DHCP lease), and also while the router has no ARP entry (not resolved yet, or expired on an idle Mac), since a fingerprint without the router's MAC would mix up every network behind a `192.168.1.1`. A name that can't be read is never cached either, so a network neither source can name is asked about on every run. A name CoreWLAN only **guessed** is stored with `guessed = true` and trusted for 15 minutes (`GUESSED_SSID_TTL`), then read again, so an exact name replaces it as soon as one can be read and a wrong guess doesn't stick until `--clear-cache`. Only interfaces with an IPv4 address are looked at, which keeps `awdl0` (link-local IPv6 only) out but also hides the line on an IPv6-only Wi-Fi network. Up to 32 networks are kept, the one added longest ago makes room, and `--clear-cache` forgets them all (for a renamed network). Names are stripped of control characters, cached ones too since the file can be edited by hand. Linux has no Wi-Fi line yet: its platform functions return None.

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