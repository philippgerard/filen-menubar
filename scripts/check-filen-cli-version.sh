#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(tr -d '\r\n' <"${repo_root}/third-party/filen-cli/VERSION")"
case "$(uname -s):$(uname -m)" in
    Darwin:arm64|Linux:x86_64) ;;
    *) echo "unsupported helper check host" >&2; exit 1 ;;
esac

node="${repo_root}/src-tauri/generated/filen-cli-node"
entrypoint="${repo_root}/src-tauri/generated/filen-cli/filen-cli.cjs"
[[ -x "$node" ]] || { echo "bundled Node sidecar is missing: ${node}" >&2; exit 1; }
[[ -f "$entrypoint" ]] || { echo "bundled CLI entrypoint is missing: ${entrypoint}" >&2; exit 1; }
[[ "$($node --version)" == "v24.18.1" ]] || { echo "unexpected bundled Node version" >&2; exit 1; }

actual="$(NO_COLOR=1 "$node" --disable-warning=DEP0169 "$entrypoint" --skip-update --version)"
expected="Filen CLI ${version}"
[[ "$actual" == "$expected" ]] || {
    echo "bundled Filen CLI version mismatch: expected '${expected}', found '${actual}'" >&2
    exit 1
}

rust_manifest="${repo_root}/src-tauri/src/cli/discovery.rs"
grep -Fq 'include_str!("../../../third-party/filen-cli/VERSION")' "$rust_manifest" || {
    echo "Rust runtime no longer reads the bundled CLI version manifest" >&2
    exit 1
}
