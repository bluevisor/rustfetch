# Flags in rustfetch

Welcome to our in depth guide on how to use flags in rustfetch!

## --help / -h
The main flag everyone should know, it prints every command with a **short description** of what they're used for. Here is an example of its output (v. 0.2.0):
```
Options:
  -a, --all                        Display all info regardless of config
      --reset-config               Regenerates the .toml config file with standard values
  -p, --padding <PADDING>          Adds padding between the logo and the text [default: 1]
  -c, --config-file <CONFIG_FILE>  Uses a different config file. Must provide a valid path
  -h, --help                       Print help
  -V, --version                    Print version
```

## --all / -a
**Enables all modules**, ignoring the `[display]` section of the config file. The `[logo]` settings (`name`, `color`) still apply.

Very useful to try features out.

## --padding / -p \<PADDING>
**Adds padding between the logo and the text**, its range is 0-255 and the default value is 1.

Here are examples of different padding inputs:
<table>
  <tr>
    <th align="left">Command</th>
    <th align="left">Output</th>
  </tr>
  <tr>
    <td>
      <code>rustfetch -p 4</code>
    </td>
    <td>
      <img src="images/flags/0.2.0_padding_4.avif"/>
    </td>
  </tr>
  <tr>
    <td>
      <code>rustfetch -p 0</code>
    </td>
    <td>
      <img src="images/flags/0.2.0_padding_0.avif"/>
    </td>
  </tr>
</table>

## --config-file / -c <CONFIG_FILE_PATH>
Lets you use a **different config file** than the default ``.config/rustfetch/config.toml``. You must provide a **valid path** to the file and in case it doesn't exist yet, the program will create it with defaults.

Here is an example of the command being used:
```bash
rustfetch -c "PATH/TO/FILE"
# or
rustfetch --config-file "PATH/TO/FILE"
```

## --reset-config
Regenerates the main config file with defaults, note that this only regenerates the file at ``.config/rustfetch/config.toml``. To regenerate a custom file, run the following commands:
```bash
rm -rf "PATH/TO/FILE"
rustfetch -c "PATH_TO_FILE"
```