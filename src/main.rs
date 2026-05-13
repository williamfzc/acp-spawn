//! Starts the ACP spawn runtime CLI binary.

use std::path::PathBuf;
use std::process::ExitCode;

use acp_spawn::cli::{Cli, Commands};
use acp_spawn::runtime::{self, RunRequest};
use clap::Parser;

fn main() -> ExitCode {
    match try_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn try_main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(args) => {
            let (agent, agent_args) = split_command(&args.command);
            let request = RunRequest {
                agent,
                agent_args,
                cwd: args.cwd.unwrap_or_else(|| PathBuf::from(".")),
                timeout: None,
                prompt: args.prompt,
            };
            runtime::run(request)?;
        }
    }

    Ok(())
}

fn split_command(raw: &[String]) -> (String, Vec<String>) {
    (raw[0].clone(), raw[1..].to_vec())
}
