#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_workflow="${repo_root}/.github/workflows/build.yml"
checks_workflow="${repo_root}/.github/workflows/checks.yml"
aur_workflow="${repo_root}/.github/workflows/aur.yml"

if grep -Eq '^[[:space:]]{2}(push|pull_request):|^[[:space:]]{4}(branches|tags):' \
    "$build_workflow"; then
    echo "privileged release workflow must be manually dispatched from trusted main" >&2
    exit 1
fi

prepare_guard="$(awk '/^  prepare:$/ { getline; print; exit }' "$build_workflow")"
if [[ "$prepare_guard" != "    if: github.ref == 'refs/heads/main'" ]]; then
    echo "release preparation must be guarded to trusted main before runner allocation" >&2
    exit 1
fi

build_guard="$(
    awk '/^  build:$/ { getline; getline; print; exit }' "$build_workflow"
)"
if [[ "$build_guard" != "    if: github.ref == 'refs/heads/main'" ]]; then
    echo "secret-bearing build job must be guarded to trusted main" >&2
    exit 1
fi

release_guard="$(
    awk '/^  release:$/ {
        while (getline) {
            if ($0 ~ /^    if:/) {
                print
                exit
            }
        }
    }' "$build_workflow"
)"
if [[ "$release_guard" != "    if: github.ref == 'refs/heads/main'" ]]; then
    echo "public release must be guarded to the reviewed main commit" >&2
    exit 1
fi

# These are intentional literal shell expressions in the workflow.
# shellcheck disable=SC2016
if [[ "$(grep -Fc 'name="${name// /.}"' "$build_workflow")" -ne 1 ]] ||
    [[ "$(grep -Fc '[[ "$name" =~ ^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9_-])?$ ]]' "$build_workflow")" -ne 1 ]]; then
    echo "release assets must use GitHub's filename normalization before checksumming" >&2
    exit 1
fi
grep -Fq 'name: Verify draft release assets' "$build_workflow"
grep -Fq 'gh api --paginate --slurp' "$build_workflow"
grep -Fq 'expected exactly one matching draft release' "$build_workflow"
if grep -Fq 'releases/tags/${RELEASE_TAG}' "$build_workflow"; then
    echo "draft verification must not use the release-by-tag endpoint" >&2
    exit 1
fi
grep -Fq 'draft release asset names differ from the checksummed files' "$build_workflow"
# shellcheck disable=SC2016
grep -Fq 'draft release asset digest mismatch: $name' "$build_workflow"

normalize_release_asset_name() {
    local name="${1// /.}"
    [[ "$name" =~ ^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9_-])?$ ]] || return 1
    printf '%s\n' "$name"
}

[[ "$(normalize_release_asset_name 'Filen Menubar_0.1.33_aarch64.dmg')" == \
    'Filen.Menubar_0.1.33_aarch64.dmg' ]]
[[ "$(normalize_release_asset_name 'already-safe.tar.gz')" == 'already-safe.tar.gz' ]]
if normalize_release_asset_name 'unsafe@name' >/dev/null ||
    normalize_release_asset_name '.leading-period' >/dev/null ||
    normalize_release_asset_name 'trailing-period.' >/dev/null; then
    echo "release asset normalization accepted a GitHub-unstable basename" >&2
    exit 1
fi
if [[ "$(normalize_release_asset_name 'Filen Menubar')" != \
    "$(normalize_release_asset_name 'Filen.Menubar')" ]]; then
    echo "release asset collision fixture did not normalize to one basename" >&2
    exit 1
fi

notarize_guard="$(
    awk '/^      - name: Verify and notarize macOS release$/ {
        getline
        print
        exit
    }' "$build_workflow"
)"
if [[ "$notarize_guard" != "        if: matrix.platform == 'macos-latest'" ]]; then
    echo "reviewed macOS releases must execute the notarization step" >&2
    exit 1
fi

# The preparation job resolves a validated tag to an immutable commit and
# rejects tags whose code has not already passed through main.
# These are intentional literal workflow and shell expressions.
# shellcheck disable=SC2016
grep -Fq 'INPUT_VERSION: ${{ inputs.version }}' "$build_workflow"
# shellcheck disable=SC2016
grep -Fq 'version="$(bash packaging/arch/validate-version.sh "$INPUT_VERSION")"' \
    "$build_workflow"
# shellcheck disable=SC2016
grep -Fq 'git merge-base --is-ancestor "$commit" "$GITHUB_SHA"' "$build_workflow"
# shellcheck disable=SC2016
grep -Fq 'bash scripts/check-version-sync.sh "$version" "$commit"' "$build_workflow"
# shellcheck disable=SC2016
if [[ "$(grep -Fc 'ref: ${{ needs.prepare.outputs.commit }}' "$build_workflow")" -ne 2 ]]; then
    echo "release build and packaging jobs must check out the validated commit" >&2
    exit 1
fi

grep -Eq '^[[:space:]]{2}pull_request:' "$checks_workflow"
grep -Eq '^[[:space:]]{2}push:' "$checks_workflow"

if [[ "$(grep -Fc 'run: npm run check' "$build_workflow")" -ne 1 ]] ||
    [[ "$(grep -Fc 'run: npm run check' "$checks_workflow")" -ne 1 ]]; then
    echo "release and branch workflows must run the complete repository check" >&2
    exit 1
fi

grep -Fq 'name: Verify macOS bundle payload' "$checks_workflow"
grep -Fq 'name: Verify Linux package payloads and ABI floor' "$checks_workflow"
# shellcheck disable=SC2016
grep -Fq 'codesign -d --entitlements :- "$helper_path"' "$checks_workflow"
# shellcheck disable=SC1003
if [[ "$(grep -Foc -- '--identifier io.filen.menubar.filen-cli \' "$build_workflow")" -ne 1 ]]; then
    echo "release signing must assign the helper's stable code-signing identifier" >&2
    exit 1
fi
grep -Fq -- '--identifier io.filen.menubar.filen-cli.keyring' "$build_workflow"
grep -Fq 'Contents/Helpers/filen-menubar-cli' "$build_workflow"
grep -Fq 'Contents/Helpers/filen-menubar-cli' "$checks_workflow"
if [[ "$(grep -Fc 'app_path="$PWD/src-tauri/target/${{ matrix.target }}/release/bundle/macos/Filen Menubar.app"' "$build_workflow")" -ne 1 ]] ||
    [[ "$(grep -Fc 'app_path="$PWD/src-tauri/target/${{ matrix.target }}/release/bundle/macos/Filen Menubar.app"' "$checks_workflow")" -ne 1 ]]; then
    echo "macOS bundle checks must use an absolute app path before changing directories" >&2
    exit 1
fi
grep -Fq '/usr/lib/Filen Menubar/filen-cli/node' "$build_workflow"
grep -Fq '/usr/lib/Filen Menubar/filen-cli/node' "$checks_workflow"

for workflow in "$build_workflow" "$checks_workflow"; do
    grep -Fq 'name: Package and smoke-rebuild corresponding source' "$workflow"
    grep -Fq 'scripts/package-filen-cli-source.sh' "$workflow"
    grep -Fq 'rebuild-source.sh' "$workflow"
    grep -Fq 'node-version: 24.18.1' "$workflow"
    grep -Fq 'libdbus-1-dev' "$workflow"
    grep -Fq 'pkg-config' "$workflow"
done

if grep -R -Eq 'BUN-LICENSE|LGPL-2\.0|externalBin|src-tauri/binaries/filen-menubar-cli' \
    "$build_workflow" "$checks_workflow"; then
    echo "workflows still reference the retired Bun/externalBin payload" >&2
    exit 1
fi

for forbidden in \
    com.apple.security.cs.allow-unsigned-executable-memory \
    com.apple.security.cs.disable-executable-page-protection \
    com.apple.security.cs.disable-library-validation; do
    grep -Fq "$forbidden" "$build_workflow"
done
# shellcheck disable=SC2016
grep -Fq -- '--entitlements src-tauri/filen-cli.entitlements "$helper"' "$build_workflow"
grep -Fq 'com.apple.security.cs.allow-jit' "${repo_root}/src-tauri/filen-cli.entitlements"
if [[ "$(python3 -c \
    'import plistlib,sys; data=plistlib.load(open(sys.argv[1], "rb")); print("\n".join(sorted(data)))' \
    "${repo_root}/src-tauri/filen-cli.entitlements")" != \
    'com.apple.security.cs.allow-jit' ]]; then
    echo "release helper entitlement file must contain only allow-jit" >&2
    exit 1
fi
if grep -Eq -- '--jit[l]ess' "$build_workflow" "$checks_workflow"; then
    echo "Node jitless mode disables WebAssembly required by the bundled HTTP/WebSocket client" >&2
    exit 1
fi

if grep -R -Fq 'awalsh128/cache-apt-pkgs-action' "${repo_root}/.github/workflows"; then
    echo "Linux dependencies must fail fast instead of using the lossy APT cache action" >&2
    exit 1
fi

for workflow in "$build_workflow" "$checks_workflow"; do
    if [[ "$(grep -Fc 'bun-version: 1.3.14' "$workflow")" -ne 1 ]] ||
        [[ "$(grep -Fc 'run: npm ci' "$workflow")" -ne 1 ]] ||
        [[ "$(grep -Fc 'bun --revision' "$workflow")" -ne 1 ]]; then
        echo "workflows must use the pinned Bun helper toolchain and deterministic npm install" >&2
        exit 1
    fi

    if [[ "$(grep -Fc 'sudo apt-get -o Acquire::Retries=3 update' "$workflow")" -ne 1 ]] ||
        [[ "$(grep -Fc 'sudo apt-get -o Acquire::Retries=3 install -y' "$workflow")" -ne 1 ]]; then
        echo "Linux workflows must refresh package lists and install dependencies explicitly" >&2
        exit 1
    fi
done

if grep -Eq '\$\{\{[[:space:]]*secrets\.|APPLE_|AUR_SSH_PRIVATE_KEY|DOCKERHUB_' \
    "$checks_workflow"; then
    echo "branch and pull-request workflow must remain secretless" >&2
    exit 1
fi

grep -Fq "github.ref == 'refs/heads/main'" "$aur_workflow"
grep -Fq 'ref: main' "$aur_workflow"
# These are intentional literal workflow and shell expressions.
# shellcheck disable=SC2016
grep -Fq 'INPUT_VERSION: ${{ inputs.version }}' "$aur_workflow"
# shellcheck disable=SC2016
grep -Fq 'RELEASE_TAG: ${{ github.event.release.tag_name }}' "$aur_workflow"
# shellcheck disable=SC2016
grep -Fq 'version="$(bash packaging/arch/validate-version.sh "$raw_version")"' "$aur_workflow"

raw_version_refs="$(
    grep -Ec '\$\{\{[[:space:]]*(inputs\.version|github\.event\.release\.tag_name)[[:space:]]*\}\}' \
        "$aur_workflow"
)"
if [[ "$raw_version_refs" -ne 2 ]]; then
    echo "raw AUR version contexts must appear only in the two env assignments" >&2
    exit 1
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/filen-menubar-workflow-test.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT
injection_marker="${tmp_dir}/injected-marker"
injection_payload="0.1.30'; printf workflow-expression-injection >\"${injection_marker}\"; #'"
if bash "${repo_root}/packaging/arch/validate-version.sh" \
    "$injection_payload" >/dev/null 2>&1; then
    echo "workflow validator accepted the original shell-injection payload" >&2
    exit 1
fi
if [[ -e "$injection_marker" ]]; then
    echo "workflow version payload executed instead of remaining data" >&2
    exit 1
fi

while IFS= read -r line; do
    if [[ "$line" =~ uses:[[:space:]]+\./ ]]; then
        continue
    fi
    if [[ ! "$line" =~ uses:[[:space:]]+[^@[:space:]]+@[0-9a-f]{40}([[:space:]]+\#.*)?$ ]]; then
        echo "external action is not pinned to a full commit SHA: $line" >&2
        exit 1
    fi
done < <(grep -R -h -E '^[[:space:]]+uses:' "${repo_root}/.github/workflows")

assert_action_pinned() {
    local action="$1"
    local matches
    local found=false

    matches="$(grep -R -h -F "uses: ${action}@" "${repo_root}/.github/workflows" || true)"
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        found=true
        if [[ ! "$line" =~ uses:[[:space:]]+${action}@[0-9a-f]{40}([[:space:]]+\#.*)?$ ]]; then
            echo "action is not pinned to a full commit SHA: $line" >&2
            exit 1
        fi
    done <<<"$matches"

    if [[ "$found" != true ]]; then
        echo "expected workflow action was not found: $action" >&2
        exit 1
    fi
}

assert_action_pinned dtolnay/rust-toolchain
assert_action_pinned swatinem/rust-cache
assert_action_pinned oven-sh/setup-bun
assert_action_pinned softprops/action-gh-release
assert_action_pinned KSXGitHub/github-actions-deploy-aur

echo "workflow security tests passed"
