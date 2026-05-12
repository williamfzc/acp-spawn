---
name: acp-spawn
description: Run and trace ACP agent invocations. Use when the user wants to run an agent with tracing, observe agent behavior, inspect lifecycle events, trace run_id/spawn_id relationships, or mentions "acp-spawn", "spawn", or "ACP". Also use when the user needs to understand what happens during an agent call or wants to see the raw ACP lifecycle as JSONL.
---

# acp-spawn

A skill for running and tracing ACP (Agent Communication Protocol) invocations using `acp-spawn`. This skill helps you run agents with full lifecycle tracing, analyze JSONL output, and understand nested run relationships.

## What acp-spawn Does

`acp-spawn` wraps any agent or command and emits structured lifecycle events while preserving the child's stdout exactly. This gives you visibility into:
- When a spawn starts and completes
- The `run_id`, `spawn_id`, and `parent_run_id` relationships for nested runs
- Raw child stdout passthrough for ACP/JSONL inspection
- Timeout and cancellation behavior

## Common Workflows

### Run an Agent with Debugging

Use `acp-spawn run` to execute an agent and capture the full lifecycle:

```bash
acp-spawn run \
  --agent <path-to-agent> \
  --goal "<what the agent should do>" \
  --cwd <working-directory> \
  --timeout-ms <timeout>
```

The output will be JSONL lines:
1. `spawn_started` - lifecycle event with metadata
2. Child stdout lines (passed through unchanged)
3. `spawn_completed` or `spawn_failed` - final lifecycle event

### Run from Config

For reusable targets, use a TOML config:

```bash
acp-spawn run --config spawn-profiles.toml [--profile <name>]
```

### Analyze Output

Extract specific information with `jq`:

```bash
# Get run identifiers
acp-spawn run ... | jq -r 'select(.event=="spawn_started") | "run_id=\(.run_id) spawn_id=\(.data.spawn_id)"'

# Get only child output (filter out lifecycle events)
acp-spawn run ... | jq -c 'select(.event | startswith("spawn_") | not)'

# Check for failures
acp-spawn run ... | jq -c 'select(.event=="spawn_failed")'
```

## Config File Format

Create a `spawn-profiles.toml`:

```toml
[run]
agent = "/path/to/agent"
agent_args = ["arg1", "arg2"]
goal = "default goal description"
cwd = "."
timeout_ms = 30000

[profiles.custom]
agent = "/path/to/other-agent"
agent_args = []
goal = "custom goal"
cwd = "/other/path"
timeout_ms = 60000
```

- `[run]` is the default profile (used when `--profile` is omitted)
- `[profiles.<name>]` are named profiles selectable with `--profile`
- Relative `cwd` is resolved relative to the config file location

## Lifecycle Events

### spawn_started
Emitted when the child process starts:
```json
{
  "run_id": "...",
  "timestamp": "...",
  "event": "spawn_started",
  "data": {
    "spawn_id": "...",
    "agent": "/path/to/agent",
    "command": ["agent", "args"...],
    "cwd": "/working/dir",
    "timeout_ms": 30000,
    "pid": 12345
  }
}
```

### spawn_completed
Emitted on successful exit:
```json
{
  "run_id": "...",
  "timestamp": "...",
  "event": "spawn_completed",
  "data": {
    "spawn_id": "...",
    "duration_ms": 123,
    "exit_code": 0,
    "result": {
      "status": "success",
      "summary": "agent completed successfully",
      "run_id": "...",
      "exit_code": 0
    }
  }
}
```

### spawn_failed
Emitted on error, timeout, or cancellation:
```json
{
  "run_id": "...",
  "timestamp": "...",
  "event": "spawn_failed",
  "data": {
    "spawn_id": "...",
    "error": "timeout after 30000ms",
    "duration_ms": 30001
  }
}
```

## Nested Run Tracing

When running nested agents, set `RUN_ID` in the environment to establish parent-child relationships:

```bash
# Parent run
export RUN_ID="run-parent-123"

# Child run will see:
# - RUN_ID: its own generated run_id
# - PARENT_RUN_ID: "run-parent-123"
# - SPAWN_ID: generated spawn_id
# - ACP_SPAWN_GOAL: the goal string
```

This enables tracing a hierarchy of agent calls.

## Environment Variables

The spawned child receives:
- `RUN_ID` - unique identifier for this run
- `PARENT_RUN_ID` - parent's RUN_ID if set (for nested runs)
- `SPAWN_ID` - unique identifier for this spawn
- `ACP_SPAWN_GOAL` - the goal string passed to `--goal`

## Output Contract

- **stdout**: Lifecycle JSONL events + native child stdout passthrough
- **stderr**: Debug output, human-readable logs, runtime errors

The child's stdout is passed through unchanged. If the child emits ACP or JSONL, you'll see it interleaved with lifecycle events.

## Troubleshooting

### Agent not found
Ensure the `--agent` path is correct and executable:
```bash
which <agent-path>
```

### Timeout issues
Increase `--timeout-ms` if the agent needs more time:
```bash
acp-spawn run --agent ... --timeout-ms 120000
```

### Parse errors
If output isn't valid JSONL, check if the child is writing to stderr (which acp-spawn forwards separately).

### Nested runs not linking
Ensure the parent sets `RUN_ID` in the environment before calling acp-spawn.

## Tips

1. **Start simple**: Run with a basic command first to verify setup:
   ```bash
   acp-spawn run --agent /bin/sh --goal "echo test" --cwd .
   ```

2. **Use jq liberally**: Filter and transform JSONL output for readability.

3. **Check exit codes**: `spawn_completed` includes `exit_code`; non-zero indicates child error.

4. **Monitor timeouts**: `spawn_failed` with "timeout" means the child exceeded `--timeout-ms`.

5. **Preserve raw output**: Pipe to `tee` if you need both analysis and raw capture:
   ```bash
   acp-spawn run ... | tee output.jsonl | jq ...
   ```

## Non-Goals

acp-spawn intentionally does NOT:
- Store traces or provide querying/aggregation
- Normalize non-ACP stdout into ACP format
- Orchestrate multiple runs (queuing, scheduling, retries)
- Integrate with shells or require global installation

It's a focused debug tool for inspecting individual agent invocations.
