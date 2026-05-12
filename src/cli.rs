//! Defines the command-line interface for the ACP spawn runtime.

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
    /// Install command hijack shims for supported agent commands.
    Install(InstallArgs),
    /// Show current command hijack status.
    Status,
    /// Remove command hijack shims and shell integration.
    Uninstall,
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// Optional config file that defines the target command and runtime settings.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Optional profile name inside the config file.
    #[arg(long, requires = "config")]
    pub profile: Option<String>,

    /// Optional file whose contents are forwarded to child stdin.
    #[arg(long, value_name = "FILE")]
    pub input_file: Option<PathBuf>,

    /// Agent executable or logical name to run.
    #[arg(long)]
    pub agent: Option<String>,

    /// Additional argument to pass to the child process. Can be specified multiple times.
    #[arg(long = "agent-arg", allow_hyphen_values = true)]
    pub agent_args: Vec<String>,

    /// Goal or task description for the target agent.
    #[arg(long)]
    pub goal: Option<String>,

    /// Working directory to run the child agent in.
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Timeout for the child process in milliseconds.
    #[arg(long, value_name = "MILLISECONDS")]
    pub timeout_ms: Option<u64>,

    /// Forward current stdin to the child process unchanged.
    #[arg(long)]
    pub forward_stdin: bool,
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
pub struct InstallArgs {
    /// Command name to hijack. When omitted, installs the default supported commands.
    #[arg(long = "command", value_name = "NAME")]
    pub commands: Vec<String>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, InstallArgs, RunArgs};

    #[test]
    fn parses_run_command() {
        let cli = Cli::try_parse_from([
            "acp-spawn",
            "run",
            "--config",
            "./examples/spawn-profiles.toml",
            "--profile",
            "opencode-acp",
            "--agent",
            "codex",
            "--agent-arg",
            "run",
            "--agent-arg",
            "--json",
            "--goal",
            "implement parser",
            "--cwd",
            "./repo",
            "--timeout-ms",
            "5000",
        ])
        .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                config: Some("./examples/spawn-profiles.toml".into()),
                profile: Some("opencode-acp".into()),
                input_file: None,
                agent: Some("codex".into()),
                agent_args: vec!["run".into(), "--json".into()],
                goal: Some("implement parser".into()),
                cwd: Some("./repo".into()),
                timeout_ms: Some(5000),
                forward_stdin: false,
            })
        );
    }

    #[test]
    fn allows_deferred_validation_for_config_driven_runs() {
        let cli = Cli::try_parse_from(["acp-spawn", "run"]).expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                config: None,
                profile: None,
                input_file: None,
                agent: None,
                agent_args: vec![],
                goal: None,
                cwd: None,
                timeout_ms: None,
                forward_stdin: false,
            })
        );
    }

    #[test]
    fn accepts_empty_agent_args() {
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
                config: None,
                profile: None,
                input_file: None,
                agent: Some("codex".into()),
                agent_args: vec![],
                goal: Some("implement parser".into()),
                cwd: Some("./repo".into()),
                timeout_ms: None,
                forward_stdin: false,
            })
        );
    }

    #[test]
    fn parses_config_only_run_command() {
        let cli = Cli::try_parse_from([
            "acp-spawn",
            "run",
            "--config",
            "./examples/spawn-profiles.toml",
            "--profile",
            "opencode-acp",
        ])
        .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Run(RunArgs {
                config: Some("./examples/spawn-profiles.toml".into()),
                profile: Some("opencode-acp".into()),
                input_file: None,
                agent: None,
                agent_args: vec![],
                goal: None,
                cwd: None,
                timeout_ms: None,
                forward_stdin: false,
            })
        );
    }

    #[test]
    fn parses_install_command_with_selected_commands() {
        let cli = Cli::try_parse_from([
            "acp-spawn",
            "install",
            "--command",
            "codex",
            "--command",
            "claude",
        ])
        .expect("cli should parse");

        assert_eq!(
            cli.command,
            Commands::Install(InstallArgs {
                commands: vec!["codex".into(), "claude".into()],
            })
        );
    }
}
