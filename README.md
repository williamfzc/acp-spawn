# acp-spawn

`acp-spawn` is a Rust CLI for running spawned agents with ACP-friendly JSONL observability. The current MVP can spawn child processes, emit structured lifecycle events to stdout, pass child stdout through natively, propagate trace context, and enforce timeout or cancellation.

## Current Capabilities

- Supports `acp-spawn run --agent --goal --cwd [--agent-arg ...] [--timeout-ms ...]`
- Supports `acp-spawn run --config <FILE> [--profile <NAME>]` for config-driven targets
- Generates or inherits `trace_id`, `span_id`, `parent_span_id`, `spawn_id`, and `session_id`
- Emits strict lifecycle JSONL events on stdout: `spawn_started`, `spawn_completed`, and `spawn_failed`
- Keeps stderr for raw child stderr and human-readable runtime errors
- Supports cancellation and timeout-driven termination
- Passes child stdout through natively without parsing, wrapping, or enriching ACP events

## Quick Start

```bash
cargo run -- run --config examples/spawn-profiles.toml
```

Example stdout:

```json
{"trace_id":"...","span_id":"...","parent_span_id":null,"session_id":"...","timestamp":"...","event":"spawn_started","data":{"spawn_id":"...","agent":"/bin/sh","command":["/bin/sh","-c","printf '{\"event\":\"demo\"}\\n'"],"cwd":"/path/to/repo","timeout_ms":1000,"pid":12345}}
{"event":"demo"}
{"trace_id":"...","span_id":"...","parent_span_id":null,"session_id":"...","timestamp":"...","event":"spawn_completed","data":{"spawn_id":"...","duration_ms":12,"exit_code":0,"result":{"status":"success","summary":"agent '/bin/sh' completed successfully","trace_id":"...","exit_code":0}}}
```

## Config Format

Use `examples/spawn-profiles.toml` as the reference format:

```toml
[run]
agent = "/bin/sh"
agent_args = ["-c", "printf '{\"event\":\"demo\"}\n'"]
goal = "demo success"
cwd = ".."
timeout_ms = 1000

[profiles.opencode-acp]
agent = "opencode"
agent_args = ["acp"]
goal = "serve acp"
cwd = ".."
timeout_ms = 3000
```

The root `[run]` section is used when `--profile` is omitted. Named profiles under `[profiles.<name>]` can be selected with `--profile`.

Relative `cwd` values are resolved relative to the config file location, not the shell working directory.

## Test With `opencode acp`

Run the predefined profile:

```bash
cargo run -- run --config examples/spawn-profiles.toml --profile opencode-acp
```

This is useful as a smoke test:

- stdout contains `acp-spawn` lifecycle JSONL plus the child process stdout lines exactly as emitted
- If `opencode acp` does not exit before the timeout, the runtime emits `spawn_failed`
- Raw child stderr is still forwarded to stderr for debugging

To drive a real ACP request through `acp-spawn`, provide a stdin payload file:

```bash
cargo run -- run --config examples/spawn-profiles.toml --profile opencode-acp --input-file examples/opencode-initialize.jsonl
```

This forwards the file contents to child stdin unchanged while preserving lifecycle events on stdout. With `opencode acp`, the example returns a real `initialize` result.

## Output Contract

- stdout: lifecycle JSONL emitted by `acp-spawn` plus native child stdout passthrough; the MVP assumes child stdout is already ACP or JSONL when machine-readable output is required
- stderr: debug output, human-readable logs, and runtime error messages only
