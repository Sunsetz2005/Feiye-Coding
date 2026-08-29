#!/usr/bin/env bash
# Launch the real Sunsetz native window with a per-user temp data home.
# Never points SUNSETZ_HOME at the git checkout.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

user="${USER:-$(id -un)}"
stamp="$(date +%Y%m%d-%H%M%S)"
out="$repo_root/test-results/macos-native-plugin-${stamp}"
home="${TMPDIR:-/tmp}/sunsetz-native-smoke-${user}"
mkdir -p "$out" "$home"
chmod 700 "$home" || true

export SUNSETZ_HOME="$home"
export SUNSETZ_ACP="${SUNSETZ_ACP:-mock}"

log() { printf '%s\n' "$*" | tee -a "$out/smoke.log"; }

case "$SUNSETZ_HOME" in
  "$repo_root"*) log "REFUSED: smoke home is inside the git checkout"; exit 1 ;;
esac

mkdir -p "$home"
cat >"$home/settings.json" <<'JSON'
{
  "theme": "dark",
  "locale": "zh",
  "sessionDataMode": "independent",
  "permissionPolicy": "ask",
  "effort": "medium",
  "mode": "agent",
  "onboardingDone": true,
  "setupSkipped": true,
  "setupWizardCompleted": true,
  "authSetupDeferred": true,
  "defaultOpenTarget": "finder",
  "composerPrefsScope": "global",
  "maxConcurrentAgents": 3,
  "agentIdleMinutes": 30,
  "streamStallSeconds": 120,
  "sandboxProfile": "off",
  "storeApiKeysInKeychain": false,
  "runtimeBackend": "sunsetz"
}
JSON
chmod 600 "$home/settings.json"

log "SUNSETZ_HOME=$SUNSETZ_HOME"
log "output=$out"

pnpm tauri dev >"$out/tauri.log" 2>&1 &
pid=$!
cleanup() {
  kill "$pid" 2>/dev/null || true
  pkill -P "$pid" 2>/dev/null || true
}
trap cleanup EXIT

log "waiting for native Sunsetz window..."
wid=""
for _ in $(seq 1 90); do
  wid="$(
    swift -e '
import CoreGraphics
import Foundation
let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in info {
  let owner = (w[kCGWindowOwnerName as String] as? String ?? "").lowercased()
  let name = w[kCGWindowName as String] as? String ?? ""
  let bounds = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
  let width = bounds["Width"] ?? 0
  if owner.contains("unsetz") && name == "Sunsetz" && width >= 900 {
    print(w[kCGWindowNumber as String] ?? "")
    break
  }
}
' 2>/dev/null || true
  )"
  if [[ -n "$wid" ]]; then
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    log "tauri process exited early; see $out/tauri.log"
    tail -n 40 "$out/tauri.log" | tee -a "$out/smoke.log"
    exit 1
  fi
  sleep 2
done
if [[ -z "$wid" ]]; then
  log "Sunsetz native window did not appear"
  tail -n 40 "$out/tauri.log" | tee -a "$out/smoke.log"
  exit 1
fi
log "window id=$wid"
screencapture -l"$wid" "$out/native-window.png" || log "screencapture failed (display not attached)"
log "isolated home entries:"
ls -la "$home" | tee -a "$out/smoke.log"
if [[ -d "$home/agent-home" ]]; then
  log "agent-home stays in user temp, not the git checkout"
fi
printf '%s\n' "PASS native window id=$wid home=$SUNSETZ_HOME" >"$out/PASS.txt"
log "native smoke passed"
