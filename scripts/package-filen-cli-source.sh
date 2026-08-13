#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
stamp_file="${repo_root}/src-tauri/generated/filen-cli.stamp"
[[ -f "$stamp_file" ]] || { echo "build the bundled backend first" >&2; exit 1; }

fingerprint="$(<"$stamp_file")"
if command -v sha256sum >/dev/null 2>&1; then
    build_hash="$(printf '%s' "$fingerprint" | sha256sum | awk '{print $1}')"
else
    build_hash="$(printf '%s' "$fingerprint" | shasum -a 256 | awk '{print $1}')"
fi
build_root="${repo_root}/src-tauri/target/filen-cli-build/${build_hash}"
source_dir="${build_root}/source"
keyring_dir="${build_root}/node-keyring"
sync_dir="${build_root}/filen-sync"
sdk_dir="${build_root}/filen-sdk-ts"
[[ -d "${source_dir}/.git" && -d "${keyring_dir}/.git" && \
    -d "${sync_dir}/.git" && -d "${sdk_dir}/.git" ]] || {
    echo "matching corresponding-source checkout is missing" >&2
    exit 1
}

version="$(tr -d '\r\n' <"${repo_root}/third-party/filen-cli/VERSION")"
target_triple="$(printf '%s' "$fingerprint" | cut -d: -f8)"
archive_root="filen-menubar-cli-${version}-${target_triple}-source"
staging="$(mktemp -d "${TMPDIR:-/tmp}/filen-menubar-source.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
mkdir -p "${staging}/${archive_root}/filen-cli" \
    "${staging}/${archive_root}/node-keyring" \
    "${staging}/${archive_root}/filen-sync" \
    "${staging}/${archive_root}/filen-sdk-ts" \
    "${staging}/${archive_root}/packaging" \
    "${staging}/${archive_root}/runtime-packages" \
    "${staging}/${archive_root}/cargo-vendor" \
    "${staging}/${archive_root}/node"

cp -R "${source_dir}/." "${staging}/${archive_root}/filen-cli/"
if [[ -d "${staging}/${archive_root}/filen-cli/.git" ]]; then
    rm -rf "${staging}/${archive_root}/filen-cli/.git"
fi
if [[ -d "${staging}/${archive_root}/filen-cli/node_modules" ]]; then
    rm -rf "${staging}/${archive_root}/filen-cli/node_modules"
fi
if [[ -d "${staging}/${archive_root}/filen-cli/dist" ]]; then
    rm -rf "${staging}/${archive_root}/filen-cli/dist"
fi
cp -R "${repo_root}/src-tauri/generated/compliance/corresponding-source/runtime-packages/." \
    "${staging}/${archive_root}/runtime-packages/"
cp -R "${keyring_dir}/." "${staging}/${archive_root}/node-keyring/"
if [[ -d "${staging}/${archive_root}/node-keyring/.git" ]]; then
    rm -rf "${staging}/${archive_root}/node-keyring/.git"
fi
GIT_CONFIG_GLOBAL=/dev/null git -C "$sync_dir" archive HEAD | \
    tar -x -C "${staging}/${archive_root}/filen-sync"
GIT_CONFIG_GLOBAL=/dev/null git -C "${staging}/${archive_root}/filen-sync" \
    apply "${repo_root}/third-party/filen-cli/filen-sync-source-state-v3.patch"
grep -Fq 'const STATE_VERSION = 3' \
    "${staging}/${archive_root}/filen-sync/src/lib/state.ts"
GIT_CONFIG_GLOBAL=/dev/null git -C "$sdk_dir" archive HEAD | \
    tar -x -C "${staging}/${archive_root}/filen-sdk-ts"
GIT_CONFIG_GLOBAL=/dev/null git -C "${staging}/${archive_root}/filen-sdk-ts" \
    apply "${repo_root}/third-party/filen-cli/filen-sdk-socket-error.patch"
grep -Fq 'this.socket.on("error", () => {})' \
    "${staging}/${archive_root}/filen-sdk-ts/src/fs/index.ts"
install -m 0644 "${repo_root}/third-party/filen-cli/node-keyring-Cargo.lock" \
    "${staging}/${archive_root}/node-keyring/Cargo.lock"
install -m 0644 "${repo_root}/third-party/filen-cli/filen-menubar.patch" \
    "${staging}/${archive_root}/packaging/filen-menubar.patch"
install -m 0644 "${repo_root}/third-party/filen-cli/filen-sync-state-v3.patch" \
    "${staging}/${archive_root}/packaging/filen-sync-state-v3.patch"
install -m 0644 "${repo_root}/third-party/filen-cli/filen-sync-source-state-v3.patch" \
    "${staging}/${archive_root}/packaging/filen-sync-source-state-v3.patch"
install -m 0644 "${repo_root}/third-party/filen-cli/filen-sdk-socket-error.patch" \
    "${staging}/${archive_root}/packaging/filen-sdk-socket-error.patch"
install -m 0644 "${repo_root}/third-party/filen-cli/bun.lock" \
    "${staging}/${archive_root}/packaging/bun.lock"
install -m 0644 "${repo_root}/third-party/filen-cli/cargo-license-supplements.json" \
    "${staging}/${archive_root}/packaging/cargo-license-supplements.json"
mkdir -p "${staging}/${archive_root}/packaging/license-supplements"
cp -R "${repo_root}/third-party/filen-cli/license-supplements/." \
    "${staging}/${archive_root}/packaging/license-supplements/"
cp -R "${repo_root}/src-tauri/generated/compliance/corresponding-source/cargo-vendor/." \
    "${staging}/${archive_root}/cargo-vendor/"
cp -R "${repo_root}/src-tauri/generated/compliance/corresponding-source/node/." \
    "${staging}/${archive_root}/node/"
install -m 0755 "${repo_root}/third-party/filen-cli/rebuild-source.sh" \
    "${staging}/${archive_root}/rebuild-source.sh"
install -m 0644 "${repo_root}/third-party/filen-cli/CORRESPONDING_SOURCE.md" \
    "${staging}/${archive_root}/README.md"
install -m 0755 "${repo_root}/scripts/generate-filen-cli-compliance.mjs" \
    "${staging}/${archive_root}/packaging/generate-filen-cli-compliance.mjs"
install -m 0755 "${repo_root}/scripts/generate-keyring-compliance.mjs" \
    "${staging}/${archive_root}/packaging/generate-keyring-compliance.mjs"
install -m 0755 "${repo_root}/scripts/validate-filen-cli-compliance.mjs" \
    "${staging}/${archive_root}/packaging/validate-filen-cli-compliance.mjs"
install -m 0644 "${repo_root}/scripts/license-evidence.mjs" \
    "${staging}/${archive_root}/packaging/license-evidence.mjs"
install -m 0644 "${repo_root}/scripts/license-evidence.test.mjs" \
    "${staging}/${archive_root}/packaging/license-evidence.test.mjs"
install -m 0755 "${repo_root}/scripts/check-filen-sdk-socket-error.mjs" \
    "${staging}/${archive_root}/packaging/check-filen-sdk-socket-error.mjs"
install -m 0644 "${repo_root}/src-tauri/generated/compliance/runtime.cdx.json" \
    "${staging}/${archive_root}/runtime.cdx.json"
install -m 0644 "${repo_root}/src-tauri/generated/licenses/filen-cli/THIRD_PARTY_NOTICES.txt" \
    "${staging}/${archive_root}/THIRD_PARTY_NOTICES.txt"
install -m 0644 "${repo_root}/src-tauri/generated/licenses/filen-cli/NODE-LICENSE.txt" \
    "${staging}/${archive_root}/NODE-LICENSE.txt"

output_dir="${repo_root}/src-tauri/generated/compliance"
archive="${output_dir}/${archive_root}.tar.gz"
archive_tar="${staging}/${archive_root}.tar"
find "${staging}/${archive_root}" -exec touch -h -t 197001010000 {} +
(
    cd "$staging"
    LC_ALL=C find "$archive_root" \( -type f -o -type l \) -print | LC_ALL=C sort | \
        COPYFILE_DISABLE=1 tar -cf "$archive_tar" -T -
)
# `tar -z` lets gzip encode wall-clock time in its header on macOS. Compress
# separately with `-n` so identical corresponding source has identical bytes.
gzip -n -c "$archive_tar" >"$archive"
tar -tzf "$archive" >/dev/null
duplicates="$(tar -tzf "$archive" | LC_ALL=C sort | uniq -d | head -1)"
[[ -z "$duplicates" ]] || {
    echo "corresponding-source archive contains duplicate entry: ${duplicates}" >&2
    exit 1
}
archive_size="$(wc -c <"$archive" | tr -d ' ')"
if (( archive_size < 10000000 || archive_size > 400000000 )); then
    echo "implausible corresponding-source archive size: ${archive_size} bytes" >&2
    exit 1
fi
echo "$archive"
