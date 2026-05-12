//! Defines the command-line interface for the ACP spawn runtime.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "acp-spawn", version, about = "ACP spawn runtime CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Commands {
    /// Run a command and emit its output wrapped with JSONL lifecycle events.
    Run(RunArgs),
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// Working directory to run the child process in.
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Command and its arguments (after `--`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1..)]
    pub command: Vec<String>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, RunArgs};

    #[test]
    fn parses_command_and_args() {
        let cli = Cli::try_parse_from([
            "acp-spawn",
            "run",
            "--cwd",
            "./repo",
            "--",
            "opencode",
            "acp",
        ])
        .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                cwd: Some("./repo".into()),
                command: vec!["opencode".into(), "acp".into()],
            })
        );
    }

    #[test]
    fn parses_command_without_cwd() {
        let cli = Cli::try_parse_from(["acp-spawn", "run", "--", "echo", "hello"])
            .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                cwd: None,
                command: vec!["echo".into(), "hello".into()],
            })
        );
    }

    #[test]
    fn parses_single_command() {
        let cli =
            Cli::try_parse_from(["acp-spawn", "run", "--", "codex"]).expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                cwd: None,
                command: vec!["codex".into()],
            })
        );
    }

    #[test]
    fn parses_command_with_flag_args() {
        let cli = Cli::try_parse_from(["acp-spawn", "run", "--", "codex", "run", "--json"])
            .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                cwd: None,
                command: vec!["codex".into(), "run".into(), "--json".into()],
            })
        );
    }
}
