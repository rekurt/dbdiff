mod commands;

use std::process::ExitCode;

use clap::Parser;

use dbdiff::cli::{Cli, ResolvedCommand};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let resolved = cli.resolve();

    let result = match resolved {
        ResolvedCommand::Diff(params) => commands::run_diff(params).await,
        ResolvedCommand::Validate(args) => commands::run_validate(args).await,
        ResolvedCommand::Tables(args) => commands::run_tables(args).await,
        ResolvedCommand::Completions(args) => {
            Cli::generate_completions(args.shell);
            Ok(())
        }
        ResolvedCommand::Init => commands::run_init(),
        ResolvedCommand::Snapshot(args) => commands::run_snapshot(args).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}
