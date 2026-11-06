use clap::Parser;
use phv::cli::{Cli, print_json, run};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            if json {
                let body =
                    serde_json::json!({"error": error.to_string(), "issues": error.issues()});
                let _ = print_json(&body);
            } else {
                eprintln!("phv: {error}");
                for issue in error.issues() {
                    eprintln!("  {issue}");
                }
            }
            error.exit_code()
        }
    }
}
