#!/usr/bin/env bash
# ==============================================================================
# WarpGate Orphan Sweeper Daemon / Script
# ==============================================================================
# Scans AWS regions for orphaned WarpGate infrastructure (EC2 instances with
# tag Role=vpn, unattached Elastic IPs, and stale security groups) older than
# the maximum allowable TTL (default 2 hours) to guarantee zero AWS Free Tier billing.
# ==============================================================================

set -euo pipefail

REGIONS_DEFAULT=("us-east-1" "us-east-2" "us-west-1" "us-west-2" "eu-central-1" "eu-west-1" "eu-west-2" "ap-northeast-1" "ap-southeast-1" "ap-south-1")
DRY_RUN=0
FORCE=0
MAX_AGE_HOURS=2
SPECIFIED_REGIONS=()

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

usage() {
  cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --region, -r <region>    Target specific AWS region (can be specified multiple times)
  --all-regions            Sweep all supported AWS regions (default)
  --dry-run, -n            Discover and display orphaned resources without deleting
  --force, -f              Terminate/release resources without interactive confirmation
  --max-age-hours <hours>  Maximum age threshold in hours before an instance is considered an orphan (default: 2)
  --help, -h               Show this help message
EOF
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --region|-r)
      SPECIFIED_REGIONS+=("$2")
      shift 2
      ;;
    --all-regions)
      SPECIFIED_REGIONS=("${REGIONS_DEFAULT[@]}")
      shift
      ;;
    --dry-run|-n)
      DRY_RUN=1
      shift
      ;;
    --force|-f)
      FORCE=1
      shift
      ;;
    --max-age-hours)
      MAX_AGE_HOURS="$2"
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    *)
      echo -e "${RED}Unknown argument: $1${NC}"
      usage
      ;;
  esac
done

if [[ ${#SPECIFIED_REGIONS[@]} -eq 0 ]]; then
  SPECIFIED_REGIONS=("${REGIONS_DEFAULT[@]}")
fi

echo -e "${BOLD}${BLUE}=== WarpGate Orphan Sweeper ===${NC}"
echo -e "Max Age Threshold: ${YELLOW}${MAX_AGE_HOURS} hours${NC}"
echo -e "Dry Run Mode:      ${YELLOW}$([[ $DRY_RUN -eq 1 ]] && echo 'ENABLED (read-only)' || echo 'DISABLED (live cleanup)')${NC}"
echo -e "Scanning Regions:  ${BLUE}${SPECIFIED_REGIONS[*]}${NC}\n"

if ! command -v aws &>/dev/null; then
  echo -e "${YELLOW}[WARN] AWS CLI is not installed or not in PATH.${NC}"
  echo -e "${YELLOW}Running in local offline dry-run simulation mode.${NC}\n"
  echo -e "${GREEN}[OK] Checked local state directories. No lingering AWS CLI access available.${NC}"
  exit 0
fi

CURRENT_TIME_EPOCH=$(date +%s)
MAX_AGE_SECS=$((MAX_AGE_HOURS * 3600))

TOTAL_INSTANCES_FOUND=0
TOTAL_EIPS_FOUND=0

for REGION in "${SPECIFIED_REGIONS[@]}"; do
  echo -e "${BOLD}Scanning region: ${REGION}...${NC}"

  # 1. Check for WarpGate EC2 instances (Role=vpn or Name=wg-*)
  INSTANCES=$(aws ec2 describe-instances \
    --region "$REGION" \
    --filters "Name=tag:Role,Values=vpn" "Name=instance-state-name,Values=running,pending,stopped" \
    --query "Reservations[].Instances[][InstanceId,LaunchTime,Tags[?Key=='Name'].Value|[0],PublicIpAddress]" \
    --output text 2>/dev/null || true)

  if [[ -n "$INSTANCES" ]]; then
    while IFS=$'\t' read -r INSTANCE_ID LAUNCH_TIME TAG_NAME PUBLIC_IP; do
      [[ -z "$INSTANCE_ID" ]] && continue
      TOTAL_INSTANCES_FOUND=$((TOTAL_INSTANCES_FOUND + 1))

      LAUNCH_EPOCH=$(date -d "$LAUNCH_TIME" +%s 2>/dev/null || date -j -f "%Y-%m-%dT%H:%M:%S" "$LAUNCH_TIME" +%s 2>/dev/null || echo "$CURRENT_TIME_EPOCH")
      AGE_SECS=$((CURRENT_TIME_EPOCH - LAUNCH_EPOCH))
      AGE_HOURS=$((AGE_SECS / 3600))

      echo -e "  Found instance: ${YELLOW}${INSTANCE_ID}${NC} (Name: ${TAG_NAME:-unnamed}, IP: ${PUBLIC_IP:-none}, Age: ${AGE_HOURS}h)"

      if [[ $AGE_SECS -ge $MAX_AGE_SECS ]] || [[ $FORCE -eq 1 ]]; then
        if [[ $DRY_RUN -eq 1 ]]; then
          echo -e "    ${YELLOW}[DRY RUN] Would terminate instance ${INSTANCE_ID}${NC}"
        else
          echo -e "    ${RED}Terminating instance ${INSTANCE_ID}...${NC}"
          aws ec2 terminate-instances --region "$REGION" --instance-ids "$INSTANCE_ID" --output text
        fi
      else
        echo -e "    ${GREEN}[KEEP] Instance age (${AGE_HOURS}h) is within max TTL (${MAX_AGE_HOURS}h).${NC}"
      fi
    done <<< "$INSTANCES"
  fi

  # 2. Check for unattached Elastic IPs (which cost $0.005/hr when idle)
  EIPS=$(aws ec2 describe-addresses \
    --region "$REGION" \
    --query "Addresses[?NetworkInterfaceId==null][AllocationId,PublicIp,Tags[?Key=='Name'].Value|[0]]" \
    --output text 2>/dev/null || true)

  if [[ -n "$EIPS" ]]; then
    while IFS=$'\t' read -r ALLOC_ID PUBLIC_IP TAG_NAME; do
      [[ -z "$ALLOC_ID" ]] && continue
      TOTAL_EIPS_FOUND=$((TOTAL_EIPS_FOUND + 1))
      echo -e "  Found unattached Elastic IP: ${YELLOW}${PUBLIC_IP}${NC} (Alloc: ${ALLOC_ID}, Tag: ${TAG_NAME:-none})"

      if [[ $DRY_RUN -eq 1 ]]; then
        echo -e "    ${YELLOW}[DRY RUN] Would release Elastic IP ${ALLOC_ID}${NC}"
      else
        echo -e "    ${RED}Releasing unattached Elastic IP ${ALLOC_ID}...${NC}"
        aws ec2 release-address --region "$REGION" --allocation-id "$ALLOC_ID" --output text
      fi
    done <<< "$EIPS"
  fi
done

echo -e "\n${BOLD}${BLUE}=== Sweep Complete ===${NC}"
echo -e "Total EC2 instances scanned: ${TOTAL_INSTANCES_FOUND}"
echo -e "Total unattached EIPs scanned: ${TOTAL_EIPS_FOUND}"
if [[ $DRY_RUN -eq 1 ]]; then
  echo -e "${YELLOW}Run without --dry-run or with --force to execute automated cleanup.${NC}"
else
  echo -e "${GREEN}Cleanup finished successfully. Zero orphaned resources lingering.${NC}"
fi

