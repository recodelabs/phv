//! Starts the FHIR browser until `phv serve` can host it.
//!
//! cargo run --example browser -- [--vault PATH_OR_NAME] [--port N]

use phv::browser::{Vaults, router};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut vault: Option<PathBuf> = None;
    let mut port: u16 = 8747;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--vault" => vault = args.next().map(PathBuf::from),
            "--port" => port = args.next().ok_or("--port needs a value")?.parse()?,
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    let env = std::env::var("PHV_VAULT").ok();
    let config = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/phv/config.toml"))
        .and_then(|path| std::fs::read_to_string(path).ok());
    let vaults = Vaults::resolve(vault.as_deref(), env.as_deref(), config.as_deref())?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("phv browser: http://127.0.0.1:{port}");
    axum::serve(listener, router(vaults)).await?;
    Ok(())
}
