# HTTP Agent Registry

## Goal

Replace the Unix socket transport with a persistent HTTP-based registry. Agents self-register with aether on startup. Aether tracks health via polling. Agents communicate with each other directly via their own configured I/O — aether is a lightweight lifecycle coordinator, not a message router.

## Design Principles

- **Agents are standalone units.** An agent runs without aether. Aether involvement is opt-in via `AETHER_REGISTRY_URL`.
- **Aether is a lifecycle coordinator, not a proxy.** It starts agents, monitors health, and triggers workflow entry points. It never carries business data between agents.
- **HTTP everywhere.** No custom TCP transport, no binary framing. Agent-to-aether and aether-to-agent communication is plain HTTP JSON.
- **Best-effort coordination.** No durable workflow guarantees. Aether detects failures via health polling; agents own their own reliability.

## What Changes

### Dropped

- `aether-core/src/transport/unix.rs` — `UnixSocketTransport`, `UnixSocketFactory`
- `aether-core/src/bin/echo_agent.rs` — Unix socket echo agent
- `avs-server/src/unix_adapter.rs` — Unix socket adapter mode
- Envelope binary framing (length-prefixed TCP wire format)
- `AETHER_SOCKET_PATH` env var

### Added to aether-core

- `src/registry_server.rs` — HTTP endpoints for agent registration, discovery, event ingestion
- `src/registry_store.rs` — SQLite-backed persistence via `rusqlite`
- `src/health_poller.rs` — background tokio task, polls registered agents periodically

### Added to avs-server

- `src/aether_client.rs` — registration on startup, deregistration on SIGTERM, event push

### Unchanged in avs-server

`/health`, `/ready`, `/invoke` — already present, no changes needed.

## Agent Operating Modes

| Mode | HTTP Server | Aether Registration | Description |
|------|-------------|---------------------|-------------|
| Standalone | No | No | Pure agent, no management plane |
| Managed standalone | Yes (avs-server) | No | HTTP management, no aether |
| Aether-managed | Yes (avs-server) | Yes | Full lifecycle management |

`AETHER_REGISTRY_URL` controls registration. If unset, agent runs in standalone or managed standalone mode with no aether involvement.

## Registry Data Model

### SQLite Schema

```sql
CREATE TABLE agents (
    instance_id   TEXT PRIMARY KEY,        -- UUID assigned by aether on registration
    name          TEXT NOT NULL,           -- logical agent name (non-unique)
    http_url      TEXT NOT NULL UNIQUE,    -- base URL of agent's HTTP server
    capabilities  TEXT NOT NULL,           -- JSON array of capability strings
    metadata      TEXT NOT NULL DEFAULT '{}',
    registered_at TEXT NOT NULL,           -- ISO 8601 timestamp
    last_health_check TEXT,               -- ISO 8601 timestamp, NULL until first poll
    status        TEXT NOT NULL DEFAULT 'unknown'  -- unknown | healthy | unhealthy
);

CREATE INDEX idx_agents_name ON agents(name);

CREATE TABLE events (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id   TEXT NOT NULL REFERENCES agents(instance_id) ON DELETE CASCADE,
    event_type    TEXT NOT NULL,           -- status | error | custom
    payload       TEXT NOT NULL,           -- JSON
    received_at   TEXT NOT NULL            -- ISO 8601 timestamp
);
```

### Agent Entry

```json
{
  "instance_id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "calculator-agent",
  "http_url": "http://127.0.0.1:8080",
  "capabilities": ["calculate", "math"],
  "metadata": {},
  "registered_at": "2026-05-21T10:00:00Z",
  "last_health_check": "2026-05-21T10:05:00Z",
  "status": "healthy"
}
```

## Registry HTTP API (aether-side)

### Register an instance

```
POST /registry/agents
```

Request body:
```json
{
  "name": "calculator-agent",
  "http_url": "http://127.0.0.1:8080",
  "capabilities": ["calculate", "math"],
  "metadata": {}
}
```

Response `200 OK`:
```json
{
  "instance_id": "550e8400-e29b-41d4-a716-446655440000",
  "poll_interval_secs": 30
}
```

`poll_interval_secs` tells the agent its crash detection latency.

Same URL re-registering (agent restart on same port): update existing row with new `instance_id`, reset status to `unknown`.

### Deregister an instance

```
DELETE /registry/instances/{instance_id}
```

Response `204 No Content`.

### List all logical agents (grouped by name)

```
GET /registry/agents
GET /registry/agents?capability=calculate
```

Response: array of logical agent summaries. Aggregate status: `healthy` if any instance is healthy, `unhealthy` if all instances are unhealthy, `unknown` if all instances are unknown.

```json
[
  { "name": "calculator-agent", "instance_count": 2, "status": "healthy" }
]
```

### List instances of a named agent

```
GET /registry/agents/{name}/instances
```

Response: array of instance entries for that name.

### Get one instance

```
GET /registry/agents/{name}/instances/{instance_id}
```

### Push a status or error event

```
POST /registry/instances/{instance_id}/events
```

Request body:
```json
{
  "event_type": "error",
  "payload": { "message": "LLM provider timeout", "code": "PROVIDER_TIMEOUT" }
}
```

Response `202 Accepted`.

## Health Polling (aether-side)

A background tokio task runs continuously:

1. Every `poll_interval_secs` (default 30s, configurable), fetch all registered instances from SQLite.
2. For each instance, `GET {http_url}/health`.
3. `200 OK` → record success. After 1 success: status = `healthy`.
4. Non-200 or connection error → record failure. After 3 consecutive failures: status = `unhealthy`.
5. Write updated `last_health_check` and `status` to SQLite.

Aether restart: all statuses reset to `unknown` on startup, re-validated on the next poll cycle.

## Registration Protocol (avs-server side)

`src/aether_client.rs` handles all aether interaction:

**On startup:**
1. Check `AETHER_REGISTRY_URL`. If unset, skip registration entirely.
2. `POST {AETHER_REGISTRY_URL}/registry/agents` with name (from `AGENT_NAME` env var or server config), http_url (own bound address), capabilities (from server config).
3. On success: store `instance_id` in memory for use in deregistration and event pushes.
4. On failure (aether unreachable): log warning, continue running standalone. Optionally retry in background with exponential backoff (cap at 5 minutes).

**On shutdown (SIGTERM):**
1. `DELETE {AETHER_REGISTRY_URL}/registry/instances/{instance_id}`.
2. Best-effort — do not block shutdown if this fails. Health polling will detect the departure within one poll interval.

**Pushing events:**
1. `POST {AETHER_REGISTRY_URL}/registry/instances/{instance_id}/events`.
2. Fire-and-forget. Failure is logged, not retried.

## Error Handling

| Scenario | Behavior |
|---|---|
| Agent can't reach aether on startup | Log warning, continue standalone. Background retry with backoff. |
| Aether can't reach agent on health poll | 3 consecutive failures → `unhealthy`. Entry stays in registry. |
| Aether restart | Read SQLite, reset all statuses to `unknown`, re-validate on next poll cycle. |
| Agent SIGTERM, deregister fails | Ignore — health poll detects departure within one interval. |
| Same URL re-registers | Update row: new `instance_id`, status = `unknown`. |
| SQLite write failure | Log error, continue with in-memory state. Retry on next write. |

## Workflow Triggering

Aether triggers a workflow by calling `POST {http_url}/invoke` on the first agent in the workflow definition. The agent processes its input and routes its output to the next agent directly via its configured output source. Aether is not in the data path after the initial trigger.

Workflow definition format is a separate concern and will be designed independently.

## What Aether Does NOT Do

- Route business data between agents
- Transform or inspect agent payloads
- Manage agent-to-agent communication
- Guarantee workflow completion or durability
