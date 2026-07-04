#!/usr/bin/env bash
# Fails if any workspace crate depends on a crate in a higher architectural
# layer than itself. Cargo's cycle detection catches dependency cycles but
# not one-directional layer violations that don't happen to cycle back.
set -euo pipefail

# Layer 0 = foundation ... Layer 4 = orchestration. A crate may depend on
# crates in the same layer or any lower-numbered layer, never a higher one.
declare -A LAYER=(
  [agentverse]=0            [agentverse-skill]=0        [agentverse-logging]=0
  [agentverse-hitl]=1       [agentverse-session]=1      [agentverse-memory]=1
  [agentverse-memory-lancedb]=1 [agentverse-memory-pgvector]=1 [agentverse-integration]=1
  [agentverse-tools]=2      [agentverse-guardrails]=2   [agentverse-mcp]=2
  [agentverse-react]=3      [agentverse-plan]=3         [agentverse-router]=3
  [agentverse-subagent]=3   [agentverse-strategy]=3
  [agentverse-agent]=4
)

violations=0

while read -r pkg dep; do
  pkg_layer="${LAYER[$pkg]:-}"
  dep_layer="${LAYER[$dep]:-}"
  # Skip pairs where either side isn't a layered workspace crate (examples,
  # test-utils, and any crate not in the map above are unconstrained).
  [ -z "$pkg_layer" ] && continue
  [ -z "$dep_layer" ] && continue
  if [ "$dep_layer" -gt "$pkg_layer" ]; then
    echo "LAYER VIOLATION: $pkg (layer $pkg_layer) depends on $dep (layer $dep_layer)"
    violations=$((violations + 1))
  fi
done < <(
  cargo metadata --no-deps --format-version=1 |
  jq -r '.packages[] | .name as $pkg | .dependencies[] | select(.kind == null) | "\($pkg) \(.name)"'
)

if [ "$violations" -gt 0 ]; then
  echo "Found $violations layer-direction violation(s)."
  exit 1
fi
echo "No layer-direction violations found."
