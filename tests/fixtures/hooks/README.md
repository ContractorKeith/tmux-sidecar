# Hook payload fixtures and live integration proofs

Current Linux skeptic run, 2026-11-07: Claude Code 2.1.311, Codex CLI 0.166.0,
Grok 1.0.77 and OpenCode 2.2.6. OpenCode failed the round-1 live check on 2.2.6
because the hook ancestor walk missed its plugin host (#19). The ancestry fix has now
landed; it still needs a fresh live 2.2.6 check. Historical captures below used
Claude 2.1.309, Grok 1.0.70 and OpenCode 2.2.5 on 2026-11-04. Home paths, session/event ids and UUIDs are
scrubbed. No token or account fields are retained.

## Proven live

- Claude: installed entries into a scratch HOME, replaced only the binary path
  with a capture program, then used the real profile with
  `claude -p --settings <scratch>/.claude/settings.json "reply ok"`.
  Exit 0; our installed command shape fired UserPromptSubmit, Stop and SessionEnd.
  The earlier real PreToolUse/PostToolUse captures asked Claude to run `true`.
- Grok: project-local capture hooks in a scratch git repo with
  `grok --trust -p "Run true with the shell tool, then reply ok."` produced the
  five real payloads listed below. StopCancelled remains synthesized.
- OpenCode: installed the generated default `{ id, setup(ctx) }` plugin in
  `<scratch XDG_CONFIG_HOME>/opencode/plugins/tmux-sidecar.js`, replacing only
  its executable path with a capture program. The installed 2.2.5 binary ran
  `opencode --standalone --print-logs run "reply ok"` with scratch HOME and XDG
  config/data/state/cache directories. Exit 0; its log named the loaded plugin;
  the capture program recorded `hook opencode` twice and captured both
  session.execution.started and session.execution.succeeded. Detached hook
  children preserve the final notification during OpenCode teardown.
- Codex notify: the original argv capture is real. The 2026-11-07 skeptic run on
  **Codex 0.166.0 proved inline UserPromptSubmit, PreToolUse, PermissionRequest,
  PostToolUse and SessionEnd live** in an isolated CODEX_HOME. The earlier scratch
  run produced no inline captures; that limitation is superseded by the skeptic
  evidence. Committed inline payload files remain synthesized schema fixtures;
  Interrupt remains unproven live.

All live runs had 50-second deadlines and process-group cleanup. The initial
capture round's incidental real Codex trust and OpenCode service writes were
restored to the original hashes. Round-1 proof runs used isolated profiles or
per-invocation settings; before/after real-config hashes are recorded separately.
Real agent CLIs are never invoked by CI.

## Versioned schemas and adapters

OpenCode's published @opencode/plugin, @opencode/client and @opencode/schema
2.2.5 packages were inspected. Its Promise API uses a default definition with
id/setup and `ctx.event.subscribe({ signal })`; V2 payloads use data/sessionID
and location/directory, not V1 properties. The filenames preserve the M3
semantic names: execution.started -> session.status with detail busy;
execution.succeeded/interrupted -> session.idle; execution.failed -> session.error.
The captured original type/data remain untouched in each fixture and stored
payload. Legacy event spellings are also accepted. Error and permission
fixtures are synthesized from the 2.2.5 package types.

Claude's installer excludes StopCancelled; it belongs to Grok only. Grok always
gets its own StopCancelled entry, with common hooks shared from Claude. If that
compatibility is disabled, common hooks use a separate native common file.
Codex installs Interrupt, not Stop. Interrupt includes all required fields and
no unknown fields from its generated command-input schema.

Sources: [Claude hooks](https://code.claude.com/docs/en/hooks),
[Codex hooks](https://learn.chatgpt.com/docs/hooks),
[Codex generated schemas](https://github.com/openai/codex/tree/main/codex-rs/hooks/schema/generated),
[OpenCode 2.2.5 plugin package](https://www.npmjs.com/package/@opencode/plugin/v/2.2.5),
and Grok's installed `~/.grok/docs/user-guide/10-hooks.md`.

## Real captures (13)

- `claude/PostToolUse.json`
- `claude/PreToolUse.json`
- `claude/SessionEnd.json`
- `claude/Stop.json`
- `claude/UserPromptSubmit.json`
- `codex/notify.json`
- `grok/PostToolUse.json`
- `grok/PreToolUse.json`
- `grok/SessionEnd.json`
- `grok/Stop.json`
- `grok/UserPromptSubmit.json`
- `opencode/session.idle.json`
- `opencode/session.status.json`

## Synthesized (18)

- `claude/Notification-idle_prompt.json` — synthesized
- `claude/Notification-permission_prompt.json` — synthesized
- `claude/PermissionRequest.json` — synthesized
- `claude/StopFailure.json` — synthesized
- `codex/Interrupt.json` — synthesized
- `codex/PermissionRequest.json` — synthesized
- `codex/PostToolUse.json` — synthesized
- `codex/PreToolUse.json` — synthesized
- `codex/SessionEnd.json` — synthesized
- `codex/UserPromptSubmit.json` — synthesized
- `grok/Notification-idle_prompt.json` — synthesized
- `grok/Notification-permission_prompt.json` — synthesized
- `grok/PermissionRequest.json` — synthesized (Claude-compatible permission entry)
- `grok/StopCancelled.json` — synthesized
- `grok/StopFailure.json` — synthesized
- `opencode/permission.asked.json` — synthesized
- `opencode/permission.replied.json` — synthesized
- `opencode/session.error.json` — synthesized
