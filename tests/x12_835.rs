//! The X12 835 importer, run against the fake files in demo/claims-835.

mod common;

use common::{add_patient, phv, records};
use phv::canon::parse_json;
use phv::error::PhvError;
use phv::import::x12_835::{ImportOptions, ImportReport};
use phv::write::{AddOptions, Phv};
use serde_json::Value;
use std::path::{Path, PathBuf};

const PROFESSIONAL: &str = "835_SYN00001_20241101_0001.edi";
/// Two transactions: one hospital claim and one office claim.
const MIXED: &str = "835_SYN00001_20260522_0001.edi";
/// One check that pays two claims.
const TWO_CLAIMS: &str = "835_SYN00001_20260515_0001.edi";
const NOTHING_PAID: &str = "835_SYN00001_20250124_0001.edi";
/// Rendered by the practitioner and billed by the clinic already in the demo Bundle.
const RIVERSIDE: &str = "835_SYN00001_20241108_0001.edi";

fn demo_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/claims-835")
}

fn demo_file(name: &str) -> String {
    std::fs::read_to_string(demo_dir().join(name)).unwrap()
}

async fn maria() -> (tempfile::TempDir, Phv) {
    let (dir, phv) = phv().await;
    let bundle = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/patient-fhir-bundle.json"),
    )
    .unwrap();
    phv.add(&parse_json(&bundle).unwrap(), &AddOptions::default())
        .await
        .unwrap();
    (dir, phv)
}

fn options() -> ImportOptions {
    ImportOptions {
        dry_run: false,
        recorded_at: "2026-11-06T12:00:00Z".into(),
    }
}

async fn import(phv: &Phv, name: &str, text: &str) -> Result<ImportReport, PhvError> {
    phv.import_x12_835(name, text.as_bytes(), &options()).await
}

fn stored(phv: &Phv, resource_type: &str) -> Vec<Value> {
    records(phv, resource_type)
        .lines()
        .map(|line| parse_json(line).unwrap())
        .collect()
}

/// The stored claim with this payer claim control number.
fn claim(phv: &Phv, control_number: &str) -> Value {
    stored(phv, "ExplanationOfBenefit")
        .into_iter()
        .find(|eob| eob["identifier"][0]["value"] == control_number)
        .unwrap_or_else(|| panic!("no claim {control_number}"))
}

fn amount(item: &Value, category: &str) -> Option<String> {
    item["adjudication"]
        .as_array()?
        .iter()
        .chain(item["total"].as_array().into_iter().flatten())
        .find(|a| a["category"]["coding"][0]["code"] == category)
        .map(|a| a["amount"]["value"].to_string())
}

fn total(eob: &Value, category: &str) -> Option<String> {
    eob["total"]
        .as_array()?
        .iter()
        .find(|t| t["category"]["coding"][0]["code"] == category)
        .map(|t| t["amount"]["value"].to_string())
}

fn message(error: PhvError) -> String {
    match error {
        PhvError::Usage(text) => text,
        PhvError::Invalid(issues) | PhvError::Conflict(issues) => format!("{issues:?}"),
        other => format!("{other:?}"),
    }
}

async fn all_demo_files(phv: &Phv) -> Vec<ImportReport> {
    let mut names: Vec<String> = std::fs::read_dir(demo_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 18);
    let mut reports = Vec::new();
    for name in names {
        reports.push(import(phv, &name, &demo_file(&name)).await.unwrap());
    }
    reports
}

#[tokio::test]
async fn every_demo_file_imports_and_the_vault_passes_check() {
    let (_d, phv) = maria().await;
    let reports = all_demo_files(&phv).await;
    assert_eq!(reports.iter().map(|r| r.claims.new).sum::<usize>(), 21);
    assert!(reports.iter().all(ImportReport::is_clean));
    assert_eq!(stored(&phv, "ExplanationOfBenefit").len(), 21);
    assert_eq!(stored(&phv, "Coverage").len(), 1);
    assert_eq!(stored(&phv, "Provenance").len(), 18);
    let errors: Vec<_> = phv
        .check()
        .await
        .unwrap()
        .into_iter()
        .filter(|i| i.is_error())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn importing_again_changes_nothing() {
    let (_d, phv) = maria().await;
    all_demo_files(&phv).await;
    let before: Vec<String> = [
        "ExplanationOfBenefit",
        "Coverage",
        "Organization",
        "Provenance",
    ]
    .iter()
    .map(|t| records(&phv, t))
    .collect();
    let again = all_demo_files(&phv).await;
    assert_eq!(again.iter().map(|r| r.claims.new).sum::<usize>(), 0);
    assert_eq!(again.iter().map(|r| r.claims.unchanged).sum::<usize>(), 21);
    let after: Vec<String> = [
        "ExplanationOfBenefit",
        "Coverage",
        "Organization",
        "Provenance",
    ]
    .iter()
    .map(|t| records(&phv, t))
    .collect();
    assert_eq!(before, after);
}

#[tokio::test]
async fn a_claim_keeps_its_amounts_and_who_owes_what() {
    let (_d, phv) = maria().await;
    import(&phv, PROFESSIONAL, &demo_file(PROFESSIONAL))
        .await
        .unwrap();
    let eob = claim(&phv, "SYN00001000000001");

    assert_eq!(eob["type"]["coding"][0]["code"], "professional");
    assert_eq!(eob["use"], "claim");
    assert_eq!(eob["outcome"], "complete");
    assert_eq!(eob["created"], "2024-11-01");
    assert_eq!(eob["billablePeriod"]["start"], "2024-10-05");
    assert_eq!(eob["billablePeriod"]["end"], "2024-10-05");
    assert_eq!(total(&eob, "submitted").unwrap(), "1959.75");
    assert_eq!(total(&eob, "eligible").unwrap(), "783.90");
    assert_eq!(total(&eob, "benefit").unwrap(), "400.72");
    assert_eq!(total(&eob, "memberliability").unwrap(), "383.18");
    assert_eq!(eob["payment"]["amount"]["value"].to_string(), "400.72");
    assert_eq!(eob["payment"]["date"], "2024-11-01");
    assert_eq!(eob["payment"]["identifier"]["value"], "SYN-0003");

    let items = eob["item"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let line = &items[1];
    assert_eq!(line["sequence"], 2);
    assert_eq!(
        line["productOrService"]["coding"][0]["system"],
        "http://www.ama-assn.org/go/cpt"
    );
    assert_eq!(line["productOrService"]["coding"][0]["code"], "99495");
    assert_eq!(line["servicedDate"], "2024-10-05");
    assert_eq!(line["locationCodeableConcept"]["coding"][0]["code"], "11");
    assert_eq!(amount(line, "submitted").unwrap(), "539.25");
    assert_eq!(amount(line, "eligible").unwrap(), "215.70");
    assert_eq!(amount(line, "benefit").unwrap(), "55.60");
    // `CAS*PR*1*146.20**2*13.90` is two adjustments on one segment.
    assert_eq!(amount(line, "deductible").unwrap(), "146.20");
    assert_eq!(amount(line, "coinsurance").unwrap(), "13.90");
    assert_eq!(amount(line, "discount").unwrap(), "323.55");
    let deductible = line["adjudication"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["category"]["coding"][0]["code"] == "deductible")
        .unwrap();
    assert_eq!(
        deductible["reason"]["coding"][0]["system"],
        "https://x12.org/codes/claim-adjustment-reason-codes"
    );
    assert_eq!(deductible["reason"]["coding"][0]["code"], "1");
}

#[tokio::test]
async fn a_hospital_claim_is_institutional_with_revenue_codes() {
    let (_d, phv) = maria().await;
    let report = import(&phv, MIXED, &demo_file(MIXED)).await.unwrap();
    assert_eq!(report.claims.new, 2);

    let hospital = claim(&phv, "SYN00001000000017");
    assert_eq!(hospital["type"]["coding"][0]["code"], "institutional");
    assert_eq!(hospital["billablePeriod"]["start"], "2026-04-25");
    let bill = hospital["supportingInfo"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["category"]["coding"][0]["code"] == "typeofbill")
        .unwrap();
    assert_eq!(bill["code"]["coding"][0]["code"], "131");
    let items = hospital["item"].as_array().unwrap();
    assert_eq!(items[0]["revenue"]["coding"][0]["code"], "0450");
    assert_eq!(items[0]["productOrService"]["coding"][0]["code"], "99283");
    assert_eq!(items[1]["revenue"]["coding"][0]["code"], "0300");
    assert_eq!(
        items[1]["productOrService"]["coding"][0]["code"],
        "not-applicable"
    );
    assert_eq!(items[1]["quantity"]["value"].to_string(), "3");
    assert!(items[1].get("locationCodeableConcept").is_none());

    let office = claim(&phv, "SYN00001000000018");
    assert_eq!(office["type"]["coding"][0]["code"], "professional");
}

#[tokio::test]
async fn one_check_can_pay_two_claims() {
    let (_d, phv) = maria().await;
    let report = import(&phv, TWO_CLAIMS, &demo_file(TWO_CLAIMS))
        .await
        .unwrap();
    assert_eq!(report.claims.new, 2);
    let first = claim(&phv, "SYN00001000000015");
    let second = claim(&phv, "SYN00001000000016");
    assert_eq!(first["payment"]["amount"]["value"].to_string(), "591.90");
    assert_eq!(second["payment"]["amount"]["value"].to_string(), "408.83");
    assert_eq!(
        first["payment"]["identifier"],
        second["payment"]["identifier"]
    );
}

#[tokio::test]
async fn a_claim_the_plan_paid_nothing_on_is_still_recorded() {
    let (_d, phv) = maria().await;
    import(&phv, NOTHING_PAID, &demo_file(NOTHING_PAID))
        .await
        .unwrap();
    let eob = claim(&phv, "SYN00001000000004");
    assert_eq!(total(&eob, "benefit").unwrap(), "0.00");
    assert_eq!(total(&eob, "memberliability").unwrap(), "85.55");
}

#[tokio::test]
async fn people_and_organizations_already_in_the_vault_are_reused() {
    let (_d, phv) = maria().await;
    let organizations = stored(&phv, "Organization");
    let practitioners = records(&phv, "Practitioner");
    import(&phv, RIVERSIDE, &demo_file(RIVERSIDE))
        .await
        .unwrap();

    // Only the payer is new. The stored clinic and doctor are not rewritten.
    let after = stored(&phv, "Organization");
    assert_eq!(after.len(), organizations.len() + 1);
    assert_eq!(records(&phv, "Practitioner"), practitioners);

    let eob = claim(&phv, "SYN00001000000002");
    let clinic = format!("Organization/{}", organizations[0]["id"].as_str().unwrap());
    assert_eq!(eob["provider"]["reference"], clinic.as_str());
    assert_eq!(eob["payee"]["party"]["reference"], clinic.as_str());
    let doctor = stored(&phv, "Practitioner")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        eob["careTeam"][0]["provider"]["reference"],
        format!("Practitioner/{doctor}").as_str()
    );
    assert_eq!(eob["item"][0]["careTeamSequence"][0], 1);
}

#[tokio::test]
async fn a_new_provider_and_the_coverage_are_created() {
    let (_d, phv) = maria().await;
    import(&phv, PROFESSIONAL, &demo_file(PROFESSIONAL))
        .await
        .unwrap();
    let group = stored(&phv, "Organization")
        .into_iter()
        .find(|o| o["name"] == "SYNTHETIC MEDICAL GROUP 1")
        .unwrap();
    assert_eq!(group["identifier"][0]["value"], "1999835881");
    let coverage = &stored(&phv, "Coverage")[0];
    assert_eq!(coverage["subscriberId"], "EHP-77120034");
    assert_eq!(coverage["status"], "active");
    let eob = claim(&phv, "SYN00001000000001");
    assert_eq!(eob["insurance"][0]["focal"], true);
    assert_eq!(
        eob["insurance"][0]["coverage"]["reference"],
        format!("Coverage/{}", coverage["id"].as_str().unwrap()).as_str()
    );
}

#[tokio::test]
async fn the_records_point_back_to_the_stored_file() {
    let (_d, phv) = maria().await;
    let report = import(&phv, MIXED, &demo_file(MIXED)).await.unwrap();
    let provenance = &stored(&phv, "Provenance")[0];
    assert_eq!(provenance["target"].as_array().unwrap().len(), 2);
    assert_eq!(
        provenance["entity"][0]["what"]["reference"],
        report.document_reference.as_deref().unwrap()
    );
    assert_eq!(stored(&phv, "DocumentReference").len(), 1);
}

#[tokio::test]
async fn a_dry_run_writes_nothing() {
    let (_d, phv) = maria().await;
    let documents = records(&phv, "DocumentReference");
    let report = phv
        .import_x12_835(
            PROFESSIONAL,
            demo_file(PROFESSIONAL).as_bytes(),
            &ImportOptions {
                dry_run: true,
                ..options()
            },
        )
        .await
        .unwrap();
    assert_eq!(report.claims.new, 1);
    assert_eq!(records(&phv, "ExplanationOfBenefit"), "");
    assert_eq!(records(&phv, "DocumentReference"), documents);
}

#[tokio::test]
async fn test_files_are_flagged() {
    let (_d, phv) = maria().await;
    let report = import(&phv, PROFESSIONAL, &demo_file(PROFESSIONAL))
        .await
        .unwrap();
    assert!(report.marked_as_test, "the demo files have ISA15 = T");
}

#[tokio::test]
async fn another_persons_claims_are_refused() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let error = import(&phv, PROFESSIONAL, &demo_file(PROFESSIONAL))
        .await
        .unwrap_err();
    assert!(message(error).contains("RODRIGUEZ"));
    assert_eq!(records(&phv, "ExplanationOfBenefit"), "");
    assert_eq!(records(&phv, "DocumentReference"), "");
}

#[tokio::test]
async fn amounts_that_do_not_add_up_are_refused() {
    let (_d, phv) = maria().await;
    let bad =
        demo_file(PROFESSIONAL).replace("SVC*HC:99396*342.00*0.00", "SVC*HC:99396*342.00*9.00");
    let error = import(&phv, PROFESSIONAL, &bad).await.unwrap_err();
    assert!(message(error).contains("add up"));
    assert_eq!(records(&phv, "ExplanationOfBenefit"), "");
}

#[tokio::test]
async fn a_check_total_that_differs_from_the_claims_is_refused() {
    let (_d, phv) = maria().await;
    let bad = demo_file(PROFESSIONAL).replace("BPR*I*400.72", "BPR*I*401.72");
    assert!(message(import(&phv, PROFESSIONAL, &bad).await.unwrap_err()).contains("add up"));
}

#[tokio::test]
async fn a_wrong_segment_count_is_refused() {
    let (_d, phv) = maria().await;
    let bad = demo_file(PROFESSIONAL).replace("SE*36*0003", "SE*35*0003");
    assert!(message(import(&phv, PROFESSIONAL, &bad).await.unwrap_err()).contains("segments"));
}

#[tokio::test]
async fn segments_phv_cannot_read_are_named_rather_than_dropped() {
    let (_d, phv) = maria().await;
    let bad = demo_file(PROFESSIONAL).replace("SE*36*0003", "PLB*1*20241231*WO:1*0.00~SE*37*0003");
    let text = message(import(&phv, PROFESSIONAL, &bad).await.unwrap_err());
    assert!(text.contains("PLB"), "{text}");
    assert_eq!(records(&phv, "ExplanationOfBenefit"), "");
}

#[tokio::test]
async fn a_claim_adjustment_outside_a_service_line_is_refused() {
    let (_d, phv) = maria().await;
    let bad =
        demo_file(PROFESSIONAL).replace("DTM*050*20241006", "CAS*OA*23*1.00~DTM*050*20241006");
    let bad = bad.replace("SE*36*0003", "SE*37*0003");
    let text = message(import(&phv, PROFESSIONAL, &bad).await.unwrap_err());
    assert!(text.contains("CAS"), "{text}");
}

#[tokio::test]
async fn a_reversal_is_refused_until_phv_can_show_one() {
    let (_d, phv) = maria().await;
    let bad = demo_file(PROFESSIONAL).replace(
        "CLP*bb14f6865e97e8783938*1*",
        "CLP*bb14f6865e97e8783938*22*",
    );
    let text = message(import(&phv, PROFESSIONAL, &bad).await.unwrap_err());
    assert!(text.contains("status 22"), "{text}");
}

#[tokio::test]
async fn a_different_version_of_a_stored_claim_is_held_not_overwritten() {
    let (_d, phv) = maria().await;
    import(&phv, PROFESSIONAL, &demo_file(PROFESSIONAL))
        .await
        .unwrap();
    let before = records(&phv, "ExplanationOfBenefit");
    let changed = demo_file(PROFESSIONAL).replace("DTM*050*20241006", "DTM*050*20241007");
    let report = import(&phv, "corrected.edi", &changed).await.unwrap();
    assert_eq!(report.held, ["SYN00001000000001"]);
    assert!(!report.is_clean());
    assert_eq!(records(&phv, "ExplanationOfBenefit"), before);
    // The stored claim did not come from the second file, so only the first one claims it.
    assert_eq!(stored(&phv, "Provenance").len(), 1);
    assert_eq!(stored(&phv, "DocumentReference").len(), 2);
}

#[tokio::test]
async fn a_file_that_is_not_an_835_is_refused() {
    let (_d, phv) = maria().await;
    for (name, text) in [
        ("empty.edi", String::new()),
        ("note.edi", "not an edi file at all".to_string()),
        (
            "claim.edi",
            demo_file(PROFESSIONAL).replace("ST*835*0003", "ST*837*0003"),
        ),
    ] {
        assert!(import(&phv, name, &text).await.is_err(), "{name}");
    }
    assert_eq!(records(&phv, "DocumentReference"), "");
}

async fn page(phv: &Phv, uri: &str) -> String {
    let app = phv::browser::router(phv::browser::Vaults::single(
        phv.vault().root().to_path_buf(),
    ));
    let request = axum::http::Request::builder()
        .uri(uri)
        .header("host", "127.0.0.1")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
    assert_eq!(response.status(), 200, "{uri}");
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn the_browser_lists_claims_with_who_billed_and_who_paid() {
    let (_d, phv) = maria().await;
    import(&phv, RIVERSIDE, &demo_file(RIVERSIDE))
        .await
        .unwrap();

    let types = page(&phv, "/v/vault/fhir").await;
    assert!(types.contains("ExplanationOfBenefit"), "{types}");
    assert!(types.contains("Coverage"), "{types}");

    let list = page(&phv, "/v/vault/fhir/ExplanationOfBenefit").await;
    assert!(list.contains("RIVERSIDE COMMUNITY HEALTH CLINIC"), "{list}");
    assert!(list.contains("plan paid $68.44"), "{list}");
    assert!(list.contains("patient owes $17.11"), "{list}");
    assert!(list.contains("2024"), "the service date shows: {list}");

    let organizations = page(&phv, "/v/vault/fhir/Organization").await;
    assert!(
        organizations.contains("EXAMPLE HEALTH PLAN"),
        "{organizations}"
    );
}

#[tokio::test]
async fn a_claims_page_links_to_its_source_file_and_the_file_counts_it() {
    let (_d, phv) = maria().await;
    import(&phv, MIXED, &demo_file(MIXED)).await.unwrap();
    let id = claim(&phv, "SYN00001000000017")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let record = page(&phv, &format!("/v/vault/fhir/ExplanationOfBenefit/{id}")).await;
    assert!(record.contains(MIXED), "{record}");

    let documents = page(&phv, "/v/vault/documents").await;
    assert!(documents.contains(MIXED), "{documents}");
}
