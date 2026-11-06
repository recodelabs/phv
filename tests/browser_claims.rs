//! The Claims topic, read from records the 835 importer writes. All test data is fake.

mod browser_common;
mod common;

use browser_common::{app, get};
use phv::browser::model::claims::{claim_rows, dollars, tidy_name};
use phv::browser::model::explorer::topic_of;
use phv::browser::{Vaults, router};
use phv::canon::parse_json;
use phv::import::x12_835::ImportOptions;
use phv::write::{AddOptions, Phv};
use serde_json::{Value, json};
use std::path::Path;

const OFFICE: &str = "835_SYN00001_20241108_0001.edi";
const NOTHING_PAID: &str = "835_SYN00001_20250124_0001.edi";
const MIXED: &str = "835_SYN00001_20260522_0001.edi";

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

async fn import(phv: &Phv, name: &str) {
    let bytes = std::fs::read(manifest().join("demo/claims-835").join(name)).unwrap();
    let options = ImportOptions {
        dry_run: false,
        recorded_at: "2026-11-06T12:00:00Z".into(),
    };
    phv.import_x12_835(name, &bytes, &options).await.unwrap();
}

async fn maria(files: &[&str]) -> (tempfile::TempDir, Phv) {
    let (dir, phv) = common::phv().await;
    let bundle = std::fs::read_to_string(manifest().join("demo/patient-fhir-bundle.json")).unwrap();
    phv.add(&parse_json(&bundle).unwrap(), &AddOptions::default())
        .await
        .unwrap();
    for file in files {
        import(&phv, file).await;
    }
    (dir, phv)
}

fn all_files() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(manifest().join("demo/claims-835"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

async fn page(phv: &Phv, uri: &str) -> String {
    let app = router(Vaults::single(phv.vault().root().to_path_buf()));
    get(&app, uri).await.text()
}

fn claim_id(phv: &Phv, control_number: &str) -> String {
    common::records(phv, "ExplanationOfBenefit")
        .lines()
        .map(|line| parse_json(line).unwrap())
        .find(|eob: &Value| eob["identifier"][0]["value"] == control_number)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn the_nav_has_claims_before_documents() {
    let html = get(&app(), "/v/vault/overview").await.text();
    let claims = html
        .find(r#"href="/v/vault/claims""#)
        .expect("a Claims link");
    let documents = html.find(r#"href="/v/vault/documents""#).unwrap();
    assert!(claims < documents);
}

#[tokio::test]
async fn a_vault_without_claims_says_so() {
    let html = get(&app(), "/v/vault/claims").await.text();
    assert!(html.contains("No claims yet"), "{html}");
}

#[tokio::test]
async fn the_totals_add_up_across_every_claim() {
    let names = all_files();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_d, phv) = maria(&names).await;
    let html = page(&phv, "/v/vault/claims").await;
    assert!(html.contains("$28,037.27"), "billed: {html}");
    assert!(html.contains("$8,226.10"), "plan paid: {html}");
    assert!(html.contains("$2,905.53"), "patient's share: {html}");
    // What is left of the charges is the discount.
    assert!(html.contains("$16,905.64"), "discount: {html}");
    assert!(html.contains("21 claims"), "{html}");
}

#[tokio::test]
async fn claims_are_newest_first_by_service_date_and_can_be_narrowed_to_a_year() {
    let (_d, phv) = maria(&[OFFICE, MIXED]).await;
    let html = page(&phv, "/v/vault/claims").await;
    let newer = html.find("2 May 2026").expect("the newer claim");
    let older = html.find("19 Oct 2024").expect("the older claim");
    assert!(newer < older);
    assert!(
        html.contains(r#"href="/v/vault/claims?year=2024""#),
        "{html}"
    );

    let year = page(&phv, "/v/vault/claims?year=2024").await;
    assert!(year.contains("19 Oct 2024"), "{year}");
    assert!(!year.contains("2 May 2026"), "{year}");
    assert!(
        year.contains("$213.88"),
        "the totals follow the year: {year}"
    );
    assert!(!year.contains("$4,516.75"), "{year}");
}

#[tokio::test]
async fn a_row_reads_in_plain_words() {
    let (_d, phv) = maria(&[OFFICE, MIXED]).await;
    let html = page(&phv, "/v/vault/claims").await;
    // The stored clinic's own name is used over the capitals in the file.
    assert!(html.contains("Riverside Community Health Clinic"), "{html}");
    assert!(!html.contains("RIVERSIDE COMMUNITY"), "{html}");
    assert!(html.contains("Synthetic General Hospital 1"), "{html}");
    assert!(html.contains("Office visit, established patient"), "{html}");
    assert!(html.contains("Emergency room visit"), "{html}");
}

#[tokio::test]
async fn a_claim_the_plan_paid_nothing_on_explains_why() {
    let (_d, phv) = maria(&[NOTHING_PAID]).await;
    let html = page(&phv, "/v/vault/claims").await;
    assert!(html.contains("$0.00"), "{html}");
    assert!(html.contains("Went toward the deductible"), "{html}");
}

#[tokio::test]
async fn a_claims_page_explains_the_money_and_who_was_involved() {
    let (_d, phv) = maria(&[OFFICE]).await;
    let id = claim_id(&phv, "SYN00001000000002");
    let html = page(&phv, &format!("/v/vault/claims/{id}")).await;
    assert!(html.contains("Riverside Community Health Clinic"), "{html}");
    assert!(html.contains("19 Oct 2024"), "{html}");
    assert!(html.contains("· Office\n"), "the place of care: {html}");
    for amount in ["$213.88", "$68.44", "$17.11", "$128.33"] {
        assert!(html.contains(amount), "{amount}: {html}");
    }
    assert!(html.contains("Office visit, established patient"), "{html}");
    assert!(html.contains("CPT 99213"), "{html}");
    assert!(html.contains("Coinsurance"), "{html}");
    assert!(html.contains("Example Health Plan"), "{html}");
    assert!(html.contains("EHP-77120034"), "{html}");
    assert!(html.contains("James Chen"), "{html}");
    assert!(html.contains("SYN00001000000002"), "{html}");
    assert!(html.contains("SYN-0004"), "{html}");
    assert!(html.contains("Original file"), "{html}");
    assert!(
        html.contains(&format!("/v/vault/fhir/ExplanationOfBenefit/{id}")),
        "{html}"
    );
}

#[tokio::test]
async fn a_hospital_claim_shows_its_revenue_lines() {
    let (_d, phv) = maria(&[MIXED]).await;
    let id = claim_id(&phv, "SYN00001000000017");
    let html = page(&phv, &format!("/v/vault/claims/{id}")).await;
    assert!(html.contains("Hospital outpatient"), "{html}");
    assert!(html.contains("Laboratory"), "{html}");
    assert!(html.contains("Revenue code 0300"), "{html}");
}

#[tokio::test]
async fn an_unknown_claim_is_not_found() {
    let app = app();
    let reply = get(&app, "/v/vault/claims/nope").await;
    assert_eq!(reply.status, 404);
}

#[tokio::test]
async fn the_fhir_page_points_to_the_claim() {
    let (_d, phv) = maria(&[OFFICE]).await;
    let id = claim_id(&phv, "SYN00001000000002");
    let html = page(&phv, &format!("/v/vault/fhir/ExplanationOfBenefit/{id}")).await;
    assert!(html.contains("Shown under"), "{html}");
    assert!(
        html.contains(&format!(r#"href="/v/vault/claims/{id}""#)),
        "{html}"
    );
}

#[test]
fn claims_entered_in_error_stay_out_of_the_topic() {
    let claim = json!({"resourceType":"ExplanationOfBenefit","id":"a","status":"entered-in-error"});
    assert_eq!(topic_of(&claim, &[]), None);
    assert!(claim_rows(&[claim], &[]).is_empty());
}

#[test]
fn a_claim_from_elsewhere_without_totals_still_shows_what_it_can() {
    let claim = json!({
        "resourceType":"ExplanationOfBenefit","id":"a","status":"active",
        "billablePeriod":{"start":"2025-03-01"},
        "provider":{"display":"A Clinic"},
        "item":[{
            "sequence":1,
            "productOrService":{"coding":[{"system":"http://www.ama-assn.org/go/cpt","code":"12345"}],"text":"A named service"},
            "adjudication":[
                {"category":{"coding":[{"code":"submitted"}]},"amount":{"value":100}},
                {"category":{"coding":[{"code":"benefit"}]},"amount":{"value":70.5}},
                {"category":{"coding":[{"code":"copay"}]},"amount":{"value":29.5}}
            ]
        }]
    });
    let rows = claim_rows(&[claim], &[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, "A Clinic");
    assert_eq!(rows[0].services, "A named service");
    assert_eq!(rows[0].billed, "$100.00");
    assert_eq!(rows[0].plan_paid, "$70.50");
    assert_eq!(rows[0].patient, "$29.50");
}

#[test]
fn a_claim_with_no_amounts_says_so_rather_than_showing_zero() {
    let claim = json!({"resourceType":"ExplanationOfBenefit","id":"a","status":"active"});
    let rows = claim_rows(&[claim], &[]);
    assert_eq!(rows[0].billed, "Not recorded");
    assert_eq!(rows[0].date, "No date");
}

#[test]
fn money_reads_like_money() {
    assert_eq!(dollars(0), "$0.00");
    assert_eq!(dollars(5), "$0.05");
    assert_eq!(dollars(195975), "$1,959.75");
    assert_eq!(dollars(123456789), "$1,234,567.89");
    assert_eq!(dollars(-1205), "-$12.05");
}

#[test]
fn names_in_capitals_are_tidied_and_others_are_left_alone() {
    assert_eq!(
        tidy_name("SYNTHETIC MEDICAL GROUP 1"),
        "Synthetic Medical Group 1"
    );
    assert_eq!(
        tidy_name("Riverside Clinic of NY"),
        "Riverside Clinic of NY"
    );
}
