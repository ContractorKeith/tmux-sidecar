---
name: delete-comment
description: "Audit and remove code comments in tmux-sidecar under ADR-0003: classify each comment as tautology, stale, excuse or decision record, delete it, file an issue for each excuse, move decisions into docs/adr/, and lower the comment ratchet baseline. Use for '/delete-comment', 'run the comment audit', 'clean up comments', or when scripts/comment_lint.py fails on a branch."
---

# delete-comment — tmux-sidecar

ADR-0003 bans code comments. `scripts/comment_lint.py` is a ratchet over
`scripts/comment-baseline.txt`: no file may gain comments, and a file that loses them must have
its baseline lowered. This skill does one audit pass and leaves the ratchet tighter.

Exempt, never touch: `// SAFETY:` comments (and their continuation lines) on `unsafe` blocks,
which clippy's `undocumented_unsafe_blocks` requires. CLI help is data in
`#[command(about = "...")]`; if a clap doc comment appears, convert it to that attribute rather
than deleting it, or `--help` loses the text.

## 1. Inventory

```bash
python3 scripts/comment_lint.py --list        # file:line: text, for every counted comment
```

Scope is the argument if one was given (a path or file list), otherwise everything listed.
Group consecutive lines into one comment. Read each with the code around it.

## 2. Classify

Pick exactly one per comment:

| Category | Test | Action |
|---|---|---|
| Tautology | Says what the next lines already say (names, types, test asserts) | Delete |
| Stale | Mentions a milestone (`M0`…`M7`), an old implementation, or code that no longer matches | Delete |
| Excuse | Justifies a shortcut: a loosened assertion, a retry/sleep, a deferred upstream fix, production code shaped by tests | Delete and file an issue |
| Decision record | A non-obvious why that someone could break without knowing: ordering, ownership, platform behaviour, compatibility | Move into an ADR, then delete |

When torn between tautology and decision record, ask: would a competent reader who deleted the
line be surprised by a bug report later? If yes, it is a decision record.

## 3. Act

- **Decision records:** add a bullet to the themed ADR that fits, naming the file and function,
  not a line number: `` - `src/hook/spool.rs` `claim`: … ``. Themes today:
  0004 daemon lifecycle and descriptors, 0005 hook delivery and spool, 0006 daemon task
  ownership, 0007 state, detection, paths and config, 0008 platform differences, 0009 test
  harness. Start a new numbered ADR only for a new theme; put a one-paragraph context first, as
  0001 and 0002 do. Merge duplicates rather than repeating a decision.
- **Excuses:** one GitHub issue each, labelled `comment-audit`:
  `gh issue create --label comment-audit --title "<fix the shortcut>" --body "<where, what the comment excused, what a real fix looks like>"`.
  Create the label first if missing (`gh label create comment-audit --color FBCA04`).
- **Delete** the comment lines. For a trailing comment, delete only the comment text.
- Never change behaviour in this pass. A deleted comment that exposes a bug becomes an issue.

## 4. Tighten and verify

```bash
cargo fmt --all
python3 scripts/comment_lint.py --update      # only ever lowers the baseline
python3 scripts/comment_lint.py
scripts/accept.sh
```

Commit as `chore: comment audit — <scope>` with the classification counts in the PR body
(tautology, stale, excuse with issue numbers, decision records with their ADRs).
