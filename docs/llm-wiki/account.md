# Official Grok Build account

Product rules for **official login, membership, quota, and usage** in Sunsetz.

## Goals

1. Sign in with the **same** Grok Build CLI auth (`grok login`), not a parallel OAuth stack.
2. Show account + membership at two depths:
   - **User menu sheet** (sidebar footer click): compact identity, plan, quota bar, login/logout, jump to settings.
   - **Settings → Account**: full profile, subscription, quota, token activity heatmap, recent session call logs, CLI path, Doctor.
3. Never log tokens, API keys, or `auth.json` secrets (redact).

## Auth sources (priority for “connected”)

| Channel | Source | Notes |
|---------|--------|--------|
| `official_oauth` | `~/.grok/auth.json` via `grok login --oauth` / `--device-auth` | Preferred for membership + billing |
| `official_key` | App secrets `officialApiKey` (OS keychain preferred; `secrets.json` fallback) | CI / paste key; limited billing |
| `relay` | App secrets relay base + key (key in OS keychain preferred) | Custom OpenAI-compatible |
| `none` | — | Prompt login |

CLI auth is shared with Grok Build TUI (hot-reload of `auth.json` is CLI-side).

### Independent mode gotcha (fixed)

| Step | Path |
|------|------|
| `grok login` / App login | writes `~/.grok/auth.json` |
| Agent spawn (`session_data_mode=independent`) | `GROK_HOME=<Sunsetz app data>/agent-home` |

Host **must** sync `auth.json` into agent-home on login and before each ACP spawn; otherwise the UI shows signed-in while the agent reports `auth_kind=none` → HTTP 401. Logout clears both copies.

## Host commands

| Command | Role |
|---------|------|
| `account_status` | Profile (redacted) + channel + billing snapshot + local heatmap + call logs |
| `account_login` | Spawn `grok login --oauth` or `--device-auth` |
| `account_logout` | Spawn `grok logout` (fallback: remove auth.json) |
| `account_open_usage` | Open `https://grok.com/?_s=usage` |
| `account_open_subscribe` | Open the upstream subscription management URL |
| `accounts_list` / `account_save_current` / `account_switch` / `account_remove` | Multi-account snapshots under the Sunsetz application data directory |
| `session_import_transcript(_file)` | Import markdown/JSON chat into a new local session |

### Multi-account

- After successful login, Host **auto-snapshots** auth into `accounts/<id>/auth.json`.
- Switch copies snapshot → `~/.grok/auth.json` + agent-home, then disconnects live ACP.
- UI: Settings → Account → **「切换账号」** opens a modal to list / switch / remove.
  **「添加账号」** saves the current profile (if signed in) then starts OAuth login.

### Login failures (Access denied)

xAI may refuse device-code generation on some networks. Product response:

1. Surface long-form error + tips (VPN / device code / custom provider).
2. Prefer **Device code** path when OAuth fails; auto-open verification URL when CLI prints it.
3. Do not invent a parallel OAuth — always go through Grok Build CLI.

### Conversation import (not Grok.com cloud history)

Grok Build CLI does **not** expose grok.com web history. Supported migration:

- Settings → Account → **Import conversation** (`.md` / `.json` / `.txt`)
- Formats: `## User` / `## Assistant` markdown, or JSON `[{role,content}]`

## Settings IA

- **Account** (`settings.nav.account`): profile, Sunsetz Pro quota, heatmap, and call logs.
- **Runtime** (`settings.nav.runtime`): built-in kernel status, optional legacy CLI path, Doctor — **not** mixed into Account.

## Billing / quota

Primary path:

- `POST https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig`
- Body: empty gRPC-web frame `00 00 00 00 00`
- Headers: Bearer OAuth token + `Content-Type: application/grpc-web+proto`, `x-grpc-web: 1`, `Origin/Referer: grok.com`

Fallback (confirmed live JSON):

- `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits` with `x-grok-client-mode: cli`
- Nested `config.creditUsagePercent`, `productUsage[]`, period start/end

### Subscription tier

Quota endpoints return upstream membership values. `runtimeCompat.ts` recognizes those raw values, while `accountUi.ts` maps them to the product-owned `Sunsetz Pro` or `Sunsetz Pro Heavy` labels. Raw upstream membership branding must not render in ordinary Sunsetz UI.

- `subscriptionTier` → `BillingSnapshot.subscriptionTier` → `SunsetzProBrandKind`;
- the empty-session mark uses `SunsetzProMark`;
- a custom relay always uses the standard `Sunsetz Pro` mark.

UI shows **remaining %** (100 − used), product tags, and reset time.

Cache successes under the Sunsetz application data directory.

## Heatmap & call logs

- Heatmap uses discrete activity levels, month labels, and accessible tooltips.
- Data: local Runtime session signals → `requests` / `tokens` for roughly 371 days; this is separate from subscription billing.
- Call logs: recent sessions with model, turns, context tokens, duration, mtime.

## UI copy

All strings via `src/i18n/messages.ts` (`account.*` keys). See [i18n.md](./i18n.md).

## Security

- Profile DTO never includes `key` / `refresh_token` / raw access tokens.
- Login stdout/stderr must not be dumped to app logs if they may contain secrets.
- Doctor / export still go through existing redact paths.
