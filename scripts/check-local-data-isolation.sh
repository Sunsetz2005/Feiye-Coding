#!/usr/bin/env bash
# Fail if the Git tree contains per-user MCP, model, or credential files.
set -eu
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

leaked="$(git ls-files | grep -E \
  '(^|/)\.env($|\.)|(^|/)secrets\.json$|(^|/)auth\.json$|(^|/)connector-credentials\.json$|(^|/)connectors\.json$|(^|/)extensions\.json$|(^|/)\.mcp\.json$|(^|/)mcp\.json$|(^|/)claude_desktop_config\.json$|\.pem$|\.key$|(^|/)config\.toml$|sunsetz-support-.*\.zip$|sunsetz-session-.*\.zip$' \
  || true)"

if [ -n "$leaked" ]; then
  printf '%s\n' "Tracked local user data would be pushed to GitHub:" "$leaked"
  exit 1
fi

printf '%s\n' "Local MCP / model / credential files are not tracked."
