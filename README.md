# wifimanager

A terminal Wi-Fi manager for Linux that drives NetworkManager directly over D-Bus.

It shows your Wi-Fi devices in one pane and the networks they can see in the other, and lets you join, forget, rescan, toggle the radio, and enable or disable a device without leaving the keyboard. State comes from NetworkManager's own signals, so the screen updates as soon as something changes rather than on a poll.

## Install

Prebuilt binaries are published for Linux on amd64 and arm64.

```sh
curl -fsSL https://raw.githubusercontent.com/andrewtheguy/wifimanager/main/install.sh | sh
```

The script downloads the latest release to `/usr/local/bin` and uses `sudo` only if that directory is not writable. Two variables adjust it:

```sh
WIFIMANAGER_VERSION=v0.0.1 WIFIMANAGER_INSTALL_DIR=~/.local/bin sh install.sh
```

Or grab a binary yourself from the [releases page](https://github.com/andrewtheguy/wifimanager/releases), or build from source with a recent Rust toolchain:

```sh
cargo install --git https://github.com/andrewtheguy/wifimanager
```

## Run

```sh
wifimanager
```

There are no options beyond `--help` and `--version`. Press `?` inside for the key reference.

NetworkManager must be running. Reading state works from any session. Changing anything (joining, scanning, toggling the radio) goes through polkit, so run from a local login session or as root. Disabling a device also writes a drop-in under `/etc/NetworkManager/conf.d` so it stays disabled across reboots, and that needs root:

```sh
sudo wifimanager
```

## Keys

| Key | Action |
| --- | --- |
| `↑ ↓` / `j k` | move within the focused pane |
| `tab` / `← →` | switch between devices and networks |
| `g` / `G` | jump to the first or last row |
| `enter` | join the selected network |
| `p` | re-enter the password, then reconnect |
| `n` | join a network by name (hidden SSID) |
| `f` | delete the saved profile for this network |
| `d` | disconnect the selected device |
| `s` / `r` | rescan on the selected device |
| `w` | turn the Wi-Fi radio on or off |
| `a` | toggle autoconnect on the selected device |
| `e` | enable or disable the selected device (persists) |
| `esc` | dismiss a message or close a dialog |
| `q` / `ctrl-c` | quit |

## Releasing

Bump `version` in `Cargo.toml`, then run the **Release (Manual)** workflow from the Actions tab. It builds both architectures, creates the tag, and publishes the binaries as `wifimanager-linux-amd64` and `wifimanager-linux-arm64`, which is what the install script expects.
