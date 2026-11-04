# Hook payload fixtures

Captured on Linux, 2026-11-04. Versions: Claude Code 2.1.309, Codex CLI
0.166.0, Grok 1.0.70, OpenCode 2.2.5. Paths and UUIDs are scrubbed;
account/token fields are omitted. These fixtures contain no credentials.

A throwaway scratch git repo used a capture script that wrote stdin to
`cap/<event>-$RANDOM.json`; notify wrote the last argv instead. Claude used
`-p --settings <inline hooks>`; Grok used `--trust -p <prompt>` and project
`.grok/hooks/capture.json`; Codex used `exec -c notify=[...]` and inline
`hooks.<Event>` overrides. Prompts asked for `true` then `ok`.
No installer was run on a real profile. Capture attempts were bounded at 50
seconds and their process groups killed/reaped in a finally guard. Codex startup
added scratch-project trust; removing only that section reproduced the exact
pre-capture checksum. OpenCode unexpectedly started a detached service and
created service.json; that capture-owned service was stopped and the new file
removed. Final checksums match the original profile files.

OpenCode captures tried `OPENCODE_CONFIG_DIR` with both plugin/ and plugins/,
then also a scratch `XDG_CONFIG_HOME` and `--standalone`. Debug config confirmed
the scratch directory on that last attempt. All runs completed, but none
emitted capture files. Codex notify was captured;
inline command hooks did not produce capture files in exec even with
features.hooks=true (existing SessionStart/tool hooks reported Failed).
The remaining events, including interactive approval/cancel/error events,
are synthesized from the documented vendor schemas. They are examples, not
claims that the installed runtime emitted those events.

Schemas: [Claude](https://code.claude.com/docs/en/hooks),
[Codex](https://learn.chatgpt.com/docs/hooks),
[OpenCode plugins](https://opencode.ai/docs/plugins/), and Grok's installed
`~/.grok/docs/user-guide/10-hooks.md` (camelCase envelope with snake_case aliases).
StopCancelled is Grok-specific in its documentation; the Claude fixture is
synthesized solely to cover PLAN §6 and its install list.

## Real captures

- `claude/UserPromptSubmit.json`
- `claude/PreToolUse.json`
- `claude/PostToolUse.json`
- `claude/Stop.json`
- `claude/SessionEnd.json`
- `grok/UserPromptSubmit.json`
- `grok/PreToolUse.json`
- `grok/PostToolUse.json`
- `grok/Stop.json`
- `grok/SessionEnd.json`
- `codex/notify.json`

## Synthesized

- `claude/Notification-permission_prompt.json` — synthesized
- `claude/PermissionRequest.json` — synthesized
- `claude/StopCancelled.json` — synthesized
- `claude/Notification-idle_prompt.json` — synthesized
- `grok/Notification-permission_prompt.json` — synthesized
- `grok/StopCancelled.json` — synthesized
- `grok/Notification-idle_prompt.json` — synthesized
- `grok/StopFailure.json` — synthesized
- `codex/Stop.json` — synthesized
- `codex/UserPromptSubmit.json` — synthesized
- `codex/PreToolUse.json` — synthesized
- `codex/PostToolUse.json` — synthesized
- `codex/PermissionRequest.json` — synthesized
- `codex/SessionEnd.json` — synthesized
- `opencode/session.status.json` — synthesized
- `opencode/session.idle.json` — synthesized
- `opencode/session.error.json` — synthesized
- `opencode/permission.asked.json` — synthesized
- `opencode/permission.replied.json` — synthesized
