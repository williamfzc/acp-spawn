#!/usr/bin/env bash
# One-click installer for acp-spawn — downloads the latest release binary from GitHub.

set -euo pipefail

REPO="williamfzc/acp-spawn"
BINARY="acp-spawn"

info()  { printf '[info]  %s\n' "$*" >&2; }
warn()  { printf '[warn]  %s\n' "$*" >&2; }
error() { printf '[error] %s\n' "$*" >&2; }

detect_os() {
    local uname_out
    uname_out="$(uname -s)"
    case "${uname_out}" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        *)       error "Unsupported OS: ${uname_out}"; exit 1 ;;
    esac
}

detect_arch() {
    local uname_m
    uname_m="$(uname -m)"
    case "${uname_m}" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *)             error "Unsupported architecture: ${uname_m}"; exit 1 ;;
    esac
}

latest_version() {
    local url="https://api.github.com/repos/${REPO}/releases/latest"
    curl -fsSL "${url}" | grep '"tag_name"' | head -1 | sed -E 's/.*"([^"]+)".*/\1/'
}

install_dir() {
    if [ -w "/usr/local/bin" ] && [ -z "${INSTALL_DIR:-}" ]; then
        echo "/usr/local/bin"
    else
        echo "${INSTALL_DIR:-${HOME}/.local/bin}"
    fi
}

download() {
    local version="$1" os="$2" arch="$3" dest="$4"
    local filename="${BINARY}-${os}-${arch}.tar.gz"
    local url="https://github.com/${REPO}/releases/download/${version}/${filename}"

    info "Downloading ${filename} ..."
    if ! curl -fsSL "${url}" -o "${dest}/${filename}"; then
        error "Download failed. ${os}-${arch} binary may not be available yet."
        error "Check available assets at: https://github.com/${REPO}/releases/${version}"
        exit 1
    fi

    echo "${dest}/${filename}"
}

main() {
    local os arch version dest_dir archive binary_path

    os="$(detect_os)"
    arch="$(detect_arch)"
    version="$(latest_version)"
    dest_dir="$(install_dir)"

    info "acp-spawn ${version}  os=${os}  arch=${arch}  dest=${dest_dir}"

    TMPDIR="$(mktemp -d)"
    export TMPDIR
    trap 'rm -rf "${TMPDIR}"' EXIT

    archive="$(download "${version}" "${os}" "${arch}" "${TMPDIR}")"

    info "Extracting ..."
    tar xzf "${archive}" -C "${TMPDIR}"

    if [ ! -f "${TMPDIR}/${BINARY}" ]; then
        error "Binary '${BINARY}' not found in archive."
        exit 1
    fi

    mkdir -p "${dest_dir}"
    install -m 755 "${TMPDIR}/${BINARY}" "${dest_dir}/${BINARY}"

    binary_path="${dest_dir}/${BINARY}"
    info "Installed acp-spawn to ${binary_path}"

    if ! command -v acp-spawn >/dev/null 2>&1; then
        warn "'acp-spawn' is not on your PATH."
        warn "Add ${dest_dir} to your PATH, for example:"
        warn "  echo 'export PATH=\"${dest_dir}:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
    else
        info "Done! $(acp-spawn --version 2>/dev/null || echo "acp-spawn is ready.")"
    fi
}

main "$@"
