#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly audit_version="0.22.2"
tool_root="${repo_root}/src-tauri/target/cargo-audit-${audit_version}"
audit_bin="${tool_root}/bin/cargo-audit"

if [[ ! -x "$audit_bin" ]]; then
    cargo install cargo-audit \
        --version "$audit_version" \
        --locked \
        --root "$tool_root"
fi
[[ "$($audit_bin --version)" == "cargo-audit ${audit_version}" ]]
GIT_CONFIG_GLOBAL=/dev/null "$audit_bin" audit \
    --file "${repo_root}/third-party/filen-cli/node-keyring-Cargo.lock"
