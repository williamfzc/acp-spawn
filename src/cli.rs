use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "acp-spawn", version, about = "ACP spawn runtime CLI skeleton")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Commands {
    /// Run a child agent with the provided goal and working directory.
    Run(RunArgs),
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// Agent executable or logical name to run.
    #[arg(long)]
    pub agent: String,

    /// Goal or task description for the target agent.
    #[arg(long)]
    pub goal: String,

    /// Working directory to run the child agent in.
    #[arg(long, value_name = "DIR")]
    pub cwd: PathBuf,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, RunArgs};

    #[test]
    fn parses_run_command() {
        let cli = Cli::try_parse_from([
            "acp-spawn",
            "run",
            "--agent",
            "codex",
            "--goal",
            "implement parser",
            "--cwd",
            "./repo",
        ])
        .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                agent: "codex".into(),
                goal: "implement parser".into(),
                cwd: "./repo".into(),
            })
        );
    }

    #[test]
    fn requires_all_run_flags() {
        let error =
            Cli::try_parse_from(["acp-spawn", "run"]).expect_err("cli should reject missing flags");

        let rendered = error.to_string();
        assert!(rendered.contains("--agent"));
        assert!(rendered.contains("--goal"));
        assert!(rendered.contains("--cwd"));
    }
}
