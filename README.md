# acp-spawn

`acp-spawn` 是一个用 Rust 编写的 ACP spawn runtime CLI。当前已完成 Task 1 的最小骨架：提供 `run` 子命令，接收目标 agent、任务目标与工作目录，并打通到 runtime 的最小调用链。

## 当前能力

- 提供 `acp-spawn run --agent --goal --cwd` 入口
- 使用 `clap` 生成参数校验与帮助信息
- 在进入 runtime 前校验工作目录是否存在且为目录
- 执行一次最小空载运行，当前仅完成 runtime 初始化与参数透传

## 快速开始

```bash
cargo run -- run --agent codex --goal "implement parser" --cwd .
```

示例输出当前写入 `stderr`，用于说明 runtime 已被成功调用：

```text
initialized spawn runtime for agent 'codex' with goal 'implement parser' in /path/to/repo
```

后续任务会继续补充 JSONL 事件输出、trace 传播、子进程管理与结果归并等能力。
