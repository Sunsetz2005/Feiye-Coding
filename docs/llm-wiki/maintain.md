# Open-source maintenance (Sunsetz)

Single playbook for humans and AI maintainers: intake → triage → review → ship.

Related: [release.md](./release.md) (tag / CHANGELOG), [CONTRIBUTING.md](../../CONTRIBUTING.md), [i18n.md](./i18n.md), [dialogs.md](./dialogs.md).

---

## Goals

1. **Capture** community feedback (GitHub Issues, X, PR comments) into trackable Issues.
2. **Triage** severity (`priority:p0|p1|p2`) and area labels within ~48h of report when possible.
3. **Review** external PRs with a fixed checklist; merge small safe fixes fast.
4. **Fix** P0/P1 on `main` with tests + i18n; close Issues from commit/PR body.
5. **Ship** via CHANGELOG + `release-tag.sh` when a batch of fixes is ready (see release.md).

---

## Labels (required vocabulary)

| Label | Use |
|-------|-----|
| `bug` / `enhancement` / `documentation` | Type |
| `priority:p0` | Blocks core usage (login, send, stuck UI, data loss) |
| `priority:p1` | Major UX / platform breakage |
| `priority:p2` | Polish, nice-to-have, backlog |
| `area:composer` | Input, paste, attachments, + menu |
| `area:session` | Streaming, history, permissions, agent connect |
| `area:auth` | OAuth / account / quota |
| `area:i18n` | Locale strings / hard-coded language |
| `platform:macos` / `platform:windows` | OS-specific |
| `from:community` | X / Discord / external report |
| `good first issue` | Small, well-scoped for newcomers |
| `triage` | Not yet prioritized |

Issue forms: `.github/ISSUE_TEMPLATE/`.

---

## Intake sources

| Source | Action |
|--------|--------|
| GitHub Issues | Primary tracker; use templates |
| X replies under launch posts | Open Issue with `from:community` + screenshot links |
| GitHub Discussions / email | Convert to Issue if actionable |
| PR comments | Fix in PR or open follow-up Issue |

**Do not** leave real bugs only in chat threads.

### X → Issue checklist

1. Quote user handle + post URL  
2. OS / app version if known  
3. Screenshot media URLs  
4. Labels + priority  
5. Link related PRs  

---

## Triage flow

```text
New Issue
  → label type + platform + area
  → priority:p0|p1|p2 (or leave triage)
  → assign or leave unassigned
  → P0: fix or workaround same day if possible
  → P1: target next patch release
  → P2: backlog / good first issue
```

### Priority guide

- **P0**: cannot log in / send / attach; infinite lock; crash loops; silent data wipe  
- **P1**: wrong platform chrome; multi-instance; duplicate history; broken permission allow  
- **P2**: thinking default collapse; Linux package; multi-account; web history import  

---

## PR review (community + maintainer)

### Must pass

- [ ] `pnpm typecheck`、`pnpm test:coverage`、`pnpm coverage:audit`、`pnpm build:ui`
- [ ] `pnpm verify:contracts` && `pnpm test:visual`
- [ ] `cd src-tauri && cargo test` (or CI green)
- [ ] User-facing strings via `src/i18n/messages.ts` (**en + zh** same keys)  
- [ ] No `window.confirm` / `prompt` / `alert`  
- [ ] No secrets, `auth.json`, local agent homes  
- [ ] Scope matches description; no drive-by refactors  

### Merge policy

| Kind | Policy |
|------|--------|
| Small bugfix, clear root cause, tests | Squash-merge after CI / local check |
| Feature / large refactor | Request changes or design note first |
| i18n / permission / agent protocol | Prefer maintainer re-verify against real CLI |
| Security | Follow SECURITY.md; do not discuss exploits in public Issues |

### Adopted community PRs (examples)

- **#1** locale-aware session titles — **merge** (correct i18n for LLM rename)  
- **#2** Grok Build underscore permission optionIds — **merge** (fixes shell tool allow failures)

After merge: thank the author on the PR and close linked Issues; add a concise CHANGELOG sentence when preparing the next version.

---

## Fix → close loop

1. Branch from latest `main`  
2. One concern per PR when possible  
3. Commit body: `Fixes #N` or `Closes #N`  
4. Update `docs/llm-wiki/*` if product rule changed  
5. After ship: verify closed Issues; reopen if regression  

---

## Maintenance automation

| Mechanism | Location |
|-----------|----------|
| CI（契约、前端覆盖率、UI build、Linux 行为视觉矩阵、cargo test macOS/Windows/Linux） | `.github/workflows/ci.yml` |
| Release builds + notes | `.github/workflows/release.yml` + `scripts/release-tag.sh` |
| Stale / needs-info (optional) | can add `actions/stale` later |
| PR template | `.github/PULL_REQUEST_TEMPLATE.md` |

### Maintainer weekly checklist

1. `gh issue list --label priority:p0` — empty or owned  
2. `gh pr list` — review open community PRs  
3. Scan X launch thread for new bugs → Issues  
4. Prepare a concise version section below the empty `UNRELEASED` heading when releasing
5. When enough P0/P1 landed → [release.md](./release.md)  

---

## Agent / AI handoff rules

When an agent maintains this repo:

1. Read this file + `AGENTS.md` + relevant llm-wiki pages
2. Prefer **Issues first**, then code  
3. Prefer **merging good community PRs** over reimplementing  
4. Never force-push `main`; never tag without CHANGELOG section  
5. Redact tokens in logs and Issue bodies  
6. After multi-issue work: leave a short status in the PR / reply (what fixed, what remains)  

---

## Current community backlog snapshot (launch feedback)

Captured from X open-source thread (2026-07-24). Track as GitHub Issues with `from:community`.

| Topic | Priority | Status |
|-------|----------|--------|
| Paste image + file picker in + menu | P0 | ✅ 0.1.1+ (paste / + Files; dead “coming later” string removed) |
| Composer lock when stream stalls | P0 | ✅ 0.1.3 #37 stall cancel + #40 send queue; type allowed except permission |
| Duplicate history on next send / session switch | P0 | ✅ 0.1.3 #35 FSM gate + `clearPriorTurnStreaming`; regression test |
| Login auth code / relay path | P0 | ⏸ deferred (official OAuth works; custom relay separate) |
| Agent connect / provider errors | P0 | ⏳ keep improving G11 copy |
| Permission optionId hyphen vs underscore | P0 | ✅ PR #2 |
| Multi-open Dock | P1 | ✅ single-instance plugin (re-verify if dual Dock icons remain) |
| Titlebar panel toggle overflow | P1 | ✅ traffic-light safe inset 96px |
| Composer placeholder occlusion | P1 | ✅ DOM-aware placeholder hide |
| Thinking / long-chat scroll flicker | P1 | ✅ height-noise filter on stick-to-bottom |
| Plan/Goal sticky bar | P1 | ✅ PR #41 |
| Session title hard-coded Chinese | P1 | ✅ PR #1 |
| Thinking collapse preference | P2 | ✅ auto-collapse default + remember |
| Multi account | P2 | ✅ 0.1.1 |
| Grok Web history import | P2 | ❌ out of scope (local sessions) |
| git worktree UX | P2 | ✅ #46 project chip switch (list + bind cwd) + create/remove (2026-09-12, Grok Build parity sync) |
| Claude-Code-like pure chat shell | P2 | ❌ product is Agent workbench |
| Arch Linux package | P2 | ✅ document AppImage on Arch (+ optional AUR later) |
| Plugin marketplace install UI | P2 | 💬 design note (CLI remains SoT for install) |

## Grok Build parity sync (2026-09-12)

Upstream Grok Build (the legacy `grok` CLI this app's ACP adapter targets) shipped 0.2.106 → 1.0.25 (Aug–Sep 2026). Compared against this codebase and synced:

- Context ring shows a compacting-in-progress state (`AcpEvent::ContextCompactStart`/`ContextCompactEnd`, kernel path only — legacy ACP has no upstream "start" notification to hook).
- Legacy ACP official route fails fast on an expired cached token instead of spawning + soft-failing (`connect_inner`, see [providers.md](providers.md)).
- Git worktree create/remove (previously list + bind cwd only) — human-triggered UI only, never exposed as an agent tool (`AGENTS.md`'s "subagents never create worktrees" is unaffected).
- `AlwaysApprove` no longer blindly auto-runs destructive shell commands (see [catalog.md](catalog.md#权限)).
- Permission preview supports expand/collapse for long `run_command` previews.
- `wait_commands`/`command_output` `timeout_ms` clamped to 1 hour (`MAX_TOOL_WAIT_MS`, `agent_loop.rs`).

**Deferred, not implemented** (documented only):
- `--sandbox strict` — redundant with this Host's own `sandboxProfile` layer (bubblewrap / sandbox-exec / AppContainer).
- Per-turn cost/usage persistence (`grok usage` parity) — no pricing table exists anywhere in this codebase; real feature, bigger than a sync pass.
