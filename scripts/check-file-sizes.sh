#!/usr/bin/env bash
# Fails if any .rs file exceeds 600 lines unless it's on the allowlist (in
# which case it may not exceed its recorded cap). Prevents the unbounded
# growth that produced the pre-decomposition agent.rs (1,829 lines).
set -euo pipefail

THRESHOLD=600
ALLOWLIST="scripts/file-size-allowlist.txt"
violations=0

declare -A CAP
while read -r path cap; do
  [ -z "$path" ] && continue
  CAP["$path"]="$cap"
done < "$ALLOWLIST"

while read -r count path; do
  rel="${path#./}"
  cap="${CAP[$rel]:-$THRESHOLD}"
  if [ "$count" -gt "$cap" ]; then
    echo "FILE TOO LARGE: $rel has $count lines (cap: $cap)"
    violations=$((violations + 1))
  fi
done < <(find . -name '*.rs' -not -path '*/target/*' -not -path './.worktrees/*' | xargs wc -l | grep -v ' total$')

if [ "$violations" -gt 0 ]; then
  echo "Found $violations file-size violation(s)."
  exit 1
fi
echo "No file-size violations found."
