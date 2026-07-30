#!/usr/bin/env bash
# Normalize and validate a release version before it reaches paths, URLs,
# workflow outputs, sed replacement text, or executable PKGBUILD source.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: ${0##*/} <version>" >&2
    exit 2
fi

version="${1#v}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "invalid release version: expected MAJOR.MINOR.PATCH" >&2
    exit 2
fi

printf '%s\n' "$version"
