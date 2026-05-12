# acp-spawn

Wrap any command with structured JSONL lifecycle events on stdout.

## What it does

`acp-spawn` runs a child process and emits lifecycle events to stdout as JSONL, interleaved with the child's own stdout output:

1. **`spawn_started`** — child process launched, includes `run_id`, `spawn_id`, command, cwd, timeout, pid
2. **`spawn_completed`** — child exited with code 0
3. **`spawn_failed`** — child exited non-zero, timed out, or was cancelled

Child stdout passes through untouched. Child stderr and runtime errors go to stderr.

## Example

```bash
$ acp-spawn run -- opencode acp
```

```json
{"run_id":"run-000140c6-...","timestamp":"2026-05-12T16:03:26.232204Z","event":"spawn_started","data":{"spawn_id":"spawn-000140c6-...","agent":"opencode","command":["opencode","acp"],"cwd":"/Users/bytedance/workspace/github/acp-spawn/.","timeout_ms":300000,"pid":41104}}
Agent Client Protocol commands
...
{"run_id":"run-000140c6-...","timestamp":"2026-05-12T16:03:27.720374Z","event":"spawn_completed","data":{"spawn_id":"spawn-000140c6-...","duration_ms":1488,"exit_code":0,"result":{"status":"success","summary":"agent 'opencode' completed successfully","run_id":"run-000140c6-...","exit_code":0}}}
```

On failure:

```bash
$ acp-spawn run -- opencode
```

```json
{"run_id":"run-000140c6-...","timestamp":"2026-05-12T17:11:01.138355Z","event":"spawn_started","data":{"spawn_id":"spawn-000140c6-...","agent":"opencode","command":["opencode"],"cwd":"/Users/bytedance/workspace/github/acp-spawn/.","timeout_ms":300000,"pid":82197}}
{"run_id":"run-000140c6-...","timestamp":"2026-05-12T17:11:01.480589Z","event":"spawn_failed","data":{"spawn_id":"spawn-000140c6-...","duration_ms":342,"reason":"child agent 'opencode' exited with code 1 (run_id=..., spawn_id=...)","exit_code":1,"result":{"status":"failed","summary":"child agent 'opencode' exited with code 1 (run_id=..., spawn_id=...)","run_id":"run-000140c6-...","error":"child agent 'opencode' exited with code 1 (run_id=..., spawn_id=...)","exit_code":1}}}
```

## Usage

```bash
acp-spawn run -- opencode acp
acp-spawn run -- opencode acp
acp-spawn run -- claude-agent-acp
```

Set a working directory:

```bash
acp-spawn run --cwd /path/to/project -- opencode acp
```

Link to a parent run by setting `RUN_ID` in the environment — the child will receive `PARENT_RUN_ID`:

```bash
RUN_ID=run-parent-123 acp-spawn run -- opencode acp
```

Extract lifecycle events from the output:

```bash
acp-spawn run -- opencode acp | jq -c 'select(.event == "spawn_started" or .event == "spawn_completed" or .event == "spawn_failed")'
```

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```