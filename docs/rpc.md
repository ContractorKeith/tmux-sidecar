# JSON-RPC contract (version 1)

The per-tmux-server `<runtime>/sock` is a Unix socket, mode 0600 inside a directory
owned by the daemon uid with mode 0700. Connections from other uids are dropped.
Each UTF-8 JSON object occupies one newline-terminated frame (maximum 1 MiB excluding the terminating newline).
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

Accepts omitted, null or empty object params. Clients warn once on stderr if
`daemon_version` differs from their binary version; they remain connected. Run
`tmux-sidecar daemon stop` so the next client starts the current binary.
`rpc_version` remains 1; doctor is a local diagnostic command, not an RPC method.

## list

Request: `{"jsonrpc":"2.0","id":2,"method":"list","params":{"state":"waiting"}}`

Response (empty): `{"jsonrpc":"2.0","id":2,"result":[]}`

Response (one run):

```json
{"jsonrpc":"2.0","id":2,"result":[{"id":"r1","server_start":1793800000,"pane_id":"%0","agent":"claude","agent_pid":1234,"agent_session_id":null,"cwd":"/work","project_root":null,"started_at":1793800000000,"ended_at":null,"state":"waiting","state_since":1793800000000,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null,"pane_name":null}]}
```

`pane_name` is a nullable string attached to the pane and survives agent restarts.
Lists active runs by default, newest id first. Optional `all: true` also includes
ended history, capped by `limit` (default 100, unsigned 32-bit count). The limit
applies after filtering; active-only results are not capped. Optional `state` filters
by working, waiting, blocked, idle, error or done.
Optional `group` selects one namespace: `manual:NAME` or `project:/absolute/root`.
A bare name means manual; a bare absolute path means project. A manual name that
looks like a path remains accessible with `manual:`; the two namespaces never union.
Project roots
are resolved afresh using session-board's git-root rules, outside the SQLite worker and
RPC task; `project_root` in each list result contains the resolved path. Group filtering
precedes the history limit. Unknown params and
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

Accepts omitted, null or empty object params. Preserves live runs, drains accepted
signal notifications for up to one second (idle clients cannot hold shutdown), closes connections,
removes socket and pid file, and exits. CLI stop waits up to five seconds for the acknowledged pid to exit (a zombie
already counts as exited) and for socket cleanup; it never spawns. SIGTERM and SIGINT also preserve runs for re-adoption at startup. Only a
confirmed tmux server exit ends all its runs. Poll errors are logged and retried;
five consecutive failures stop the daemon without ending runs. RPC requests remain
available while a process snapshot is being collected. Losing or replacing
the database file is fatal immediately and logged with its path.

## attention

Accepts omitted, null or empty object params. Returns an array of active unread runs
with project roots resolved as for `list` and nullable `pane_name`. Runs are
in blocked, error, waiting order, then `state_since` ascending (numeric rowid breaks ties).
A read mark at or after `state_since` removes the run. Each state change clears `read_at`.
Polling marks focused panes read only when pane_active, window_active and session_attached
are all true; detached sessions and hidden windows stay unread.

## history

Params: `{"sel":"r1","limit":100}`. `sel` accepts a run id or current pane selector;
`limit` is an unsigned 32-bit count, default 100. Returns all evidence and state changes,
newest `at` first, then newest row id, capped by limit. Each row contains `id`, `run_id`,
`at`, `source`, `event`, `detail` and decoded `payload`. State-change payloads contain
`run_id`, `pane_id`, `server_start`, `from`, `to`, `at`, and `cause` (the hook or transcript signal row id, or null
for process and clock evidence). State-change rows have source `engine`; transcript
readings have source `transcript` and event `transcript_reading`.

## wait

Params: `{"sel":"r1","until":"waiting","timeout":30}`. `until` is waiting,
blocked or done; optional `timeout` is an unsigned integer in seconds. Omitted or
null timeout waits indefinitely. The deadline includes selector resolution; zero can time out before the initial read.
Returns `{"run_id":"r1","state":"waiting","at":1793800000000}` when the target state is observed,
immediately if it already matches within the deadline (`at` is state_since). Subsequent matches use
the committed transition timestamp, including transient states followed quickly by
another change. The daemon subscribes before its initial read
and enforces the deadline itself; clients do not poll. Pane selectors resolve once
to a run id, so a replacement agent cannot satisfy an earlier wait.
A missing selector returns -32005; timeout returns -32004; a run ending in done
while waiting for another state returns -32001. Subscription overflow fails with
-32001 rather than silently missing transitions.

## send

Params: `{"sel":"%2","text":"next prompt","force":false}`. `force` defaults
to false. Returns `{}` after paste and Enter, and commits an optimistic working
state with source `send` plus its state_changed history. A new accepted hook receipt
after the send history row confirms it, independent of its capture timestamp.
Without confirmation within `send_confirm_secs`
(default 30), the next poll restores the prior state, state age and read mark.
Rollback is visible in history and state subscriptions with `notify: false`; it
never generates desktop or tmux notifications.
The daemon requires a live run in waiting or idle at paste time, else -32003 with the
current state. `force` skips only the state guard; ended or missing runs return
-32005. Empty or NUL text returns -32602; terminal failures before paste return -32603.
A per-run lock covers guard, paste and optimistic commit. Hook, transcript and
poll state mutations acquire current send locks in stable order before committing.
SQLite and read RPCs remain available during terminal work. Sends to distinct runs
can paste concurrently.
Scan and process probes each have a one-second deadline and run before acquiring
any run lock. The whole send has a three-second daemon deadline. Under the lock,
the daemon re-reads state and probes pid liveness with signal 0 before pasting.
Terminal calls each have a one-second cap bounded by the remaining send budget.
A deadline before paste aborts without pasting (-32004, safe to retry, CLI exit 4).
Once paste-buffer may have started, a terminal error or deadline returns -32007,
"outcome unknown after paste" (CLI exit 1); retrying may duplicate the prompt.
The same code covers Enter or state-commit failures after paste. The daemon joins terminal
work before replying, and an expired blocking task cannot start a later paste.
Terminal work runs off the RPC dispatcher and SQLite owner. Each paste uses a
unique named buffer: load-buffer reads text from stdin, paste-buffer uses
`-p -d -r -b NAME -t PANE` (`-r` preserves literal newlines), then send-keys sends exactly one Enter. Text never enters
the tmux key-name parser; failed pastes remove their buffer.

## ack

Params: `{"selectors":["r1","%2"]}` (or `{"sel":"r1"}`). All selectors are resolved
before mutation. Sets `read_at` to daemon time; returns `{}`. Pane selectors use the
daemon-cached current tmux server start, even when newer stale
run records exist. Missing runs return -32005.
An empty selector list or malformed selector returns -32602.

## name

Params: `{"sel":"r1","name":"build"}`. Omitted or null `name` clears the pane name.
Names contain 1–64 Unicode characters and no control characters; invalid values return
-32602. Returns `{}`; names attach to pane keys and survive agent restarts. Missing runs return -32005.

## group.add

Params: `{"group":"review","selectors":["r1","%2"]}` (single `sel` also accepted).
Manual names contain 1–64 Unicode characters and no control characters. Creates the
manual group if necessary and adds pane keys idempotently; returns `{}`.
All selectors are validated first. Membership survives agent restarts in the same pane,
and does not transfer across tmux server restarts. Missing runs return -32005.

## group.rm

Same params as `group.add`. Removes matching pane keys idempotently; returns `{}`.
Empty existing groups remain listed; removing an absent group does not create one.

## group.ls

Accepts omitted, null or empty object params. Returns manual memberships as an array
of `{"name":"review","server_start":1793800000,"pane_id":"%2"}` rows, sorted by name
and pane key. An empty group has one row with null `server_start` and `pane_id`.
Project groups are computed by `list` and are not stored here.

## subscribe

Accepts omitted, null or empty object params. Returns `{"subscribed":true}`, then pushes
`state_changed` notifications on the same connection for subsequent committed changes.
There is no historical replay; repeating subscribe does not duplicate delivery. Requests
can still be sent on the connection. Each subscriber has a bounded 256-change broadcast
queue. On overflow the daemon attempts a final `subscription_error` notification with
params `{"code":-32001,"message":"slow subscriber exceeded 256 changes"}` and closes
the connection. Socket writes have a 250 ms deadline; an unresponsive socket may prevent
the final error from reaching the peer. Subscribers never block daemon mutation or polling.

## signal

Hooks append a durable frame first, call `hello` and verify `server_key`, then
attempt live delivery of the same invocation:

```json
{"jsonrpc":"2.0","method":"signal","params":{"server_start":1793800000,"pane_id":"%0","agent":"grok","agent_pid":1234,"at":1793800000000,"nonce":"8f221dfd42762f2bef936db976722638c","source":"hook:claude","event":"Notification","detail":"permission_prompt","agent_session_id":"fake-session","cwd":"/work","transcript_path":null,"payload":{"hook_event_name":"Notification","notification_type":"permission_prompt"}}}
```

`agent` and `agent_pid` identify the outermost agent ancestor within the pane's
process tree, independent of the payload parser named by `source` (`hook:claude`, `hook:codex`,
`hook:codex-notify`, `hook:grok`, `hook:opencode`). Linux ancestry reads `/proc`
directly; macOS reads ppid and comm per pid with `proc_pidinfo`. Neither hook ancestor
walk spawns a process. Pane-root pids are cached by polling; a missing or stale
cache uses a bounded tmux query. If ancestry does not reach that pane (including
a deleted pane), hooks set the out-of-band `orphaned` field and log the reason.
The daemon first matches the agent session id, then an identified `parent_agent_pid`,
then the single same-agent, same-cwd active run without a session id. Ambiguous or
missing owners are dropped and logged; no run is created on the stale pane.
A failed pane-pid lookup is unknown and preserves normal pane/pid delivery; a direct
agent child of init still proves orphaning. Routing fields are not stored in payload.
`server_start` is tmux's start
time in seconds; `at` is capture time in Unix milliseconds. `event` is the hook
event name; optional `detail` holds notification type, tool matcher, reason or
status type. Both are stored separately. Hook-derived done rejects later hooks
from the same agent session; a different
session id or UserPromptSubmit can reopen it. Claude SessionEnd clear/logout is
waiting; other SessionEnd reasons are done. Process exit sets ended_at and is final.
For Codex, only inline hooks establish native session identity; notify never does.
OpenCode V2 execution.started maps to
session.status/busy, execution.succeeded/interrupted to session.idle, and
execution.failed to session.error; the original V2 event remains in `payload`.

`nonce` is 128 random bits encoded as 32 hex characters, generated once per hook
invocation. Receipt and signal commit together, deduplicating only that nonce.
Two genuine calls with identical payload and timestamp are both retained.
Unknown runs are created immediately with an initial waiting state. Session id
and transcript metadata attach to existing runs, including ended runs. Each signal applies
the exact per-agent event/detail state table. The receipt, signal,
run state and any `state_changed` history row commit in one transaction. Unknown events
retain state but establish hook precedence. Signals older than the newest stored hook
timestamp for the run retain
history and metadata without changing state. Same-state hooks advance that high-water
mark without changing `state_since` or read marks. The first hook is always eligible. Spool
batches are sorted by capture time. Process exit always wins over hooks and transcripts.
Once a hook has arrived, transcript scanning stops for that run; subsequent metadata
comes from hook payloads. Transcript-only runs follow session-board source derivation.
A separate task scans at most every two seconds and drops ticks while a scan is busy;
slow transcript work never delays pane/process polling. The v0.1.0 readers apply age
filters after parsing; pre-parse file filtering is deferred to
[session-board#130](https://github.com/ContractorKeith/session-board/issues/130).
Read waiting runs become idle after config `idle_after_secs` (default 1800). Re-reading
the same completed transcript turn does not undo clock-derived idle. Requests may use an
id and receive `{}` after storage;
invalid params return -32602. Notifications receive no response.

Stdin formats drain to EOF before context/ancestor checks, keeping up to 64 MiB
of input. Bulky payload values become bounded previews (up to 4 KiB per string,
with `…truncated` and a root `_tmux_sidecar_truncated` marker). Stored payloads
remain below 256 KiB; ordinary payloads retain their fields. Codex notify takes
its payload from argv instead. The transport frame limit remains 1 MiB.

The daemon writes `<runtime>/server_start` outside the SQLite worker, tied to the
tmux socket device/inode and change time so a replacement invalidates stale context.
Hooks read that cache; only an absent cache requires a bounded tmux query. A durable frame
may carry `server_start: 0` until the daemon's first poll supplies that context.
The daemon resolves it before store validation. One lock-free O_APPEND write of the
complete frame precedes tmux fallback,
socket delivery, and detached startup. After writing, an appender verifies that its inode
still names the live spool. If replay renamed it meanwhile, the appender retries the same
nonce in the fresh live spool; receipts deduplicate either copy. Live
delivery uses at most 100 ms of the remaining 180 ms budget. Overruns are logged;
there is no worker thread that can abandon or tear an append. Append failure is
logged as a lost signal. Spool, rejected-file and hook-log opens refuse symlinks
and non-regular files.

Replay runs after initial context discovery and on poll ticks, in order. Bad or
incomplete lines move to `spool.rejected` and are logged; good lines continue.
Replay errors are logged independently and never fail startup or consume the
five-failure process-poll budget. Replay first recovers leftover `spool.replaying.*` files,
then claims the live spool by
renaming it to `spool.replaying.<pid>.<nonce>` and creates a fresh live spool. A claim is
removed only after its good signals commit and rejected lines are preserved. Failed or
interrupted claims remain for restart recovery; concurrent appenders never need a lock.
A new append separates an existing partial tail
with a newline. SQLite work, file I/O and parsing retain their separate owners.

`hooks install --chain` wraps an existing Codex notify program: record the last
argv payload first, then exec the previous program with its configured arguments
and the unchanged appended payload. Uninstall restores the prior notify entry.

### state_changed notification

```json
{"jsonrpc":"2.0","method":"state_changed","params":{"run_id":"r1","pane_id":"%0","server_start":1793800000,"from":"working","to":"waiting","at":1793800000000,"cause":42,"agent":"claude","project_root":"/work/project","pane_name":"review"}}
```

`agent` identifies the run's agent. `project_root` and `pane_name` are nullable;
`pane_id` remains available when the pane has no user name. These labels are
captured in the same transaction as the state change. When a stored project root
is unavailable, the desktop notification worker resolves it through `list` outside
the SQLite owner and RPC tasks.

### Errors

Error response example:
`{"jsonrpc":"2.0","id":4,"error":{"code":-32601,"message":"unknown method"}}`

| Code | Meaning |
|---|---|
| -32700 | Malformed JSON; id null |
| -32600 | Invalid request (including unsupported JSON-RPC version) |
| -32601 | Unknown method |
| -32602 | Invalid params |
| -32603 | Internal failure |
| -32001 | General error; slow subscription overflow |
| -32002 | Usage error (reserved) |
| -32003 | Refused by send state guard |
| -32004 | Wait timeout |
| -32005 | Selector not found |
| -32006 | Daemon unreachable (reserved) |
| -32007 | Send outcome unknown after paste may have started (CLI exit 1) |

### CLI output

Read verbs expose their RPC result unchanged with `--json`. `watch --json` emits
one state_changed params object per line (NDJSON); human watch emits a compact
state table. Watch exits 0 on Ctrl-C, 1 on daemon exit or subscription failure.
Other human reads use compact tab-separated tables; control characters in text
are escaped. `-q` suppresses successful output. `send SEL -` reads UTF-8 stdin.
Prompts must fit the complete 1 MiB JSON request frame, including escaping,
selector and framing bytes; the CLI checks the same serialized frame as the client.
Send's client budget is the daemon deadline plus two seconds. A lost send response
returns exit 1 with "send outcome unknown — check the pane before retrying".
A connection lost during an established wait returns exit 1, "daemon stopped while
waiting". Exit 6 covers failure to reach the daemon after auto-spawn.
CLI errors map to 1 general, 2 usage, 3 refused, 4 timeout, 5 selector not found,
6 unreachable after auto-spawn. Mutation commands report a brief success message.

### UI view invalidation (M5)

`list` results additionally include nullable `pane_name`, attached to the run's pane
key. `subscribe` accepts optional `views: true` (boolean). These subscribers also
receive `{"jsonrpc":"2.0","method":"view_changed","params":{}}` when committed
run data, read marks, pane names or manual groups change. Invalidation coalesces to
one pending event per connection; reload `list` and `group.ls` after receiving it.
Subscribe before the initial load to avoid missing concurrent mutations. Empty
subscribe params preserve the state-change-only stream. The UI reloads only after
pushes and never reads the database. Disconnects require a fresh hello, subscribe
and initial load.

State-change payloads include `pane_id` and `server_start` from the committed run,
so notification consumers can target its pane without another RPC. Historical rows
created before M5 may omit these fields.
