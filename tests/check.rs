mod common;

use common::{add_patient, lab, phv, records};
use phv::write::AddOptions;
use std::fs;

fn observation_path(phv: &phv::write::Phv) -> std::path::PathBuf {
    phv.vault().root().join("records/Observation.ndjson")
}

async fn codes(phv: &phv::write::Phv) -> Vec<String> {
    phv.check()
        .await
        .unwrap()
        .into_iter()
        .filter(|i| i.is_error())
        .map(|i| i.code)
        .collect()
}

#[tokio::test]
async fn a_healthy_vault_has_no_errors() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    phv.source_add("a.pdf", b"bytes").await.unwrap();
    assert_eq!(codes(&phv).await, Vec::<String>::new());
}

#[tokio::test]
async fn check_finds_lines_out_of_order_without_changing_anything() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    phv.add(&lab(&patient, "2.00"), &AddOptions::default())
        .await
        .unwrap();
    let lines: Vec<String> = records(&phv, "Observation")
        .lines()
        .map(str::to_string)
        .collect();
    let swapped = format!("{}\n{}\n", lines[1], lines[0]);
    fs::write(observation_path(&phv), &swapped).unwrap();
    assert!(codes(&phv).await.contains(&"PHV-ORDER".to_string()));
    assert_eq!(records(&phv, "Observation"), swapped);
}

#[tokio::test]
async fn check_finds_a_duplicate_id() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let line = records(&phv, "Observation");
    fs::write(observation_path(&phv), format!("{line}{line}")).unwrap();
    assert!(codes(&phv).await.contains(&"PHV-DUPLICATE".to_string()));
}

#[tokio::test]
async fn check_finds_an_unreadable_line() {
    let (_d, phv) = phv().await;
    fs::write(observation_path(&phv), "<<<<<<< HEAD\n").unwrap();
    assert!(codes(&phv).await.contains(&"PHV-JSON".to_string()));
}

#[tokio::test]
async fn check_finds_a_resource_in_the_wrong_file() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let line = phv
        .get("Patient", patient.split_once('/').unwrap().1)
        .unwrap();
    fs::write(observation_path(&phv), format!("{line}\n")).unwrap();
    assert!(codes(&phv).await.contains(&"PHV-FILE".to_string()));
}

#[tokio::test]
async fn check_finds_formatting_drift() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let line = records(&phv, "Observation");
    fs::write(
        observation_path(&phv),
        line.replace(r#""status":"final""#, r#""status": "final""#),
    )
    .unwrap();
    assert!(codes(&phv).await.contains(&"PHV-FORMAT".to_string()));
}

#[tokio::test]
async fn check_finds_invalid_content_and_broken_references() {
    let (_d, phv) = phv().await;
    fs::write(
        observation_path(&phv),
        "{\"resourceType\":\"Observation\",\"id\":\"a\",\"code\":{\"text\":\"x\"},\"subject\":{\"reference\":\"Patient/none\"}}\n",
    )
    .unwrap();
    let found = codes(&phv).await;
    assert!(found.contains(&"PHV-REF".to_string()), "{found:?}");
    assert!(found.iter().any(|c| c.starts_with("FS")), "{found:?}");
}

#[tokio::test]
async fn check_flags_a_stored_line_with_a_bare_choice_name() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    fs::write(
        observation_path(&phv),
        format!(
            "{{\"code\":{{\"text\":\"x\"}},\"id\":\"a\",\"resourceType\":\"Observation\",\"status\":\"final\",\"subject\":{{\"reference\":\"{patient}\"}},\"value\":\"garbage\"}}\n"
        ),
    )
    .unwrap();
    let found = codes(&phv).await;
    assert!(found.contains(&"PHV-CHOICE".to_string()), "{found:?}");
}

#[tokio::test]
async fn check_finds_windows_line_endings_and_a_missing_final_newline() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let line = records(&phv, "Observation");
    fs::write(observation_path(&phv), line.replace('\n', "\r\n")).unwrap();
    assert!(codes(&phv).await.contains(&"PHV-NEWLINE".to_string()));
    fs::write(observation_path(&phv), line.trim_end()).unwrap();
    assert!(codes(&phv).await.contains(&"PHV-NEWLINE".to_string()));
}

#[tokio::test]
async fn check_finds_a_missing_source_file() {
    let (_d, phv) = phv().await;
    let source = phv.source_add("a.pdf", b"bytes").await.unwrap();
    fs::remove_file(
        phv.vault()
            .root()
            .join(format!("sources/{}.pdf", source.sha256)),
    )
    .unwrap();
    assert!(codes(&phv).await.contains(&"PHV-SOURCE".to_string()));
}

// Beyond the plan: files that the store would skip or fail on must still be reported.

#[tokio::test]
async fn check_reports_a_records_file_with_an_unusable_name() {
    let (_d, phv) = phv().await;
    let line = "{\"resourceType\":\"Observation\",\"id\":\"a\"}\n";
    fs::write(phv.vault().root().join("records/Bad.Name.ndjson"), line).unwrap();
    assert!(codes(&phv).await.contains(&"PHV-FILE".to_string()));
}

#[tokio::test]
async fn check_reports_a_file_that_is_not_utf8_and_keeps_going() {
    let (_d, phv) = phv().await;
    fs::write(observation_path(&phv), b"{\"resourceType\":\"\xff\"}\n").unwrap();
    fs::write(
        phv.vault().root().join("records/Condition.ndjson"),
        "<<<<<<< HEAD\n",
    )
    .unwrap();
    let found = codes(&phv).await;
    assert!(
        found.iter().filter(|c| *c == "PHV-JSON").count() >= 2,
        "{found:?}"
    );
}

#[tokio::test]
async fn check_reports_a_blank_line_and_a_missing_id() {
    let (_d, phv) = phv().await;
    fs::write(
        observation_path(&phv),
        "\n{\"resourceType\":\"Observation\",\"code\":{\"text\":\"x\"}}\n",
    )
    .unwrap();
    let found = codes(&phv).await;
    assert!(found.contains(&"PHV-JSON".to_string()), "{found:?}");
    assert!(found.contains(&"PHV-ID".to_string()), "{found:?}");
}

#[tokio::test]
async fn check_does_not_follow_a_source_url_out_of_sources() {
    let (_d, phv) = phv().await;
    fs::write(phv.vault().root().join("outside.pdf"), b"x").unwrap();
    let line = "{\"resourceType\":\"DocumentReference\",\"id\":\"d\",\"content\":[{\"attachment\":{\"url\":\"sources/../outside.pdf\"}}],\"status\":\"current\"}\n";
    fs::write(
        phv.vault().root().join("records/DocumentReference.ndjson"),
        line,
    )
    .unwrap();
    assert!(codes(&phv).await.contains(&"PHV-SOURCE".to_string()));
}

#[cfg(unix)]
#[tokio::test]
async fn check_works_on_a_read_only_vault_and_creates_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let root = phv.vault().root().to_path_buf();
    // Remove what the writes left behind, so any file check makes shows up.
    fs::remove_dir_all(root.join(".phv")).unwrap();
    let listing = |root: &std::path::Path| {
        let mut names: Vec<_> = walk(root);
        names.sort();
        names
    };
    fn walk(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            out.push(path.display().to_string());
            if path.is_dir() {
                out.extend(walk(&path));
            }
        }
        out
    }
    let before = listing(&root);
    let dirs: Vec<_> = before
        .iter()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .chain([root.clone()])
        .collect();
    for dir in &dirs {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();
    }
    let result = phv.check().await;
    for dir in &dirs {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let issues = result.unwrap();
    assert!(issues.iter().all(|i| !i.is_error()), "{issues:?}");
    assert_eq!(listing(&root), before);
}

#[tokio::test]
async fn check_finds_more_than_one_patient() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let path = phv.vault().root().join("records/Patient.ndjson");
    let first = fs::read_to_string(&path).unwrap();
    let second = first.replace("\"id\":\"", "\"id\":\"z");
    fs::write(&path, format!("{first}{second}")).unwrap();
    let found = codes(&phv).await;
    assert!(found.contains(&"PHV-PATIENT".to_string()), "{found:?}");
}

#[tokio::test]
async fn one_bad_id_gives_one_id_error_and_no_duplicate_or_order_error() {
    let (_d, phv) = phv().await;
    let bad = |id: &str| {
        format!(
            "{{\"resourceType\":\"Observation\",\"id\":\"{id}\",\"code\":{{\"text\":\"x\"}},\"status\":\"final\"}}\n"
        )
    };
    // Two lines with the same invalid ID, then a missing-ID line, then a valid one out of order.
    let missing =
        "{\"resourceType\":\"Observation\",\"code\":{\"text\":\"x\"},\"status\":\"final\"}\n";
    fs::write(
        observation_path(&phv),
        format!("{}{}{missing}{}", bad("b_d"), bad("b_d"), bad("a")),
    )
    .unwrap();
    let found = codes(&phv).await;
    // Each of the three bad lines gets exactly one PHV-ID.
    assert_eq!(
        found.iter().filter(|c| *c == "PHV-ID").count(),
        3,
        "{found:?}"
    );
    assert!(!found.contains(&"PHV-DUPLICATE".to_string()), "{found:?}");
    assert!(!found.contains(&"PHV-ORDER".to_string()), "{found:?}");
}
