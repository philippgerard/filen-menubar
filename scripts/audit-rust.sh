#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Reuse the pinned auditor and also audit the independently built native addon.
# shellcheck source=scripts/audit-keyring.sh
source "${repo_root}/scripts/audit-keyring.sh"
node "${repo_root}/scripts/audit-rust-backports.mjs" "$audit_bin"
