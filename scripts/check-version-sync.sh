#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
expected_version="${1:-}"
git_ref="${2:-}"

read_file() {
    local path="$1"

    if [[ -n "$git_ref" ]]; then
        git -C "$repo_root" show "${git_ref}:${path}"
    else
        command cat "${repo_root}/${path}"
    fi
}

json_version() {
    awk '
        !found && /^[[:space:]]*"version":[[:space:]]*"/ {
            value = $0
            sub(/^[[:space:]]*"version":[[:space:]]*"/, "", value)
            sub(/".*/, "", value)
            found = 1
        }
        END { if (found) print value }
    '
}

package_version="$(read_file package.json | json_version)"
if [[ -z "$expected_version" ]]; then
    expected_version="$package_version"
fi

if ! [[ "$expected_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "invalid expected application version: ${expected_version}" >&2
    exit 1
fi

tauri_version="$(read_file src-tauri/tauri.conf.json | json_version)"
cargo_toml_version="$({
    read_file src-tauri/Cargo.toml
} | awk '
    /^\[package\]$/ { in_package = 1; next }
    /^\[/ { in_package = 0 }
    in_package && /^version = "/ {
        value = $0
        sub(/^version = "/, "", value)
        sub(/"$/, "", value)
        found = 1
    }
    END { if (found) print value }
')"
cargo_lock_version="$({
    read_file src-tauri/Cargo.lock
} | awk '
    $0 == "name = \"filen-menubar\"" { in_package = 1; next }
    in_package && !found && /^version = "/ {
        value = $0
        sub(/^version = "/, "", value)
        sub(/"$/, "", value)
        found = 1
    }
    END { if (found) print value }
')"

# macOS ships Bash 3.2, so collect the two root package-lock versions
# without relying on mapfile/readarray.
package_lock_versions="$({
    read_file package-lock.json
} | awk '
    count < 2 && /^[[:space:]]*"version":[[:space:]]*"/ {
        value = $0
        sub(/^[[:space:]]*"version":[[:space:]]*"/, "", value)
        sub(/".*/, "", value)
        print value
        count++
    }
')"

assert_version() {
    local label="$1"
    local actual="$2"

    if [[ "$actual" != "$expected_version" ]]; then
        echo "${label} version is '${actual:-missing}', expected '${expected_version}'" >&2
        exit 1
    fi
}

assert_version package.json "$package_version"
assert_version src-tauri/tauri.conf.json "$tauri_version"
assert_version src-tauri/Cargo.toml "$cargo_toml_version"
assert_version src-tauri/Cargo.lock "$cargo_lock_version"

if [[ "$(printf '%s\n' "$package_lock_versions" | sed '/^$/d' | wc -l | tr -d ' ')" != "2" ]]; then
    echo "package-lock.json must contain exactly two root application versions" >&2
    exit 1
fi
while IFS= read -r lock_version; do
    [[ -n "$lock_version" ]] || continue
    assert_version package-lock.json "$lock_version"
done <<<"$package_lock_versions"

echo "application version ${expected_version} is synchronized"
