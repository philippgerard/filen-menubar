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

notarize_guard="$(
    awk '/^      - name: Notarize and verify macOS release disk image$/ {
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

if grep -R -Fq 'awalsh128/cache-apt-pkgs-action' "${repo_root}/.github/workflows"; then
    echo "Linux dependencies must fail fast instead of using the lossy APT cache action" >&2
    exit 1
fi

for workflow in "$build_workflow" "$checks_workflow"; do
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
assert_action_pinned softprops/action-gh-release
assert_action_pinned KSXGitHub/github-actions-deploy-aur

echo "workflow security tests passed"
