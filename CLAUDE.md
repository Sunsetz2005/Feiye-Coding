# Claude instructions — Sunsetz

Follow `AGENTS.md` as the primary repository instruction file.

## Obsidian knowledge base is part of the delivery

- Vault: `/Users/a10954/Documents/Documents - Sunsetz的MacBook Pro/Obsidian Vault/Sunsetz/`
- Entry: `Sunsetz 项目知识库 MOC.md`
- Before work: read the handoff overview, current status, roadmap, and relevant topic notes; run `scripts/sync-obsidian-knowledge.sh`.
- Before reporting completion: update the current status and every affected progress, roadmap, issue, decision, operations, or timeline note; then run the sync script again.
- `.githooks/post-commit`, `.githooks/post-checkout`, and `.githooks/post-merge` automatically refresh objective Git and repository-document mirrors. Install them with `scripts/install-obsidian-hooks.sh`.
- Completion claims require code, test, Git, or authoritative-document evidence. Do not write plans as completed work.
- Record only server IP, login account name, and key file location. Never copy private-key contents, passwords, API keys, tokens, cookies, `.env` values, or database secrets into Git, the Vault, logs, or chat.
