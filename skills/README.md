# Skills

This directory contains project-specific skills for use with Claude Code.

## Available Skills

### acp-spawn

Run an ACP agent with a prompt and capture its full output as JSONL. Helps you:

- Run agents with ACP protocol handshake via `--prompt`
- Analyze JSONL output with jq patterns
- Trace run_id/spawn_id relationships for nested runs
- Inspect thinking, tool calls, and agent responses

**Usage**: The skill triggers automatically when you mention "acp-spawn", "ACP", or ask about tracing agent calls.
