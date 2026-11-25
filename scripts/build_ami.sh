#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKER_DIR="$(cd "${SCRIPT_DIR}/../infra/packer" && pwd)"

REGION="us-east-1"
VALIDATE_ONLY=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --region|-r)
      REGION="$2"
      shift 2
      ;;
    --validate-only)
      VALIDATE_ONLY=1
      shift
      ;;
    --help|-h)
      cat <<EOF
Usage: $0 [--region <aws-region>] [--validate-only]

Builds a pre-baked WarpGate Golden AMI with WireGuard, TCP BBR, AdGuard Home,
and systemd watchdog pre-installed to cut deployment boot time under 30 seconds.
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

if ! command -v packer &>/dev/null; then
  echo "Error: HashiCorp Packer is not installed. Install from https://developer.hashicorp.com/packer/install"
  exit 1
fi

cd "${PACKER_DIR}"
packer init warpgate.pkr.hcl
packer validate -var "aws_region=${REGION}" warpgate.pkr.hcl

if [[ "${VALIDATE_ONLY}" -eq 1 ]]; then
  echo "Packer template validated successfully."
  exit 0
fi

echo "Building WarpGate Golden AMI in region ${REGION}..."
packer build -var "aws_region=${REGION}" warpgate.pkr.hcl

