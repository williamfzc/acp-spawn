mod cli;
mod runtime;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Commands};
use runtime::RunRequest;

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
            let outcome = runtime::run(request)?;

            eprintln!("{}", outcome.summary);
        }
    }

    Ok(())
}
