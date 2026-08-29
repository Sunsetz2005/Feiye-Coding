# Official model gateway capacity (design only)

This page is a **design draft**. The gateway is not implemented. Desktop Host is not a multi-tenant server. Do not treat these numbers as measured capacity.

Sunsetz stays local-first: projects, journals, permissions, and tools run on the user's machine. When Sunsetz later provides official models, the desktop talks to a hosted OpenAI-compatible gateway. Until then, users attach their own models through Settings → My Models (`providers_*` + Keychain).

## Target

- **1,000 concurrent online users** = 1,000 authenticated desktop clients that can reach the gateway (including idle).
- **Peak streaming** is designed for 1,000 simultaneous SSE `POST /v1/chat/completions` responses. Everyday load is expected around 10–20% of that.
- Local Agent process limits stay at 3 (hard cap 8). Those limits are per machine, not cloud capacity.

## Draft interface (unimplemented)

Keep the existing Composer/model menu. Official models later swap the base URL, not the UI.

| Method | Path | Notes |
|---|---|---|
| POST | `/v1/chat/completions` | SSE, OpenAI-compatible |
| GET | `/v1/models` | Official catalog |
| GET | `/v1/usage` | Quota for the account panel |

- Auth: `Authorization: Bearer`. The token lives in Keychain, never in the journal.
- Rate-limit headers: `X-RateLimit-*`. The desktop shows the existing error banner; it does not use `alert`.
- Per-user concurrent streams: 3 (matches the local Agent feel).
- Fail-closed: if the gateway is down, user-provided models still work. Official model entries show an actionable empty state and never pretend to be connected.

## Capacity sketch (assumptions, not a load test)

- One active SSE is one HTTP/2 stream. 1,000 concurrent streams need more than one instance; size a single instance around 250–400 active streams first.
- The scarce resource is upstream model quota and token throughput, not the desktop WebView.
- Gateway should be stateless. Usage lands in an external ledger. Streams do not pass through the local Host.
- Observe connection count, time-to-first-token, disconnects, 4xx/5xx, and per-model QPS. No vendor is wired here.

## What this slice did not add

- No new Tauri commands or events.
- No change to `providers_*`.
- No account OAuth broker, WebSocket cluster, or model proxy binary.
