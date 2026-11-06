use clap::{Parser, Subcommand};
use phv::error::PhvError;
use phv::import::cli::ImportCommand;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "phv", about = "Personal health vault")]
struct Cli {
    /// A vault path or registered name. Otherwise PHV_VAULT, then the default vault.
    #[arg(long, global = true)]
    vault: Option<PathBuf>,
    /// Machine-readable output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Imports an export from another system
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let Command::Import { command } = cli.command;
    match phv::import::cli::run(command, cli.vault.as_deref(), cli.json).await {
        Ok(code) => code,
        Err(error) => {
            report_error(&error);
            error.exit_code()
        }
    }
}

fn report_error(error: &PhvError) {
    eprintln!("phv: {error}");
    for issue in error.issues() {
        eprintln!("  {issue}");
    }
}
