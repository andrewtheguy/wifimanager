#!/bin/sh
# Quick installer for wifimanager.
#
#   curl -fsSL https://raw.githubusercontent.com/andrewtheguy/wifimanager/main/install.sh | sh
#
# Downloads the latest release binary for this machine and puts it on PATH.
#
#   WIFIMANAGER_VERSION=v0.0.1   install a specific tag instead of the latest
#   WIFIMANAGER_INSTALL_DIR=DIR  install somewhere other than /usr/local/bin
set -eu

repo="andrewtheguy/wifimanager"
version="${WIFIMANAGER_VERSION:-latest}"
install_dir="${WIFIMANAGER_INSTALL_DIR:-/usr/local/bin}"

die() { echo "install.sh: $*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "wifimanager drives NetworkManager and only runs on Linux"

case "$(uname -m)" in
  x86_64 | amd64) arch="amd64" ;;
  aarch64 | arm64) arch="arm64" ;;
  *) die "no prebuilt binary for $(uname -m); build from source with cargo install --git https://github.com/$repo" ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  die "need curl or wget"
fi

if [ "$version" = "latest" ]; then
  url="https://github.com/$repo/releases/latest/download/wifimanager-linux-$arch"
else
  url="https://github.com/$repo/releases/download/$version/wifimanager-linux-$arch"
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

echo "downloading $url"
fetch "$url" "$tmp" || die "download failed; does release '$version' exist with a linux-$arch asset?"
chmod 755 "$tmp"

if [ -w "$install_dir" ] || { [ ! -e "$install_dir" ] && [ -w "$(dirname "$install_dir")" ]; }; then
  sudo=""
elif command -v sudo >/dev/null 2>&1; then
  sudo="sudo"
  echo "installing to $install_dir needs root; sudo may prompt for your password"
else
  die "$install_dir is not writable and sudo is not available; set WIFIMANAGER_INSTALL_DIR"
fi

$sudo mkdir -p "$install_dir"
$sudo install -m 755 "$tmp" "$install_dir/wifimanager"

echo "installed $("$install_dir/wifimanager" --version) to $install_dir/wifimanager"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) echo "note: $install_dir is not on your PATH" ;;
esac
