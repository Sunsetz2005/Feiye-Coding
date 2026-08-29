# First-run setup gate

Product rules for the **full-screen initialization wizard** before the workbench home.

## Goals

1. **Soft gate:** Sign in with a Sunsetz account, or skip into the workbench.
2. Custom APIs live in Settings → My models. Official unified models are a later slice.
3. Grok CLI, console.x.ai keys, custom relay, and legacy companion import are **not** part of first-run.
4. Match app chrome (tokens, logo, dark/light); **no scrollbars** on the gate page.

## Flow

```
boot → load settings
  ├─ !setupWizardCompleted && !legacyDone → SetupWizard (sign in or skip)
  └─ setupWizardCompleted | onboardingDone | setupSkipped → home
```

Entering home does **not** require a grok binary. Default `runtimeBackend` is `sunsetz`.

### Sign in (skippable)

One entry: **Sign in with Sunsetz** (`account_login` OAuth). Busy state can cancel. **Skip for now** always enters the workbench even if the OAuth backend is unready.

Persists:

| Field | After sign-in | After skip |
|-------|---------------|------------|
| `setupWizardCompleted` | true | true |
| `onboardingDone` | true | true |
| `authSetupDeferred` | false | true |
| `setupSkipped` | false | true |
| `runtimeBackend` | `sunsetz` | `sunsetz` |

## Settings fields

| Field | Role |
|-------|------|
| `setupWizardCompleted` | Wizard finished |
| `authSetupDeferred` | User skipped account |
| `onboardingDone` / `setupSkipped` | Legacy; treated as done so the gate does not reappear |

## UI

- Component: `src/components/SetupWizard.tsx`
- Styles: `src/styles/setup-wizard.css` (overflow hidden, no scrollbars)
- i18n: `setup.*` keys in `src/i18n/messages.ts`

## Commands still used by the host

CLI probe and install commands remain for Settings → Runtime (legacy fold). They are not a first-run hard gate. The Runtime page presents the built-in kernel first.

| Command | Role |
|---------|------|
| `probe_cli` | Detect binary (legacy / Settings) |
| `cli_install_latest` | Download + link (legacy) |
| `cli_install_commands` | Platform shell command + docs URL |
| `pick_cli_binary` | File picker |
| `account_login` / `account_login_cancel` | Sunsetz account OAuth |
