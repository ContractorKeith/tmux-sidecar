# State from agent hooks and transcripts, not from the screen

An agent run's state comes from the agent's own hooks first (Claude Code, Grok, Codex,
OpenCode plugin) and from session-board's transcript readers when no hook has spoken. Pane
contents are never read for state. Herdr and Muxy read the bottom of the screen against rule
files; we skip that because rules break on every TUI redesign and need constant upkeep, while
hooks are a contract the agent vendors document. This is the opposite of session-board's
ADR-0001 ("observe, never install hooks"): that tool must work with no setup, this one owns
sessions and can ask for `hooks install`. The cost is setup per agent and breakage when a
vendor renames an event; the transcript fallback and `doctor`'s drift check cover that.
Screen rules stay a Later row, added beside the hooks if an agent ever loses them.
