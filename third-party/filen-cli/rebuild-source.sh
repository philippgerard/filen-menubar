#!/usr/bin/env bash
set -euo pipefail

readonly bun_version="1.3.14"
readonly bun_revision="1.3.14+0d9b296af"
readonly node_version="v24.18.1"

source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
output_dir="${1:-${source_root}/rebuilt}"
bun_bin="${BUN_BIN:-$(command -v bun || true)}"
node_bin="${NODE_BIN:-$(command -v node || true)}"

[[ -n "$bun_bin" && "$($bun_bin --version)" == "$bun_version" ]] || {
    echo "Bun ${bun_version} is required to rebuild the helper" >&2
    exit 1
}
[[ "$($bun_bin --revision)" == "$bun_revision" ]] || {
    echo "expected Bun revision ${bun_revision}" >&2
    exit 1
}
[[ -n "$node_bin" && "$($node_bin --version)" == "$node_version" ]] || {
    echo "official Node ${node_version} is required to verify the source build" >&2
    exit 1
}
for command in cargo npm; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "${command} is required to rebuild the helper" >&2
        exit 1
    }
done

grep -Fq 'const STATE_VERSION = 3' "${source_root}/filen-sync/src/lib/state.ts"
(
    cd "${source_root}/filen-sync"
    npm ci --ignore-scripts --no-audit
    npm run tsc
    grep -Fq 'const STATE_VERSION = 3;' dist/lib/state.js
)

(
    cd "${source_root}/filen-sdk-ts"
    npm ci --ignore-scripts --no-audit
    npm run build:node
    "$node_bin" "${source_root}/packaging/check-filen-sdk-socket-error.mjs" \
        "${source_root}/filen-sdk-ts"
)

(
    cd "${source_root}/filen-cli"
    "$bun_bin" install --frozen-lockfile --ignore-scripts
    # Make the included preferred TypeScript source authoritative for the CJS
    # rebuild instead of silently reusing npm's generated sync distribution.
    cp -R "${source_root}/filen-sync/dist/." node_modules/@filen/sync/dist/
    grep -Fq 'const STATE_VERSION = 3;' node_modules/@filen/sync/dist/lib/state.js
    rm -rf node_modules/@filen/sdk/dist
    cp -R "${source_root}/filen-sdk-ts/dist" node_modules/@filen/sdk/dist
    grep -Eq 'this\.socket\.on\("error",[[:space:]]*\(\)[[:space:]]*=>[[:space:]]*\{[[:space:]]*\}\)' \
        node_modules/@filen/sdk/dist/node/fs/index.js
    "$bun_bin" ./node_modules/typescript/bin/tsc --noEmit
    "$bun_bin" run lint
    "$bun_bin" test --preload ./src/test/keyringMock.ts \
        src/framework/app.test.ts \
        src/app/featureInterfaces/syncInterface.test.ts \
        --timeout 30000
    mkdir -p "$output_dir"
    ./node_modules/esbuild/bin/esbuild src/index.ts \
        --bundle \
        --platform=node \
        --format=cjs \
        --target=node24 \
        --minify \
        --external:@jupiterpi/node-keyring \
        --external:msgpackr-extract \
        --define:VERSION='"v0.0.39-menubar.2"' \
        --define:IS_RUNNING_AS_BINARY=true \
        --define:IS_RUNNING_AS_CONTAINER=false \
        --define:IS_RUNNING_AS_NPM_PACKAGE=false \
        --outfile="${output_dir}/filen-cli.cjs"
)

keyring_target="${output_dir}/keyring-target"
(
    cd "${source_root}/node-keyring"
    CARGO_TARGET_DIR="$keyring_target" cargo build \
        --release \
        --locked \
        --offline \
        --config 'source.crates-io.replace-with="vendored-sources"' \
        --config "source.vendored-sources.directory='${source_root}/cargo-vendor'"
)

case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
        keyring_library="${keyring_target}/release/libnode_keyring.dylib"
        keyring_filename="node-keyring.darwin-arm64.node"
        ;;
    Linux:x86_64)
        keyring_library="${keyring_target}/release/libnode_keyring.so"
        keyring_filename="node-keyring.linux-x64-gnu.node"
        ;;
    *)
        echo "unsupported corresponding-source rebuild host" >&2
        exit 1
        ;;
esac
runtime_dir="${output_dir}/runtime"
mkdir -p "${runtime_dir}/node_modules/@jupiterpi/node-keyring"
install -m 0644 "${output_dir}/filen-cli.cjs" "${runtime_dir}/filen-cli.cjs"
install -m 0644 "${source_root}/filen-cli/node_modules/@jupiterpi/node-keyring/index.js" \
    "${runtime_dir}/node_modules/@jupiterpi/node-keyring/index.js"
install -m 0644 "${source_root}/filen-cli/node_modules/@jupiterpi/node-keyring/package.json" \
    "${runtime_dir}/node_modules/@jupiterpi/node-keyring/package.json"
install -m 0755 "$keyring_library" \
    "${runtime_dir}/node_modules/@jupiterpi/node-keyring/${keyring_filename}"

(
    cd "$runtime_dir"
    "$node_bin" -e 'const keyring = require("@jupiterpi/node-keyring"); if (typeof keyring.getPassword !== "function") process.exit(1)'
)

grep -Fq 'Filen CLI v0.0.39-menubar.2' <(
    NO_COLOR=1 "$node_bin" \
        --disable-warning=DEP0169 \
        "${runtime_dir}/filen-cli.cjs" \
        --skip-update \
        --data-dir "${output_dir}/data" \
        --version
)
echo "Rebuilt patched Filen CLI and native keyring from corresponding source in ${output_dir}"
