# AGENTS

## Project Rules

- Write the project in English, including source comments, documentation, config examples, and user-facing text.
- Add a file-level header comment at the top of every source file to explain the file purpose in one concise sentence.
- Follow Occam's razor: prefer the smallest design that satisfies the requirement and avoid adding extra abstractions, states, or protocol layers without a clear need.
- Prefer configuration over hardcoded target definitions. Runtime targets must live in config, not in fixed code paths.
- Keep stdout machine-readable and structured. Human-readable logs belong on stderr.
- Preserve ACP-native behavior: trace propagation and lifecycle JSONL should stay explicit and minimal.
- Do not parse, wrap, enrich, or reinterpret child ACP events in the MVP. Pass child stdout through natively.

## Configuration Guidance

- Put reusable spawn targets in TOML config files.
- Use `[run]` for the default target and `[profiles.<name>]` for named targets.
- Resolve relative `cwd` values relative to the config file location.
- Treat CLI flags as overrides for config values instead of introducing separate execution paths.
- Avoid compatibility layers for non-ACP child stdout in the MVP. Require native ACP or JSONL output when structured downstream consumption matters.

## Review Checklist

- Is the change still in English?
- Does each source file have a file-level header comment?
- Does the change remove hardcoded runtime target logic when config can express it?
- Is the design still the simplest workable shape?
- Does the runtime avoid interpreting child stdout beyond lifecycle emission and native passthrough?
