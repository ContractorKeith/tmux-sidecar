# No code comments; decisions live in ADRs

Source files carry no comments. A comment either repeats the code (tautology), describes code
that has since changed (stale), excuses a shortcut that should be an issue instead, or records a
decision. Decisions go in `docs/adr/`, where they are reviewed as decisions and cannot drift
silently beside the line they described. Shortcuts become GitHub issues. Everything else is
deleted. We took this after the 2026-11 audit found 339 Rust comment lines, of which about a
quarter were tautologies or stale milestone notes and the rest were decisions scattered across
most of the source tree.

Two kinds of text stay in source:

- `// SAFETY:` comments on `unsafe` blocks. Clippy's `undocumented_unsafe_blocks` lint requires
  them, and the invariant belongs at the call site. Continuation lines of a `SAFETY:` comment
  count as part of it.
- CLI help text. clap reads it from `#[command(about = "...")]` attributes, not doc comments,
  so help strings are data, not comments.

`scripts/comment_lint.py` enforces this as a ratchet. It counts comments in every tracked Rust,
Python, shell and YAML file and compares each file with `scripts/comment-baseline.txt`. A file
may not gain comments. When a file loses comments the baseline must be lowered in the same
change (`python3 scripts/comment_lint.py --update` only ever lowers it). A file absent from the
baseline must have none. `scripts/accept.sh` runs the lint, so CI does too.

The `/delete-comment` skill (`.agents/skills/delete-comment/`) repeats the audit: classify each
remaining comment, delete or move it, file issues for excuses, lower the baseline.
