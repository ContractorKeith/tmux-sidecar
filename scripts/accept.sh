#!/usr/bin/env bash
# scripts/accept.sh — the acceptance for tmux-sidecar. Worker, reviewer and CI run this
# exact script; the orchestrator never retypes it. Keep it under a few minutes.
# Bash 3.2 compatible (macOS): no mapfile, no ${var,,}, no associative arrays.
# Each milestone appends its live checks below; never weaken an earlier one.
set -eu

cd "$(dirname "$0")/.."
command -v tmux >/dev/null || { echo "accept: tmux is required" >&2; exit 1; }

scratch="$(mktemp -d)"
tsock="tsx-accept-$$"
cleanup() {
  tmux -L "$tsock" kill-server 2>/dev/null || true
  rm -rf "$scratch"
}
trap cleanup EXIT

echo "== gate"
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked

echo "== build"
cargo build --release --locked
bin="${ACCEPT_BIN:-$PWD/target/release/tmux-sidecar}"
"$bin" --version

echo "== e2e (real tmux)"
TMUX_SIDECAR_E2E=1 cargo test --locked --test e2e

echo "== isolate"
# Nothing below may touch real agent configs, real state or the default tmux server.
export HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_STATE_HOME="$scratch/state"
export TMUX_SIDECAR_RUNTIME_DIR="$scratch/run" TMUX_SIDECAR_STATE_DIR="$scratch/data"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
unset TMUX TMUX_PANE

echo "== live checks"
# One block per milestone, expected result in a comment. M0 has none beyond --version.

echo "accept: ok"
