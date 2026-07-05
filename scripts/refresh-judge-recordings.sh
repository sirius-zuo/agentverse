#!/usr/bin/env bash
# Refreshes agentverse-eval's judge-harness recorded LLM/judge responses
# (avs-eval/fixtures/recordings/*.toml) against LIVE models. Never run this
# in CI — it requires real API keys and makes real network calls, which
# this project deliberately never automates (see DEVELOPMENT.md's
# "Eval Harness" section).
#
# Usage:
#   ANTHROPIC_API_KEY=... OPENAI_API_KEY=... ./scripts/refresh-judge-recordings.sh
#
# This script does NOT automatically rewrite the recording files. To refresh
# one case (e.g. avs-eval/fixtures/recordings/react_tool_call.toml):
#   1. In avs-eval/tests/judge_test.rs, temporarily change that case's test
#      to point its ConnectionManager(s) at the real provider's live base URL
#      (e.g. "https://api.anthropic.com") instead of a MockServer, using a
#      real API key from your environment — for both the agent-under-test
#      connection and the judge connection.
#   2. Run the single test with
#      `cargo test -p agentverse-eval <test_name> -- --nocapture`
#      and add temporary `eprintln!("{}", response.content)` calls after each
#      real LLM/judge call to capture the actual response text.
#   3. Copy each captured response into the matching `content` field of the
#      case's `fixtures/recordings/<case>.toml` file, in turn order
#      (agent_turns[0].content, agent_turns[1].content, ..., judge_turn.content).
#   4. Revert judge_test.rs's temporary live-endpoint change back to pointing
#      at a MockServer + `load_recording(...)` (this should require no code
#      change if you only edited the TOML file's content in step 3 — the
#      test itself doesn't change, only the recording it loads).
#   5. Run the test again against the refreshed recording and confirm it
#      still passes.
#   6. Review the diff (this is real model output — read it before
#      committing) and commit as a normal fixture update.
#
# This manual, per-case process is intentional: it forces a human to read
# and approve what a live model actually said before it becomes a permanent
# regression-locked expectation.

set -euo pipefail

if [ -z "${ANTHROPIC_API_KEY:-}" ] && [ -z "${OPENAI_API_KEY:-}" ]; then
  echo "ERROR: set at least one of ANTHROPIC_API_KEY / OPENAI_API_KEY before running this script." >&2
  exit 1
fi

echo "This script is a documented manual procedure, not an automated rewriter."
echo "See the comment block at the top of this file for the exact steps."
echo "Recording files live in avs-eval/fixtures/recordings/*.toml."
echo "No files were changed by running this script."
