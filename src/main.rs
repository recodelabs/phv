use clap::Parser;
use phv::cli::{Cli, run};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("phv: {error}");
            for issue in error.issues() {
                eprintln!("  {issue}");
            }
            error.exit_code()
        }
    }
}
