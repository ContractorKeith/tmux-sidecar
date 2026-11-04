# JSON-RPC contract (version 1)

The per-tmux-server `<runtime>/sock` is a Unix socket, mode 0600 inside a directory
owned by the daemon uid with mode 0700. Connections from other uids are dropped.
Each UTF-8 JSON object occupies one newline-terminated frame (maximum 1 MiB).
An oversized frame receives -32600, "frame exceeds 1 MiB", with id null; the
daemon then closes that connection.
Requests require `jsonrpc: "2.0"`, a string or integer `id`, and `method`.
Responses echo `id` and contain exactly one of `result` or `error`.
Notifications have no id and receive no response. Batch requests are unsupported.
A connection supports multiple sequential requests. Run timestamps are Unix milliseconds;
`server_start` is tmux's start time in Unix seconds. Clients call `hello` once per
connection and verify its server key before issuing other requests.

## hello

Request: `{"jsonrpc":"2.0","id":1,"method":"hello"}`

Response: `{"jsonrpc":"2.0","id":1,"result":{"rpc_version":1,"daemon_version":"0.1.0","server_key":"012345abcdef"}}`

Accepts omitted, null or empty object params.

## list

Request: `{"jsonrpc":"2.0","id":2,"method":"list","params":{"state":"waiting"}}`

Response (empty): `{"jsonrpc":"2.0","id":2,"result":[]}`

Response (one run):

```json
{"jsonrpc":"2.0","id":2,"result":[{"id":"r1","server_start":1793800000,"pane_id":"%0","agent":"claude","agent_pid":1234,"agent_session_id":null,"cwd":"/work","project_root":null,"started_at":1793800000000,"ended_at":null,"state":"waiting","state_since":1793800000000,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null}]}
```

Lists active runs by default, newest id first. Optional `all: true` also includes
ended history, capped by `limit` (default 100, unsigned 32-bit count). The limit
applies after filtering; active-only results are not capped. Optional `state` filters
by working, waiting, blocked, idle, error or done. M1 creates waiting and done only.
Optional `group` returns -32601, "not implemented yet (M3)". Unknown params and
invalid states return -32602. Null and omitted filters are accepted.
`list --json` prints the result array directly. `list --all --limit 100 --json`
requests bounded history with `params: {"all":true,"limit":100}`.

## daemon.status

Request: `{"jsonrpc":"2.0","id":3,"method":"daemon.status"}`

Response: `{"jsonrpc":"2.0","id":3,"result":{"running":true,"pid":123,"server_key":"012345abcdef","poll_ms":1000}}`

Accepts omitted, null or empty object params. CLI status never auto-spawns; an
unreachable daemon gives exit 6.

## daemon.stop

Request: `{"jsonrpc":"2.0","id":4,"method":"daemon.stop"}`

Response: `{"jsonrpc":"2.0","id":4,"result":{"stopping":true,"pid":123}}`

Accepts omitted, null or empty object params. Preserves live runs, closes connections,
removes socket and pid file, and exits. CLI stop waits up to five seconds for the acknowledged pid to exit (a zombie
already counts as exited) and for socket cleanup; it never spawns. SIGTERM and SIGINT also preserve runs for re-adoption at startup. Only a
confirmed tmux server exit ends all its runs. Poll errors are logged and retried;
five consecutive failures stop the daemon without ending runs. RPC requests remain
available while a process snapshot is being collected. Losing or replacing
the database file is fatal immediately and logged with its path.

## attention

Reserved for M3; returns -32601, "not implemented yet (M3)".

## history

Reserved for M4; returns -32601, "not implemented yet (M4)".

## wait

Reserved for M4; returns -32601, "not implemented yet (M4)".

## send

Reserved for M4; returns -32601, "not implemented yet (M4)".

## ack

Reserved for M4; returns -32601, "not implemented yet (M4)".

## name

Reserved for M4; returns -32601, "not implemented yet (M4)".

## group.add

Reserved for M3; returns -32601, "not implemented yet (M3)".

## group.rm

Reserved for M3; returns -32601, "not implemented yet (M3)".

## group.ls

Reserved for M3; returns -32601, "not implemented yet (M3)".

## subscribe

Reserved for M3; returns -32601, "not implemented yet (M3)".

## signal

Hooks call `hello` and verify `server_key`, then send one notification:

```json
{"jsonrpc":"2.0","method":"signal","params":{"server_start":1793800000,"pane_id":"%0","agent":"grok","agent_pid":1234,"at":1793800000000,"source":"hook:claude","kind":"Notification:permission_prompt","agent_session_id":"fake-session","cwd":"/work","transcript_path":null,"payload":{"hook_event_name":"Notification","notification_type":"permission_prompt"}}}
```

`agent` and `agent_pid` identify the nearest agent ancestor, independent of the
payload parser named by `source` (`hook:claude`, `hook:codex`,
`hook:codex-notify`, `hook:grok`, `hook:opencode`). `server_start` is tmux's
start time in seconds; `at` is capture time in Unix milliseconds. `kind` is
the vendor event, with `:<notification type/tool matcher/reason/status type>`
when supplied. `payload` is the complete JSON payload, not a normalized copy.
Session id, cwd and transcript path are extracted from vendor-specific fields.
All keys shown above are required except nullable metadata fields. Unknown keys
and malformed identity values return -32602 for requests. An optional request
id receives `{}` on successful storage; notifications receive no response.

Unknown runs are created immediately with M1's initial waiting state. Metadata
attaches to an existing run, including ended runs during replay. Signals never
change state in M2. Source, kind, timestamp and raw payload are persisted.
Receipt and signal commit in one transaction; repeated payload/timestamp pairs
for the same run are idempotent. The receipt includes a payload hash and the
canonical payload to avoid hash collisions losing evidence.

On connect/hello/write failure, the hook appends this exact notification frame
under `spool.lock`, triggers detached startup, and exits silently. Delivery has
a 100 ms timeout; the entire hook has a 180 ms deadline. The daemon replays at
startup and each poll tick, then atomically removes only the committed prefix
under the same lock, retaining concurrent appends. Incomplete trailing frames
are retained. SQLite calls run on the store worker; spool I/O runs outside it.

`hooks install --chain` wraps an existing Codex notify program: record the last
argv payload first, then exec the previous program with its configured arguments
and the unchanged appended payload. Uninstall restores the prior notify entry.

### Reserved state_changed notification (M3)

```json
{"jsonrpc":"2.0","method":"state_changed","params":{"run_id":"r1","pane_id":"%0","state":"waiting","state_since":1793800000000}}
```

### Errors

Error response example:
`{"jsonrpc":"2.0","id":4,"error":{"code":-32601,"message":"not implemented yet (M3)"}}`

| Code | Meaning |
|---|---|
| -32700 | Malformed JSON; id null |
| -32600 | Invalid request (including unsupported JSON-RPC version) |
| -32601 | Unknown or not yet implemented method |
| -32602 | Invalid params |
| -32603 | Internal failure |
| -32001 | General error (reserved) |
| -32002 | Usage error (reserved) |
| -32003 | Refused (reserved) |
| -32004 | Timeout (reserved) |
| -32005 | Selector not found (reserved) |
| -32006 | Daemon unreachable (reserved) |
