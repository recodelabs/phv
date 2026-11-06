//! `phv import`.

use super::apple_activity::{ImportOptions, ImportReport};
use crate::error::PhvError;
use crate::store::Vault;
use crate::write::Phv;
use clap::Subcommand;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Subcommand)]
pub enum ImportCommand {
    /// Imports steps and workouts from the HealthKit export app's activity file
    AppleActivity {
        file: PathBuf,
        /// Report what would change without writing
        #[arg(long)]
        dry_run: bool,
        /// Import both workouts when two sources recorded one session
        #[arg(long)]
        keep_overlaps: bool,
    },
}

fn open_vault(flag: Option<&Path>) -> Result<Vault, PhvError> {
    Vault::open(&crate::cli::vault_path(flag)?)
}

pub async fn run(
    command: ImportCommand,
    vault: Option<&Path>,
    json: bool,
) -> Result<ExitCode, PhvError> {
    let ImportCommand::AppleActivity {
        file,
        dry_run,
        keep_overlaps,
    } = command;
    let text = std::fs::read_to_string(&file)
        .map_err(|e| PhvError::Usage(format!("cannot read {}: {e}", file.display())))?;
    let phv = Phv::new(open_vault(vault)?).await?;
    let options = ImportOptions {
        dry_run,
        keep_overlaps,
    };
    let report = phv.import_apple_activity(&text, &options).await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("the report serializes")
        );
    } else {
        print_report(&report);
    }
    Ok(if report.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn held_keys<'a>(keys: impl Iterator<Item = &'a str>) -> String {
    keys.collect::<Vec<_>>().join(", ")
}

fn print_report(report: &ImportReport) {
    let tally = |t: super::apple_activity::Tally| {
        format!(
            "{} new, {} changed, {} unchanged",
            t.new, t.changed, t.unchanged
        )
    };
    println!("Daily steps: {}", tally(report.daily_steps));
    println!("Workouts: {}", tally(report.workouts));
    if report.duplicates_in_file > 0 {
        println!("Repeated rows in the file: {}", report.duplicates_in_file);
    }
    if !report.changed_days.is_empty() {
        println!("Days with a new total: {}", report.changed_days.join(", "));
    }
    if !report.overlaps.is_empty() {
        println!(
            "Skipped {} workouts another source already recorded:",
            report.overlaps.len()
        );
        for o in &report.overlaps {
            println!(
                "  {} {} from {} ({}), same session as {} from {}",
                o.start, o.activity_type, o.skipped_source, o.skipped, o.kept, o.kept_source
            );
        }
    }
    if let (Some(first), n @ 1..) = (report.other_time_zone.first(), report.other_time_zone.len()) {
        println!(
            "Kept {n} stored days, because this export counts days in {} and they were stored in {}",
            first.exported_time_zone,
            first
                .stored_time_zone
                .as_deref()
                .unwrap_or("an unknown zone")
        );
    }
    if let Some(first) = report.older_export.first() {
        println!(
            "Kept {} stored days and workouts that this export would change, because it ends {} and the vault already holds an export that ends {}: {}",
            report.older_export.len(),
            first.file_through,
            first.vault_through,
            held_keys(report.older_export.iter().map(|h| h.key.as_str()))
        );
    }
    if !report.edited_since_import.is_empty() {
        println!(
            "Kept {} stored days and workouts that were edited after they were imported: {}",
            report.edited_since_import.len(),
            held_keys(report.edited_since_import.iter().map(|h| h.key.as_str()))
        );
    }
    for kept in &report.kept_members {
        println!(
            "Kept {} ({} of workout {}), because {} references it",
            kept.member,
            kept.measure,
            kept.workout,
            kept.referenced_by.join(", ")
        );
    }
    if !report.export_failures.is_empty() {
        println!(
            "The export app reported failed queries, so this file may be missing rows: {}",
            serde_json::Value::Array(report.export_failures.clone())
        );
    }
    for warning in &report.warnings {
        println!("{warning}");
    }
    if !report.conflicts.is_empty() {
        println!(
            "Not imported, because the file has different rows for one key: {}",
            report.conflicts.join(", ")
        );
    }
    for rejected in &report.rejected {
        println!("Rejected {}: {}", rejected.key, rejected.message);
    }
    let verb = if report.dry_run {
        "would write"
    } else {
        "wrote"
    };
    println!("Observations: {verb} {}", report.resources_written);
}
