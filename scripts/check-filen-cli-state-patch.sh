#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
patch_file="${repo_root}/third-party/filen-cli/filen-sync-state-v3.patch"
source_patch_file="${repo_root}/third-party/filen-cli/filen-sync-source-state-v3.patch"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/filen-sync-state-patch.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

mkdir -p "$work_dir/dist/lib"
cat >"$work_dir/dist/lib/state.js" <<'EOF'
const readline_1 = __importDefault(require("readline"));
const uuid_1 = require("uuid");
const fast_glob_1 = __importDefault(require("fast-glob"));
const STATE_VERSION = 2;
/**
 * State
 * @date 3/1/2024 - 11:11:32 PM
EOF

git -C "$work_dir" init --quiet
git -C "$work_dir" apply --check "$patch_file"
git -C "$work_dir" apply "$patch_file"
grep -Fq 'const STATE_VERSION = 3;' "$work_dir/dist/lib/state.js"
if grep -Fq 'const STATE_VERSION = 2;' "$work_dir/dist/lib/state.js"; then
    echo "sync state patch left the incompatible v2 namespace enabled" >&2
    exit 1
fi

mkdir -p "$work_dir/src/lib"
cat >"$work_dir/src/lib/state.ts" <<'EOF'
import readline from "readline"
import { v4 as uuidv4 } from "uuid"
import FastGlob from "fast-glob"

const STATE_VERSION = 2

/**
 * State
EOF
git -C "$work_dir" apply --check "$source_patch_file"
git -C "$work_dir" apply "$source_patch_file"
grep -Fq 'const STATE_VERSION = 3' "$work_dir/src/lib/state.ts"
if grep -Fq 'const STATE_VERSION = 2' "$work_dir/src/lib/state.ts"; then
    echo "sync source patch left the incompatible v2 namespace enabled" >&2
    exit 1
fi
