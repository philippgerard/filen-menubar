#!/usr/bin/env bash
set -euo pipefail

readonly cli_commit="ca966d86d1fe3ed204088e448299d174288085f6"
readonly bun_version="1.3.14"
readonly bun_revision="1.3.14+0d9b296af"
readonly node_version="24.18.1"
readonly node_source_sha256="b62cd76de0a0a28dd9ff88580c92344bdeb008f21c1d7479c5d8659cd96ef4e2"
readonly keyring_commit="165e4334ff365792d9b1274761e8afeedcccaffe"
readonly sync_commit="0d025bae60f493a42c2f49a4fcbbb46a31bea4ab"
readonly sdk_commit="6f272ffac11802d5d1a64fb8796871b402db6a71"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
third_party_dir="${repo_root}/third-party/filen-cli"
patch_file="${third_party_dir}/filen-menubar.patch"
sync_patch_file="${third_party_dir}/filen-sync-state-v3.patch"
sync_source_patch_file="${third_party_dir}/filen-sync-source-state-v3.patch"
sdk_socket_patch_file="${third_party_dir}/filen-sdk-socket-error.patch"
lock_file="${third_party_dir}/bun.lock"
keyring_lock_file="${third_party_dir}/node-keyring-Cargo.lock"
keyring_license_supplements="${third_party_dir}/cargo-license-supplements.json"
version_file="${third_party_dir}/VERSION"
generated_dir="${repo_root}/src-tauri/generated"
runtime_dir="${generated_dir}/filen-cli"
license_dir="${generated_dir}/licenses/filen-cli"
stamp_file="${generated_dir}/filen-cli.stamp"

hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

hash_text() {
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s' "$1" | sha256sum | awk '{print $1}'
    else
        printf '%s' "$1" | shasum -a 256 | awk '{print $1}'
    fi
}

verify_sha256() {
    local file="$1"
    local expected="$2"
    local actual
    actual="$(hash_file "$file")"
    if [[ "$actual" != "$expected" ]]; then
        echo "SHA-256 mismatch for ${file}: expected ${expected}, found ${actual}" >&2
        exit 1
    fi
}

case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
        target_triple="aarch64-apple-darwin"
        node_platform="darwin-arm64"
        node_archive_sha256="eb02f7fab96d3d67de40c5ec8566096fcb4c2026728787683ae5a97eb612b941"
        keyring_filename="node-keyring.darwin-arm64.node"
        keyring_library="libnode_keyring.dylib"
        ;;
    Linux:x86_64)
        target_triple="x86_64-unknown-linux-gnu"
        node_platform="linux-x64"
        node_archive_sha256="9f5eb6ac21845a66c493c91a253b1da32fd684e89e9b7202d4936982336be4ca"
        keyring_filename="node-keyring.linux-x64-gnu.node"
        keyring_library="libnode_keyring.so"
        ;;
    *)
        echo "unsupported bundled backend build host: $(uname -s) $(uname -m)" >&2
        exit 1
        ;;
esac

if [[ -n "${TAURI_ENV_ARCH:-}" ]]; then
    case "${target_triple}:${TAURI_ENV_ARCH}" in
        aarch64-apple-darwin:aarch64|aarch64-apple-darwin:arm64|x86_64-unknown-linux-gnu:x86_64) ;;
        *)
            echo "refusing to cross-build native backend dependencies for TAURI_ENV_ARCH=${TAURI_ENV_ARCH}" >&2
            exit 1
            ;;
    esac
fi

for command in cargo curl git tar; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "${command} is required to build the bundled Filen backend" >&2
        exit 1
    }
done

bun_bin="${BUN_BIN:-}"
if [[ -z "$bun_bin" ]]; then
    bun_bin="$(command -v bun || true)"
fi
if [[ -z "$bun_bin" ]]; then
    echo "Bun ${bun_version} is required only to build the bundled Filen backend" >&2
    exit 1
fi
[[ "$("$bun_bin" --version)" == "$bun_version" ]] || {
    echo "expected Bun ${bun_version} at ${bun_bin}" >&2
    exit 1
}
[[ "$("$bun_bin" --revision)" == "$bun_revision" ]] || {
    echo "expected Bun revision ${bun_revision} at ${bun_bin}" >&2
    exit 1
}

cli_version_display="$(tr -d '\r\n' <"$version_file")"
if [[ ! "$cli_version_display" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-menubar\.[0-9]+$ ]]; then
    echo "invalid bundled Filen CLI version manifest: ${cli_version_display}" >&2
    exit 1
fi

fingerprint="${cli_commit}:${cli_version_display}:${bun_revision}:node-${node_version}:${node_archive_sha256}:${node_source_sha256}:${keyring_commit}:${target_triple}:${sync_commit}:${sdk_commit}:$(hash_file "$version_file"):$(hash_file "$patch_file"):$(hash_file "$sync_patch_file"):$(hash_file "$sync_source_patch_file"):$(hash_file "$sdk_socket_patch_file"):$(hash_file "$lock_file"):$(hash_file "$keyring_lock_file"):$(hash_file "$keyring_license_supplements"):$(hash_file "${third_party_dir}/license-supplements/napi-rs-LICENSE.txt"):$(hash_file "${third_party_dir}/license-supplements/r-efi-AUTHORS.txt"):$(hash_file "${repo_root}/scripts/check-filen-sdk-socket-error.mjs"):$(hash_file "${repo_root}/scripts/license-evidence.mjs"):$(hash_file "${repo_root}/scripts/license-evidence.test.mjs"):$(hash_file "${repo_root}/scripts/generate-filen-cli-compliance.mjs"):$(hash_file "${repo_root}/scripts/generate-keyring-compliance.mjs"):$(hash_file "${repo_root}/scripts/validate-filen-cli-compliance.mjs"):$(hash_file "${repo_root}/scripts/audit-keyring.sh"):$(hash_file "${BASH_SOURCE[0]}")"
sidecar_file="${generated_dir}/filen-cli-node"
if [[ -x "$sidecar_file" && -f "${runtime_dir}/filen-cli.cjs" && -f "$stamp_file" ]] &&
    [[ "$(<"$stamp_file")" == "$fingerprint" ]]; then
    echo "Bundled Filen backend ${cli_version_display} is already prepared for ${target_triple}"
    exit 0
fi

build_root="${repo_root}/src-tauri/target/filen-cli-build/$(hash_text "$fingerprint")"
source_dir="${build_root}/source"
keyring_source_dir="${build_root}/node-keyring"
sync_source_dir="${build_root}/filen-sync"
sdk_source_dir="${build_root}/filen-sdk-ts"
node_cache_dir="${repo_root}/src-tauri/target/node-runtime-cache"
node_archive="${node_cache_dir}/node-v${node_version}-${node_platform}.tar.gz"
node_source_archive="${node_cache_dir}/node-v${node_version}.tar.gz"
node_extract_dir="${node_cache_dir}/node-v${node_version}-${node_platform}"
stage_dir="${build_root}/stage"

case "$build_root" in
    "${repo_root}/src-tauri/target/filen-cli-build/"*) ;;
    *)
        echo "refusing unexpected helper build path: ${build_root}" >&2
        exit 1
        ;;
esac

mkdir -p "$build_root" "$node_cache_dir"
if [[ ! -f "$node_archive" ]] || [[ "$(hash_file "$node_archive")" != "$node_archive_sha256" ]]; then
    curl --fail --location --retry 3 --output "$node_archive" \
        "https://nodejs.org/dist/v${node_version}/node-v${node_version}-${node_platform}.tar.gz"
fi
verify_sha256 "$node_archive" "$node_archive_sha256"
if [[ ! -f "$node_source_archive" ]] || [[ "$(hash_file "$node_source_archive")" != "$node_source_sha256" ]]; then
    curl --fail --location --retry 3 --output "$node_source_archive" \
        "https://nodejs.org/dist/v${node_version}/node-v${node_version}.tar.gz"
fi
verify_sha256 "$node_source_archive" "$node_source_sha256"

if [[ ! -x "${node_extract_dir}/bin/node" ]]; then
    tar -xzf "$node_archive" -C "$node_cache_dir"
fi
[[ "$("${node_extract_dir}/bin/node" --version)" == "v${node_version}" ]]

source_commit="$(GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" rev-parse HEAD 2>/dev/null || true)"
if [[ "$source_commit" != "$cli_commit" ]]; then
    if [[ -e "$source_dir" ]]; then
        rm -rf "$source_dir"
    fi
    mkdir -p "$source_dir"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" init --quiet
    GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" remote add origin https://github.com/FilenCloudDienste/filen-cli.git
    GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" fetch --quiet --depth 1 origin "$cli_commit"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" checkout --quiet --detach FETCH_HEAD
    GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" apply --check "$patch_file"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" apply "$patch_file"
    install -m 0644 "$sync_patch_file" "${source_dir}/patches/@filen%2Fsync@0.3.7.patch"
    install -m 0644 "$lock_file" "${source_dir}/bun.lock"
fi
[[ "$(GIT_CONFIG_GLOBAL=/dev/null git -C "$source_dir" rev-parse HEAD)" == "$cli_commit" ]]

(
    cd "$source_dir"
    "$bun_bin" install --frozen-lockfile --ignore-scripts
    "$bun_bin" audit --prod --audit-level high
    grep -Fq 'const STATE_VERSION = 3;' node_modules/@filen/sync/dist/lib/state.js
    "$bun_bin" ./node_modules/typescript/bin/tsc --noEmit
    "$bun_bin" run lint
    "$bun_bin" test \
        src/framework/app.test.ts \
        src/app/featureInterfaces/syncInterface.test.ts \
        --timeout 30000

)

if [[ ! -d "${keyring_source_dir}/.git" ]]; then
    GIT_CONFIG_GLOBAL=/dev/null git clone --quiet https://github.com/JupiterPi/node-keyring.git "$keyring_source_dir"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$keyring_source_dir" checkout --quiet --detach "$keyring_commit"
fi
[[ "$(GIT_CONFIG_GLOBAL=/dev/null git -C "$keyring_source_dir" rev-parse HEAD)" == "$keyring_commit" ]]

if [[ ! -d "${sync_source_dir}/.git" ]]; then
    GIT_CONFIG_GLOBAL=/dev/null git clone --quiet --no-checkout \
        https://github.com/FilenCloudDienste/filen-sync.git "$sync_source_dir"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$sync_source_dir" checkout --quiet --detach "$sync_commit"
fi
[[ "$(GIT_CONFIG_GLOBAL=/dev/null git -C "$sync_source_dir" rev-parse HEAD)" == "$sync_commit" ]]
if ! grep -Fq 'const STATE_VERSION = 3' "${sync_source_dir}/src/lib/state.ts"; then
    GIT_CONFIG_GLOBAL=/dev/null git -C "$sync_source_dir" apply --check "$sync_source_patch_file"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$sync_source_dir" apply "$sync_source_patch_file"
fi
grep -Fq 'const STATE_VERSION = 3' "${sync_source_dir}/src/lib/state.ts"
(
    cd "$sync_source_dir"
    PATH="${node_extract_dir}/bin:${PATH}" npm ci --ignore-scripts --no-audit
    PATH="${node_extract_dir}/bin:${PATH}" npm run tsc
    grep -Fq 'const STATE_VERSION = 3;' dist/lib/state.js
)

if [[ ! -d "${sdk_source_dir}/.git" ]]; then
    GIT_CONFIG_GLOBAL=/dev/null git clone --quiet --no-checkout \
        https://github.com/FilenCloudDienste/filen-sdk-ts.git "$sdk_source_dir"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$sdk_source_dir" checkout --quiet --detach "$sdk_commit"
fi
[[ "$(GIT_CONFIG_GLOBAL=/dev/null git -C "$sdk_source_dir" rev-parse HEAD)" == "$sdk_commit" ]]
if ! grep -Fq 'this.socket.on("error", () => {})' "${sdk_source_dir}/src/fs/index.ts"; then
    GIT_CONFIG_GLOBAL=/dev/null git -C "$sdk_source_dir" apply --check "$sdk_socket_patch_file"
    GIT_CONFIG_GLOBAL=/dev/null git -C "$sdk_source_dir" apply "$sdk_socket_patch_file"
fi
grep -Fq 'this.socket.on("error", () => {})' "${sdk_source_dir}/src/fs/index.ts"
(
    cd "$sdk_source_dir"
    PATH="${node_extract_dir}/bin:${PATH}" npm ci --ignore-scripts --no-audit
    PATH="${node_extract_dir}/bin:${PATH}" npm run build:node
    PATH="${node_extract_dir}/bin:${PATH}" node \
        "${repo_root}/scripts/check-filen-sdk-socket-error.mjs" "$sdk_source_dir"
)
rm -rf "${source_dir}/node_modules/@filen/sdk/dist"
cp -R "${sdk_source_dir}/dist" "${source_dir}/node_modules/@filen/sdk/dist"
(
    cd "$source_dir"
    mkdir -p dist
    ./node_modules/esbuild/bin/esbuild src/index.ts \
        --bundle \
        --platform=node \
        --format=cjs \
        --target=node24 \
        --minify \
        --external:@jupiterpi/node-keyring \
        --external:msgpackr-extract \
        --define:VERSION="\"${cli_version_display}\"" \
        --define:IS_RUNNING_AS_BINARY=true \
        --define:IS_RUNNING_AS_CONTAINER=false \
        --define:IS_RUNNING_AS_NPM_PACKAGE=false \
        --outfile=dist/filen-cli.cjs \
        --metafile=dist/filen-cli.meta.json
)
install -m 0644 "$keyring_lock_file" "${keyring_source_dir}/Cargo.lock"
[[ "$(hash_file "${keyring_source_dir}/Cargo.lock")" == "8156ea37b77d183209a23f52e857ee5c81a08d1f746fe059fa04fa479d005f7d" ]]

keyring_target_dir="${build_root}/keyring-target"
(
    cd "$keyring_source_dir"
    export CARGO_TARGET_DIR="$keyring_target_dir"
    if [[ "$(uname -s)" == "Darwin" ]]; then
        export MACOSX_DEPLOYMENT_TARGET="13.5"
    fi
    cargo build --locked --release
)
keyring_built_file="${keyring_target_dir}/release/${keyring_library}"
[[ -f "$keyring_built_file" ]] || {
    echo "native keyring build did not produce ${keyring_built_file}" >&2
    exit 1
}

stage_runtime="${stage_dir}/filen-cli"
stage_licenses="${stage_dir}/licenses"
stage_compliance="${stage_dir}/compliance"
stage_sidecar="${stage_dir}/filen-menubar-cli"
if [[ -e "$stage_dir" ]]; then
    rm -rf "$stage_dir"
fi
mkdir -p "${stage_runtime}/node_modules/@jupiterpi/node-keyring" "$stage_licenses" "$stage_compliance"

keyring_metadata="${build_root}/node-keyring-metadata.json"
keyring_vendor="${build_root}/cargo-vendor"
(
    cd "$keyring_source_dir"
    cargo metadata --locked --format-version 1 >"$keyring_metadata"
)
if [[ -e "$keyring_vendor" ]]; then
    rm -rf "$keyring_vendor"
fi
(
    cd "$keyring_source_dir"
    cargo vendor --locked "$keyring_vendor" >/dev/null
)
node "${repo_root}/scripts/generate-keyring-compliance.mjs" \
    "$keyring_metadata" "$keyring_vendor" "$keyring_lock_file" "$stage_compliance" \
    "$keyring_license_supplements"
bash "${repo_root}/scripts/audit-keyring.sh"
node "${repo_root}/scripts/generate-filen-cli-compliance.mjs" \
    "$source_dir" \
    "${source_dir}/dist/filen-cli.meta.json" \
    "$stage_compliance" \
    "$cli_version_display" \
    "${node_extract_dir}/bin/node" \
    "$node_version" \
    "$node_platform" \
    "$node_archive_sha256" \
    "$node_source_archive" \
    "$node_source_sha256" \
    "${stage_compliance}/cargo-components.json" \
    "${stage_compliance}/CARGO_THIRD_PARTY_NOTICES.txt"
node "${repo_root}/scripts/validate-filen-cli-compliance.mjs" \
    "${stage_compliance}/runtime.cdx.json" \
    "${stage_compliance}/THIRD_PARTY_NOTICES.txt"
install -m 0644 "${stage_compliance}/THIRD_PARTY_NOTICES.txt" \
    "${stage_licenses}/THIRD_PARTY_NOTICES.txt"
install -m 0644 "${stage_compliance}/runtime.cdx.json" \
    "${stage_licenses}/runtime.cdx.json"
install -m 0755 "${node_extract_dir}/bin/node" "$stage_sidecar"
install -m 0644 "${source_dir}/dist/filen-cli.cjs" "${stage_runtime}/filen-cli.cjs"
install -m 0644 "${source_dir}/node_modules/@jupiterpi/node-keyring/index.js" \
    "${stage_runtime}/node_modules/@jupiterpi/node-keyring/index.js"
install -m 0644 "${source_dir}/node_modules/@jupiterpi/node-keyring/package.json" \
    "${stage_runtime}/node_modules/@jupiterpi/node-keyring/package.json"
install -m 0644 "${source_dir}/node_modules/@jupiterpi/node-keyring/LICENSE" \
    "${stage_runtime}/node_modules/@jupiterpi/node-keyring/LICENSE"
install -m 0755 "$keyring_built_file" \
    "${stage_runtime}/node_modules/@jupiterpi/node-keyring/${keyring_filename}"
install -m 0644 "${node_extract_dir}/LICENSE" "${stage_licenses}/NODE-LICENSE.txt"

if [[ "$(uname -s)" == "Darwin" ]]; then
    codesign --force --sign - --options runtime \
        "${stage_runtime}/node_modules/@jupiterpi/node-keyring/${keyring_filename}"
    # Node needs JIT for the WebAssembly parser used by its built-in HTTP and
    # WebSocket client. Ad-hoc signatures have no common Team ID, so local
    # builds additionally need library validation disabled for the keyring.
    # Release CI signs both files with the same Developer ID and grants only
    # the helper-specific allow-jit entitlement checked into src-tauri/.
    local_entitlements="${build_root}/filen-cli-local-entitlements.plist"
    printf '%s\n' \
        '<?xml version="1.0" encoding="UTF-8"?>' \
        '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
        '<plist version="1.0">' \
        '<dict>' \
        '  <key>com.apple.security.cs.allow-jit</key>' \
        '  <true/>' \
        '  <key>com.apple.security.cs.disable-library-validation</key>' \
        '  <true/>' \
        '</dict>' \
        '</plist>' >"$local_entitlements"
    codesign --force --sign - \
        --identifier io.filen.menubar.filen-cli \
        --options runtime \
        --entitlements "$local_entitlements" \
        "$stage_sidecar"
fi

smoke_dir="$(mktemp -d "${TMPDIR:-/tmp}/filen-menubar-node-smoke.XXXXXX")"
trap 'rm -rf "$smoke_dir"' EXIT
version_output="$(NO_COLOR=1 "$stage_sidecar" --disable-warning=DEP0169 \
    "${stage_runtime}/filen-cli.cjs" \
    --skip-update --data-dir "$smoke_dir" --version)"
[[ "$version_output" == "Filen CLI ${cli_version_display}" ]] || {
    echo "unexpected bundled Filen CLI version: ${version_output}" >&2
    exit 1
}
NO_COLOR=1 "$stage_sidecar" --disable-warning=DEP0169 "${stage_runtime}/filen-cli.cjs" \
    --skip-update --data-dir "$smoke_dir" --help sync >/dev/null
(
    cd "$stage_runtime"
    "$stage_sidecar" -e 'const keyring = require("@jupiterpi/node-keyring"); if (typeof keyring.getPassword !== "function") process.exit(1)'
)
"$stage_sidecar" -e 'if (typeof WebAssembly !== "object") process.exit(1)'

mkdir -p "$generated_dir"
if [[ -e "$runtime_dir" ]]; then
    rm -rf "$runtime_dir"
fi
if [[ -e "$license_dir" ]]; then
    rm -rf "$license_dir"
fi
if [[ -e "${generated_dir}/compliance" ]]; then
    rm -rf "${generated_dir}/compliance"
fi
mkdir -p "$(dirname "$license_dir")"
mv "$stage_runtime" "$runtime_dir"
mv "$stage_licenses" "$license_dir"
mv "$stage_compliance" "${generated_dir}/compliance"
install -m 0755 "$stage_sidecar" "$sidecar_file"
printf '%s' "$fingerprint" >"$stamp_file"
echo "Prepared bundled Filen backend ${cli_version_display} with Node v${node_version} for ${target_triple}"
