# acp-spawn

Wrap any ACP agent with structured JSONL lifecycle events on stdout.

## What it does

`acp-spawn` runs an ACP agent and emits lifecycle events to stdout as JSONL, interleaved with the agent's own stdout output:

1. **`spawn_started`** — agent launched, includes `run_id`, `spawn_id`, command, cwd, timeout, pid
2. **`spawn_completed`** — agent exited with code 0
3. **`spawn_failed`** — agent exited non-zero, timed out, or was cancelled

Agent stdout passes through untouched. Agent stderr and runtime errors go to stderr.

## Quick start

Send a prompt to an ACP agent and watch its output as JSONL:

```bash
acp-spawn run --prompt "say hello" -- opencode acp
```

```json
{"id":1,"jsonrpc":"2.0","result":{"protocolVersion":1,"agentCapabilities":{...}}}
{"id":2,"jsonrpc":"2.0","result":{"sessionId":"bf76e3d9-...","models":{...}}}
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"bf76e3d9-...","update":{"sessionUpdate":"agent_thought_chunk","content":{"text":"Let me think...","type":"text"}}}}
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"bf76e3d9-...","update":{"sessionUpdate":"agent_message_chunk","content":{"text":"Hello","type":"text"}}}}
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"bf76e3d9-...","update":{"sessionUpdate":"agent_message_chunk","content":{"text":", I'm ready to help","type":"text"}}}}
{"id":3,"jsonrpc":"2.0","result":{"stopReason":"end_turn"}}
{"run_id":"run-0001253b-...","event":"spawn_started","data":{"spawn_id":"spawn-0001253b-...","agent":"opencode","command":["opencode","acp"],...}}
{"run_id":"run-0001253b-...","event":"spawn_completed","data":{"duration_ms":5432,"exit_code":0,...}}
```

## Usage

Run an agent with a prompt:

```bash
acp-spawn run --prompt "fix the bug" -- opencode acp
acp-spawn run --prompt "say hello" -- codex-acp
acp-spawn run --prompt "say hello" -- claude-agent-acp
```

Run an agent without a prompt (just wrap its lifecycle):

```bash
acp-spawn run -- opencode acp
```

Set a working directory:

```bash
acp-spawn run --cwd /path/to/project --prompt "review the code" -- opencode acp
```

Link to a parent run by setting `RUN_ID` in the environment — the child will receive `PARENT_RUN_ID`:

```bash
RUN_ID=run-parent-123 acp-spawn run --prompt "do the task" -- opencode acp
```

Extract lifecycle events from the output:

```bash
acp-spawn run --prompt "say hello" -- opencode acp | jq -c 'select(.event == "spawn_started" or .event == "spawn_completed" or .event == "spawn_failed")'
```

## How --prompt works

When you pass `--prompt`, acp-spawn performs the full ACP handshake before streaming output:

1. Sends `initialize` → reads response
2. Sends `session/new` → extracts `sessionId`
3. Sends `session/prompt` with your text → reads response
4. All three handshake responses and any interleaved notifications are passed through to stdout
5. Streams the agent's remaining output as JSONL until it finishes

Without `--prompt`, acp-spawn just wraps the child process lifecycle and passes through stdout.

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```
