#!/usr/bin/env bash
# Render packaging/arch/PKGBUILD.in into a build directory.
#
# Usage: packaging/arch/render.sh <version> <outdir> [path/to/local.deb]
#
# Both sources are placed in <outdir> under the exact names the PKGBUILD's
# source array declares, so a later makepkg run reuses them instead of
# fetching. That matters in CI, where the release is still a draft and the
# download URL does not resolve yet.
#
# The recorded sha256sums are computed from those local files. They are
# therefore only valid for the AUR if the .deb handed in here is the very
# artifact published to the release, and LICENSE in this checkout is the one
# tagged v<version>.
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
    echo "usage: ${0##*/} <version> <outdir> [local.deb]" >&2
    exit 2
fi

outdir="$2"
local_deb="${3:-}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
template="${repo_root}/packaging/arch/PKGBUILD.in"
url="https://github.com/philippgerard/filen-menubar"
version="$(bash "${repo_root}/packaging/arch/validate-version.sh" "$1")"

[[ -f "$template" ]] || { echo "missing template: $template" >&2; exit 1; }

mkdir -p "$outdir"
deb_dest="${outdir}/filen-menubar-bin-${version}.deb"

if [[ -n "$local_deb" ]]; then
    [[ -f "$local_deb" ]] || { echo "no such .deb: $local_deb" >&2; exit 1; }
    cp -- "$local_deb" "$deb_dest"
else
    curl -fsSL --retry 3 -o "$deb_dest" \
        "${url}/releases/download/v${version}/Filen.Menubar_${version}_amd64.deb"
fi

cp -- "${repo_root}/LICENSE" "${outdir}/LICENSE-${version}"

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        echo "neither sha256sum nor shasum is available" >&2
        return 1
    fi
}

sha_deb="$(sha256_file "$deb_dest")"
sha_license="$(sha256_file "${outdir}/LICENSE-${version}")"

# Drop the "#@" template notes first, so the placeholder names documented
# there are not themselves substituted. sed applies -e in order per line.
sed -e '/^#@/d' \
    -e "s|@PKGVER@|${version}|g" \
    -e "s|@SHA256_DEB@|${sha_deb}|g" \
    -e "s|@SHA256_LICENSE@|${sha_license}|g" \
    "$template" >"${outdir}/PKGBUILD"

if grep -q '@[A-Z0-9_]*@' "${outdir}/PKGBUILD"; then
    echo "unsubstituted placeholders remain in ${outdir}/PKGBUILD" >&2
    grep -n '@[A-Z0-9_]*@' "${outdir}/PKGBUILD" >&2
    exit 1
fi

echo "rendered ${outdir}/PKGBUILD for v${version}"
