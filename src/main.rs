//! Starts the ACP spawn runtime CLI binary.

use std::ffi::OsString;
use std::process::ExitCode;

use acp_spawn::cli::{Cli, Commands};
use acp_spawn::hijack;
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
    if let Some(command_name) = hijack::proxied_invocation_name(std::env::args_os().next()) {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        hijack::proxy_invocation(&command_name, args)?;
        return Ok(());
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Run(args) => {
            let request = RunRequest::try_from(args)?;
            runtime::run(request)?;
        }
        Commands::Install(args) => {
            hijack::install(args.commands)?;
        }
        Commands::Status => {
            hijack::status(std::io::stdout().lock())?;
        }
        Commands::Uninstall => {
            hijack::uninstall()?;
        }
    }

    Ok(())
}
