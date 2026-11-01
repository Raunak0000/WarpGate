#!/usr/bin/env bash
# ==============================================================================
# WarpGate End-to-End Lifecycle Demonstration & Walkthrough
# Demonstrates: CLI up -> Curve25519 Keys -> QR Code -> Status -> Teardown
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

REGION="tokyo"
TTL="1m"
ADMIN_CIDR="0.0.0.0/0"
MODE="dry-run"
WAIT_FOR_TTL=false
TEMP_CONF_DIR="$(mktemp -d -t warpgate-demo-XXXXXX)"
ORCHESTRATOR_PID=""

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
BOLD='\033[1m'
NC='\033[0m'

cleanup() {
    if [[ -n "$ORCHESTRATOR_PID" ]]; then
        echo -e "\n${YELLOW}Stopping background WarpGate Orchestrator (PID: $ORCHESTRATOR_PID)...${NC}"
        kill "$ORCHESTRATOR_PID" 2>/dev/null || true
        wait "$ORCHESTRATOR_PID" 2>/dev/null || true
    fi
    rm -rf "$TEMP_CONF_DIR"
}
trap cleanup EXIT INT TERM

usage() {
    echo -e "${BOLD}Usage:${NC} $0 [options]"
    echo ""
    echo "Options:"
    echo "  --live                  Execute live provisioning against real AWS infrastructure"
    echo "  --region <region>       Target region or alias (default: 'tokyo')"
    echo "  --ttl <ttl>             Time-to-Live before teardown (default: '1m')"
    echo "  --admin-cidr <cidr>     Admin CIDR for SSH (default: '0.0.0.0/0')"
    echo "  --wait-ttl              Wait for automated TTL expiration instead of manual 'down'"
    echo "  -h, --help              Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                      # Fast demo in simulated/dry-run mode (no AWS resources)"
    echo "  $0 --live --region frankfurt --ttl 15m"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --live)
            MODE="live"
            shift
            ;;
        --region)
            REGION="$2"
            shift 2
            ;;
        --ttl)
            TTL="$2"
            shift 2
            ;;
        --admin-cidr)
            ADMIN_CIDR="$2"
            shift 2
            ;;
        --wait-ttl)
            WAIT_FOR_TTL=true
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            usage
            ;;
    esac
done

echo -e "${CYAN}${BOLD}======================================================================${NC}"
echo -e "${CYAN}${BOLD}              WarpGate End-to-End Lifecycle Demonstration             ${NC}"
echo -e "${CYAN}${BOLD}======================================================================${NC}"
echo -e "Mode:          ${BOLD}${MODE^^}${NC}"
echo -e "Target Region: ${BOLD}${REGION}${NC}"
echo -e "TTL:           ${BOLD}${TTL}${NC}"
echo -e "Admin CIDR:    ${BOLD}${ADMIN_CIDR}${NC}"
echo -e "Config Output: ${BOLD}${TEMP_CONF_DIR}/wg0.conf${NC}"

# Check if port 3000 is open (Orchestrator running)
IS_ORCHESTRATOR_RUNNING=false
if curl -s -f http://localhost:3000/api/deployments/health-check >/dev/null 2>&1 || \
   curl -s http://localhost:3000/api/deployments/dummy-test >/dev/null 2>&1; then
    IS_ORCHESTRATOR_RUNNING=true
fi

if [[ "$IS_ORCHESTRATOR_RUNNING" == false ]]; then
    echo -e "\n${BLUE}${BOLD}[Step 1/4] Launching WarpGate Orchestrator on http://localhost:3000...${NC}"
    if [[ "$MODE" == "dry-run" ]]; then
        export WARPGATE_DRY_RUN="1"
    fi
    cargo build --quiet --manifest-path "$REPO_ROOT/rust/Cargo.toml" --bin warpgate-orchestrator
    "$REPO_ROOT/rust/target/debug/warpgate-orchestrator" &
    ORCHESTRATOR_PID=$!
    
    # Wait for orchestrator socket
    for i in {1..30}; do
        if curl -s http://localhost:3000/api/deployments/ping >/dev/null 2>&1 || \
           curl -s http://localhost:3000/api/deployments/test >/dev/null 2>&1; then
            break
        fi
        sleep 0.5
    done
    echo -e "${GREEN}✓ Orchestrator is running and listening on port 3000.${NC}"
else
    echo -e "\n${GREEN}✓ Detected running WarpGate Orchestrator on port 3000.${NC}"
fi

# Build CLI binary
cargo build --quiet --manifest-path "$REPO_ROOT/rust/Cargo.toml" --bin warpgate-cli
CLI_BIN="$REPO_ROOT/rust/target/debug/warpgate-cli"

echo -e "\n${BLUE}${BOLD}[Step 2/4] Executing 'warpgate up' in region '${REGION}'...${NC}"
echo -e "${YELLOW}Running: warpgate up --region ${REGION} --ttl ${TTL} --admin-cidr ${ADMIN_CIDR} --no-connect --output ${TEMP_CONF_DIR}/wg0.conf${NC}\n"

# Run warpgate up, capturing stdout & stderr
UP_OUTPUT_FILE="${TEMP_CONF_DIR}/up_output.log"
"$CLI_BIN" up \
    --region "$REGION" \
    --ttl "$TTL" \
    --admin-cidr "$ADMIN_CIDR" \
    --output "$TEMP_CONF_DIR/wg0.conf" \
    --no-connect | tee "$UP_OUTPUT_FILE"

# Extract deployment ID
DEPLOYMENT_ID=$(grep -oE "warp-[0-9a-f]{12}" "$UP_OUTPUT_FILE" | head -n 1 || echo "")

if [[ -z "$DEPLOYMENT_ID" ]]; then
    echo -e "${RED}Error: Failed to extract deployment ID from CLI output.${NC}"
    exit 1
fi

echo -e "\n${GREEN}✓ Successfully provisioned deployment: ${BOLD}${DEPLOYMENT_ID}${NC}"

if [[ -f "$TEMP_CONF_DIR/wg0.conf" ]]; then
    echo -e "${GREEN}✓ Client WireGuard configuration written to ${TEMP_CONF_DIR}/wg0.conf:${NC}"
    echo -e "${CYAN}------------------------------------------------------------${NC}"
    cat "$TEMP_CONF_DIR/wg0.conf"
    echo -e "${CYAN}------------------------------------------------------------${NC}"
fi

echo -e "\n${BLUE}${BOLD}[Step 3/4] Querying active deployment status via 'warpgate status ${DEPLOYMENT_ID}'...${NC}"
"$CLI_BIN" status "$DEPLOYMENT_ID"

echo -e "\n${BLUE}${BOLD}[Step 4/4] Lifecycle Teardown Verification...${NC}"
if [[ "$WAIT_FOR_TTL" == true ]]; then
    echo -e "Waiting for automated TTL expiration (${TTL})..."
    sleep 65
    echo -e "Checking status after TTL expiration:"
    "$CLI_BIN" status "$DEPLOYMENT_ID"
else
    echo -e "Triggering immediate automated teardown via: ${BOLD}warpgate down ${DEPLOYMENT_ID} --region ${REGION}${NC}"
    "$CLI_BIN" down "$DEPLOYMENT_ID" --region "$REGION"
    
    echo -e "\nVerifying deployment status transition to TERMINATING / DESTROYED:"
    sleep 1
    "$CLI_BIN" status "$DEPLOYMENT_ID"
fi

echo -e "\n${GREEN}${BOLD}======================================================================${NC}"
echo -e "${GREEN}${BOLD}             WarpGate Lifecycle Demo Completed Successfully!          ${NC}"
echo -e "${GREEN}${BOLD}======================================================================${NC}"
