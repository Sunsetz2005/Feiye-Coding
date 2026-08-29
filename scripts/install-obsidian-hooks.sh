#!/usr/bin/env bash
set -eu

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

git config core.hooksPath .githooks
chmod +x .githooks/post-commit .githooks/post-checkout .githooks/post-merge .githooks/pre-commit
scripts/sync-obsidian-knowledge.sh

printf '%s\n' 'Installed Sunsetz Git hooks via core.hooksPath=.githooks'
