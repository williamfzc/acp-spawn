# acp-spawn

Wrap any ACP agent with structured JSONL lifecycle events on stdout.

## What it does

`acp-spawn` runs an ACP agent and emits lifecycle events (`spawn_started`, `spawn_completed`, `spawn_failed`) to stdout as JSONL, interleaved with the agent's own output. Agent stdout passes through untouched.

```bash
acp-spawn run --prompt "say hello" -- opencode acp
```

```json
{"id":1,"jsonrpc":"2.0","result":{"protocolVersion":1,"agentCapabilities":{...}}}
{"id":2,"jsonrpc":"2.0","result":{"sessionId":"bf76e3d9-...","models":{...}}}
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"bf76e3d9-...","update":{"sessionUpdate":"agent_message_chunk","content":{"text":"Hello","type":"text"}}}}
{"id":3,"jsonrpc":"2.0","result":{"stopReason":"end_turn"}}
{"run_id":"run-0001253b-...","event":"spawn_started","data":{"spawn_id":"spawn-0001253b-...","agent":"opencode","command":["opencode","acp"],...}}
{"run_id":"run-0001253b-...","event":"spawn_completed","data":{"duration_ms":5432,"exit_code":0,...}}
```

## Install

One-click install (Linux / macOS, x86_64 / aarch64):

```bash
curl -fsSL https://raw.githubusercontent.com/williamfzc/acp-spawn/main/install.sh | bash
```

Or download a binary from [Releases](https://github.com/williamfzc/acp-spawn/releases).

Install as an agent skill:

```bash
npx skills add williamfzc/acp-spawn
```

## Usage

### Run with a prompt

```bash
acp-spawn run --prompt "fix the bug" -- opencode acp
acp-spawn run --prompt "say hello" -- codex-acp
acp-spawn run --prompt "say hello" -- claude-agent-acp
```

### Run without a prompt

Just wraps the agent lifecycle and passes through stdout:

```bash
acp-spawn run -- opencode acp
```

### Set a working directory

```bash
acp-spawn run --cwd /path/to/project --prompt "review the code" -- opencode acp
```

### Link to a parent run

Set `RUN_ID` in the environment — the child will receive `PARENT_RUN_ID`:

```bash
RUN_ID=run-parent-123 acp-spawn run --prompt "do the task" -- opencode acp
```

### Filter lifecycle events

```bash
acp-spawn run --prompt "say hello" -- opencode acp | jq -c 'select(.event)'
```

## License

[MIT](LICENSE)
