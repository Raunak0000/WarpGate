#!/usr/bin/env bash
# ==============================================================================
# WarpGate Universal Installer
# One-line install: curl -fsSL https://raw.githubusercontent.com/Raunak0000/WarpGate/main/install.sh | bash
# ==============================================================================

set -euo pipefail

REPO="Raunak0000/WarpGate"
GITHUB_URL="https://github.com/${REPO}"
GITHUB_API="https://api.github.com/repos/${REPO}/releases/latest"

BOLD='\033[1m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${BOLD}${BLUE}"
echo "============================================================"
echo "          Installing WarpGate Ephemeral Cloud VPN           "
echo "============================================================"
echo -e "${NC}"

# 1. Detect Operating System
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$OS" in
    linux)   OS="linux" ;;
    darwin)  OS="darwin" ;;
    msys*|mingw*|cygwin*) OS="windows" ;;
    *)
        echo -e "${RED}[ERROR] Unsupported operating system: ${OS}${NC}"
        exit 1
        ;;
esac

# 2. Detect CPU Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)   ARCH="amd64" ;;
    aarch64|arm64)  ARCH="arm64" ;;
    *)
        echo -e "${RED}[ERROR] Unsupported CPU architecture: ${ARCH}${NC}"
        exit 1
        ;;
esac

ASSET_NAME="warpgate-${OS}-${ARCH}.tar.gz"
echo -e "Detected Platform: ${BOLD}${OS} (${ARCH})${NC}"

# 3. Determine Installation Directory
if [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
elif [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    INSTALL_DIR="$HOME/.local/bin"
else
    INSTALL_DIR="/usr/local/bin"
fi

# 4. Check if local compiled binary is available in repo (for local installation)
LOCAL_BIN="rust/target/release/warpgate"
LOCAL_DEBUG_BIN="rust/target/debug/warpgate"

if [ -f "$LOCAL_BIN" ] && [ "${1:-}" = "--local" ]; then
    echo -e "Installing from local release build: ${LOCAL_BIN}..."
    cp "$LOCAL_BIN" "${INSTALL_DIR}/warpgate"
    chmod +x "${INSTALL_DIR}/warpgate"
elif [ -f "$LOCAL_DEBUG_BIN" ] && [ "${1:-}" = "--local" ]; then
    echo -e "Installing from local debug build: ${LOCAL_DEBUG_BIN}..."
    cp "$LOCAL_DEBUG_BIN" "${INSTALL_DIR}/warpgate"
    chmod +x "${INSTALL_DIR}/warpgate"
else
    # Download latest release asset from GitHub
    echo -e "Fetching latest release information from GitHub (${REPO})..."
    
    TMP_DIR="$(mktemp -d -t warpgate-install-XXXXXX)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    LATEST_TAG=$(curl -sSL "$GITHUB_API" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
    
    if [ -z "$LATEST_TAG" ]; then
        echo -e "${YELLOW}[WARN] Could not retrieve release tag from GitHub API.${NC}"
        echo -e "If running from source, run: cargo build --release --manifest-path rust/Cargo.toml && ./install.sh --local"
        
        # Check if we have source code locally to compile directly
        if [ -f "rust/Cargo.toml" ]; then
            echo -e "\n${BLUE}Building directly from local source with Cargo...${NC}"
            cargo build --release --manifest-path rust/Cargo.toml --bin warpgate
            cp "rust/target/release/warpgate" "${INSTALL_DIR}/warpgate"
            chmod +x "${INSTALL_DIR}/warpgate"
        else
            echo -e "${RED}[ERROR] No release asset found and no local source available.${NC}"
            exit 1
        fi
    else
        DOWNLOAD_URL="${GITHUB_URL}/releases/download/${LATEST_TAG}/${ASSET_NAME}"
        echo -e "Downloading ${BOLD}${ASSET_NAME}${NC} (${LATEST_TAG})..."
        curl -sSL -o "${TMP_DIR}/${ASSET_NAME}" "$DOWNLOAD_URL"

        echo -e "Extracting binary..."
        tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "$TMP_DIR"

        if [ ! -w "$INSTALL_DIR" ]; then
            echo -e "Requesting sudo to install to ${INSTALL_DIR}..."
            sudo cp "${TMP_DIR}/warpgate" "${INSTALL_DIR}/warpgate"
            sudo chmod +x "${INSTALL_DIR}/warpgate"
        else
            cp "${TMP_DIR}/warpgate" "${INSTALL_DIR}/warpgate"
            chmod +x "${INSTALL_DIR}/warpgate"
        fi
    fi
fi

echo -e "\n${GREEN}[✓] WarpGate successfully installed to: ${BOLD}${INSTALL_DIR}/warpgate${NC}\n"

# 5. Check PATH
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo -e "${YELLOW}[NOTICE] ${INSTALL_DIR} is not in your current PATH.${NC}"
    echo -e "Add it to your shell profile by running:"
    echo -e "  export PATH=\"\$PATH:${INSTALL_DIR}\""
    echo -e "  echo 'export PATH=\"\$PATH:${INSTALL_DIR}\"' >> ~/.bashrc (or ~/.zshrc)\n"
fi

# 6. Verify Installation and Run Diagnostics
echo -e "${BLUE}Running initial WarpGate diagnostics...${NC}\n"
"${INSTALL_DIR}/warpgate" doctor || true

echo -e "${GREEN}${BOLD}Installation complete! Type 'warpgate' anytime to get started.${NC}"
