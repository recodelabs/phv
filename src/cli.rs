//! Command line. A thin layer over the library. The clock is read here, not in the library.

use crate::error::{Issue, PhvError};
use crate::store::{Vault, resolve_vault_path};
use crate::write::{AddOptions, Phv, WriteReport};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "phv",
    version,
    about = "Personal health vault: FHIR records as files on your own machine"
)]
pub struct Cli {
    /// Path to the data repo. Defaults to PHV_VAULT, then ~/.config/phv/config.toml.
    #[arg(long, global = true)]
    pub vault: Option<PathBuf>,
    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Walk through creating a vault for one person. Run again for each family member.
    Setup,
    /// List registered vaults. The default is marked with *.
    Vaults,
    /// Change the default vault.
    Use { name: String },
    /// Create a new data repo layout.
    Init { path: PathBuf },
    /// Check a resource or Bundle without writing. Use - for stdin.
    Validate { file: String },
    /// Validate and write a resource or Bundle. Use - for stdin.
    Add {
        file: String,
        /// SHA-256 from `phv source add`, linking the records to that file.
        #[arg(long, requires = "agent")]
        source: Option<String>,
        /// Who mapped the data. Required with --source.
        #[arg(long, requires = "source")]
        agent: Option<String>,
        /// Provenance time. Defaults to now in UTC.
        #[arg(long, requires = "source")]
        recorded_at: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Replace a resource, keeping its ID. Target is Type/id. Use - for stdin.
    Replace {
        target: String,
        file: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a resource from the files. It stays in Git history.
    Rm { target: String },
    /// Print one resource. Target is Type/id.
    Get { target: String },
    /// Print the resources of a type.
    List {
        resource_type: String,
        /// Keep resources where this FHIRPath expression is true.
        #[arg(long = "where")]
        filter: Option<String>,
    },
    /// Manage original files.
    #[command(subcommand)]
    Source(SourceCommand),
    /// Check the whole vault. Changes nothing.
    Check,
    /// Run the local REST API on 127.0.0.1.
    Serve {
        #[arg(long, default_value_t = 8747)]
        port: u16,
    },
    /// Run a local MCP server over stdio for one vault, for Claude and other AI tools.
    Mcp,
    /// Import an export from another system.
    Import {
        #[command(subcommand)]
        command: crate::import::cli::ImportCommand,
    },
}

#[derive(Subcommand)]
pub enum SourceCommand {
    /// Store an original file and create its DocumentReference.
    Add { file: PathBuf },
}

pub(crate) fn vault_path(flag: Option<&Path>) -> Result<PathBuf, PhvError> {
    let env = std::env::var("PHV_VAULT").ok();
    let config = std::env::var("HOME").ok().and_then(|home| {
        std::fs::read_to_string(Path::new(&home).join(".config/phv/config.toml")).ok()
    });
    resolve_vault_path(flag, env.as_deref(), config.as_deref())
}

fn home_dir() -> Result<PathBuf, PhvError> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| PhvError::Usage("HOME is not set".into()))
}

/// Without Git, these changes have no history to restore from.
fn warn_if_no_history(phv: &Phv) {
    if !phv.vault().root().join(".git").exists() {
        eprintln!("warning: this vault has no Git history, so this change cannot be undone");
    }
}

fn read_json(file: &str) -> Result<Value, PhvError> {
    let mut text = String::new();
    if file == "-" {
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| PhvError::Usage(format!("cannot read stdin: {e}")))?;
    } else {
        text = std::fs::read_to_string(file)
            .map_err(|e| PhvError::Usage(format!("cannot read {file}: {e}")))?;
    }
    crate::canon::parse_json(&text).map_err(|e| PhvError::Usage(format!("{file}: {e}")))
}

fn split_target(target: &str) -> Result<(&str, &str), PhvError> {
    target
        .split_once('/')
        .ok_or_else(|| PhvError::Usage(format!("expected Type/id, got {target}")))
}

fn now() -> String {
    humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()
}

/// Writes one line to stdout. A closed pipe, as in `phv list X | head -1`, ends the process
/// quietly with status 0. Other write errors are returned.
fn out_line(line: impl std::fmt::Display) -> Result<(), PhvError> {
    match writeln!(std::io::stdout().lock(), "{line}") {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(0),
        other => Ok(other?),
    }
}

/// Prints a value as pretty JSON. Also used by `main` for error output.
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<(), PhvError> {
    out_line(serde_json::to_string_pretty(value).expect("reports serialize"))
}

fn print_issues(issues: &[Issue]) {
    for issue in issues {
        eprintln!("{issue}");
    }
}

fn print_write(report: &WriteReport, json: bool) -> Result<(), PhvError> {
    if json {
        print_json(report)?;
        return Ok(());
    }
    let prefix = if report.dry_run { "would " } else { "" };
    for r in &report.created {
        out_line(format!("{prefix}create {r}"))?;
    }
    for r in &report.updated {
        out_line(format!("{prefix}update {r}"))?;
    }
    for r in &report.unchanged {
        out_line(format!("unchanged {r}"))?;
    }
    print_issues(&report.warnings);
    Ok(())
}

/// Runs a command. Findings that mean failure are returned as errors or exit code 1.
pub async fn run(cli: Cli) -> Result<ExitCode, PhvError> {
    match &cli.command {
        Command::Setup => {
            crate::setup::run(&home_dir()?).await?;
            return Ok(ExitCode::SUCCESS);
        }
        Command::Vaults => {
            let registry = crate::setup::load_registry(&home_dir()?)?;
            if cli.json {
                print_json(&registry)?;
            } else if registry.vaults.is_empty() {
                println!("no vaults yet: run phv setup");
            } else {
                for (name, path) in &registry.vaults {
                    let mark = if registry.default.as_deref() == Some(name) {
                        "*"
                    } else {
                        " "
                    };
                    println!("{mark} {name}  {}", path.display());
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        Command::Use { name } => {
            let home = home_dir()?;
            let mut registry = crate::setup::load_registry(&home)?;
            registry.use_vault(name)?;
            crate::setup::save_registry(&home, &registry)?;
            println!("default vault is now {name}");
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }

    if let Command::Init { path } = &cli.command {
        let vault = Vault::init(path)?;
        if cli.json {
            print_json(&serde_json::json!({"vault": vault.root()}))?;
        } else {
            println!("created vault at {}", vault.root().display());
            println!("keep it private. Git is optional: phv setup can set it up for you");
        }
        return Ok(ExitCode::SUCCESS);
    }

    if let Command::Import { command } = cli.command {
        return crate::import::cli::run(command, cli.vault.as_deref(), cli.json).await;
    }

    let vault = Vault::open(&vault_path(cli.vault.as_deref())?)?;
    let phv = Phv::new(vault).await?;

    match cli.command {
        Command::Setup
        | Command::Vaults
        | Command::Use { .. }
        | Command::Init { .. }
        | Command::Import { .. } => unreachable!("handled above"),
        Command::Validate { file } => {
            let issues = phv.validate_submission(&read_json(&file)?).await?;
            let failed = issues.iter().any(Issue::is_error);
            if cli.json {
                print_json(&issues)?;
            } else {
                print_issues(&issues);
                if !failed {
                    println!("valid");
                }
            }
            Ok(if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Add {
            file,
            source,
            agent,
            recorded_at,
            dry_run,
        } => {
            let input = read_json(&file)?;
            let recorded_at = recorded_at.or_else(|| source.as_ref().map(|_| now()));
            let options = AddOptions {
                source,
                agent,
                recorded_at,
                dry_run,
            };
            print_write(&phv.add(&input, &options).await?, cli.json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Replace {
            target,
            file,
            dry_run,
        } => {
            let (resource_type, id) = split_target(&target)?;
            let report = phv
                .replace(resource_type, id, &read_json(&file)?, dry_run)
                .await?;
            print_write(&report, cli.json)?;
            if !dry_run && !report.updated.is_empty() {
                warn_if_no_history(&phv);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Rm { target } => {
            let (resource_type, id) = split_target(&target)?;
            phv.remove(resource_type, id).await?;
            warn_if_no_history(&phv);
            if cli.json {
                print_json(&serde_json::json!({"removed": target}))?;
            } else {
                println!(
                    "removed {target}. In a vault with Git, it stays in history until you rewrite that history"
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Get { target } => {
            let (resource_type, id) = split_target(&target)?;
            out_line(phv.get(resource_type, id)?)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::List {
            resource_type,
            filter,
        } => {
            let result = phv.search(&resource_type, filter.as_deref()).await?;
            for line in &result.lines {
                out_line(line)?;
            }
            if let Some(warning) = result.warning() {
                eprintln!("warning: {warning}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Source(SourceCommand::Add { file }) => {
            let bytes = std::fs::read(&file)
                .map_err(|e| PhvError::Usage(format!("cannot read {}: {e}", file.display())))?;
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("source")
                .to_string();
            let report = phv.source_add(&name, &bytes).await?;
            if cli.json {
                print_json(&report)?;
            } else {
                if report.restored {
                    println!("restored the stored file, which was missing or damaged");
                }
                if report.already_imported {
                    println!("already imported as {}", report.document_reference);
                } else {
                    println!("stored as {}", report.document_reference);
                }
                println!("sha256 {}", report.sha256);
                print_issues(&report.warnings);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Check => {
            let issues = phv.check().await?;
            let failed = issues.iter().any(Issue::is_error);
            if cli.json {
                print_json(&issues)?;
            } else {
                print_issues(&issues);
                if !failed {
                    println!("ok");
                }
            }
            Ok(if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Serve { port } => {
            crate::api::serve(phv, port).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Mcp => {
            crate::mcp::serve(phv).await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
