#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
audit_bin="$(node "${repo_root}/scripts/install-cargo-audit.mjs")"
# The auditor's compiled dependencies are part of the build trust boundary too.
GIT_CONFIG_GLOBAL=/dev/null "$audit_bin" audit \
    --file "${repo_root}/third-party/cargo-audit/Cargo.lock" \
    --deny warnings
# Memory-unsoundness advisories must fail the build even when RustSec marks
# them informational. Keep upstream maintenance notices visible separately.
GIT_CONFIG_GLOBAL=/dev/null "$audit_bin" audit \
    --file "${repo_root}/third-party/filen-cli/node-keyring-Cargo.lock" \
    --deny unsound
