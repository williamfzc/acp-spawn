//! Starts the ACP spawn runtime CLI binary.

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
            let request = RunRequest::try_from(args)?;
            runtime::run(request)?;
        }
    }

    Ok(())
}
