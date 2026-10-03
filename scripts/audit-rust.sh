#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Reuse the pinned auditor and also audit the independently built native addon.
bash "${repo_root}/scripts/audit-keyring.sh"
node "${repo_root}/scripts/audit-rust-backports.mjs" \
    "${repo_root}/src-tauri/target/cargo-audit-0.22.2/bin/cargo-audit"
