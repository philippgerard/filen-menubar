#!/bin/sh

set -eu

deny() {
  printf '%s\n' \
    "This repository requires Codex tasks that use shell commands or edit files to run in a Git worktree. Start the task with Worktree selected, or use Handoff to move this task to a worktree." \
    >&2
  exit 2
}

git_dir=$(git rev-parse --absolute-git-dir 2>/dev/null) || deny
common_dir=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || deny

if [ "$git_dir" = "$common_dir" ]; then
  deny
fi
