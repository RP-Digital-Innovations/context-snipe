#!/bin/sh
# context-snipe installer — macOS & Linux
#
#   curl -fsSL https://raw.githubusercontent.com/RP-Digital-Innovations/context-snipe/main/install.sh | sh
#
# Downloads the right prebuilt binary for your platform from the latest
# GitHub release and installs it onto your PATH. No package manager, no build.
set -eu

REPO="RP-Digital-Innovations/context-snipe"
BIN="context-snipe"

# Allow overriding the install dir: INSTALL_DIR=~/bin sh install.sh
INSTALL_DIR="${INSTALL_DIR:-}"

err() { printf 'context-snipe install: %s\n' "$1" >&2; exit 1; }

os="$(uname -s)"
arch="$(uname -m)"

case "${os}-${arch}" in
  Darwin-arm64)        asset="context-snipe-aarch64-apple-darwin" ;;
  Darwin-x86_64)       asset="context-snipe-x86_64-apple-darwin" ;;
  Linux-x86_64)        asset="context-snipe-x86_64-linux" ;;
  Linux-aarch64|Linux-arm64) asset="context-snipe-aarch64-linux" ;;
  *) err "unsupported platform: ${os}-${arch}. See https://github.com/${REPO}/releases for manual downloads." ;;
esac

url="https://github.com/${REPO}/releases/latest/download/${asset}"

# Pick an install dir: explicit override, else a writable system dir, else ~/.local/bin.
if [ -z "${INSTALL_DIR}" ]; then
  if [ -w /usr/local/bin ] 2>/dev/null; then
    INSTALL_DIR=/usr/local/bin
  elif command -v sudo >/dev/null 2>&1; then
    INSTALL_DIR=/usr/local/bin
    USE_SUDO=1
  else
    INSTALL_DIR="${HOME}/.local/bin"
  fi
fi
mkdir -p "${INSTALL_DIR}" 2>/dev/null || true

tmp="$(mktemp)"
printf 'Downloading %s...\n' "${asset}"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "${url}" -o "${tmp}" || err "download failed: ${url}"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "${tmp}" "${url}" || err "download failed: ${url}"
else
  err "need curl or wget to download the binary"
fi

chmod +x "${tmp}"

dest="${INSTALL_DIR}/${BIN}"
if [ "${USE_SUDO:-0}" = "1" ]; then
  sudo mv "${tmp}" "${dest}"
else
  mv "${tmp}" "${dest}" 2>/dev/null || err "could not write to ${INSTALL_DIR}. Re-run with INSTALL_DIR=~/.local/bin"
fi

printf '\n  Installed %s -> %s\n' "${BIN}" "${dest}"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *) printf '\n  Note: %s is not on your PATH. Add this to your shell profile:\n      export PATH="%s:$PATH"\n' "${INSTALL_DIR}" "${INSTALL_DIR}" ;;
esac

printf '\n  Verify:   %s --version\n  Next:     add it to your AI tool — https://github.com/%s#add-to-your-ai-tool-60-seconds\n\n' "${BIN}" "${REPO}"
