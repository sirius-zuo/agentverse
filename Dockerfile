# syntax=docker/dockerfile:1
FROM rust:1.75-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies before copying source
COPY Cargo.toml Cargo.lock ./
COPY avs-core/Cargo.toml            avs-core/Cargo.toml
COPY avs-agent/Cargo.toml           avs-agent/Cargo.toml
COPY avs-skill/Cargo.toml           avs-skill/Cargo.toml
COPY avs-session/Cargo.toml         avs-session/Cargo.toml
COPY avs-hitl/Cargo.toml            avs-hitl/Cargo.toml
COPY avs-tools/Cargo.toml           avs-tools/Cargo.toml
COPY avs-strategy/Cargo.toml        avs-strategy/Cargo.toml
COPY avs-react/Cargo.toml           avs-react/Cargo.toml
COPY avs-plan/Cargo.toml            avs-plan/Cargo.toml
COPY avs-router/Cargo.toml          avs-router/Cargo.toml
COPY avs-mcp/Cargo.toml             avs-mcp/Cargo.toml
COPY avs-memory/Cargo.toml          avs-memory/Cargo.toml
COPY avs-memory-lancedb/Cargo.toml  avs-memory-lancedb/Cargo.toml
COPY avs-memory-pgvector/Cargo.toml avs-memory-pgvector/Cargo.toml
COPY avs-guardrails/Cargo.toml      avs-guardrails/Cargo.toml
COPY avs-integration/Cargo.toml     avs-integration/Cargo.toml
COPY avs-logging/Cargo.toml         avs-logging/Cargo.toml
COPY avs-subagent/Cargo.toml        avs-subagent/Cargo.toml
COPY avs-test-utils/Cargo.toml      avs-test-utils/Cargo.toml
COPY examples/hello-agent/Cargo.toml            examples/hello-agent/Cargo.toml
COPY examples/slack-hr-assistant/Cargo.toml     examples/slack-hr-assistant/Cargo.toml
COPY examples/react-calculator/Cargo.toml       examples/react-calculator/Cargo.toml
COPY examples/web-search-agent/Cargo.toml       examples/web-search-agent/Cargo.toml
COPY examples/code-review-agent/Cargo.toml      examples/code-review-agent/Cargo.toml
COPY examples/anthropic-react/Cargo.toml        examples/anthropic-react/Cargo.toml
COPY examples/http-agent/Cargo.toml             examples/http-agent/Cargo.toml
COPY examples/mcp-demo/Cargo.toml               examples/mcp-demo/Cargo.toml
COPY examples/demo-tools/Cargo.toml             examples/demo-tools/Cargo.toml
COPY examples/project-feasibility/Cargo.toml    examples/project-feasibility/Cargo.toml
COPY examples/business-report/Cargo.toml        examples/business-report/Cargo.toml
COPY examples/doc-pipeline/Cargo.toml           examples/doc-pipeline/Cargo.toml
COPY examples/support-router/Cargo.toml         examples/support-router/Cargo.toml
COPY examples/accountant-workflow/Cargo.toml    examples/accountant-workflow/Cargo.toml

# Stub src dirs so cargo fetch succeeds without real source
RUN find . -name "Cargo.toml" -not -path "./Cargo.toml" | while read f; do \
      dir=$(dirname "$f"); \
      mkdir -p "$dir/src"; \
      printf 'fn main(){}' > "$dir/src/main.rs"; \
      printf 'pub fn stub(){}' > "$dir/src/lib.rs"; \
    done
RUN cargo build --release --package example-http-agent 2>/dev/null || true

# Now copy real source
COPY . .
RUN cargo build --release --package example-http-agent

# ── Runtime image ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/example-http-agent /app/agent
COPY --from=builder /build/examples/http-agent/skills/ /app/skills/

ENV RUST_LOG=info
ENV LOG_FORMAT=json
ENV HOST=0.0.0.0
ENV PORT=3000

EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/v1/health || exit 1

ENTRYPOINT ["/app/agent"]
