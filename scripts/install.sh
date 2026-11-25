#!/usr/bin/env bash
# Install api-tester on macOS or Linux from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/SamanPandey-in/curlpit/main/scripts/install.sh | bash
#   curl -fsSL .../install.sh | bash -s -- --version 0.1.0
#
# What it does, and nothing more:
#   1. picks the release asset for this OS and CPU,
#   2. downloads it and its .sha256 file into a temporary directory,
#   3. verifies the SHA-256 checksum (aborts on mismatch),
#   4. copies the binary to a user-local bin dir (default ~/.local/bin).
# It never uses sudo and never edits your shell profile; it prints a PATH hint instead.
set -euo pipefail

BIN="api-tester"
REPO="${API_TESTER_REPO:-SamanPandey-in/curlpit}"
INSTALL_DIR="${API_TESTER_INSTALL_DIR:-$HOME/.local/bin}"
VERSION=""

usage() {
  cat <<EOF
Install ${BIN} from https://github.com/${REPO}/releases

Usage: install.sh [--version X.Y.Z] [--bin-dir DIR] [--repo OWNER/NAME]

  --version X.Y.Z   install this release (tag vX.Y.Z) instead of the latest
  --bin-dir DIR     where to put the binary (default: ~/.local/bin,
                    or \$API_TESTER_INSTALL_DIR)
  --repo OWNER/NAME GitHub repository to download from (default: ${REPO})
  -h, --help        show this help
EOF
}

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) [[ $# -ge 2 ]] || die "--version needs a value"; VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --bin-dir) [[ $# -ge 2 ]] || die "--bin-dir needs a value"; INSTALL_DIR="$2"; shift 2 ;;
    --bin-dir=*) INSTALL_DIR="${1#*=}"; shift ;;
    --repo) [[ $# -ge 2 ]] || die "--repo needs a value"; REPO="$2"; shift 2 ;;
    --repo=*) REPO="${1#*=}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done

# --- Which asset? -----------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$arch" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) die "unsupported CPU architecture: $arch" ;;
esac
case "$os" in
  Darwin) target="${arch}-apple-darwin" ;;
  Linux) target="${arch}-unknown-linux-gnu" ;;
  *) die "unsupported OS: $os (on Windows, use scripts/install.ps1)" ;;
esac

asset="${BIN}-${target}.tar.gz"
if [[ -n "$VERSION" ]]; then
  tag="v${VERSION#v}"   # accept both 0.1.0 and v0.1.0
  base="https://github.com/${REPO}/releases/download/${tag}"
else
  tag="latest"
  base="https://github.com/${REPO}/releases/latest/download"
fi

# --- Tools ------------------------------------------------------------------
if command -v curl > /dev/null 2>&1; then
  fetch() { curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"; }
elif command -v wget > /dev/null 2>&1; then
  fetch() { wget --https-only -q -O "$2" "$1"; }
else
  die "need curl or wget"
fi

if command -v sha256sum > /dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum > /dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "need sha256sum or shasum to verify the download"
fi

command -v tar > /dev/null 2>&1 || die "need tar"

# --- Download and verify ----------------------------------------------------
tmp="$(mktemp -d "${TMPDIR:-/tmp}/${BIN}-install.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

say "Downloading ${asset} (${tag}) from ${REPO}..."
fetch "${base}/${asset}" "${tmp}/${asset}" || die "download failed: ${base}/${asset}"
fetch "${base}/${asset}.sha256" "${tmp}/${asset}.sha256" \
  || die "checksum download failed: ${base}/${asset}.sha256"

expected="$(cut -d' ' -f1 < "${tmp}/${asset}.sha256" | tr 'A-F' 'a-f')"
actual="$(sha256 "${tmp}/${asset}")"
[[ ${#expected} -eq 64 ]] || die "malformed checksum file for ${asset}"
if [[ "$expected" != "$actual" ]]; then
  die "checksum mismatch for ${asset}: expected ${expected}, got ${actual}"
fi
say "Checksum OK (sha256 ${actual})."

# --- Install ----------------------------------------------------------------
tar -xzf "${tmp}/${asset}" -C "$tmp"
src="${tmp}/${BIN}-${target}/${BIN}"
[[ -f "$src" ]] || die "archive did not contain ${BIN}-${target}/${BIN}"

mkdir -p "$INSTALL_DIR"
dest="${INSTALL_DIR%/}/${BIN}"
# Copy then rename, so a running copy of the old binary is never half-overwritten.
cp "$src" "${dest}.new"
chmod 755 "${dest}.new"
mv -f "${dest}.new" "$dest"
say "Installed ${BIN} to ${dest}"

case ":${PATH}:" in
  *":${INSTALL_DIR%/}:"*) say "Run it with: ${BIN}" ;;
  *)
    say ""
    say "${INSTALL_DIR%/} is not on your PATH. Add it, e.g. in ~/.bashrc or ~/.zshrc:"
    say "  export PATH=\"${INSTALL_DIR%/}:\$PATH\""
    ;;
esac
