#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
renderer="${repo_root}/packaging/arch/render.sh"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/filen-menubar-render-test.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

input_deb="${tmp_dir}/input.deb"
printf 'test deb payload\n' >"$input_deb"

valid_out="${tmp_dir}/valid"
"$renderer" v0.1.30 "$valid_out" "$input_deb"

[[ "$(grep -c '^pkgver=0\.1\.30$' "${valid_out}/PKGBUILD")" -eq 1 ]]
if grep -q '^#@' "${valid_out}/PKGBUILD"; then
    echo "rendered PKGBUILD retained template comments" >&2
    exit 1
fi
if grep -q '@[A-Z0-9_]*@' "${valid_out}/PKGBUILD"; then
    echo "rendered PKGBUILD retained a template placeholder" >&2
    exit 1
fi
[[ -f "${valid_out}/filen-menubar-bin-0.1.30.deb" ]]
[[ -f "${valid_out}/LICENSE-0.1.30" ]]
bash -n "${valid_out}/PKGBUILD"
grep -Fq "license=('MIT' 'AGPL-3.0-only')" "${valid_out}/PKGBUILD"
grep -Fq 'local _runtime="${pkgdir}/usr/lib/Filen Menubar/filen-cli/node"' \
    "${valid_out}/PKGBUILD"
grep -Fq 'local _entrypoint="${pkgdir}/usr/lib/Filen Menubar/filen-cli/filen-cli.cjs"' \
    "${valid_out}/PKGBUILD"
grep -Fq 'node_modules/@jupiterpi/node-keyring' "${valid_out}/PKGBUILD"
if grep -Eq 'filen-cli-bin|nodejs' "${valid_out}/PKGBUILD"; then
    echo "rendered package still depends on an external Filen CLI runtime" >&2
    exit 1
fi
if grep -Eq '^optdepends=' "${valid_out}/PKGBUILD"; then
    echo "rendered package still advertises an optional external runtime" >&2
    exit 1
fi

assert_rejected() {
    local label="$1"
    local version="$2"
    local outdir="${tmp_dir}/invalid-${label}"

    if "$renderer" "$version" "$outdir" "$input_deb" >/dev/null 2>&1; then
        echo "renderer accepted unsafe version case: $label" >&2
        exit 1
    fi

    [[ ! -e "${outdir}/PKGBUILD" ]]
}

assert_rejected too-short '1.2'
assert_rejected prerelease '1.2.3-rc.1'
assert_rejected space '1.2.3 injected'
assert_rejected tab $'1.2.3\tinjected'
assert_rejected newline $'1.2.3\ninjected'
assert_rejected quote "1.2.3'injected"
assert_rejected backslash '1.2.3\injected'
assert_rejected ampersand '1.2.3&injected'
# Literal exploit inputs; expansion here would invalidate the test.
# shellcheck disable=SC2016
assert_rejected command-substitution '1.2.3$(touch injected-marker)'
# shellcheck disable=SC2016
assert_rejected backticks '1.2.3`touch injected-marker`'
assert_rejected slash '1.2.3/injected'
assert_rejected control-byte $'1.2.3\001injected'
assert_rejected original-poc '1.2.3;printf pkgbuild-expression-injection>injected-marker;#'

[[ ! -e "${repo_root}/injected-marker" ]]
[[ ! -e "${tmp_dir}/injected-marker" ]]

echo "render.sh security tests passed"
