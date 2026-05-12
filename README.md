# acp-spawn

Wrap any command with structured JSONL lifecycle events on stdout.

## What it does

`acp-spawn` runs a child process and emits lifecycle events to stdout as JSONL, interleaved with the child's own stdout output:

1. **`spawn_started`** — child process launched, includes `run_id`, `spawn_id`, command, cwd, timeout, pid
2. **`spawn_completed`** — child exited with code 0
3. **`spawn_failed`** — child exited non-zero, timed out, or was cancelled

Child stdout passes through untouched. Child stderr and runtime errors go to stderr.

## Example (success)

```bash
$ acp-spawn run -- codex-acp
```

```json
{"run_id":"run-0001578b-...","timestamp":"2026-05-12T17:15:30.685775Z","event":"spawn_started","data":{"spawn_id":"spawn-0001578b-...","agent":"codex-acp","command":["codex-acp"],"cwd":"/tmp","timeout_ms":300000,"pid":87949}}
{"run_id":"run-0001578b-...","timestamp":"2026-05-12T17:15:31.066356Z","event":"spawn_completed","data":{"spawn_id":"spawn-0001578b-...","duration_ms":380,"exit_code":0,"result":{"status":"success","summary":"agent 'codex-acp' completed successfully","run_id":"run-0001578b-...","exit_code":0}}}
```

## Example (failure)

```bash
$ acp-spawn run -- gemini
```

```json
{"run_id":"run-000159aa-...","timestamp":"2026-05-12T17:16:00.266457Z","event":"spawn_started","data":{"spawn_id":"spawn-000159aa-...","agent":"gemini","command":["gemini"],"cwd":"/tmp","timeout_ms":300000,"pid":88492}}
{"run_id":"run-000159aa-...","timestamp":"2026-05-12T17:16:05.382974Z","event":"spawn_failed","data":{"spawn_id":"spawn-000159aa-...","duration_ms":5116,"reason":"received SIGTERM","exit_code":0,"result":{"status":"failed","summary":"received SIGTERM","run_id":"run-000159aa-...","error":"received SIGTERM","exit_code":0}}}
```

## Usage

```bash
acp-spawn run -- opencode acp
acp-spawn run -- claude-agent-acp
acp-spawn run -- codex-acp
acp-spawn run -- gemini
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
acp-spawn run -- codex-acp | jq -c 'select(.event == "spawn_started" or .event == "spawn_completed" or .event == "spawn_failed")'
```

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```