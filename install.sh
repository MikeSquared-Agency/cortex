#!/usr/bin/env sh
# Cortex installer — downloads the correct binary for your platform.
# Usage: curl -sSf https://raw.githubusercontent.com/MikeSquared-Agency/cortex/main/install.sh | sh

set -e

REPO="MikeSquared-Agency/cortex"
BINARY="cortex"

# ── Colours ───────────────────────────────────────────────────────────────────
if [ -t 1 ]; then
    RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; RESET='\033[0m'
else
    RED=''; GREEN=''; YELLOW=''; BOLD=''; RESET=''
fi

info()  { printf "${GREEN}info${RESET}  %s\n" "$1"; }
warn()  { printf "${YELLOW}warn${RESET}  %s\n" "$1"; }
error() { printf "${RED}error${RESET} %s\n" "$1" >&2; exit 1; }

# ── Platform detection ────────────────────────────────────────────────────────
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        *)       error "Unsupported OS: $(uname -s). Please build from source: https://github.com/${REPO}" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "arm64" ;;
        *)             error "Unsupported architecture: $(uname -m). Please build from source: https://github.com/${REPO}" ;;
    esac
}

OS=$(detect_os)
ARCH=$(detect_arch)
ASSET_NAME="${BINARY}-${OS}-${ARCH}"

# ── Install directory ─────────────────────────────────────────────────────────
if [ "$(id -u)" = "0" ]; then
    INSTALL_DIR="/usr/local/bin"
else
    INSTALL_DIR="${HOME}/.local/bin"
fi

# ── Fetch the latest release tag ──────────────────────────────────────────────
info "Detecting latest Cortex release..."

if command -v curl >/dev/null 2>&1; then
    FETCH="curl -sSfL"
elif command -v wget >/dev/null 2>&1; then
    FETCH="wget -qO-"
else
    error "Neither curl nor wget found. Please install one and retry."
fi

API_URL="https://api.github.com/repos/${REPO}/releases/latest"
RELEASE_JSON=$(${FETCH} "${API_URL}" 2>/dev/null) || error "Failed to fetch release info from GitHub."

# Extract tag_name from JSON without jq
VERSION=$(printf '%s' "${RELEASE_JSON}" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
[ -z "${VERSION}" ] && error "Could not determine the latest release version. Is the repo public with releases?"

info "Latest version: ${BOLD}${VERSION}${RESET}"

# ── Build download URL ────────────────────────────────────────────────────────
BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
TARBALL="${BINARY}-${VERSION}-${OS}-${ARCH}.tar.gz"
DOWNLOAD_URL="${BASE_URL}/${TARBALL}"
CHECKSUM_URL="${BASE_URL}/SHA256SUMS"

# ── Download ──────────────────────────────────────────────────────────────────
TMP_DIR=$(mktemp -d)
trap 'rm -rf "${TMP_DIR}"' EXIT

info "Downloading ${TARBALL}..."
if command -v curl >/dev/null 2>&1; then
    curl -sSfL --progress-bar "${DOWNLOAD_URL}" -o "${TMP_DIR}/${TARBALL}" \
        || error "Download failed. Check that ${DOWNLOAD_URL} exists."
else
    wget -q --show-progress "${DOWNLOAD_URL}" -O "${TMP_DIR}/${TARBALL}" \
        || error "Download failed. Check that ${DOWNLOAD_URL} exists."
fi

# ── Verify checksum (optional, skipped if SHA256SUMS not available) ───────────
if command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1; then
    info "Verifying checksum..."
    SUMS_FILE="${TMP_DIR}/SHA256SUMS"
    if command -v curl >/dev/null 2>&1; then
        curl -sSfL "${CHECKSUM_URL}" -o "${SUMS_FILE}" 2>/dev/null || true
    else
        wget -qO "${SUMS_FILE}" "${CHECKSUM_URL}" 2>/dev/null || true
    fi

    if [ -s "${SUMS_FILE}" ]; then
        EXPECTED=$(grep "${TARBALL}" "${SUMS_FILE}" | awk '{print $1}')
        if [ -n "${EXPECTED}" ]; then
            if command -v sha256sum >/dev/null 2>&1; then
                ACTUAL=$(sha256sum "${TMP_DIR}/${TARBALL}" | awk '{print $1}')
            else
                ACTUAL=$(shasum -a 256 "${TMP_DIR}/${TARBALL}" | awk '{print $1}')
            fi
            if [ "${EXPECTED}" != "${ACTUAL}" ]; then
                error "Checksum mismatch! Expected ${EXPECTED}, got ${ACTUAL}. Aborting."
            fi
            info "Checksum verified."
        else
            warn "Could not find checksum for ${TARBALL}, skipping verification."
        fi
    else
        warn "Checksum file not available, skipping verification."
    fi
fi

# ── Extract ───────────────────────────────────────────────────────────────────
info "Extracting..."
tar -xzf "${TMP_DIR}/${TARBALL}" -C "${TMP_DIR}"

EXTRACTED_BIN="${TMP_DIR}/${ASSET_NAME}"
[ -f "${EXTRACTED_BIN}" ] || error "Expected binary '${ASSET_NAME}' not found in tarball."

# ── Install ───────────────────────────────────────────────────────────────────
mkdir -p "${INSTALL_DIR}"
chmod +x "${EXTRACTED_BIN}"
mv "${EXTRACTED_BIN}" "${INSTALL_DIR}/${BINARY}"

info "Installed ${BOLD}${BINARY}${RESET} to ${INSTALL_DIR}/${BINARY}"

# ── PATH hint ─────────────────────────────────────────────────────────────────
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        warn "${INSTALL_DIR} is not in your PATH."
        printf "  Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):\n"
        printf "    ${BOLD}export PATH=\"\${HOME}/.local/bin:\${PATH}\"${RESET}\n"
        ;;
esac

# ── Done ──────────────────────────────────────────────────────────────────────
printf "\n${GREEN}${BOLD}Cortex ${VERSION} installed!${RESET}\n\n"
printf "Get started:\n"
printf "  ${BOLD}cortex init${RESET}    — initialise a new Cortex project\n"
printf "  ${BOLD}cortex serve${RESET}   — start the gRPC + HTTP server\n"
printf "  ${BOLD}cortex --help${RESET}  — full CLI reference\n\n"
printf "Docs: https://github.com/${REPO}#readme\n\n"
