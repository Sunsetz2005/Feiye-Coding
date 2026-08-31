# Session continuity & context compact

## Problem

Sunsetz keeps a **UI journal** (`~/.…/sessions/<appSessionId>/messages.json`) separate from the **Agent session** under `GROK_HOME` (`agent-home/sessions/<encoded-cwd>/<agentSessionId>/`).

If the Host always called `session/new` on reconnect, the model only saw the latest user turn while the UI still showed full history — context looked “broken”.

## Strategy (Host)

### 1. Prefer native resume — `session/load`

On `session_connect` for an existing App session:

1. Spawn `grok agent stdio` with the same `GROK_HOME` / cwd.
2. `initialize` + `authenticate`.
3. If meta has `agentSessionId`, try **`session/load`** with that id + cwd.
4. On success → full agent context (tools, prior turns) restored.  
5. On failure → **`session/new`**, then mark **history bootstrap**.

### 2. Fallback — journal bootstrap (reasonable turns)

When a **new** agent session is created but the App journal already has turns:

- On the **first** `session_send` only, prefix the agent prompt with a compact transcript of recent journal messages:
  - Up to **16** user/assistant messages  
  - **~2k chars** per message (truncated with note)  
  - **~14k chars** total for the block  
- Journal storage still writes only the user-facing turn (no bootstrap text in UI).
- Flag `needs_history_bootstrap` clears after that one send.

This covers load failure, wiped agent dirs, or agent version mismatches.

### 3. Soft-respawn

Permission / mode soft-respawn **keeps** `agentSessionId` so the next connect prefers `session/load`. Bootstrap runs only if load fails.

### 4. Process limits & idle recycle (I01–I03)

| Setting | Default | Behavior |
|---------|---------|----------|
| `maxConcurrentAgents` | **3** | Cap on live + parked warm agent processes. Switching Ready chats **parks** the prior process (same session id). Over capacity → `PROCESS_LIMIT` + UI toast; LRU parked may be recycled for capacity. |
| `agentIdleMinutes` | **30** | Background watchdog soft-kills idle Ready agents (live + parked). **Session meta + journal stay**; next send reconnects (`session/load` or bootstrap). Emits `session://idle_recycled`. |

Same-cwd warm reuse (one process, switch ACP session) still applies when spawn flags match; otherwise multi-session parks up to the concurrent cap.

## Who does `/compact`?

| Layer | Behavior |
|-------|----------|
| **Sunsetz kernel (default)** | **Primary for product sessions.** Host intercepts `/compact [note]` before `run_turn`. A sidecar `context-compact.v1.json` stores the model-facing summary and the first retained message id. Auto-compact runs when last occupancy is ≥ **85%** of a known window, or when reconstructed history would otherwise hit the 24-message / 32k-character cap. Visible journal is not rewritten. |
| **Grok ACP adapter (legacy)** | Unchanged: the CLI auto-compacts near 85% and handles `/compact` when `runtimeBackend=grok_acp`. |
| **App Host / UI** | Slash **`/compact`** is a user action: confirm dialog → send `/compact …` as a normal prompt. Host records `context_compact` markers and shows the banner. Host journal is **not** rewritten by compact (UI history stays full). |

### UI surface for compact (required)

Host listens for agent compact signals (`session/update` kinds such as `tokens_used` / `*compact*` / compact tools) and:

1. Appends a **journal marker** (`role: tool`, `marker: context_compact`)
2. Emits `session://context_compact` for live UI
3. Chat shows a **compact banner** (auto vs manual + optional token before→after + summary)
4. Short **toast** on live event

App history still shows full prior bubbles; the banner signals that **agent context** was compressed.

## Agent activity visibility (Codex-style)

Historical and live activity use the same low-noise timeline:

1. Host force-commits pending assistant text when the first activity arrives.
2. The `tool_step`, ask_user, or `context_compact` marker is inserted or updated at that journal position.
3. Later assistant text gets a new stable phase id, so reload keeps the real sequence.
4. `ConversationThread` renders those phases as one conversational turn while preserving activity between them.
5. Adjacent completed activities of the same semantic category may collapse; running, failed, and cancelled entries remain separate.

The normalized UI model covers skill reads, compact, files, commands, images, browser actions, subtasks, Agent questions, and generic tools. Unknown legacy tool rows show bounded metadata only; raw output is not copied into the transcript.

`context_compact` follows the same phase-boundary rule for both foreground and background sessions. Old journals without phase evidence remain in stored order; the UI does not invent historical interleaving.

The complete rendering and compatibility contract is in [workbench-conversation.md](./workbench-conversation.md).

## Pending Agent questions

AskUser is orthogonal to the session FSM and stored by live session:

- `session_pending_interactions` returns pending interactions across foreground and background tasks.
- The App restores the active dock and sidebar badges after a task switch or WebView reload while the Agent process remains alive.
- Answers are copied to pending `partialAnswers` before the Runtime response is written. A failed write leaves the answers available for retry.
- All skipped questions resolve as accepted with empty `answers` / `partial_answers`; closing the dock cancels the request.

This pending state is not a disk-persistence guarantee across a complete application process restart.

## Acceptance

1. Reopen a multi-turn App session after killing the agent process → next send either loads the same `agentSessionId` or injects bootstrap so the model knows prior turns.  
2. Soft-respawn (permission change) → resume preferred.  
3. Brand-new chat → no bootstrap, plain `session/new`.  
4. `/compact` still only runs when the user (or auto-threshold) triggers it on the kernel/adapter side.
5. Assistant → activity/compact → assistant ordering survives journal reload for newly recorded turns.
6. A failed ask_user reply can be restored with its partial answers while the Agent process remains alive.
