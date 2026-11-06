# FHIR Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a local, read-only web browser that shows a phv vault's records in everyday terms, as specified in `docs/superpowers/specs/2026-11-06-fhir-browser-design.md`.

**Architecture:** A new `src/browser/` module. `browser::model` turns raw NDJSON lines into plain view data with no HTTP in it. `browser::routes` has one file per topic, each holding its Askama templates' structs and its GET handlers. Each request reads the vault's files, with no cache or index. Nothing in this module writes to a vault.

**Tech Stack:** Rust 1.99.1, Axum 0.8, Askama 0.16, htmx 2.0.11 (embedded in the binary), and the existing `phv::store` and `phv::canon`.

**How far this has been checked:** Tasks 1 to 11 were replayed in order in a clean clone before this plan was written. Each task's tests passed at its own stage, and the end state passed the full suite, clippy with warnings denied, and `cargo fmt --check`. Tasks 12 and 13 depend on `vault-core` code that did not exist yet, so **they were not compiled**. Check them against the merged code.

---

## Working here

- Work in the worktree `/Users/mberg/github/phv/.worktrees/fhir-browser`, on branch `fhir-browser`.
- Commit only on this branch, and do not push unless asked.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Test data must stay synthetic. Real health data never goes in this repo.

## File structure

| Path | Responsibility |
|---|---|
| `src/browser/mod.rs` | Module root, exports `router`, `Vaults` and `model` |
| `src/browser/vaults.rs` | Which vaults this server shows. Paths never come from a URL |
| `src/browser/guard.rs` | Rejects non-local requests, sets headers on every response |
| `src/browser/assets.rs` | Serves the embedded stylesheet and htmx |
| `src/browser/model/mod.rs` | `Records`: reads resources and counts unreadable lines |
| `src/browser/model/value.rs` | Shows FHIR values exactly as stored, dates, sort order, URL encoding |
| `src/browser/model/observation.rs` | One Observation as a row, grouping keys |
| `src/browser/model/{vitals,labs,clinical,documents,overview,explorer}.rs` | One topic's rules each, with no HTTP |
| `src/browser/routes/mod.rs` | `router()`, `run()`, layout data, error pages |
| `src/browser/routes/{vitals,labs,clinical,documents,overview,explorer}.rs` | One topic's template structs and handlers each |
| `templates/` | Askama HTML with automatic escaping. Never mark record text as safe |
| `assets/` | Stylesheet and the pinned htmx file |
| `examples/browser.rs` | Starts the browser until `phv serve` can host it |
| `tests/fixtures/sample-vault/` | Synthetic vault used by all browser tests |
| `tests/browser_*.rs` | One test file per task |

## Tasks

### Task 1: Merge vault-core and add the assets

`vault-core` is still moving, so start from its latest code. This task adds Askama, the pinned htmx file and the stylesheet. Nothing uses them yet.

**Files:**

- Create: `assets/style.css`
- Create: `assets/htmx-2.0.11.min.js`
- Modify: `Cargo.toml`

- [ ] **Step 1: Merge the latest `vault-core`**

```bash
git merge --no-edit vault-core
```

- [ ] **Step 2: Download htmx and check it is the pinned file**

```bash
mkdir -p assets
curl -sSfL -o assets/htmx-2.0.11.min.js https://cdn.jsdelivr.net/npm/htmx.org@2.0.11/dist/htmx.min.js
echo 'd6fdc75f204e6bdefa99b69bf1e6d4ac69b8a364f77929f45c13476b4000f717  assets/htmx-2.0.11.min.js' | shasum -a 256 -c
# Expected: assets/htmx-2.0.11.min.js: OK
```

- [ ] **Step 3: Create `assets/style.css`**

```css
:root {
  color-scheme: light dark;
  --bg: #f6f6f4;
  --panel: #ffffff;
  --muted-bg: #ecece8;
  --line: #d9d9d3;
  --text: #1e1f1c;
  --muted: #676a62;
  --accent: #1f6f5c;
  --accent-text: #ffffff;
  --flag: #a8480d;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #171816;
    --panel: #20221f;
    --muted-bg: #2a2c28;
    --line: #3a3d37;
    --text: #ecece6;
    --muted: #a3a69d;
    --accent: #5cc4a9;
    --accent-text: #10201b;
    --flag: #f0a26b;
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 15px/1.5 system-ui, sans-serif; }
a { color: var(--accent); }
h1 { font-size: 1.4rem; margin: 0 0 .25rem; }
h2 { font-size: 1rem; margin: 1.5rem 0 .5rem; }
.top { display: flex; align-items: center; justify-content: space-between; gap: 1rem; padding: .6rem 1rem; background: var(--panel); border-bottom: 1px solid var(--line); }
.brand { font-weight: 700; }
.switcher { position: relative; }
.switcher summary { cursor: pointer; border: 1px solid var(--line); border-radius: 6px; padding: .15rem .6rem; }
.switcher ul { position: absolute; right: 0; z-index: 1; margin: .25rem 0 0; padding: .35rem; list-style: none; background: var(--panel); border: 1px solid var(--line); border-radius: 6px; min-width: 10rem; }
.switcher li { padding: .15rem .4rem; }
.layout { display: flex; min-height: calc(100vh - 3rem); }
.nav { flex: none; width: 10rem; padding: .75rem .5rem; background: var(--muted-bg); }
.nav a { display: block; padding: .25rem .6rem; border-radius: 6px; color: var(--text); text-decoration: none; }
.nav a[aria-current] { background: var(--accent); color: var(--accent-text); }
.nav .fhir { margin-top: .75rem; }
main { flex: 1; min-width: 0; padding: 1.25rem 1.5rem; max-width: 62rem; }
.sub { color: var(--muted); margin: 0 0 1rem; font-size: .9rem; }
.muted { color: var(--muted); }
.notice { border: 1px solid var(--flag); color: var(--flag); border-radius: 6px; padding: .5rem .75rem; margin-bottom: 1rem; }
.tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr)); gap: .6rem; margin-bottom: 1rem; }
.tile-link { color: inherit; text-decoration: none; }
.tile { background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: .6rem .75rem; }
.tile b { display: block; font-size: 1.25rem; line-height: 1.3; }
.tile span { color: var(--muted); font-size: .8rem; }
table { width: 100%; border-collapse: collapse; background: var(--panel); margin-bottom: 1rem; }
th, td { padding: .35rem .6rem; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; }
th { color: var(--muted); font-size: .8rem; font-weight: 400; }
td.history { padding: 0 0 0 1rem; border-bottom: none; }
td.history:empty { display: none; }
.flag { color: var(--flag); font-weight: 600; }
.chips { display: flex; flex-wrap: wrap; gap: .35rem; margin-bottom: .75rem; }
.chip { border: 1px solid var(--line); border-radius: 1rem; padding: 0 .7rem; color: var(--text); text-decoration: none; font-size: .85rem; }
.chip[aria-current] { background: var(--accent); border-color: var(--accent); color: var(--accent-text); }
pre { background: var(--panel); border: 1px solid var(--line); border-radius: 6px; padding: .75rem; overflow: auto; font: 13px/1.5 ui-monospace, Menlo, monospace; }
ul.plain { list-style: none; padding: 0; margin: 0; }
@media (max-width: 40rem) {
  .layout { flex-direction: column; }
  .nav { width: auto; display: flex; flex-wrap: wrap; gap: .15rem; }
  .nav .fhir { margin-top: 0; }
  main { padding: 1rem; }
  table { display: block; overflow-x: auto; }
}
```

- [ ] **Step 4: Make the edits**

In `Cargo.toml`, replace:

```toml
axum = "0.8"
```

with:

```toml
askama = "0.16"
axum = "0.8"
```


- [ ] **Step 5: Run the dependency check**

Run: `cargo test --test deps`

Expected: 1 test passes, so Askama brought in no HTTP client or SQLite crate.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock assets
git commit -m "chore: add Askama, htmx and the browser stylesheet"
```


### Task 2: Sample vault

All browser tests use this small synthetic vault. These tests keep it honest: lines must be canonical and sorted, files must be named for their hash, and every record must pass the real validator. FHIR's `ait-1` rule requires `clinicalStatus` on an allergy, so the missing-status case for allergies is tested inline later, not stored here.

**Files:**

- Create: `tests/browser_common/mod.rs`
- Create: `tests/fixtures/sample-vault/` (`phv.toml`, `records/`, `sources/`)
- Test: `tests/browser_fixture.rs`

- [ ] **Step 1: Create the shared test helpers in `tests/browser_common/mod.rs`**

These grow in Task 4.

```rust
#![allow(dead_code)]

use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-vault")
}

/// Every resource of a type in the sample vault.
pub fn fixture(resource_type: &str) -> Vec<Value> {
    let path = fixture_path().join(format!("records/{resource_type}.ndjson"));
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| phv::canon::parse_json(line).unwrap())
        .collect()
}

/// A private copy of the sample vault that a test can change.
pub fn copy_fixture(to: &Path) {
    copy_dir(&fixture_path(), to);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
```

- [ ] **Step 2: Write the failing tests in `tests/browser_fixture.rs`**

```rust
//! The sample vault must look like a real one, or the other browser tests prove little.

mod browser_common;

use browser_common::{copy_fixture, fixture_path};
use phv::canon::{canon_resource, parse_json};
use phv::store::{Vault, line_id};
use phv::write::Phv;

#[test]
fn every_line_is_canonical_and_sorted() {
    let vault = Vault::open(&fixture_path()).unwrap();
    for resource_type in vault.resource_types().unwrap() {
        let lines = vault.lines(&resource_type).unwrap();
        for line in &lines {
            assert_eq!(*line, canon_resource(&parse_json(line).unwrap()));
        }
        let ids: Vec<_> = lines.iter().map(|l| line_id(l).unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "{resource_type} is out of order");
    }
}

#[test]
fn source_files_are_named_for_their_hash() {
    for entry in std::fs::read_dir(fixture_path().join("sources")).unwrap() {
        let path = entry.unwrap().path();
        let hash = phv::canon::sha256_hex(&std::fs::read(&path).unwrap());
        assert_eq!(path.file_stem().unwrap().to_str().unwrap(), hash);
    }
}

#[tokio::test]
async fn every_record_passes_validation() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let vault = Vault::open(dir.path()).unwrap();
    let phv = Phv::new(vault.clone()).await.unwrap();
    // A vault holds one Patient, so the sample's own Patient must be checked against an empty one.
    let empty = Phv::new(Vault::init(&dir.path().join("empty")).unwrap())
        .await
        .unwrap();
    let mut problems = Vec::new();
    for resource_type in vault.resource_types().unwrap() {
        for line in vault.lines(&resource_type).unwrap() {
            let resource = parse_json(&line).unwrap();
            let checker = if resource_type == "Patient" {
                &empty
            } else {
                &phv
            };
            let issues = checker.validate_submission(&resource).await.unwrap();
            for issue in issues.iter().filter(|i| i.is_error()) {
                problems.push(format!("{}: {}", resource["id"], issue.message));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --test browser_fixture`

Expected: FAIL, because `tests/fixtures/sample-vault` does not exist.

- [ ] **Step 4: Create `tests/fixtures/sample-vault/phv.toml`**

```toml
format_version = 1
fhir_version = "4.0.1"
```

- [ ] **Step 5: Create the synthetic source files**

The byte content matters, because each file is named for its SHA-256.

```bash
mkdir -p tests/fixtures/sample-vault/sources
cat > tests/fixtures/sample-vault/sources/1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412.pdf <<'EOF'
%PDF-1.4
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 144]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj
4 0 obj<</Length 54>>stream
BT /F1 14 Tf 20 100 Td (Synthetic lab report) Tj ET
endstream
endobj
5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj
trailer<</Root 1 0 R>>
EOF
printf 'Synthetic pharmacy receipt. Not real data.\n' > tests/fixtures/sample-vault/sources/82f7258fe85a893e067ec1c27ac161dafd511455dce49426a6d973d4e5ce4479.txt
shasum -a 256 tests/fixtures/sample-vault/sources/*
# Expected: each hash matches its file name
```

- [ ] **Step 6: Create the records**

Each file must match exactly, because the test compares lines with canonical output.

`tests/fixtures/sample-vault/records/AllergyIntolerance.ndjson`:

```json
{"resourceType":"AllergyIntolerance","id":"allergy-penicillin","clinicalStatus":{"coding":[{"code":"active","system":"http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical"}]},"code":{"text":"Penicillin"},"patient":{"reference":"Patient/patient-maya"},"reaction":[{"manifestation":[{"text":"Hives"}]}],"recordedDate":"2010-04-02"}
```

`tests/fixtures/sample-vault/records/Condition.ndjson`:

```json
{"resourceType":"Condition","id":"condition-asthma","clinicalStatus":{"coding":[{"code":"active","system":"http://terminology.hl7.org/CodeSystem/condition-clinical"}]},"code":{"text":"Asthma"},"onsetDateTime":"2001-05-01","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"Condition","id":"condition-error","code":{"text":"Wrong diagnosis"},"subject":{"reference":"Patient/patient-maya"},"verificationStatus":{"coding":[{"code":"entered-in-error","system":"http://terminology.hl7.org/CodeSystem/condition-ver-status"}]}}
{"resourceType":"Condition","id":"condition-no-status","code":{"text":"Hypothyroidism"},"recordedDate":"2019-07-22","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"Condition","id":"condition-resolved","clinicalStatus":{"coding":[{"code":"resolved","system":"http://terminology.hl7.org/CodeSystem/condition-clinical"}]},"code":{"text":"Sinusitis"},"onsetDateTime":"2023-02-10","subject":{"reference":"Patient/patient-maya"}}
```

`tests/fixtures/sample-vault/records/DiagnosticReport.ndjson`:

```json
{"resourceType":"DiagnosticReport","id":"report-2025","category":[{"coding":[{"code":"LAB","system":"http://terminology.hl7.org/CodeSystem/v2-0074"}]}],"code":{"text":"Glucose test"},"effectiveDateTime":"2025-03-11","result":[{"reference":"Observation/lab-glucose-2025"}],"status":"final","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"DiagnosticReport","id":"report-2026","code":{"text":"Metabolic panel"},"effectiveDateTime":"2026-10-14","result":[{"reference":"Observation/lab-glucose-2026"},{"reference":"Observation/lab-creatinine-2026"},{"reference":"Observation/lab-crp-2026"}],"status":"final","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"DiagnosticReport","id":"report-imaging","category":[{"coding":[{"code":"RAD","system":"http://terminology.hl7.org/CodeSystem/v2-0074"}]}],"code":{"text":"Chest X-ray"},"effectiveDateTime":"2024-01-15","status":"final","subject":{"reference":"Patient/patient-maya"}}
```

`tests/fixtures/sample-vault/records/DocumentReference.ndjson`:

```json
{"resourceType":"DocumentReference","id":"document-lab","category":[{"coding":[{"code":"lab","system":"urn:phv:document-category"}]}],"content":[{"attachment":{"contentType":"application/pdf","title":"metabolic-panel.pdf","url":"sources/1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412.pdf"}}],"date":"2026-10-15T09:00:00Z","identifier":[{"system":"urn:phv:sha256","value":"1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412"}],"status":"current","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"DocumentReference","id":"document-no-file","content":[{"attachment":{"contentType":"text/plain","title":"Hospital note","url":"https://hospital.example/notes/1"}}],"status":"current","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"DocumentReference","id":"document-receipt","category":[{"coding":[{"code":"receipt","system":"urn:phv:document-category"}]}],"content":[{"attachment":{"contentType":"text/plain","title":"pharmacy-receipt.txt","url":"sources/82f7258fe85a893e067ec1c27ac161dafd511455dce49426a6d973d4e5ce4479.txt"}}],"date":"2026-10-02T17:30:00Z","identifier":[{"system":"urn:phv:sha256","value":"82f7258fe85a893e067ec1c27ac161dafd511455dce49426a6d973d4e5ce4479"}],"status":"current","subject":{"reference":"Patient/patient-maya"}}
```

`tests/fixtures/sample-vault/records/Encounter.ndjson`:

```json
{"resourceType":"Encounter","id":"encounter-1","class":{"code":"AMB","system":"http://terminology.hl7.org/CodeSystem/v3-ActCode"},"status":"finished","subject":{"reference":"Patient/patient-maya"}}
```

`tests/fixtures/sample-vault/records/Immunization.ndjson`:

```json
{"resourceType":"Immunization","id":"vaccine-flu","occurrenceDateTime":"2026-09-02","patient":{"reference":"Patient/patient-maya"},"status":"completed","vaccineCode":{"text":"Influenza"}}
{"resourceType":"Immunization","id":"vaccine-mmr-not-given","occurrenceString":"Spring 2020","patient":{"reference":"Patient/patient-maya"},"status":"not-done","vaccineCode":{"text":"MMR"}}
```

`tests/fixtures/sample-vault/records/Medication.ndjson`:

```json
{"resourceType":"Medication","id":"medication-amoxicillin","code":{"text":"Amoxicillin 500 mg"}}
```

`tests/fixtures/sample-vault/records/MedicationRequest.ndjson`:

```json
{"resourceType":"MedicationRequest","id":"request-amoxicillin","authoredOn":"2023-02-10","dosageInstruction":[{"text":"Three times a day for 7 days"}],"intent":"order","medicationReference":{"reference":"Medication/medication-amoxicillin"},"status":"completed","subject":{"reference":"Patient/patient-maya"}}
```

`tests/fixtures/sample-vault/records/MedicationStatement.ndjson`:

```json
{"resourceType":"MedicationStatement","id":"statement-levothyroxine","dosage":[{"text":"One tablet each morning"}],"effectiveDateTime":"2019-08-01","medicationCodeableConcept":{"text":"Levothyroxine 50 mcg"},"status":"active","subject":{"reference":"Patient/patient-maya"}}
```

`tests/fixtures/sample-vault/records/Observation.ndjson`:

```json
{"resourceType":"Observation","id":"lab-creatinine-2026","category":[{"coding":[{"code":"laboratory","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"2160-0","system":"http://loinc.org"}],"text":"Creatinine"},"effectiveDateTime":"2026-10-14","interpretation":[{"text":"H"}],"referenceRange":[{"high":{"unit":"mg/dL","value":1.30},"low":{"unit":"mg/dL","value":0.70}}],"status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"mg/dL","value":1.50}}
{"resourceType":"Observation","id":"lab-crp-2026","category":[{"coding":[{"code":"laboratory","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"text":"CRP"},"effectiveDateTime":"2026-10-14","status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"comparator":"<","unit":"mg/L","value":0.5}}
{"resourceType":"Observation","id":"lab-glucose-2025","category":[{"coding":[{"code":"laboratory","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"2345-7","system":"http://loinc.org"}],"text":"Blood sugar"},"effectiveDateTime":"2025-03-11","interpretation":[{"text":"H"}],"status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"mg/dL","value":101}}
{"resourceType":"Observation","id":"lab-glucose-2026","category":[{"coding":[{"code":"laboratory","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"2345-7","system":"http://loinc.org"}],"text":"Glucose"},"effectiveDateTime":"2026-10-14","referenceRange":[{"text":"70 - 99 mg/dL"}],"status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"mg/dL","value":92}}
{"resourceType":"Observation","id":"lab-vitamin-d-loose","category":[{"coding":[{"code":"laboratory","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"text":"Vitamin D/25-OH"},"effectiveDateTime":"2026-05-20","status":"final","subject":{"reference":"Patient/patient-maya"},"valueString":"Sufficient"}
{"resourceType":"Observation","id":"obs-no-category","code":{"text":"Mood score"},"effectiveDateTime":"2026-06-01","status":"final","subject":{"reference":"Patient/patient-maya"},"valueInteger":7}
{"resourceType":"Observation","id":"vital-bp-2025","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"85354-9","display":"Blood pressure panel","system":"http://loinc.org"}],"text":"Blood pressure"},"component":[{"code":{"coding":[{"code":"8480-6","system":"http://loinc.org"}]},"valueQuantity":{"unit":"mmHg","value":121}},{"code":{"coding":[{"code":"8462-4","system":"http://loinc.org"}]},"valueQuantity":{"unit":"mmHg","value":79}}],"effectiveDateTime":"2025-03-11","status":"final","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"Observation","id":"vital-bp-2026","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"85354-9","display":"Blood pressure panel","system":"http://loinc.org"}],"text":"Blood pressure"},"component":[{"code":{"coding":[{"code":"8480-6","system":"http://loinc.org"}]},"valueQuantity":{"unit":"mmHg","value":118}},{"code":{"coding":[{"code":"8462-4","system":"http://loinc.org"}]},"valueQuantity":{"unit":"mmHg","value":76}}],"effectiveDateTime":"2026-09-02","status":"final","subject":{"reference":"Patient/patient-maya"}}
{"resourceType":"Observation","id":"vital-height-2025","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"8302-2","system":"http://loinc.org"}],"text":"Height"},"effectiveDateTime":"2025-03-11","status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"cm","value":168}}
{"resourceType":"Observation","id":"vital-hr-2026","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"8867-4","system":"http://loinc.org"}],"text":"Heart rate"},"effectiveDateTime":"2026-09-02","status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"/min","value":62}}
{"resourceType":"Observation","id":"vital-temp-undated","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"8310-5","system":"http://loinc.org"}],"text":"Temperature"},"status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"Cel","value":36.60}}
{"resourceType":"Observation","id":"vital-weight-2025","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"29463-7","system":"http://loinc.org"}],"text":"Body weight"},"effectiveDateTime":"2025-03-11","status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"kg","value":72.50}}
{"resourceType":"Observation","id":"vital-weight-2026","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"29463-7","system":"http://loinc.org"}],"text":"Body weight"},"effectiveDateTime":"2026-09-02","status":"final","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"kg","value":71.2}}
{"resourceType":"Observation","id":"vital-weight-error","category":[{"coding":[{"code":"vital-signs","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"29463-7","system":"http://loinc.org"}],"text":"Body weight"},"effectiveDateTime":"2026-10-01","status":"entered-in-error","subject":{"reference":"Patient/patient-maya"},"valueQuantity":{"unit":"kg","value":700}}
```

`tests/fixtures/sample-vault/records/Patient.ndjson`:

```json
{"resourceType":"Patient","id":"patient-maya","birthDate":"1988-03-14","gender":"female","name":[{"family":"Example","given":["Maya"]}]}
```

`tests/fixtures/sample-vault/records/Provenance.ndjson`:

```json
{"resourceType":"Provenance","id":"provenance-lab","agent":[{"who":{"display":"phv-add-lab"}}],"entity":[{"role":"source","what":{"reference":"DocumentReference/document-lab"}}],"recorded":"2026-10-15T09:01:00Z","target":[{"reference":"DiagnosticReport/report-2026"},{"reference":"Observation/lab-creatinine-2026"},{"reference":"Observation/lab-crp-2026"},{"reference":"Observation/lab-glucose-2026"}]}
```


- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --test browser_fixture`

Expected: 3 tests pass.

- [ ] **Step 8: Commit**

```bash
git add tests/browser_common tests/browser_fixture.rs tests/fixtures/sample-vault
git commit -m "test: add a synthetic sample vault for the browser"
```


### Task 3: Reading FHIR values for display

These helpers hold the rule that matters most: show what the record says, exactly as stored. `arbitrary_precision` in `serde_json` is what keeps `1.50` as `1.50`.

**Files:**

- Create: `src/browser/mod.rs`
- Create: `src/browser/model/mod.rs`
- Create: `src/browser/model/value.rs`
- Create: `src/browser/model/observation.rs`
- Modify: `src/lib.rs`
- Test: `tests/browser_values.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_values.rs`**

```rust
use phv::browser::model::value::{
    Dated, concept_name, encode_segment, friendly_date, newest_first, quantity_text, result_lines,
};
use serde_json::{Value, json};

fn parsed(text: &str) -> Value {
    phv::canon::parse_json(text).unwrap()
}

#[test]
fn numbers_keep_their_written_form() {
    let quantity = parsed(r#"{"value":1.50,"unit":"mg/dL"}"#);
    assert_eq!(quantity_text(&quantity).unwrap(), "1.50 mg/dL");
}

#[test]
fn comparators_are_kept() {
    let quantity = parsed(r#"{"value":0.5,"comparator":"<","unit":"mg/L"}"#);
    assert_eq!(quantity_text(&quantity).unwrap(), "<0.5 mg/L");
}

#[test]
fn a_quantity_without_a_value_has_no_text() {
    assert_eq!(quantity_text(&json!({"unit": "kg"})), None);
}

#[test]
fn names_prefer_text_then_display_then_code() {
    assert_eq!(
        concept_name(&json!({"text": "T", "coding": [{"display": "D", "code": "C"}]})).unwrap(),
        "T"
    );
    assert_eq!(
        concept_name(&json!({"coding": [{"display": "D", "code": "C"}]})).unwrap(),
        "D"
    );
    assert_eq!(
        concept_name(&json!({"coding": [{"code": "C"}]})).unwrap(),
        "C"
    );
    assert_eq!(concept_name(&json!({})), None);
}

#[test]
fn blood_pressure_with_both_components_is_one_line() {
    let observation = parsed(
        r#"{"component":[
        {"code":{"coding":[{"system":"http://loinc.org","code":"8480-6"}]},"valueQuantity":{"value":118,"unit":"mmHg"}},
        {"code":{"coding":[{"system":"http://loinc.org","code":"8462-4"}]},"valueQuantity":{"value":76,"unit":"mmHg"}}
    ]}"#,
    );
    assert_eq!(result_lines(&observation), ["118/76 mmHg"]);
}

#[test]
fn other_components_get_a_line_each() {
    let observation = json!({"component": [
        {"code": {"text": "Left"}, "valueString": "Clear"},
        {"code": {"text": "Right"}}
    ]});
    assert_eq!(
        result_lines(&observation),
        ["Left: Clear", "Right: See FHIR"]
    );
}

#[test]
fn an_unknown_value_type_gives_no_lines() {
    assert!(result_lines(&json!({"valueSampledData": {}})).is_empty());
}

#[test]
fn dates_stay_as_written_with_no_zone_conversion() {
    assert_eq!(friendly_date("2026-10-04"), "4 Oct 2026");
    assert_eq!(friendly_date("2026-10"), "Oct 2026");
    assert_eq!(friendly_date("2026"), "2026");
    assert_eq!(
        friendly_date("2026-10-04T23:30:00-04:00"),
        "4 Oct 2026 23:30:00-04:00"
    );
    assert_eq!(friendly_date("soon"), "soon");
    assert_eq!(friendly_date("2026-13-01"), "2026-13-01");
}

struct Row(Option<&'static str>, &'static str);

impl Dated for Row {
    fn sort_date(&self) -> Option<&str> {
        self.0
    }
    fn sort_id(&self) -> &str {
        self.1
    }
}

#[test]
fn newest_comes_first_and_undated_last() {
    let mut rows = [
        Row(None, "z"),
        Row(Some("2025-01-01"), "b"),
        Row(Some("2026-01-01"), "c"),
        Row(Some("2025-01-01"), "a"),
    ];
    newest_first(&mut rows);
    let ids: Vec<&str> = rows.iter().map(|r| r.1).collect();
    assert_eq!(ids, ["c", "a", "b", "z"]);
}

#[test]
fn segments_are_percent_encoded() {
    assert_eq!(encode_segment("text:A/B c"), "text%3AA%2FB%20c");
    assert_eq!(encode_segment("a-b.c_d~1"), "a-b.c_d~1");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_values`

Expected: compile error, because `phv::browser` does not exist.

- [ ] **Step 3: Create `src/browser/mod.rs`**

```rust
//! A local, read-only web browser for vault records. See
//! `docs/superpowers/specs/2026-11-06-fhir-browser-design.md`.

pub mod model;
```

- [ ] **Step 4: Create `src/browser/model/mod.rs`**

```rust
//! Turns raw vault lines into plain view data. No HTTP or HTML in here.

pub mod observation;
pub mod value;

use crate::canon::parse_json;
use crate::error::PhvError;
use crate::store::Vault;
use serde_json::Value;
use std::collections::HashMap;

/// Reads resources for one request and counts lines it could not read.
pub struct Records {
    vault: Vault,
    unreadable: HashMap<String, usize>,
}

impl Records {
    pub fn new(vault: Vault) -> Self {
        Self {
            vault,
            unreadable: HashMap::new(),
        }
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// Every readable resource of a type, in file order.
    pub fn of_type(&mut self, resource_type: &str) -> Result<Vec<Value>, PhvError> {
        let mut resources = Vec::new();
        let mut bad = 0;
        for line in self.vault.lines(resource_type)? {
            match parse_resource(&line, resource_type) {
                Some(resource) => resources.push(resource),
                None => bad += 1,
            }
        }
        self.unreadable.insert(resource_type.to_string(), bad);
        Ok(resources)
    }

    /// Lines that could not be read, across every type loaded so far.
    pub fn unreadable(&self) -> usize {
        self.unreadable.values().sum()
    }
}

/// A line counts as readable only if it is valid JSON with the expected type and an `id`.
pub fn parse_resource(line: &str, resource_type: &str) -> Option<Value> {
    let resource = parse_json(line).ok()?;
    (resource["resourceType"] == resource_type && resource["id"].is_string()).then_some(resource)
}
```

- [ ] **Step 5: Create `src/browser/model/value.rs`**

```rust
//! Reading FHIR values for display, always as stored.

use serde_json::Value;
use std::cmp::Ordering;

pub const OBSERVATION_CATEGORY: &str = "http://terminology.hl7.org/CodeSystem/observation-category";
pub const DIAGNOSTIC_SERVICE: &str = "http://terminology.hl7.org/CodeSystem/v2-0074";
pub const LOINC: &str = "http://loinc.org";
const SYSTOLIC: &str = "8480-6";
const DIASTOLIC: &str = "8462-4";

pub fn id_of(resource: &Value) -> String {
    resource["id"].as_str().unwrap_or("").to_string()
}

/// `text`, then the first coding's `display`, then its `code`.
pub fn concept_name(concept: &Value) -> Option<String> {
    let coding = &concept["coding"][0];
    [&concept["text"], &coding["display"], &coding["code"]]
        .into_iter()
        .find_map(|v| v.as_str())
        .map(str::to_string)
}

/// Whether a CodeableConcept, or an array of them, has this code in this system.
pub fn has_code(concepts: &Value, system: &str, code: &str) -> bool {
    let matches = |concept: &Value| {
        concept["coding"].as_array().is_some_and(|codings| {
            codings
                .iter()
                .any(|c| c["system"] == system && c["code"] == code)
        })
    };
    match concepts {
        Value::Array(items) => items.iter().any(matches),
        other => matches(other),
    }
}

pub fn code_in_system(concept: &Value, system: &str) -> Option<String> {
    concept["coding"]
        .as_array()?
        .iter()
        .find(|c| c["system"] == system)
        .and_then(|c| c["code"].as_str())
        .map(str::to_string)
}

/// The number exactly as written, so `1.50` stays `1.50`.
pub fn number_text(value: &Value) -> Option<String> {
    value.as_number().map(ToString::to_string)
}

/// Comparator, value and unit as stored, such as `<0.5 mg/dL`.
pub fn quantity_text(quantity: &Value) -> Option<String> {
    let number = number_text(&quantity["value"])?;
    let comparator = quantity["comparator"].as_str().unwrap_or("");
    let unit = quantity["unit"].as_str().or(quantity["code"].as_str());
    Some(match unit {
        Some(unit) => format!("{comparator}{number} {unit}"),
        None => format!("{comparator}{number}"),
    })
}

/// The text of a resource's or component's `value[x]`, for the types phv knows how to show.
pub fn value_text(holder: &Value) -> Option<String> {
    if let Some(text) = quantity_text(&holder["valueQuantity"]) {
        return Some(text);
    }
    if let Some(name) = concept_name(&holder["valueCodeableConcept"]) {
        return Some(name);
    }
    for key in ["valueString", "valueDateTime", "valueTime"] {
        if let Some(text) = holder[key].as_str() {
            return Some(text.to_string());
        }
    }
    if let Some(flag) = holder["valueBoolean"].as_bool() {
        return Some(if flag { "Yes" } else { "No" }.to_string());
    }
    number_text(&holder["valueInteger"])
}

fn blood_pressure(components: &[Value]) -> Option<String> {
    let find = |code: &str| {
        components
            .iter()
            .find(|c| has_code(&c["code"], LOINC, code))
            .map(|c| &c["valueQuantity"])
    };
    let systolic = find(SYSTOLIC)?;
    let diastolic = find(DIASTOLIC)?;
    let top = number_text(&systolic["value"])?;
    let bottom = number_text(&diastolic["value"])?;
    Some(match systolic["unit"].as_str() {
        Some(unit) => format!("{top}/{bottom} {unit}"),
        None => format!("{top}/{bottom}"),
    })
}

/// What an Observation says, one line per value. Empty means "See FHIR".
pub fn result_lines(observation: &Value) -> Vec<String> {
    let mut lines: Vec<String> = value_text(observation).into_iter().collect();
    let Some(components) = observation["component"].as_array() else {
        return lines;
    };
    if let Some(pressure) = blood_pressure(components) {
        lines.push(pressure);
        return lines;
    }
    for component in components {
        let name = concept_name(&component["code"]).unwrap_or_else(|| "Unnamed".into());
        let value = value_text(component).unwrap_or_else(|| "See FHIR".into());
        lines.push(format!("{name}: {value}"));
    }
    lines
}

/// `2026-10-14` as `14 Oct 2026`. A time stays as written, with no time zone conversion.
pub fn friendly_date(written: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (date, time) = match written.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (written, None),
    };
    let parts: Vec<&str> = date.split('-').collect();
    let month = |text: &str| {
        text.parse::<usize>()
            .ok()
            .and_then(|m| m.checked_sub(1))
            .and_then(|m| MONTHS.get(m))
    };
    let friendly = match parts.as_slice() {
        [year] if year.len() == 4 => return written.to_string(),
        [year, m] if month(m).is_some() => format!("{} {year}", month(m).unwrap()),
        [year, m, day] if month(m).is_some() => {
            format!(
                "{} {} {year}",
                day.trim_start_matches('0'),
                month(m).unwrap()
            )
        }
        _ => return written.to_string(),
    };
    match time {
        Some(time) => format!("{friendly} {time}"),
        None => friendly,
    }
}

pub fn date_or_none(date: Option<&str>) -> String {
    date.map_or_else(|| "No date".to_string(), friendly_date)
}

/// Rows that sort newest first, with undated rows last and `id` breaking ties.
pub trait Dated {
    fn sort_date(&self) -> Option<&str>;
    fn sort_id(&self) -> &str;
}

/// Sorts by the date's text. Times on one day with different offsets may be out of order.
pub fn newest_first<T: Dated>(rows: &mut [T]) {
    rows.sort_by(|a, b| {
        let by_date = match (a.sort_date(), b.sort_date()) {
            (Some(x), Some(y)) => y.cmp(x),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        by_date.then_with(|| a.sort_id().cmp(b.sort_id()))
    });
}

/// Percent-encodes a value for one URL path or query segment.
pub fn encode_segment(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// First letter uppercase, for showing a status code such as `active`.
pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
```

- [ ] **Step 6: Create `src/browser/model/observation.rs`**

```rust
//! One Observation as a table row, shared by vitals and labs.

use super::value::{
    Dated, LOINC, code_in_system, concept_name, date_or_none, encode_segment, id_of, number_text,
    result_lines,
};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsRow {
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub name: String,
    pub key: String,
    pub key_url: String,
    pub lines: Vec<String>,
    pub flag: Option<String>,
    pub range: Option<String>,
}

impl Dated for ObsRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

pub fn is_entered_in_error(resource: &Value) -> bool {
    resource["status"] == "entered-in-error"
}

/// The clinically relevant date of an Observation or DiagnosticReport.
pub fn clinical_date(resource: &Value) -> Option<String> {
    [
        &resource["effectiveDateTime"],
        &resource["effectiveInstant"],
        &resource["effectivePeriod"]["start"],
        &resource["issued"],
    ]
    .into_iter()
    .find_map(|v| v.as_str())
    .map(str::to_string)
}

/// Groups by LOINC code, otherwise by exact text. Different names are never merged.
pub fn group_key(observation: &Value) -> String {
    if let Some(code) = code_in_system(&observation["code"], LOINC) {
        return format!("loinc:{code}");
    }
    match concept_name(&observation["code"]) {
        Some(name) => format!("text:{name}"),
        None => format!("id:{}", id_of(observation)),
    }
}

fn range_text(observation: &Value) -> Option<String> {
    let range = &observation["referenceRange"][0];
    if let Some(text) = range["text"].as_str() {
        return Some(text.to_string());
    }
    match (
        number_text(&range["low"]["value"]),
        number_text(&range["high"]["value"]),
    ) {
        (Some(low), Some(high)) => Some(format!("{low}–{high}")),
        (Some(low), None) => Some(format!("≥{low}")),
        (None, Some(high)) => Some(format!("≤{high}")),
        (None, None) => None,
    }
}

pub fn obs_row(observation: &Value) -> ObsRow {
    let raw_date = clinical_date(observation);
    let key = group_key(observation);
    ObsRow {
        id: id_of(observation),
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        name: concept_name(&observation["code"]).unwrap_or_else(|| "Unnamed".into()),
        key_url: encode_segment(&key),
        key,
        lines: result_lines(observation),
        flag: concept_name(&observation["interpretation"][0]),
        range: range_text(observation),
    }
}
```

- [ ] **Step 7: Make the edits**

In `src/lib.rs`, replace:

```rust
pub mod canon;
```

with:

```rust
pub mod browser;
pub mod canon;
```


- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --test browser_values`

Expected: 10 tests pass.

- [ ] **Step 9: Commit**

```bash
git add src tests/browser_values.rs
git commit -m "feat: read FHIR values for display exactly as stored"
```


### Task 4: Server foundation and All FHIR

This is useful alone: every record in a vault can now be browsed. It adds the vault list, the local-only guard, security headers, the layout and the All FHIR pages. Until later tasks land, the other navigation links return a 404 page and `/` redirects to an Overview page that does not exist yet.

**Files:**

- Create: `src/browser/mod.rs`
- Create: `src/browser/vaults.rs`
- Create: `src/browser/guard.rs`
- Create: `src/browser/assets.rs`
- Create: `src/browser/routes/mod.rs`
- Create: `src/browser/routes/explorer.rs`
- Create: `src/browser/model/explorer.rs`
- Create: `templates/base.html`
- Create: `templates/error.html`
- Create: `templates/fhir_types.html`
- Create: `templates/fhir_list.html`
- Create: `templates/fhir_record.html`
- Create: `examples/browser.rs`
- Modify: `src/browser/model/mod.rs`
- Replace: `tests/browser_common/mod.rs`
- Test: `tests/browser_explorer.rs`

- [ ] **Step 1: Replace the shared test helpers with the HTTP version**

```rust
#![allow(dead_code)]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use phv::browser::{Vaults, router};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-vault")
}

/// Every resource of a type in the sample vault.
pub fn fixture(resource_type: &str) -> Vec<Value> {
    let path = fixture_path().join(format!("records/{resource_type}.ndjson"));
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| phv::canon::parse_json(line).unwrap())
        .collect()
}

pub fn app() -> Router {
    router(Vaults::single(fixture_path()))
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .map_or("", |value| value.to_str().unwrap())
    }
}

pub async fn send(app: &Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8747");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
    }
}

pub async fn get(app: &Router, uri: &str) -> Reply {
    send(app, "GET", uri, &[]).await
}

/// A private copy of the sample vault that a test can change.
pub fn copy_fixture(to: &Path) {
    copy_dir(&fixture_path(), to);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
```

- [ ] **Step 2: Write the failing tests in `tests/browser_explorer.rs`**

```rust
//! The server's foundations and the All FHIR pages.

mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture_path, get, send};
use phv::browser::model::explorer::{page_count, page_of};
use phv::browser::{Vaults, router};
use std::fs;
use std::path::Path;

const CONFIG: &str = r#"
default = "maya"
[vaults]
maya = "/nowhere/maya"
ben = "/nowhere/ben"
"#;

#[test]
fn a_registered_name_opens_the_whole_registry_on_that_vault() {
    let vaults = Vaults::resolve(Some(Path::new("ben")), None, Some(CONFIG)).unwrap();
    assert_eq!(vaults.default_name(), "ben");
    assert_eq!(vaults.entries().len(), 2);
}

#[test]
fn a_path_is_the_only_entry() {
    let vaults = Vaults::resolve(None, Some("/some/path"), Some(CONFIG)).unwrap();
    assert_eq!(vaults.entries().len(), 1);
    assert_eq!(vaults.entries()[0].path, Path::new("/some/path"));
}

#[test]
fn no_flag_uses_the_registry_default() {
    let vaults = Vaults::resolve(None, None, Some(CONFIG)).unwrap();
    assert_eq!(vaults.default_name(), "maya");
}

#[test]
fn no_vault_at_all_is_an_error() {
    assert!(Vaults::resolve(None, None, None).is_err());
}

#[tokio::test]
async fn home_redirects_to_the_default_vault() {
    let reply = get(&app(), "/").await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header("location"), "/v/vault/overview");
}

#[tokio::test]
async fn pages_send_the_security_headers() {
    let reply = get(&app(), "/v/vault/fhir").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply
            .header("content-security-policy")
            .contains("default-src 'none'")
    );
    assert!(
        reply
            .header("content-security-policy")
            .contains("script-src 'self'")
    );
    assert_eq!(reply.header("cache-control"), "no-store");
    assert_eq!(reply.header("x-content-type-options"), "nosniff");
    assert_eq!(reply.header("referrer-policy"), "no-referrer");
    assert_eq!(reply.header("vary"), "HX-Request");
}

#[tokio::test]
async fn assets_are_served_and_unknown_ones_are_not() {
    let app = app();
    let htmx = get(&app, "/assets/htmx.min.js").await;
    assert_eq!(htmx.status, StatusCode::OK);
    assert!(htmx.header("content-type").starts_with("text/javascript"));
    assert_eq!(get(&app, "/assets/style.css").await.status, StatusCode::OK);
    assert_eq!(
        get(&app, "/assets/secrets").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn htmx_is_configured_to_work_under_the_policy_and_keep_no_copies() {
    let html = get(&app(), "/v/vault/fhir").await.text();
    assert!(html.contains(r#""allowEval":false"#));
    assert!(html.contains(r#""selfRequestsOnly":true"#));
    assert!(html.contains(r#""includeIndicatorStyles":false"#));
    assert!(html.contains(r#""historyCacheSize":0"#));
}

#[tokio::test]
async fn other_hosts_and_origins_are_rejected() {
    let app = app();
    let cases: [&[(&str, &str)]; 3] = [
        &[("host", "evil.example")],
        &[("origin", "https://evil.example")],
        &[("origin", "null")],
    ];
    for headers in cases {
        let mut request = axum::http::Request::builder().uri("/v/vault/fhir");
        if !headers.iter().any(|(name, _)| *name == "host") {
            request = request.header("host", "localhost:8747");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = tower::ServiceExt::oneshot(
            app.clone(),
            request.body(axum::body::Body::empty()).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{headers:?}");
    }
    let local = send(
        &app,
        "GET",
        "/v/vault/fhir",
        &[("origin", "http://localhost:8747")],
    )
    .await;
    assert_eq!(local.status, StatusCode::OK);
}

#[tokio::test]
async fn nothing_but_reads_is_allowed() {
    let app = app();
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        for uri in [
            "/v/vault/fhir",
            "/v/vault/fhir/Patient/patient-maya",
            "/nowhere",
        ] {
            let reply = send(&app, method, uri, &[]).await;
            assert_eq!(
                reply.status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {uri}"
            );
        }
    }
}

#[tokio::test]
async fn all_fhir_lists_types_with_counts() {
    let html = get(&app(), "/v/vault/fhir").await.text();
    assert!(html.contains(">Encounter</a>"));
    assert!(html.contains(">Observation</a>"));
}

#[tokio::test]
async fn a_type_lists_its_records() {
    let html = get(&app(), "/v/vault/fhir/Condition").await.text();
    assert!(html.contains("Asthma"));
    assert!(html.contains("condition-asthma"));
}

#[tokio::test]
async fn a_record_shows_its_json_exactly_and_links_references_that_resolve() {
    let html = get(&app(), "/v/vault/fhir/Observation/lab-creatinine-2026")
        .await
        .text();
    assert!(html.contains("1.50"), "{html}");
    assert!(
        html.contains(r#"href="/v/vault/fhir/Patient/patient-maya""#),
        "{html}"
    );
}

#[tokio::test]
async fn a_reference_that_does_not_resolve_is_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    fs::write(
        dir.path().join("records/Encounter.ndjson"),
        r#"{"resourceType":"Encounter","id":"e","subject":{"reference":"Patient/gone"}}
"#,
    )
    .unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    let html = get(&app, "/v/vault/fhir/Encounter/e").await.text();
    assert!(html.contains("Patient/gone"));
    assert!(!html.contains("fhir/Patient/gone"));
}

#[tokio::test]
async fn unknown_vaults_types_and_ids_are_404() {
    let app = app();
    for uri in [
        "/v/nobody/fhir",
        "/v/vault/fhir/Nope",
        "/v/vault/fhir/..%2F..%2Fphv",
        "/v/vault/fhir/Patient/nobody",
        "/nowhere",
    ] {
        assert_eq!(get(&app, uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn record_text_is_escaped() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    fs::write(
        dir.path().join("records/Encounter.ndjson"),
        r#"{"resourceType":"Encounter","id":"e","type":[],"code":{"text":"<script>alert(1)</script>"}}
"#,
    )
    .unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    for uri in ["/v/vault/fhir/Encounter", "/v/vault/fhir/Encounter/e"] {
        let html = get(&app, uri).await.text();
        assert!(!html.contains("<script>alert"), "{uri}");
        assert!(html.contains("alert(1)"), "{uri}");
    }
}

#[tokio::test]
async fn unreadable_lines_are_counted_and_the_rest_still_show() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let path = dir.path().join("records/Condition.ndjson");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("{not json\n");
    fs::write(&path, text).unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    let reply = get(&app, "/v/vault/fhir/Condition").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply.text().contains("1 records couldn&#39;t be read")
            || reply.text().contains("1 records couldn't be read")
    );
    assert!(reply.text().contains("Asthma"));
}

#[tokio::test]
async fn an_unavailable_vault_explains_why_and_others_keep_working() {
    let config = format!(
        "default = \"maya\"\n[vaults]\nmaya = {:?}\ngone = \"/nowhere/gone\"\n",
        fixture_path()
    );
    let app = router(Vaults::resolve(None, None, Some(&config)).unwrap());
    let good = get(&app, "/v/maya/fhir").await;
    assert_eq!(good.status, StatusCode::OK);
    assert!(
        good.text().contains("(unavailable)"),
        "the switcher marks it"
    );
    let bad = get(&app, "/v/gone/fhir").await;
    assert_eq!(bad.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(bad.text().contains("not a phv vault"));
    assert!(bad.text().contains(r#"href="/v/maya/overview""#));
}

#[tokio::test]
async fn the_switcher_is_hidden_with_one_vault() {
    assert!(
        !get(&app(), "/v/vault/fhir")
            .await
            .text()
            .contains("switcher")
    );
}

#[test]
fn pages_hold_two_hundred_records() {
    let items: Vec<usize> = (0..450).collect();
    assert_eq!(page_of(&items, 1).len(), 200);
    assert_eq!(page_of(&items, 3), (400..450).collect::<Vec<_>>());
    assert!(page_of(&items, 4).is_empty());
    assert_eq!(page_count(450), 3);
    assert_eq!(page_count(0), 1);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --test browser_explorer`

Expected: compile error, because `phv::browser::router` does not exist.

- [ ] **Step 4: Create `src/browser/mod.rs`**

```rust
//! A local, read-only web browser for vault records. See
//! `docs/superpowers/specs/2026-11-06-fhir-browser-design.md`.

mod assets;
mod guard;
pub mod model;
mod routes;
mod vaults;

pub use routes::router;
pub use vaults::{VaultEntry, Vaults};
```

- [ ] **Step 5: Create `src/browser/vaults.rs`**

```rust
//! Which vaults this server shows. Paths never come from a URL.

use crate::error::PhvError;
use crate::store::Registry;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vaults {
    entries: Vec<VaultEntry>,
    default: String,
}

impl Vaults {
    /// Precedence matches `resolve_vault_path`. A given name opens the whole registry on that
    /// vault. A given path that is not a registered name is the only entry.
    pub fn resolve(
        flag: Option<&Path>,
        env: Option<&str>,
        config_text: Option<&str>,
    ) -> Result<Self, PhvError> {
        let registry = config_text
            .map(Registry::parse)
            .transpose()?
            .unwrap_or_default();
        let given = flag
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| env.filter(|e| !e.is_empty()).map(PathBuf::from));
        let entries: Vec<VaultEntry> = registry
            .vaults
            .iter()
            .map(|(name, path)| VaultEntry {
                name: name.clone(),
                path: path.clone(),
            })
            .collect();
        if let Some(given) = given {
            if let Some(name) = given.to_str().filter(|n| registry.vaults.contains_key(*n)) {
                return Ok(Self {
                    default: name.to_string(),
                    entries,
                });
            }
            return Ok(Self::single(given));
        }
        match registry.default {
            Some(default) if registry.vaults.contains_key(&default) => {
                Ok(Self { entries, default })
            }
            _ => Err(PhvError::Usage(
                "no vault given: run phv setup, use --vault, or set PHV_VAULT".into(),
            )),
        }
    }

    pub fn single(path: PathBuf) -> Self {
        let name = "vault".to_string();
        Self {
            entries: vec![VaultEntry {
                name: name.clone(),
                path,
            }],
            default: name,
        }
    }

    pub fn entries(&self) -> &[VaultEntry] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&VaultEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn default_name(&self) -> &str {
        &self.default
    }
}
```

- [ ] **Step 6: Create `src/browser/guard.rs`**

```rust
//! Keeps other web pages away from this server, and sets the headers on every response.

use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

const PAGE_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'";

fn is_local_host(authority: &str) -> bool {
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit()))
        .map_or(authority, |(host, _)| host);
    host == "127.0.0.1" || host == "localhost"
}

fn is_local_origin(origin: &str) -> bool {
    origin.strip_prefix("http://").is_some_and(is_local_host)
}

/// Rejects a `Host` that is not local, which stops DNS rebinding, and an `Origin` from elsewhere.
pub async fn local_only(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(is_local_host);
    let origin_ok = headers
        .get(header::ORIGIN)
        .is_none_or(|o| o.to_str().is_ok_and(is_local_origin));
    if !host_ok || !origin_ok {
        return (
            StatusCode::FORBIDDEN,
            "this server only answers local requests",
        )
            .into_response();
    }
    next.run(request).await
}

/// Adds any header a handler has not set itself. Original files set their own policy.
pub async fn headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let defaults = [
        (header::CONTENT_SECURITY_POLICY, PAGE_POLICY),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::VARY, "HX-Request"),
    ];
    for (name, value) in defaults {
        headers
            .entry(name)
            .or_insert(HeaderValue::from_static(value));
    }
    response
}
```

- [ ] **Step 7: Create `src/browser/assets.rs`**

```rust
//! The stylesheet and htmx, embedded in the binary.

use axum::{
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const STYLE: &str = include_str!("../../assets/style.css");
const HTMX: &str = include_str!("../../assets/htmx-2.0.11.min.js");

pub async fn asset(Path(file): Path<String>) -> Response {
    let (content_type, body) = match file.as_str() {
        "style.css" => ("text/css; charset=utf-8", STYLE),
        "htmx.min.js" => ("text/javascript; charset=utf-8", HTMX),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}
```

- [ ] **Step 8: Create `src/browser/routes/mod.rs`**

```rust
//! Routes, GET only. Each topic has its own file with its templates and handlers.

mod explorer;

use super::guard;
use super::model::Records;
use super::model::explorer::patient_name;
use super::vaults::Vaults;
use crate::error::PhvError;
use crate::store::Vault;
use askama::Template;
use axum::{
    Router,
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct App {
    vaults: Arc<Vaults>,
}

pub struct NavItem {
    pub label: &'static str,
    pub href: String,
    pub on: bool,
}

pub struct SwitcherItem {
    pub name: String,
    pub href: String,
    pub available: bool,
    pub on: bool,
}

/// What every page's layout needs.
pub struct Chrome {
    pub person: String,
    pub vault: String,
    pub nav: Vec<NavItem>,
    pub switcher: Vec<SwitcherItem>,
    pub unreadable: usize,
}

const SECTIONS: [(&str, &str); 9] = [
    ("Overview", "overview"),
    ("Vitals", "vitals"),
    ("Labs", "labs"),
    ("Conditions", "conditions"),
    ("Medicines", "medicines"),
    ("Allergies", "allergies"),
    ("Vaccines", "vaccines"),
    ("Documents", "documents"),
    ("All FHIR", "fhir"),
];

/// One request's view of one vault.
pub struct Ctx {
    pub records: Records,
    pub vault_name: String,
    section: &'static str,
    app: App,
}

impl Ctx {
    /// Build this after loading records, so the unreadable count is complete.
    pub fn chrome(&mut self) -> Result<Chrome, PageError> {
        let patients = self.records.of_type("Patient")?;
        let name = &self.vault_name;
        Ok(Chrome {
            person: patients
                .first()
                .and_then(patient_name)
                .unwrap_or_else(|| name.clone()),
            vault: name.clone(),
            nav: SECTIONS
                .iter()
                .map(|(label, slug)| NavItem {
                    label,
                    href: format!("/v/{name}/{slug}"),
                    on: *slug == self.section,
                })
                .collect(),
            switcher: self.switcher(),
            unreadable: self.records.unreadable(),
        })
    }

    fn switcher(&self) -> Vec<SwitcherItem> {
        let entries = self.app.vaults.entries();
        if entries.len() < 2 {
            return Vec::new();
        }
        entries
            .iter()
            .map(|e| SwitcherItem {
                href: format!("/v/{}/{}", e.name, self.section),
                available: Vault::open(&e.path).is_ok(),
                on: e.name == self.vault_name,
                name: e.name.clone(),
            })
            .collect()
    }
}

#[derive(Debug)]
pub enum PageError {
    NotFound(String),
    Unavailable(String),
    Internal,
}

impl From<PhvError> for PageError {
    fn from(error: PhvError) -> Self {
        // No record contents reach these errors: reads fail on IO, before any parsing.
        eprintln!("phv browser: {error}");
        PageError::Internal
    }
}

pub fn not_found(what: impl Into<String>) -> PageError {
    PageError::NotFound(what.into())
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    title: String,
    message: String,
    vaults: Vec<String>,
}

fn error_response(error: PageError, vaults: &Vaults) -> Response {
    let (status, title, message) = match error {
        PageError::NotFound(what) => (StatusCode::NOT_FOUND, "Not found".to_string(), what),
        PageError::Unavailable(why) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Vault unavailable".to_string(),
            why,
        ),
        PageError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Something went wrong".to_string(),
            "phv could not read this page. The server's log has the details.".to_string(),
        ),
    };
    let page = ErrorPage {
        title,
        message,
        vaults: vaults.entries().iter().map(|e| e.name.clone()).collect(),
    };
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => status.into_response(),
    }
}

/// Opens the vault and runs one page off the async threads, turning errors into pages.
pub async fn run<F>(app: App, vault_name: String, section: &'static str, page: F) -> Response
where
    F: FnOnce(&mut Ctx) -> Result<Response, PageError> + Send + 'static,
{
    let vaults = app.vaults.clone();
    let task = tokio::task::spawn_blocking(move || {
        let entry = app
            .vaults
            .get(&vault_name)
            .ok_or_else(|| not_found(format!("No vault named {vault_name}.")))?;
        let vault = Vault::open(&entry.path).map_err(|e| PageError::Unavailable(e.to_string()))?;
        let mut cx = Ctx {
            records: Records::new(vault),
            vault_name,
            section,
            app: app.clone(),
        };
        page(&mut cx)
    });
    let result = task.await.unwrap_or(Err(PageError::Internal));
    result.unwrap_or_else(|error| error_response(error, &vaults))
}

pub fn render<T: Template>(page: T) -> Result<Response, PageError> {
    match page.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(PageError::Internal),
    }
}

async fn home(State(app): State<App>) -> Response {
    Redirect::to(&format!("/v/{}/overview", app.vaults.default_name())).into_response()
}

async fn fallback(State(app): State<App>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    error_response(not_found("There is no such page."), &app.vaults)
}

pub fn router(vaults: Vaults) -> Router {
    let app = App {
        vaults: Arc::new(vaults),
    };
    Router::new()
        .route("/", get(home))
        .route("/assets/{file}", get(super::assets::asset))
        .merge(explorer::routes())
        .fallback(fallback)
        .with_state(app)
        .layer(middleware::from_fn(guard::headers))
        .layer(middleware::from_fn(guard::local_only))
}
```

- [ ] **Step 9: Create `src/browser/routes/explorer.rs`**

```rust
//! All FHIR pages.

use super::{App, Chrome, Ctx, PageError, not_found, render, run};
use crate::browser::model::explorer::{
    RecordRow, RefRow, TypeCount, label_of, page_count, page_of, pretty_json, record_row,
    references,
};
use crate::browser::model::parse_resource;
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::Response,
    routing::get,
};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "fhir_types.html")]
struct TypesPage {
    chrome: Chrome,
    types: Vec<TypeCount>,
}

#[derive(Template)]
#[template(path = "fhir_list.html")]
struct ListPage {
    chrome: Chrome,
    resource_type: String,
    total: usize,
    rows: Vec<RecordRow>,
    page: usize,
    pages: usize,
}

#[derive(Template)]
#[template(path = "fhir_record.html")]
struct RecordPage {
    chrome: Chrome,
    resource_type: String,
    id: String,
    label: String,
    json: String,
    references: Vec<RefRow>,
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<usize>,
}

fn known_type(cx: &Ctx, resource_type: &str) -> Result<(), PageError> {
    if cx
        .records
        .vault()
        .resource_types()?
        .iter()
        .any(|t| t == resource_type)
    {
        Ok(())
    } else {
        Err(not_found(format!(
            "This vault has no {resource_type} records."
        )))
    }
}

async fn types(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "fhir", |cx| {
        let mut types = Vec::new();
        for name in cx.records.vault().resource_types()? {
            let count = cx.records.of_type(&name)?.len();
            types.push(TypeCount { name, count });
        }
        render(TypesPage {
            chrome: cx.chrome()?,
            types,
        })
    })
    .await
}

async fn list(
    State(app): State<App>,
    Path((vault, resource_type)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
) -> Response {
    run(app, vault, "fhir", move |cx| {
        known_type(cx, &resource_type)?;
        let resources = cx.records.of_type(&resource_type)?;
        let rows: Vec<RecordRow> = resources.iter().map(record_row).collect();
        let page = query.page.unwrap_or(1).max(1);
        render(ListPage {
            chrome: cx.chrome()?,
            total: rows.len(),
            pages: page_count(rows.len()),
            rows: page_of(&rows, page),
            page,
            resource_type,
        })
    })
    .await
}

async fn record(
    State(app): State<App>,
    Path((vault, resource_type, id)): Path<(String, String, String)>,
) -> Response {
    run(app, vault, "fhir", move |cx| {
        known_type(cx, &resource_type)?;
        let line = cx
            .records
            .vault()
            .get(&resource_type, &id)?
            .ok_or_else(|| not_found(format!("No {resource_type} with this id.")))?;
        let resource = parse_resource(&line, &resource_type)
            .ok_or_else(|| not_found("This record could not be read. Run phv check."))?;
        let vault_ref = cx.records.vault().clone();
        render(RecordPage {
            chrome: cx.chrome()?,
            label: label_of(&resource),
            json: pretty_json(&resource),
            references: references(&resource, |kind, target| {
                vault_ref.get(kind, target).is_ok_and(|line| line.is_some())
            }),
            resource_type,
            id,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/fhir", get(types))
        .route("/v/{vault}/fhir/{type}", get(list))
        .route("/v/{vault}/fhir/{type}/{id}", get(record))
}
```

- [ ] **Step 10: Create `src/browser/model/explorer.rs`**

```rust
//! All FHIR: every record of every type.

use super::observation::clinical_date;
use super::value::{concept_name, encode_segment, friendly_date, id_of};
use crate::refs::{collect_refs, local_target};
use serde_json::Value;

pub const PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRow {
    pub id: String,
    pub id_url: String,
    pub label: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefRow {
    pub reference: String,
    /// `Type/id` path to link to, when it resolves in this vault.
    pub target: Option<String>,
}

/// A short label for a record of any type, or an empty string.
pub fn label_of(resource: &Value) -> String {
    for key in ["code", "vaccineCode", "medicationCodeableConcept", "type"] {
        if let Some(name) = concept_name(&resource[key]) {
            return name;
        }
    }
    if let Some(title) = resource["content"][0]["attachment"]["title"].as_str() {
        return title.to_string();
    }
    if let Some(name) = patient_name(resource) {
        return name;
    }
    String::new()
}

/// `name[0].text`, otherwise given names followed by the family name.
pub fn patient_name(resource: &Value) -> Option<String> {
    let name = &resource["name"][0];
    if let Some(text) = name["text"].as_str() {
        return Some(text.to_string());
    }
    let mut parts: Vec<&str> = name["given"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str())
        .collect();
    parts.extend(name["family"].as_str());
    (!parts.is_empty()).then(|| parts.join(" "))
}

pub fn record_row(resource: &Value) -> RecordRow {
    let id = id_of(resource);
    let date = clinical_date(resource)
        .or_else(|| resource["date"].as_str().map(str::to_string))
        .or_else(|| resource["recorded"].as_str().map(str::to_string))
        .map(|d| friendly_date(&d))
        .unwrap_or_default();
    RecordRow {
        id_url: encode_segment(&id),
        id,
        label: label_of(resource),
        date,
    }
}

/// Pages count from 1. A page past the end is empty.
pub fn page_of<T: Clone>(items: &[T], page: usize) -> Vec<T> {
    let start = page.saturating_sub(1).saturating_mul(PAGE_SIZE);
    items.iter().skip(start).take(PAGE_SIZE).cloned().collect()
}

pub fn page_count(total: usize) -> usize {
    total.div_ceil(PAGE_SIZE).max(1)
}

pub fn pretty_json(resource: &Value) -> String {
    serde_json::to_string_pretty(resource).unwrap_or_default()
}

/// Every reference in a record, with a link when it resolves.
pub fn references(resource: &Value, exists: impl Fn(&str, &str) -> bool) -> Vec<RefRow> {
    let mut rows: Vec<RefRow> = collect_refs(resource)
        .into_iter()
        .map(|reference| {
            let target = local_target(&reference)
                .filter(|(kind, id)| exists(kind, id))
                .map(|(kind, id)| format!("{kind}/{}", encode_segment(id)));
            RefRow { reference, target }
        })
        .collect();
    rows.sort_by(|a, b| a.reference.cmp(&b.reference));
    rows.dedup();
    rows
}
```

- [ ] **Step 11: Create `templates/base.html`**

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="htmx-config" content='{"allowEval":false,"selfRequestsOnly":true,"includeIndicatorStyles":false,"historyCacheSize":0,"refreshOnHistoryMiss":true}'>
<title>{% block title %}{% endblock %} · {{ chrome.person }} · phv</title>
<link rel="stylesheet" href="/assets/style.css">
<script defer src="/assets/htmx.min.js"></script>
</head>
<body>
<header class="top">
  <span class="brand">phv</span>
  {% if !chrome.switcher.is_empty() %}
  <details class="switcher">
    <summary>{{ chrome.vault }}</summary>
    <ul>
      {% for item in &chrome.switcher %}
      <li>
        {% if item.available %}<a href="{{ item.href }}"{% if item.on %} aria-current="true"{% endif %}>{{ item.name }}</a>
        {% else %}{{ item.name }} <span class="muted">(unavailable)</span>{% endif %}
      </li>
      {% endfor %}
    </ul>
  </details>
  {% endif %}
</header>
<div class="layout">
  <nav class="nav">
    {% for item in &chrome.nav %}
    <a href="{{ item.href }}"{% if item.on %} aria-current="page"{% endif %}{% if item.label == "All FHIR" %} class="fhir"{% endif %}>{{ item.label }}</a>
    {% endfor %}
  </nav>
  <main>
    {% if chrome.unreadable > 0 %}
    <p class="notice">{{ chrome.unreadable }} records couldn't be read. Run <code>phv check</code>.</p>
    {% endif %}
    {% block main %}{% endblock %}
  </main>
</div>
</body>
</html>
```

- [ ] **Step 12: Create `templates/error.html`**

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{ title }} · phv</title>
<link rel="stylesheet" href="/assets/style.css">
</head>
<body>
<main>
  <h1>{{ title }}</h1>
  <p>{{ message }}</p>
  <ul class="plain">
    {% for name in &vaults %}<li><a href="/v/{{ name }}/overview">{{ name }}</a></li>{% endfor %}
  </ul>
</main>
</body>
</html>
```

- [ ] **Step 13: Create `templates/fhir_types.html`**

```html
{% extends "base.html" %}
{% block title %}All FHIR{% endblock %}
{% block main %}
<h1>All FHIR</h1>
<p class="sub">Every record in this vault, by FHIR resource type.</p>
{% if types.is_empty() %}
<p>No records yet.</p>
{% else %}
<table>
  <tr><th>Type</th><th>Records</th></tr>
  {% for t in &types %}
  <tr><td><a href="/v/{{ chrome.vault }}/fhir/{{ t.name }}">{{ t.name }}</a></td><td>{{ t.count }}</td></tr>
  {% endfor %}
</table>
{% endif %}
{% endblock %}
```

- [ ] **Step 14: Create `templates/fhir_list.html`**

```html
{% extends "base.html" %}
{% block title %}{{ resource_type }}{% endblock %}
{% block main %}
<h1>{{ resource_type }}</h1>
<p class="sub"><a href="/v/{{ chrome.vault }}/fhir">All FHIR</a> · {{ total }} records</p>
<table>
  <tr><th>Record</th><th>Date</th><th>ID</th></tr>
  {% for row in &rows %}
  <tr>
    <td><a href="/v/{{ chrome.vault }}/fhir/{{ resource_type }}/{{ row.id_url }}">{% if row.label.is_empty() %}Unnamed{% else %}{{ row.label }}{% endif %}</a></td>
    <td>{{ row.date }}</td>
    <td class="muted">{{ row.id }}</td>
  </tr>
  {% endfor %}
</table>
{% if pages > 1 %}
<p>
  {% if page > 1 %}<a href="/v/{{ chrome.vault }}/fhir/{{ resource_type }}?page={{ page - 1 }}">Previous</a>{% endif %}
  Page {{ page }} of {{ pages }}
  {% if page < pages %}<a href="/v/{{ chrome.vault }}/fhir/{{ resource_type }}?page={{ page + 1 }}">Next</a>{% endif %}
</p>
{% endif %}
{% endblock %}
```

- [ ] **Step 15: Create `templates/fhir_record.html`**

```html
{% extends "base.html" %}
{% block title %}{{ resource_type }}{% endblock %}
{% block main %}
<h1>{% if label.is_empty() %}{{ resource_type }}{% else %}{{ label }}{% endif %}</h1>
<p class="sub">
  <a href="/v/{{ chrome.vault }}/fhir/{{ resource_type }}">{{ resource_type }}</a> / {{ id }}
</p>
<pre>{{ json }}</pre>
{% if !references.is_empty() %}
<h2>References</h2>
<ul class="plain">
  {% for row in &references %}
  <li>{% if let Some(target) = &row.target %}<a href="/v/{{ chrome.vault }}/fhir/{{ target }}">{{ row.reference }}</a>{% else %}{{ row.reference }}{% endif %}</li>
  {% endfor %}
</ul>
{% endif %}
{% endblock %}
```

- [ ] **Step 16: Create `examples/browser.rs`**

```rust
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
```

- [ ] **Step 17: Make the edits**

In `src/browser/model/mod.rs`, replace:

```rust
pub mod observation;
```

with:

```rust
pub mod explorer;
pub mod observation;
```


- [ ] **Step 18: Run the tests to verify they pass**

Run: `cargo test --test browser_explorer`

Expected: 20 tests pass.

- [ ] **Step 19: Commit**

```bash
git add src templates examples tests/browser_common tests/browser_explorer.rs
git commit -m "feat: browse every record through a local read-only server"
```


### Task 5: Vitals

The first topic. It also adds the htmx partial pattern that Labs reuses: one route, a partial when `HX-Request` is present and a full page otherwise.

**Files:**

- Create: `src/browser/model/vitals.rs`
- Create: `src/browser/routes/vitals.rs`
- Create: `templates/vitals.html`
- Create: `templates/vital_history.html`
- Create: `templates/history_table.html`
- Modify: `src/browser/model/mod.rs`
- Modify: `src/browser/routes/mod.rs`
- Test: `tests/browser_vitals.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_vitals.rs`**

```rust
mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, fixture, get, send};
use phv::browser::model::vitals::{history, measures, vital_rows};

#[test]
fn only_vital_signs_count() {
    let rows = vital_rows(&fixture("Observation"));
    assert!(rows.iter().any(|r| r.id == "vital-hr-2026"));
    assert!(!rows.iter().any(|r| r.id == "lab-glucose-2026"));
    assert!(!rows.iter().any(|r| r.id == "obs-no-category"));
}

#[test]
fn entered_in_error_is_left_out() {
    let rows = vital_rows(&fixture("Observation"));
    assert!(!rows.iter().any(|r| r.id == "vital-weight-error"));
}

#[test]
fn measures_show_the_newest_record_in_a_fixed_order() {
    let found = measures(&vital_rows(&fixture("Observation")));
    let names: Vec<&str> = found.iter().map(|m| m.latest.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Blood pressure",
            "Heart rate",
            "Body weight",
            "Height",
            "Temperature"
        ]
    );
    assert_eq!(found[0].latest.lines, ["118/76 mmHg"]);
    assert_eq!(found[0].count, 2);
    assert_eq!(found[2].latest.lines, ["71.2 kg"]);
}

#[test]
fn a_record_with_no_date_says_so() {
    let found = measures(&vital_rows(&fixture("Observation")));
    let temperature = found
        .iter()
        .find(|m| m.latest.name == "Temperature")
        .unwrap();
    assert_eq!(temperature.latest.date, "No date");
    assert_eq!(temperature.latest.lines, ["36.60 Cel"]);
}

#[test]
fn history_is_newest_first_and_keeps_written_numbers() {
    let rows = history(&vital_rows(&fixture("Observation")), "loinc:29463-7");
    let lines: Vec<&str> = rows.iter().map(|r| r.lines[0].as_str()).collect();
    assert_eq!(lines, ["71.2 kg", "72.50 kg"]);
}

#[tokio::test]
async fn the_page_lists_measures() {
    let reply = get(&app(), "/v/vault/vitals").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.text().contains("118/76 mmHg"));
    assert!(
        reply
            .text()
            .contains(r#"hx-get="/v/vault/vitals/loinc%3A29463-7""#)
    );
}

#[tokio::test]
async fn a_measure_returns_a_full_page_or_a_partial_for_htmx() {
    let app = app();
    let uri = "/v/vault/vitals/loinc%3A29463-7";
    let full = get(&app, uri).await.text();
    assert!(full.contains("<html"));
    assert!(full.contains("72.50 kg"));
    let partial = send(&app, "GET", uri, &[("hx-request", "true")])
        .await
        .text();
    assert!(!partial.contains("<html"));
    assert!(partial.contains("72.50 kg"));
}

#[tokio::test]
async fn an_unknown_measure_is_404() {
    let reply = get(&app(), "/v/vault/vitals/loinc%3Anope").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_vitals`

Expected: compile error, because `model::vitals` does not exist.

- [ ] **Step 3: Create `src/browser/model/vitals.rs`**

```rust
//! The Vitals topic: Observations with category `vital-signs`.

use super::observation::{ObsRow, is_entered_in_error, obs_row};
use super::value::{OBSERVATION_CATEGORY, has_code, newest_first};
use serde_json::Value;

/// Fixed order for common measures, by LOINC code. Others follow by name.
const MEASURE_ORDER: [&str; 10] = [
    "loinc:85354-9", // blood pressure panel
    "loinc:55284-4", // blood pressure
    "loinc:8867-4",  // heart rate
    "loinc:29463-7", // body weight
    "loinc:8302-2",  // height
    "loinc:8310-5",  // temperature
    "loinc:59408-5", // oxygen saturation, pulse oximetry
    "loinc:2708-6",  // oxygen saturation, arterial
    "loinc:9279-1",  // respiratory rate
    "loinc:39156-5", // BMI
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measure {
    pub latest: ObsRow,
    pub count: usize,
}

pub fn is_vital(observation: &Value) -> bool {
    has_code(
        &observation["category"],
        OBSERVATION_CATEGORY,
        "vital-signs",
    )
}

/// Vital records, newest first, leaving out those entered in error.
pub fn vital_rows(observations: &[Value]) -> Vec<ObsRow> {
    let mut rows: Vec<ObsRow> = observations
        .iter()
        .filter(|o| is_vital(o) && !is_entered_in_error(o))
        .map(obs_row)
        .collect();
    newest_first(&mut rows);
    rows
}

/// The newest record of each measure. Expects rows from `vital_rows`.
pub fn measures(rows: &[ObsRow]) -> Vec<Measure> {
    let mut found: Vec<Measure> = Vec::new();
    for row in rows {
        match found.iter_mut().find(|m| m.latest.key == row.key) {
            Some(measure) => measure.count += 1,
            None => found.push(Measure {
                latest: row.clone(),
                count: 1,
            }),
        }
    }
    let rank = |key: &str| {
        MEASURE_ORDER
            .iter()
            .position(|k| *k == key)
            .unwrap_or(MEASURE_ORDER.len())
    };
    found.sort_by(|a, b| {
        rank(&a.latest.key)
            .cmp(&rank(&b.latest.key))
            .then_with(|| a.latest.name.cmp(&b.latest.name))
    });
    found
}

/// One measure's records, newest first. Expects rows from `vital_rows`.
pub fn history(rows: &[ObsRow], key: &str) -> Vec<ObsRow> {
    rows.iter().filter(|r| r.key == key).cloned().collect()
}
```

- [ ] **Step 4: Create `src/browser/routes/vitals.rs`**

```rust
//! Vitals pages.

use super::{App, Chrome, not_found, render, run, wants_partial};
use crate::browser::model::observation::ObsRow;
use crate::browser::model::vitals::{Measure, history, measures, vital_rows};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "vitals.html")]
struct VitalsPage {
    chrome: Chrome,
    measures: Vec<Measure>,
}

#[derive(Template)]
#[template(path = "history_table.html")]
struct HistoryPartial {
    rows: Vec<ObsRow>,
}

#[derive(Template)]
#[template(path = "vital_history.html")]
struct HistoryPage {
    chrome: Chrome,
    name: String,
    rows: Vec<ObsRow>,
}

async fn page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "vitals", |cx| {
        let observations = cx.records.of_type("Observation")?;
        let rows = vital_rows(&observations);
        render(VitalsPage {
            chrome: cx.chrome()?,
            measures: measures(&rows),
        })
    })
    .await
}

async fn measure(
    State(app): State<App>,
    Path((vault, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "vitals", move |cx| {
        let observations = cx.records.of_type("Observation")?;
        let rows = history(&vital_rows(&observations), &key);
        let name = rows
            .first()
            .ok_or_else(|| not_found("No records for this measure."))?
            .name
            .clone();
        if wants_partial(&headers) {
            return render(HistoryPartial { rows });
        }
        render(HistoryPage {
            chrome: cx.chrome()?,
            name,
            rows,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/vitals", get(page))
        .route("/v/{vault}/vitals/{measure}", get(measure))
}
```

- [ ] **Step 5: Create `templates/vitals.html`**

```html
{% extends "base.html" %}
{% block title %}Vitals{% endblock %}
{% block main %}
<h1>Vitals</h1>
{% if measures.is_empty() %}
<p>No vitals yet.</p>
{% else %}
<p class="sub">The latest value of each measure. Click a name for its history.</p>
<table>
  <tr><th>Measure</th><th>Latest</th><th>Date</th><th>Records</th></tr>
  {% for m in &measures %}
  <tr>
    <td><a href="/v/{{ chrome.vault }}/vitals/{{ m.latest.key_url }}" hx-get="/v/{{ chrome.vault }}/vitals/{{ m.latest.key_url }}" hx-target="#history-{{ loop.index }}">{{ m.latest.name }}</a></td>
    <td>{% for line in &m.latest.lines %}{{ line }}{% if !loop.last %}<br>{% endif %}{% else %}See FHIR{% endfor %}</td>
    <td>{{ m.latest.date }}</td>
    <td>{{ m.count }}</td>
  </tr>
  <tr><td colspan="4" class="history" id="history-{{ loop.index }}"></td></tr>
  {% endfor %}
</table>
{% endif %}
{% endblock %}
```

- [ ] **Step 6: Create `templates/vital_history.html`**

```html
{% extends "base.html" %}
{% block title %}{{ name }}{% endblock %}
{% block main %}
<h1>{{ name }}</h1>
<p class="sub"><a href="/v/{{ chrome.vault }}/vitals">Vitals</a></p>
{% include "history_table.html" %}
{% endblock %}
```

- [ ] **Step 7: Create `templates/history_table.html`**

```html
<table>
  <tr><th>Date</th><th>Result</th><th>Reference range</th><th>Flag</th></tr>
  {% for row in &rows %}
  <tr>
    <td>{{ row.date }}</td>
    <td>{% for line in &row.lines %}{{ line }}{% if !loop.last %}<br>{% endif %}{% else %}See FHIR{% endfor %}</td>
    <td>{% if let Some(range) = &row.range %}{{ range }}{% endif %}</td>
    <td class="flag">{% if let Some(flag) = &row.flag %}{{ flag }}{% endif %}</td>
  </tr>
  {% endfor %}
</table>
```

- [ ] **Step 8: Make the edits**

In `src/browser/model/mod.rs`, replace:

```rust
pub mod value;
```

with:

```rust
pub mod value;
pub mod vitals;
```

In `src/browser/routes/mod.rs`, replace:

```rust
mod explorer;
```

with:

```rust
mod explorer;
mod vitals;
```

In `src/browser/routes/mod.rs`, replace:

```rust
        .merge(explorer::routes())
```

with:

```rust
        .merge(explorer::routes())
        .merge(vitals::routes())
```

In `src/browser/routes/mod.rs`, replace:

```rust
    http::{Method, StatusCode},
```

with:

```rust
    http::{HeaderMap, Method, StatusCode},
```

In `src/browser/routes/mod.rs`, replace:

```rust
async fn home(
```

with:

```rust
/// htmx asks for a partial with this header. Anything else gets the full page.
pub fn wants_partial(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

async fn home(
```


- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test --test browser_vitals`

Expected: 8 tests pass.

- [ ] **Step 10: Commit**

```bash
git add src templates tests/browser_vitals.rs
git commit -m "feat: show vitals with their history"
```


### Task 6: Documents

The catalog and the original-file route, which is the most security-sensitive code here. Only a checked hash reaches the filesystem, symlinks do not count, and only PDF, PNG, JPEG and plain text open in the browser. The Provenance helpers here are also used by Labs and the record page.

**Files:**

- Create: `src/browser/model/documents.rs`
- Create: `src/browser/routes/documents.rs`
- Create: `templates/documents.html`
- Create: `templates/documents_list.html`
- Modify: `src/browser/model/mod.rs`
- Modify: `src/browser/routes/mod.rs`
- Test: `tests/browser_documents.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_documents.rs`**

```rust
mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture, fixture_path, get, send};
use phv::browser::model::documents::{chips, documents, is_sha256, source_file, source_of};
use phv::browser::{Vaults, router};
use serde_json::json;
use std::fs;

const LAB: &str = "1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412";
const RECEIPT: &str = "82f7258fe85a893e067ec1c27ac161dafd511455dce49426a6d973d4e5ce4479";

fn rows() -> Vec<phv::browser::model::documents::DocRow> {
    documents(
        &fixture("DocumentReference"),
        &fixture("Provenance"),
        &fixture_path(),
    )
}

#[test]
fn documents_are_newest_first_with_undated_last() {
    let ids: Vec<String> = rows().into_iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        ["document-lab", "document-receipt", "document-no-file"]
    );
}

#[test]
fn a_document_shows_its_category_file_and_record_count() {
    let rows = rows();
    assert_eq!(rows[0].title, "metabolic-panel.pdf");
    assert_eq!(rows[0].category_label, "Lab");
    assert_eq!(rows[0].record_count, 4);
    assert!(rows[0].has_file);
    assert_eq!(rows[1].category_label, "Receipt");
    assert_eq!(rows[1].record_count, 0);
}

#[test]
fn no_category_or_a_foreign_one_counts_as_uncategorized() {
    let foreign = [json!({
        "id": "d", "status": "current",
        "category": [{"coding": [{"system": "http://hospital.example", "code": "lab"}]}]
    })];
    let rows = documents(&foreign, &[], &fixture_path());
    assert_eq!(rows[0].category, "uncategorized");
    assert_eq!(rows[0].title, "Untitled document");
    assert_eq!(rows[0].date, "No date");
    assert!(!rows[0].has_file);
}

#[test]
fn chips_count_only_categories_in_use() {
    let chips = chips(&rows(), Some("receipt"));
    let shown: Vec<(&str, usize, bool)> = chips
        .iter()
        .map(|c| (c.label.as_str(), c.count, c.on))
        .collect();
    assert_eq!(
        shown,
        [
            ("All", 3, false),
            ("Lab", 1, false),
            ("Receipt", 1, true),
            ("Uncategorized", 1, false)
        ]
    );
}

#[test]
fn a_record_finds_its_source_through_provenance() {
    let provenance = fixture("Provenance");
    let source = source_of("Observation/lab-glucose-2026", &provenance).unwrap();
    assert_eq!(source.document_id, "document-lab");
    assert_eq!(source.agent.as_deref(), Some("phv-add-lab"));
    assert_eq!(source_of("Observation/vital-hr-2026", &provenance), None);
}

#[test]
fn hashes_must_be_exactly_64_lowercase_hex_characters() {
    assert!(is_sha256(LAB));
    assert!(!is_sha256(&LAB.to_uppercase()));
    assert!(!is_sha256(&LAB[..63]));
    assert!(!is_sha256("../../../../etc/passwd"));
}

#[cfg(unix)]
#[test]
fn a_symlink_in_sources_does_not_count() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, "secret").unwrap();
    let hash = "a".repeat(64);
    std::os::unix::fs::symlink(&outside, dir.path().join(format!("sources/{hash}.txt"))).unwrap();
    assert_eq!(source_file(dir.path(), &hash), None);
}

#[tokio::test]
async fn the_catalog_links_files_and_filters_by_category() {
    let app = app();
    let all = get(&app, "/v/vault/documents").await.text();
    assert!(all.contains(&format!("/v/vault/sources/{LAB}")));
    assert!(all.contains("No file stored"));
    let filtered = get(&app, "/v/vault/documents?category=receipt")
        .await
        .text();
    assert!(filtered.contains("pharmacy-receipt.txt"));
    assert!(!filtered.contains("metabolic-panel.pdf"));
}

#[tokio::test]
async fn a_category_returns_a_partial_for_htmx() {
    let reply = send(
        &app(),
        "GET",
        "/v/vault/documents?category=lab",
        &[("hx-request", "true")],
    )
    .await;
    assert!(!reply.text().contains("<html"));
    assert!(reply.text().contains("metabolic-panel.pdf"));
}

#[tokio::test]
async fn a_pdf_opens_in_the_browser_without_a_sandbox() {
    let reply = get(&app(), &format!("/v/vault/sources/{LAB}")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header("content-type"), "application/pdf");
    assert_eq!(reply.header("content-disposition"), "inline");
    assert_eq!(
        reply.header("content-security-policy"),
        "frame-ancestors 'none'"
    );
    assert_eq!(reply.header("cache-control"), "no-store");
    assert_eq!(reply.header("x-content-type-options"), "nosniff");
    assert!(reply.bytes.starts_with(b"%PDF"));
}

#[tokio::test]
async fn plain_text_opens_sandboxed() {
    let reply = get(&app(), &format!("/v/vault/sources/{RECEIPT}")).await;
    assert_eq!(reply.header("content-type"), "text/plain; charset=utf-8");
    assert_eq!(reply.header("content-security-policy"), "sandbox");
}

#[tokio::test]
async fn html_and_svg_download_instead_of_opening() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    for (hash, extension) in [("b".repeat(64), "html"), ("c".repeat(64), "svg")] {
        fs::write(
            dir.path().join(format!("sources/{hash}.{extension}")),
            "<script>alert(1)</script>",
        )
        .unwrap();
        let app = router(Vaults::single(dir.path().to_path_buf()));
        let reply = get(&app, &format!("/v/vault/sources/{hash}")).await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.header("content-type"), "application/octet-stream");
        assert!(
            reply
                .header("content-disposition")
                .starts_with("attachment")
        );
        assert_eq!(reply.header("content-security-policy"), "sandbox");
    }
}

#[tokio::test]
async fn bad_or_unknown_hashes_are_404() {
    let app = app();
    for uri in [
        "/v/vault/sources/nope".to_string(),
        "/v/vault/sources/..%2F..%2Fphv.toml".to_string(),
        format!("/v/vault/sources/{}", "d".repeat(64)),
    ] {
        assert_eq!(get(&app, &uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_documents`

Expected: compile error, because `model::documents` does not exist.

- [ ] **Step 3: Create `src/browser/model/documents.rs`**

```rust
//! The Documents topic and links between records and their source files.

use super::value::{Dated, date_or_none, id_of, newest_first};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub const CATEGORY_SYSTEM: &str = "urn:phv:document-category";
pub const HASH_SYSTEM: &str = "urn:phv:sha256";
pub const UNCATEGORIZED: &str = "uncategorized";

/// Category codes and their labels, in the order the filter chips show them.
pub const CATEGORIES: [(&str, &str); 6] = [
    ("lab", "Lab"),
    ("receipt", "Receipt"),
    ("visit-note", "Visit note"),
    ("imaging", "Imaging"),
    ("other", "Other"),
    (UNCATEGORIZED, "Uncategorized"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRow {
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub title: String,
    pub category: String,
    pub category_label: String,
    pub sha256: Option<String>,
    pub has_file: bool,
    pub record_count: usize,
}

impl Dated for DocRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub code: String,
    pub label: String,
    pub count: usize,
    pub on: bool,
}

/// Where a record came from, according to Provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSource {
    pub document_id: String,
    pub agent: Option<String>,
}

fn document_entity(provenance: &Value) -> Option<String> {
    provenance["entity"]
        .as_array()?
        .iter()
        .filter_map(|e| e["what"]["reference"].as_str())
        .find_map(|r| r.strip_prefix("DocumentReference/"))
        .map(str::to_string)
}

fn targets(provenance: &Value) -> Vec<&str> {
    provenance["target"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["reference"].as_str())
        .collect()
}

/// The source of `Type/id`, from the first Provenance that names it.
pub fn source_of(reference: &str, provenance: &[Value]) -> Option<RecordSource> {
    provenance.iter().find_map(|p| {
        if !targets(p).contains(&reference) {
            return None;
        }
        Some(RecordSource {
            document_id: document_entity(p)?,
            agent: p["agent"][0]["who"]["display"].as_str().map(str::to_string),
        })
    })
}

fn records_from(document_id: &str, provenance: &[Value]) -> usize {
    provenance
        .iter()
        .filter(|p| document_entity(p).as_deref() == Some(document_id))
        .map(|p| targets(p).len())
        .sum()
}

fn category_code(document: &Value) -> String {
    let code = document["category"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|c| c["coding"].as_array().into_iter().flatten())
        .find(|c| c["system"] == CATEGORY_SYSTEM)
        .and_then(|c| c["code"].as_str());
    match code {
        Some(code) if CATEGORIES.iter().any(|(known, _)| *known == code) => code.to_string(),
        _ => UNCATEGORIZED.to_string(),
    }
}

fn category_label(code: &str) -> String {
    CATEGORIES
        .iter()
        .find(|(known, _)| *known == code)
        .map_or(code, |(_, label)| label)
        .to_string()
}

pub fn sha256_of(document: &Value) -> Option<String> {
    document["identifier"]
        .as_array()?
        .iter()
        .find(|i| i["system"] == HASH_SYSTEM)
        .and_then(|i| i["value"].as_str())
        .map(str::to_string)
}

pub fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The regular file `sources/<sha256>.<ext>`. Symlinks and anything else do not count.
pub fn source_file(vault_root: &Path, sha256: &str) -> Option<PathBuf> {
    if !is_sha256(sha256) {
        return None;
    }
    let prefix = format!("{sha256}.");
    fs::read_dir(vault_root.join("sources"))
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .find(|entry| {
            fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.file_type().is_file())
        })
        .map(|entry| entry.path())
}

/// Every document, newest first, leaving out those entered in error.
pub fn documents(resources: &[Value], provenance: &[Value], vault_root: &Path) -> Vec<DocRow> {
    let mut rows: Vec<DocRow> = resources
        .iter()
        .filter(|d| d["status"] != "entered-in-error")
        .map(|d| {
            let id = id_of(d);
            let raw_date = d["date"].as_str().map(str::to_string);
            let sha256 = sha256_of(d);
            let category = category_code(d);
            DocRow {
                date: date_or_none(raw_date.as_deref()),
                raw_date,
                title: d["content"][0]["attachment"]["title"]
                    .as_str()
                    .unwrap_or("Untitled document")
                    .to_string(),
                category_label: category_label(&category),
                category,
                has_file: sha256
                    .as_deref()
                    .is_some_and(|hash| source_file(vault_root, hash).is_some()),
                sha256,
                record_count: records_from(&id, provenance),
                id,
            }
        })
        .collect();
    newest_first(&mut rows);
    rows
}

/// Filter chips for categories that have documents, after an "All" chip.
pub fn chips(rows: &[DocRow], selected: Option<&str>) -> Vec<Chip> {
    let mut chips = vec![Chip {
        code: String::new(),
        label: "All".to_string(),
        count: rows.len(),
        on: selected.is_none(),
    }];
    for (code, label) in CATEGORIES {
        let count = rows.iter().filter(|r| r.category == code).count();
        if count > 0 {
            chips.push(Chip {
                code: code.to_string(),
                label: label.to_string(),
                count,
                on: selected == Some(code),
            });
        }
    }
    chips
}

/// Whether a file with this extension may open in the browser, and its media type.
pub fn inline_media_type(extension: &str) -> Option<&'static str> {
    match extension {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "txt" => Some("text/plain; charset=utf-8"),
        _ => None,
    }
}
```

- [ ] **Step 4: Create `src/browser/routes/documents.rs`**

```rust
//! The document catalog and original files.

use super::{App, Chrome, PageError, not_found, render, run, wants_partial};
use crate::browser::model::documents::{
    Chip, DocRow, chips, documents, inline_media_type, is_sha256, source_file,
};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "documents.html")]
struct DocumentsPage {
    chrome: Chrome,
    chips: Vec<Chip>,
    rows: Vec<DocRow>,
}

#[derive(Template)]
#[template(path = "documents_list.html")]
struct DocumentsPartial {
    vault: String,
    chips: Vec<Chip>,
    rows: Vec<DocRow>,
}

#[derive(Deserialize)]
struct CategoryQuery {
    category: Option<String>,
}

async fn page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<CategoryQuery>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "documents", move |cx| {
        let resources = cx.records.of_type("DocumentReference")?;
        let provenance = cx.records.of_type("Provenance")?;
        let root = cx.records.vault().root().to_path_buf();
        let all = documents(&resources, &provenance, &root);
        let selected = query.category.filter(|c| !c.is_empty());
        let chips = chips(&all, selected.as_deref());
        let rows: Vec<DocRow> = all
            .into_iter()
            .filter(|r| selected.as_deref().is_none_or(|c| r.category == c))
            .collect();
        if wants_partial(&headers) {
            return render(DocumentsPartial {
                vault: cx.vault_name.clone(),
                chips,
                rows,
            });
        }
        render(DocumentsPage {
            chrome: cx.chrome()?,
            chips,
            rows,
        })
    })
    .await
}

fn file_response(extension: &str, sha256: &str, bytes: Vec<u8>) -> Response {
    let no_store = (header::CACHE_CONTROL, "no-store".to_string());
    match inline_media_type(extension) {
        // A sandbox policy can stop browsers' built-in PDF viewers, and a PDF cannot
        // script this origin.
        Some("application/pdf") => (
            [
                (header::CONTENT_TYPE, "application/pdf".to_string()),
                (header::CONTENT_DISPOSITION, "inline".to_string()),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "frame-ancestors 'none'".to_string(),
                ),
                no_store,
            ],
            bytes,
        )
            .into_response(),
        Some(media_type) => (
            [
                (header::CONTENT_TYPE, media_type.to_string()),
                (header::CONTENT_DISPOSITION, "inline".to_string()),
                (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
                no_store,
            ],
            bytes,
        )
            .into_response(),
        None => (
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{sha256}.{extension}\""),
                ),
                (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
                no_store,
            ],
            bytes,
        )
            .into_response(),
    }
}

async fn source(State(app): State<App>, Path((vault, sha256)): Path<(String, String)>) -> Response {
    run(app, vault, "documents", move |cx| {
        if !is_sha256(&sha256) {
            return Err(not_found("No such file."));
        }
        let path = source_file(cx.records.vault().root(), &sha256)
            .ok_or_else(|| not_found("No such file."))?;
        let bytes = std::fs::read(&path).map_err(|_| PageError::Internal)?;
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // The extension is safe to echo: it came from a file name starting with a checked hash,
        // but it may hold anything else, so keep only plain characters.
        let extension: String = extension
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        Ok(file_response(&extension, &sha256, bytes))
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/documents", get(page))
        .route("/v/{vault}/sources/{sha256}", get(source))
}
```

- [ ] **Step 5: Create `templates/documents.html`**

```html
{% extends "base.html" %}
{% block title %}Documents{% endblock %}
{% block main %}
<h1>Documents</h1>
<div id="doc-list">
{% let vault = &chrome.vault %}
{% include "documents_list.html" %}
</div>
{% endblock %}
```

- [ ] **Step 6: Create `templates/documents_list.html`**

```html
{% if chips.len() == 1 %}
<p>No documents yet.</p>
{% else %}
<div class="chips">
  {% for chip in &chips %}
  <a class="chip" href="/v/{{ vault }}/documents{% if !chip.code.is_empty() %}?category={{ chip.code }}{% endif %}" hx-get="/v/{{ vault }}/documents{% if !chip.code.is_empty() %}?category={{ chip.code }}{% endif %}" hx-target="#doc-list" hx-push-url="true"{% if chip.on %} aria-current="true"{% endif %}>{{ chip.label }} {{ chip.count }}</a>
  {% endfor %}
</div>
<table>
  <tr><th>Added</th><th>File</th><th>Category</th><th>Records from it</th></tr>
  {% for row in &rows %}
  <tr>
    <td>{{ row.date }}</td>
    <td>
      {% if row.has_file %}{% if let Some(hash) = &row.sha256 %}<a href="/v/{{ vault }}/sources/{{ hash }}">{{ row.title }}</a>{% endif %}
      {% else %}{{ row.title }} <span class="muted">No file stored</span>{% endif %}
      · <a class="muted" href="/v/{{ vault }}/fhir/DocumentReference/{{ row.id }}">FHIR</a>
    </td>
    <td>{{ row.category_label }}</td>
    <td>{{ row.record_count }}</td>
  </tr>
  {% endfor %}
</table>
{% endif %}
```

- [ ] **Step 7: Make the edits**

In `src/browser/model/mod.rs`, replace:

```rust
pub mod explorer;
```

with:

```rust
pub mod documents;
pub mod explorer;
```

In `src/browser/routes/mod.rs`, replace:

```rust
mod explorer;
```

with:

```rust
mod documents;
mod explorer;
```

In `src/browser/routes/mod.rs`, replace:

```rust
        .merge(vitals::routes())
```

with:

```rust
        .merge(vitals::routes())
        .merge(documents::routes())
```


- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --test browser_documents`

Expected: 13 tests pass.

- [ ] **Step 9: Commit**

```bash
git add src templates tests/browser_documents.rs
git commit -m "feat: catalog documents and serve original files safely"
```


### Task 7: Labs

The second condition in `is_lab_report` exists because the `phv-add-lab` skill's example report has no category of its own. Only its results do.

**Files:**

- Create: `src/browser/model/labs.rs`
- Create: `src/browser/routes/labs.rs`
- Create: `templates/labs.html`
- Create: `templates/lab_report.html`
- Create: `templates/lab_test.html`
- Modify: `src/browser/model/mod.rs`
- Modify: `src/browser/routes/mod.rs`
- Test: `tests/browser_labs.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_labs.rs`**

```rust
mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, fixture, get, send};
use phv::browser::model::labs::{
    is_lab_report, lab_reports, report_detail, results_not_in_a_report, test_history,
};
use serde_json::json;

#[test]
fn a_report_is_a_lab_report_by_category_or_by_its_results() {
    let observations = fixture("Observation");
    let reports = fixture("DiagnosticReport");
    let find = |id: &str| reports.iter().find(|r| r["id"] == id).unwrap();
    assert!(
        is_lab_report(find("report-2025"), &observations),
        "LAB category"
    );
    assert!(
        is_lab_report(find("report-2026"), &observations),
        "laboratory results"
    );
    assert!(!is_lab_report(find("report-imaging"), &observations));
}

#[test]
fn reports_are_newest_first() {
    let rows = lab_reports(&fixture("DiagnosticReport"), &fixture("Observation"));
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["report-2026", "report-2025"]);
    assert_eq!(rows[0].result_count, 3);
}

#[test]
fn a_report_shows_results_as_stored_in_its_own_order() {
    let detail = report_detail(
        "report-2026",
        &fixture("DiagnosticReport"),
        &fixture("Observation"),
    )
    .unwrap();
    let names: Vec<&str> = detail.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Glucose", "Creatinine", "CRP"]);
    assert_eq!(detail.results[1].lines, ["1.50 mg/dL"]);
    assert_eq!(detail.results[1].range.as_deref(), Some("0.70–1.30"));
    assert_eq!(detail.results[1].flag.as_deref(), Some("H"));
    assert_eq!(detail.results[0].range.as_deref(), Some("70 - 99 mg/dL"));
    assert_eq!(detail.results[0].flag, None);
    assert_eq!(detail.results[2].lines, ["<0.5 mg/L"]);
}

#[test]
fn a_missing_result_is_reported_not_hidden() {
    let reports = [json!({
        "resourceType": "DiagnosticReport", "id": "r",
        "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/v2-0074", "code": "LAB"}]}],
        "result": [{"reference": "Observation/gone"}]
    })];
    let detail = report_detail("r", &reports, &[]).unwrap();
    assert!(detail.results.is_empty());
    assert_eq!(detail.missing, ["Observation/gone"]);
}

#[test]
fn an_imaging_report_is_not_a_lab_report() {
    assert_eq!(
        report_detail(
            "report-imaging",
            &fixture("DiagnosticReport"),
            &fixture("Observation")
        ),
        None
    );
}

#[test]
fn results_outside_every_report_are_listed_separately() {
    let rows = results_not_in_a_report(&fixture("DiagnosticReport"), &fixture("Observation"));
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["lab-vitamin-d-loose"]);
    assert_eq!(rows[0].lines, ["Sufficient"]);
}

#[test]
fn test_history_groups_by_loinc_across_different_names() {
    let rows = test_history(
        "loinc:2345-7",
        &fixture("DiagnosticReport"),
        &fixture("Observation"),
    );
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Glucose", "Blood sugar"]);
}

#[test]
fn test_history_without_loinc_needs_identical_text() {
    let reports = fixture("DiagnosticReport");
    let observations = fixture("Observation");
    assert_eq!(test_history("text:CRP", &reports, &observations).len(), 1);
    assert!(test_history("text:crp", &reports, &observations).is_empty());
}

#[tokio::test]
async fn the_page_lists_reports_and_loose_results_but_not_imaging() {
    let html = get(&app(), "/v/vault/labs").await.text();
    assert!(html.contains("Metabolic panel"));
    assert!(html.contains("Results not in a report"));
    assert!(html.contains("Vitamin D/25-OH"));
    assert!(!html.contains("Chest X-ray"));
}

#[tokio::test]
async fn a_report_page_links_its_original_file_and_fhir() {
    let html = get(&app(), "/v/vault/labs/reports/report-2026")
        .await
        .text();
    assert!(html.contains("1.50 mg/dL"));
    assert!(html.contains(
        "/v/vault/sources/1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412"
    ));
    assert!(html.contains("/v/vault/fhir/DiagnosticReport/report-2026"));
}

#[tokio::test]
async fn a_test_key_with_a_slash_round_trips_through_the_url() {
    let html = get(&app(), "/v/vault/labs").await.text();
    assert!(
        html.contains("/v/vault/labs/tests/text%3AVitamin%20D%2F25-OH"),
        "{html}"
    );
    let reply = get(&app(), "/v/vault/labs/tests/text%3AVitamin%20D%2F25-OH").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.text().contains("Sufficient"));
}

#[tokio::test]
async fn a_test_returns_a_partial_for_htmx() {
    let reply = send(
        &app(),
        "GET",
        "/v/vault/labs/tests/loinc%3A2345-7",
        &[("hx-request", "true")],
    )
    .await;
    assert!(!reply.text().contains("<html"));
    assert!(reply.text().contains("101 mg/dL"));
}

#[tokio::test]
async fn unknown_reports_and_tests_are_404() {
    let app = app();
    for uri in [
        "/v/vault/labs/reports/report-imaging",
        "/v/vault/labs/reports/nope",
        "/v/vault/labs/tests/nope",
    ] {
        assert_eq!(get(&app, uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_labs`

Expected: compile error, because `model::labs` does not exist.

- [ ] **Step 3: Create `src/browser/model/labs.rs`**

```rust
//! The Labs topic: lab reports and their results.

use super::observation::{ObsRow, clinical_date, is_entered_in_error, obs_row};
use super::value::{
    DIAGNOSTIC_SERVICE, Dated, OBSERVATION_CATEGORY, concept_name, date_or_none, has_code, id_of,
    newest_first,
};
use crate::refs::local_target;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub name: String,
    pub status: String,
    pub result_count: usize,
}

impl Dated for ReportRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDetail {
    pub report: ReportRow,
    pub results: Vec<ObsRow>,
    /// References in the report that point at no stored Observation.
    pub missing: Vec<String>,
}

pub fn is_lab_observation(observation: &Value) -> bool {
    has_code(&observation["category"], OBSERVATION_CATEGORY, "laboratory")
}

fn by_id(observations: &[Value]) -> HashMap<String, &Value> {
    observations.iter().map(|o| (id_of(o), o)).collect()
}

fn result_ids(report: &Value) -> Vec<String> {
    report["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["reference"].as_str())
        .map(str::to_string)
        .collect()
}

fn observation_id(reference: &str) -> Option<&str> {
    match local_target(reference) {
        Some(("Observation", id)) => Some(id),
        _ => None,
    }
}

/// `LAB` category, or at least one result that is a `laboratory` Observation.
pub fn is_lab_report(report: &Value, observations: &[Value]) -> bool {
    if has_code(&report["category"], DIAGNOSTIC_SERVICE, "LAB") {
        return true;
    }
    let index = by_id(observations);
    result_ids(report).iter().any(|reference| {
        observation_id(reference)
            .and_then(|id| index.get(id))
            .is_some_and(|o| is_lab_observation(o))
    })
}

fn report_row(report: &Value) -> ReportRow {
    let raw_date = clinical_date(report);
    ReportRow {
        id: id_of(report),
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        name: concept_name(&report["code"]).unwrap_or_else(|| "Unnamed report".into()),
        status: report["status"].as_str().unwrap_or("").to_string(),
        result_count: result_ids(report).len(),
    }
}

/// Lab reports, newest first, leaving out those entered in error.
pub fn lab_reports(reports: &[Value], observations: &[Value]) -> Vec<ReportRow> {
    let mut rows: Vec<ReportRow> = reports
        .iter()
        .filter(|r| !is_entered_in_error(r) && is_lab_report(r, observations))
        .map(report_row)
        .collect();
    newest_first(&mut rows);
    rows
}

/// One lab report with its results in the report's own order. `None` if it is not one.
pub fn report_detail(id: &str, reports: &[Value], observations: &[Value]) -> Option<ReportDetail> {
    let report = reports
        .iter()
        .find(|r| id_of(r) == id && !is_entered_in_error(r) && is_lab_report(r, observations))?;
    let index = by_id(observations);
    let mut results = Vec::new();
    let mut missing = Vec::new();
    for reference in result_ids(report) {
        match observation_id(&reference).and_then(|id| index.get(id)) {
            Some(observation) if is_entered_in_error(observation) => {}
            Some(observation) => results.push(obs_row(observation)),
            None => missing.push(reference),
        }
    }
    Some(ReportDetail {
        report: report_row(report),
        results,
        missing,
    })
}

fn referenced_by_lab_reports(reports: &[Value], observations: &[Value]) -> HashSet<String> {
    reports
        .iter()
        .filter(|r| is_lab_report(r, observations))
        .flat_map(result_ids)
        .filter_map(|reference| observation_id(&reference).map(str::to_string))
        .collect()
}

/// `laboratory` Observations in no lab report, newest first.
pub fn results_not_in_a_report(reports: &[Value], observations: &[Value]) -> Vec<ObsRow> {
    let referenced = referenced_by_lab_reports(reports, observations);
    let mut rows: Vec<ObsRow> = observations
        .iter()
        .filter(|o| is_lab_observation(o) && !is_entered_in_error(o))
        .filter(|o| !referenced.contains(&id_of(o)))
        .map(obs_row)
        .collect();
    newest_first(&mut rows);
    rows
}

/// Every result of one test across reports, newest first.
pub fn test_history(key: &str, reports: &[Value], observations: &[Value]) -> Vec<ObsRow> {
    let referenced = referenced_by_lab_reports(reports, observations);
    let mut rows: Vec<ObsRow> = observations
        .iter()
        .filter(|o| is_lab_observation(o) || referenced.contains(&id_of(o)))
        .filter(|o| !is_entered_in_error(o))
        .map(obs_row)
        .filter(|row| row.key == key)
        .collect();
    newest_first(&mut rows);
    rows
}
```

- [ ] **Step 4: Create `src/browser/routes/labs.rs`**

```rust
//! Labs pages.

use super::{App, Chrome, Ctx, PageError, not_found, render, run, wants_partial};
use crate::browser::model::documents::{sha256_of, source_of};
use crate::browser::model::labs::{
    ReportDetail, ReportRow, lab_reports, report_detail, results_not_in_a_report, test_history,
};
use crate::browser::model::observation::ObsRow;
use crate::browser::model::value::id_of;
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "labs.html")]
struct LabsPage {
    chrome: Chrome,
    reports: Vec<ReportRow>,
    loose: Vec<ObsRow>,
}

#[derive(Template)]
#[template(path = "lab_report.html")]
struct ReportPage {
    chrome: Chrome,
    detail: ReportDetail,
    original: Option<String>,
}

#[derive(Template)]
#[template(path = "history_table.html")]
struct HistoryPartial {
    rows: Vec<ObsRow>,
}

#[derive(Template)]
#[template(path = "lab_test.html")]
struct TestPage {
    chrome: Chrome,
    name: String,
    rows: Vec<ObsRow>,
}

async fn page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "labs", |cx| {
        let reports = cx.records.of_type("DiagnosticReport")?;
        let observations = cx.records.of_type("Observation")?;
        render(LabsPage {
            chrome: cx.chrome()?,
            reports: lab_reports(&reports, &observations),
            loose: results_not_in_a_report(&reports, &observations),
        })
    })
    .await
}

fn original_file(cx: &mut Ctx, report_id: &str) -> Result<Option<String>, PageError> {
    let provenance = cx.records.of_type("Provenance")?;
    let Some(source) = source_of(&format!("DiagnosticReport/{report_id}"), &provenance) else {
        return Ok(None);
    };
    let documents = cx.records.of_type("DocumentReference")?;
    Ok(documents
        .iter()
        .find(|d| id_of(d) == source.document_id)
        .and_then(sha256_of)
        .map(|hash| format!("/v/{}/sources/{hash}", cx.vault_name)))
}

async fn report(State(app): State<App>, Path((vault, id)): Path<(String, String)>) -> Response {
    run(app, vault, "labs", move |cx| {
        let reports = cx.records.of_type("DiagnosticReport")?;
        let observations = cx.records.of_type("Observation")?;
        let detail = report_detail(&id, &reports, &observations)
            .ok_or_else(|| not_found("No lab report with this id."))?;
        render(ReportPage {
            original: original_file(cx, &id)?,
            chrome: cx.chrome()?,
            detail,
        })
    })
    .await
}

async fn test(
    State(app): State<App>,
    Path((vault, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "labs", move |cx| {
        let reports = cx.records.of_type("DiagnosticReport")?;
        let observations = cx.records.of_type("Observation")?;
        let rows = test_history(&key, &reports, &observations);
        let name = rows
            .first()
            .ok_or_else(|| not_found("No results for this test."))?
            .name
            .clone();
        if wants_partial(&headers) {
            return render(HistoryPartial { rows });
        }
        render(TestPage {
            chrome: cx.chrome()?,
            name,
            rows,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/labs", get(page))
        .route("/v/{vault}/labs/reports/{id}", get(report))
        .route("/v/{vault}/labs/tests/{test}", get(test))
}
```

- [ ] **Step 5: Create `templates/labs.html`**

```html
{% extends "base.html" %}
{% block title %}Labs{% endblock %}
{% block main %}
<h1>Labs</h1>
{% if reports.is_empty() && loose.is_empty() %}
<p>No labs yet.</p>
{% endif %}
{% if !reports.is_empty() %}
<table>
  <tr><th>Report</th><th>Date</th><th>Status</th><th>Results</th></tr>
  {% for r in &reports %}
  <tr>
    <td><a href="/v/{{ chrome.vault }}/labs/reports/{{ r.id }}">{{ r.name }}</a></td>
    <td>{{ r.date }}</td>
    <td>{{ r.status }}</td>
    <td>{{ r.result_count }}</td>
  </tr>
  {% endfor %}
</table>
{% endif %}
{% if !loose.is_empty() %}
<h2>Results not in a report</h2>
<table>
  <tr><th>Test</th><th>Result</th><th>Date</th></tr>
  {% for row in &loose %}
  <tr>
    <td><a href="/v/{{ chrome.vault }}/labs/tests/{{ row.key_url }}">{{ row.name }}</a></td>
    <td>{% for line in &row.lines %}{{ line }}{% if !loop.last %}<br>{% endif %}{% else %}See FHIR{% endfor %}</td>
    <td>{{ row.date }}</td>
  </tr>
  {% endfor %}
</table>
{% endif %}
{% endblock %}
```

- [ ] **Step 6: Create `templates/lab_report.html`**

```html
{% extends "base.html" %}
{% block title %}{{ detail.report.name }}{% endblock %}
{% block main %}
<h1>{{ detail.report.name }}</h1>
<p class="sub">
  {{ detail.report.date }} · {{ detail.report.status }}
  {% if let Some(href) = &original %} · <a href="{{ href }}">Original file</a>{% endif %}
  · <a href="/v/{{ chrome.vault }}/fhir/DiagnosticReport/{{ detail.report.id }}">View FHIR</a>
</p>
<table>
  <tr><th>Test</th><th>Result</th><th>Reference range</th><th>Flag</th></tr>
  {% for row in &detail.results %}
  <tr>
    <td><a href="/v/{{ chrome.vault }}/labs/tests/{{ row.key_url }}" hx-get="/v/{{ chrome.vault }}/labs/tests/{{ row.key_url }}" hx-target="#history-{{ loop.index }}">{{ row.name }}</a></td>
    <td>{% for line in &row.lines %}{{ line }}{% if !loop.last %}<br>{% endif %}{% else %}See FHIR{% endfor %}</td>
    <td>{% if let Some(range) = &row.range %}{{ range }}{% else %}<span class="muted">Not recorded</span>{% endif %}</td>
    <td class="flag">{% if let Some(flag) = &row.flag %}{{ flag }}{% endif %}</td>
  </tr>
  <tr><td colspan="4" class="history" id="history-{{ loop.index }}"></td></tr>
  {% endfor %}
</table>
{% for reference in &detail.missing %}
<p class="notice">Result not found: {{ reference }}. Run <code>phv check</code>.</p>
{% endfor %}
{% endblock %}
```

- [ ] **Step 7: Create `templates/lab_test.html`**

```html
{% extends "base.html" %}
{% block title %}{{ name }}{% endblock %}
{% block main %}
<h1>{{ name }}</h1>
<p class="sub"><a href="/v/{{ chrome.vault }}/labs">Labs</a></p>
{% include "history_table.html" %}
{% endblock %}
```

- [ ] **Step 8: Make the edits**

In `src/browser/model/mod.rs`, replace:

```rust
pub mod observation;
```

with:

```rust
pub mod labs;
pub mod observation;
```

In `src/browser/routes/mod.rs`, replace:

```rust
mod explorer;
```

with:

```rust
mod explorer;
mod labs;
```

In `src/browser/routes/mod.rs`, replace:

```rust
        .merge(vitals::routes())
```

with:

```rust
        .merge(vitals::routes())
        .merge(labs::routes())
```


- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test --test browser_labs`

Expected: 13 tests pass.

- [ ] **Step 10: Commit**

```bash
git add src templates tests/browser_labs.rs
git commit -m "feat: show lab reports and test history"
```


### Task 8: Conditions, medicines, allergies and vaccines

Simple lists that share one template. `active_now` is what the Overview uses later: a record with no status counts as active and says so, so a missing status can never hide an allergy.

**Files:**

- Create: `src/browser/model/clinical.rs`
- Create: `src/browser/routes/clinical.rs`
- Create: `templates/clinical_list.html`
- Modify: `src/browser/model/mod.rs`
- Modify: `src/browser/routes/mod.rs`
- Test: `tests/browser_clinical.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_clinical.rs`**

```rust
mod browser_common;

use browser_common::{app, fixture, get};
use phv::browser::model::clinical::{active_now, allergies, conditions, medicines, vaccines};
use serde_json::json;

#[test]
fn conditions_leave_out_entered_in_error() {
    let rows = conditions(&fixture("Condition"));
    assert!(!rows.iter().any(|r| r.id == "condition-error"));
    assert_eq!(rows.len(), 3);
}

#[test]
fn active_now_includes_records_with_no_status_and_says_so() {
    let active = active_now(&conditions(&fixture("Condition")));
    let names: Vec<(&str, &str)> = active
        .iter()
        .map(|r| (r.name.as_str(), r.status.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("Hypothyroidism", "Status not recorded"),
            ("Asthma", "Active")
        ]
    );
}

#[test]
fn allergies_show_reactions_and_a_missing_status_never_hides_one() {
    let rows = allergies(&fixture("AllergyIntolerance"));
    let penicillin = rows.iter().find(|r| r.name == "Penicillin").unwrap();
    assert_eq!(penicillin.detail.as_deref(), Some("Hives"));
    // FHIR requires this status, but a hand-merged file could still lack it.
    let no_status = [json!({"id": "a", "code": {"text": "Latex"}})];
    let active = active_now(&allergies(&no_status));
    assert_eq!(active[0].status, "Status not recorded");
}

#[test]
fn medicine_names_fall_back_in_order() {
    let medications = fixture("Medication");
    let rows = medicines(
        &fixture("MedicationStatement"),
        &fixture("MedicationRequest"),
        &medications,
    );
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Amoxicillin 500 mg", "Levothyroxine 50 mcg"]);

    let by_display =
        [json!({"id": "a", "status": "active", "medicationReference": {"display": "Shown"}})];
    assert_eq!(medicines(&by_display, &[], &[])[0].name, "Shown");
    let unnamed = [json!({"id": "b", "status": "active"})];
    assert_eq!(medicines(&unnamed, &[], &[])[0].name, "Unnamed medicine");
}

#[test]
fn only_active_medicines_are_active_now() {
    let rows = medicines(
        &fixture("MedicationStatement"),
        &fixture("MedicationRequest"),
        &fixture("Medication"),
    );
    let active = active_now(&rows);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "Levothyroxine 50 mcg");
}

#[test]
fn vaccines_not_given_say_so_and_keep_a_written_date() {
    let rows = vaccines(&fixture("Immunization"));
    let mmr = rows.iter().find(|r| r.name == "MMR").unwrap();
    assert_eq!(mmr.status, "Not given");
    assert_eq!(mmr.date, "Spring 2020");
    assert!(
        active_now(&rows).is_empty(),
        "vaccines are never active now"
    );
}

#[tokio::test]
async fn each_list_page_shows_its_records() {
    let app = app();
    for (uri, expected) in [
        ("/v/vault/conditions", "Asthma"),
        ("/v/vault/medicines", "Three times a day for 7 days"),
        ("/v/vault/allergies", "Hives"),
        ("/v/vault/vaccines", "Not given"),
    ] {
        let html = get(&app, uri).await.text();
        assert!(html.contains(expected), "{uri}");
    }
}

#[tokio::test]
async fn an_empty_list_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let vault = phv::store::Vault::init(&dir.path().join("v")).unwrap();
    let app = phv::browser::router(phv::browser::Vaults::single(vault.root().to_path_buf()));
    assert!(
        get(&app, "/v/vault/conditions")
            .await
            .text()
            .contains("No conditions yet.")
    );
    assert!(
        get(&app, "/v/vault/vitals")
            .await
            .text()
            .contains("No vitals yet.")
    );
    assert!(
        get(&app, "/v/vault/labs")
            .await
            .text()
            .contains("No labs yet.")
    );
    assert!(
        get(&app, "/v/vault/documents")
            .await
            .text()
            .contains("No documents yet.")
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_clinical`

Expected: compile error, because `model::clinical` does not exist.

- [ ] **Step 3: Create `src/browser/model/clinical.rs`**

```rust
//! Conditions, medicines, allergies and vaccines as simple lists.

use super::value::{Dated, capitalized, concept_name, date_or_none, id_of, newest_first};
use crate::refs::local_target;
use serde_json::Value;

const STATUS_NOT_RECORDED: &str = "Status not recorded";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClinicalRow {
    pub resource_type: String,
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub name: String,
    pub status: String,
    pub detail: Option<String>,
    /// Status `active`, or no status at all. `false` for vaccines, which are never "active now".
    pub active_now: bool,
}

impl Dated for ClinicalRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

fn first_text(values: &[&Value]) -> Option<String> {
    values.iter().find_map(|v| v.as_str()).map(str::to_string)
}

/// The code of a status held as a CodeableConcept, such as `clinicalStatus`.
fn concept_code(concept: &Value) -> Option<String> {
    concept["coding"][0]["code"]
        .as_str()
        .map(str::to_string)
        .or_else(|| concept_name(concept))
}

fn dated_row(
    resource: &Value,
    raw_date: Option<String>,
    name: String,
    status: Option<String>,
    detail: Option<String>,
    can_be_active: bool,
) -> ClinicalRow {
    ClinicalRow {
        resource_type: resource["resourceType"].as_str().unwrap_or("").to_string(),
        id: id_of(resource),
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        name,
        active_now: can_be_active && status.as_deref().is_none_or(|s| s == "active"),
        status: status.map_or_else(|| STATUS_NOT_RECORDED.to_string(), |s| capitalized(&s)),
        detail,
    }
}

fn verified_in_error(resource: &Value) -> bool {
    concept_code(&resource["verificationStatus"]).as_deref() == Some("entered-in-error")
}

pub fn conditions(resources: &[Value]) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = resources
        .iter()
        .filter(|r| !verified_in_error(r))
        .map(|r| {
            dated_row(
                r,
                first_text(&[
                    &r["onsetDateTime"],
                    &r["onsetPeriod"]["start"],
                    &r["recordedDate"],
                ]),
                concept_name(&r["code"]).unwrap_or_else(|| "Unnamed condition".into()),
                concept_code(&r["clinicalStatus"]),
                None,
                true,
            )
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn allergies(resources: &[Value]) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = resources
        .iter()
        .filter(|r| !verified_in_error(r))
        .map(|r| {
            let reactions: Vec<String> = r["reaction"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|reaction| reaction["manifestation"].as_array().into_iter().flatten())
                .filter_map(concept_name)
                .collect();
            dated_row(
                r,
                first_text(&[&r["onsetDateTime"], &r["recordedDate"]]),
                concept_name(&r["code"]).unwrap_or_else(|| "Unnamed allergy".into()),
                concept_code(&r["clinicalStatus"]),
                (!reactions.is_empty()).then(|| reactions.join(", ")),
                true,
            )
        })
        .collect();
    newest_first(&mut rows);
    rows
}

/// The concept, then the referenced `Medication`, then the reference's `display`.
fn medicine_name(resource: &Value, medications: &[Value]) -> String {
    if let Some(name) = concept_name(&resource["medicationCodeableConcept"]) {
        return name;
    }
    let reference = &resource["medicationReference"];
    let referenced = reference["reference"]
        .as_str()
        .and_then(local_target)
        .filter(|(kind, _)| *kind == "Medication")
        .and_then(|(_, id)| medications.iter().find(|m| m["id"] == id))
        .and_then(|m| concept_name(&m["code"]));
    referenced
        .or_else(|| reference["display"].as_str().map(str::to_string))
        .unwrap_or_else(|| "Unnamed medicine".to_string())
}

/// `MedicationStatement` and `MedicationRequest` together.
pub fn medicines(
    statements: &[Value],
    requests: &[Value],
    medications: &[Value],
) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = statements
        .iter()
        .chain(requests)
        .filter(|r| r["status"] != "entered-in-error")
        .map(|r| {
            dated_row(
                r,
                first_text(&[
                    &r["effectiveDateTime"],
                    &r["effectivePeriod"]["start"],
                    &r["authoredOn"],
                    &r["dateAsserted"],
                ]),
                medicine_name(r, medications),
                r["status"].as_str().map(str::to_string),
                first_text(&[&r["dosage"][0]["text"], &r["dosageInstruction"][0]["text"]]),
                true,
            )
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn vaccines(resources: &[Value]) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = resources
        .iter()
        .filter(|r| r["status"] != "entered-in-error")
        .map(|r| {
            let mut row = dated_row(
                r,
                r["occurrenceDateTime"].as_str().map(str::to_string),
                concept_name(&r["vaccineCode"]).unwrap_or_else(|| "Unnamed vaccine".into()),
                r["status"].as_str().map(str::to_string),
                None,
                false,
            );
            if r["status"] == "not-done" {
                row.status = "Not given".to_string();
            }
            if row.raw_date.is_none()
                && let Some(written) = r["occurrenceString"].as_str()
            {
                row.date = written.to_string();
            }
            row
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn active_now(rows: &[ClinicalRow]) -> Vec<ClinicalRow> {
    rows.iter().filter(|r| r.active_now).cloned().collect()
}
```

- [ ] **Step 4: Create `src/browser/routes/clinical.rs`**

```rust
//! Conditions, medicines, allergies and vaccines.

use super::{App, Chrome, render, run};
use crate::browser::model::clinical::{ClinicalRow, allergies, conditions, medicines, vaccines};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "clinical_list.html")]
struct ListPage {
    chrome: Chrome,
    title: &'static str,
    empty: &'static str,
    show_detail: bool,
    rows: Vec<ClinicalRow>,
}

async fn conditions_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "conditions", |cx| {
        let rows = conditions(&cx.records.of_type("Condition")?);
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Conditions",
            empty: "No conditions yet.",
            show_detail: false,
            rows,
        })
    })
    .await
}

async fn medicines_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "medicines", |cx| {
        let statements = cx.records.of_type("MedicationStatement")?;
        let requests = cx.records.of_type("MedicationRequest")?;
        let medications = cx.records.of_type("Medication")?;
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Medicines",
            empty: "No medicines yet.",
            show_detail: true,
            rows: medicines(&statements, &requests, &medications),
        })
    })
    .await
}

async fn allergies_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "allergies", |cx| {
        let rows = allergies(&cx.records.of_type("AllergyIntolerance")?);
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Allergies",
            empty: "No allergies yet.",
            show_detail: true,
            rows,
        })
    })
    .await
}

async fn vaccines_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "vaccines", |cx| {
        let rows = vaccines(&cx.records.of_type("Immunization")?);
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Vaccines",
            empty: "No vaccines yet.",
            show_detail: false,
            rows,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/conditions", get(conditions_page))
        .route("/v/{vault}/medicines", get(medicines_page))
        .route("/v/{vault}/allergies", get(allergies_page))
        .route("/v/{vault}/vaccines", get(vaccines_page))
}
```

- [ ] **Step 5: Create `templates/clinical_list.html`**

```html
{% extends "base.html" %}
{% block title %}{{ title }}{% endblock %}
{% block main %}
<h1>{{ title }}</h1>
{% if rows.is_empty() %}
<p>{{ empty }}</p>
{% else %}
<table>
  <tr><th>Name</th><th>Status</th>{% if show_detail %}<th>Details</th>{% endif %}<th>Date</th></tr>
  {% for row in &rows %}
  <tr>
    <td><a href="/v/{{ chrome.vault }}/fhir/{{ row.resource_type }}/{{ row.id }}">{{ row.name }}</a></td>
    <td>{{ row.status }}</td>
    {% if show_detail %}<td>{% if let Some(detail) = &row.detail %}{{ detail }}{% endif %}</td>{% endif %}
    <td>{{ row.date }}</td>
  </tr>
  {% endfor %}
</table>
{% endif %}
{% endblock %}
```

- [ ] **Step 6: Make the edits**

In `src/browser/model/mod.rs`, replace:

```rust
pub mod documents;
```

with:

```rust
pub mod clinical;
pub mod documents;
```

In `src/browser/routes/mod.rs`, replace:

```rust
mod documents;
```

with:

```rust
mod clinical;
mod documents;
```

In `src/browser/routes/mod.rs`, replace:

```rust
        .merge(labs::routes())
```

with:

```rust
        .merge(labs::routes())
        .merge(clinical::routes())
```


- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --test browser_clinical`

Expected: 8 tests pass.

- [ ] **Step 8: Commit**

```bash
git add src templates tests/browser_clinical.rs
git commit -m "feat: list conditions, medicines, allergies and vaccines"
```


### Task 9: Link records to their topic and source

The record page now says where a record is shown and which file it came from.

**Files:**

- Modify: `src/browser/model/explorer.rs`
- Modify: `src/browser/routes/explorer.rs`
- Modify: `templates/fhir_record.html`
- Test: `tests/browser_record_links.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_record_links.rs`**

```rust
mod browser_common;

use browser_common::get;
use browser_common::{app, fixture};
use phv::browser::model::explorer::topic_of;

#[test]
fn records_know_which_topic_shows_them() {
    let observations = fixture("Observation");
    let find = |id: &str| observations.iter().find(|o| o["id"] == id).unwrap().clone();
    let topic = |resource| topic_of(&resource, &observations).map(|(label, _)| label);
    assert_eq!(topic(find("vital-hr-2026")), Some("Vitals"));
    assert_eq!(topic(find("lab-glucose-2026")), Some("Labs"));
    assert_eq!(topic(find("obs-no-category")), None);
    let reports = fixture("DiagnosticReport");
    let report = |id: &str| reports.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(
        topic_of(&report("report-2026"), &observations),
        Some(("Labs", "labs/reports/report-2026".to_string()))
    );
    assert_eq!(topic_of(&report("report-imaging"), &observations), None);
    assert_eq!(topic_of(&fixture("Encounter")[0], &observations), None);
}

#[tokio::test]
async fn a_record_page_names_its_topic_and_source() {
    let html = get(&app(), "/v/vault/fhir/Observation/lab-glucose-2026")
        .await
        .text();
    assert!(html.contains("Shown under"));
    assert!(html.contains(r#"href="/v/vault/labs""#));
    assert!(html.contains("metabolic-panel.pdf"));
    assert!(html.contains("Added by phv-add-lab"));
}

#[tokio::test]
async fn a_record_with_no_topic_or_source_leaves_them_out() {
    let html = get(&app(), "/v/vault/fhir/Encounter/encounter-1")
        .await
        .text();
    assert!(!html.contains("Shown under"));
    assert!(!html.contains("Added by"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_record_links`

Expected: compile error, because `topic_of` does not exist.

- [ ] **Step 3: Make the edits**

In `src/browser/model/explorer.rs`, replace:

```rust
use super::observation::clinical_date;
```

with:

```rust
use super::labs::{is_lab_observation, is_lab_report};
use super::observation::clinical_date;
```

In `src/browser/model/explorer.rs`, replace:

```rust
use crate::refs::{collect_refs, local_target};
```

with:

```rust
use super::vitals::is_vital;
use crate::refs::{collect_refs, local_target};
```

In `src/browser/model/explorer.rs`, replace:

```rust
//! All FHIR: every record of every type.
```

with:

```rust
//! All FHIR: every record of every type, and the topic page that shows each one.
```

At the end of `src/browser/model/explorer.rs`, add:

```rust
/// The topic page a record shows up under, as a label and a path below the vault.
pub fn topic_of(resource: &Value, observations: &[Value]) -> Option<(&'static str, String)> {
    let simple = |label, path: &str| Some((label, path.to_string()));
    match resource["resourceType"].as_str()? {
        "Observation" if is_vital(resource) => simple("Vitals", "vitals"),
        "Observation" if is_lab_observation(resource) => simple("Labs", "labs"),
        "DiagnosticReport" if is_lab_report(resource, observations) => Some((
            "Labs",
            format!("labs/reports/{}", encode_segment(&id_of(resource))),
        )),
        "Condition" => simple("Conditions", "conditions"),
        "MedicationStatement" | "MedicationRequest" => simple("Medicines", "medicines"),
        "AllergyIntolerance" => simple("Allergies", "allergies"),
        "Immunization" => simple("Vaccines", "vaccines"),
        "DocumentReference" => simple("Documents", "documents"),
        _ => None,
    }
}
```

In `src/browser/routes/explorer.rs`, replace:

```rust
use super::{App, Chrome, Ctx, PageError, not_found, render, run};
```

with:

```rust
use super::{App, Chrome, Ctx, PageError, not_found, render, run};
use crate::browser::model::documents::source_of;
```

In `src/browser/routes/explorer.rs`, replace:

```rust
    references,
};
```

with:

```rust
    references, topic_of,
};
```

In `src/browser/routes/explorer.rs`, replace:

```rust
use crate::browser::model::parse_resource;
```

with:

```rust
use crate::browser::model::parse_resource;
use crate::browser::model::value::id_of;
```

In `src/browser/routes/explorer.rs`, replace:

```rust
#[derive(Template)]
#[template(path = "fhir_record.html")]
```

with:

```rust
struct SourceLink {
    title: String,
    href: Option<String>,
    agent: Option<String>,
}

#[derive(Template)]
#[template(path = "fhir_record.html")]
```

In `src/browser/routes/explorer.rs`, replace:

```rust
    references: Vec<RefRow>,
}
```

with:

```rust
    references: Vec<RefRow>,
    topic: Option<(&'static str, String)>,
    source: Option<SourceLink>,
}
```

In `src/browser/routes/explorer.rs`, replace:

```rust
async fn record(
```

with:

```rust
fn source_link(cx: &mut Ctx, reference: &str) -> Result<Option<SourceLink>, PageError> {
    let provenance = cx.records.of_type("Provenance")?;
    let Some(source) = source_of(reference, &provenance) else {
        return Ok(None);
    };
    let documents = cx.records.of_type("DocumentReference")?;
    let document = documents.iter().find(|d| id_of(d) == source.document_id);
    Ok(Some(SourceLink {
        title: document.map_or_else(|| source.document_id.clone(), label_of),
        href: document.map(|_| {
            format!(
                "/v/{}/fhir/DocumentReference/{}",
                cx.vault_name, source.document_id
            )
        }),
        agent: source.agent,
    }))
}

async fn record(
```

In `src/browser/routes/explorer.rs`, replace:

```rust
        let vault_ref = cx.records.vault().clone();
```

with:

```rust
        let observations = cx.records.of_type("Observation")?;
        let vault_ref = cx.records.vault().clone();
        let source = source_link(cx, &format!("{resource_type}/{id}"))?;
```

In `src/browser/routes/explorer.rs`, replace:

```rust
            }),
            resource_type,
            id,
```

with:

```rust
            }),
            topic: topic_of(&resource, &observations),
            source,
            resource_type,
            id,
```

In `templates/fhir_record.html`, replace:

```html
  <a href="/v/{{ chrome.vault }}/fhir/{{ resource_type }}">{{ resource_type }}</a> / {{ id }}
```

with:

```html
  <a href="/v/{{ chrome.vault }}/fhir/{{ resource_type }}">{{ resource_type }}</a> / {{ id }}
  {% if let Some((topic_label, topic_path)) = &topic %} · Shown under <a href="/v/{{ chrome.vault }}/{{ topic_path }}">{{ topic_label }}</a>{% endif %}
  {% if let Some(source) = &source %} · Source {% if let Some(href) = &source.href %}<a href="{{ href }}">{{ source.title }}</a>{% else %}{{ source.title }}{% endif %}{% if let Some(agent) = &source.agent %} · Added by {{ agent }}{% endif %}{% endif %}
```


- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test browser_record_links`

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src templates tests/browser_record_links.rs
git commit -m "feat: show where a record appears and which file it came from"
```


### Task 10: Overview

The page `/` has been redirecting to. It only gathers what the topic models already produce, so it holds no new rules.

**Files:**

- Create: `src/browser/model/overview.rs`
- Create: `src/browser/routes/overview.rs`
- Create: `templates/overview.html`
- Modify: `src/browser/model/mod.rs`
- Modify: `src/browser/routes/mod.rs`
- Test: `tests/browser_overview.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_overview.rs`**

```rust
mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, fixture, get};
use phv::browser::model::overview::{Sources, overview};

#[test]
fn the_overview_gathers_each_topic() {
    let patients = fixture("Patient");
    let observations = fixture("Observation");
    let reports = fixture("DiagnosticReport");
    let conditions = fixture("Condition");
    let statements = fixture("MedicationStatement");
    let requests = fixture("MedicationRequest");
    let medications = fixture("Medication");
    let allergies = fixture("AllergyIntolerance");
    let view = overview(&Sources {
        patients: &patients,
        observations: &observations,
        reports: &reports,
        conditions: &conditions,
        statements: &statements,
        requests: &requests,
        medications: &medications,
        allergies: &allergies,
    });
    assert_eq!(view.birth_date.as_deref(), Some("14 Mar 1988"));
    assert_eq!(view.measures[0].latest.lines, ["118/76 mmHg"]);
    assert_eq!(view.latest_report.unwrap().name, "Metabolic panel");
    assert_eq!(view.conditions.len(), 2);
    assert_eq!(view.medicines.len(), 1);
    assert_eq!(view.allergies.len(), 1);
}

#[tokio::test]
async fn the_page_shows_the_person_and_what_is_active() {
    let reply = get(&app(), "/v/vault/overview").await;
    assert_eq!(reply.status, StatusCode::OK);
    let html = reply.text();
    assert!(html.contains("Maya Example"));
    assert!(html.contains("Born 14 Mar 1988"));
    assert!(html.contains("118/76 mmHg"));
    assert!(html.contains("Metabolic panel"));
    assert!(html.contains("Levothyroxine 50 mcg"));
    assert!(html.contains("Status not recorded"));
    assert!(
        !html.contains("Sinusitis"),
        "resolved conditions are not active"
    );
    assert!(
        !html.contains("Wrong diagnosis"),
        "entered in error stays out"
    );
    assert!(html.contains("3 documents"));
}

#[tokio::test]
async fn a_vault_with_no_patient_falls_back_to_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let vault = phv::store::Vault::init(&dir.path().join("v")).unwrap();
    let app = phv::browser::router(phv::browser::Vaults::single(vault.root().to_path_buf()));
    let html = get(&app, "/v/vault/overview").await.text();
    assert!(html.contains("<h1>vault</h1>"));
    assert!(html.contains("No vitals yet."));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test browser_overview`

Expected: compile error, because `model::overview` does not exist.

- [ ] **Step 3: Create `src/browser/model/overview.rs`**

```rust
//! The Overview page: the person, latest vitals, latest lab report, what is active now.

use super::clinical::{self, ClinicalRow};
use super::labs::{self, ReportRow};
use super::value::friendly_date;
use super::vitals::{self, Measure};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub birth_date: Option<String>,
    pub measures: Vec<Measure>,
    pub latest_report: Option<ReportRow>,
    pub conditions: Vec<ClinicalRow>,
    pub medicines: Vec<ClinicalRow>,
    pub allergies: Vec<ClinicalRow>,
}

/// Input for `overview`, one slice per resource type.
pub struct Sources<'a> {
    pub patients: &'a [Value],
    pub observations: &'a [Value],
    pub reports: &'a [Value],
    pub conditions: &'a [Value],
    pub statements: &'a [Value],
    pub requests: &'a [Value],
    pub medications: &'a [Value],
    pub allergies: &'a [Value],
}

pub fn overview(sources: &Sources) -> Overview {
    let rows = vitals::vital_rows(sources.observations);
    Overview {
        birth_date: sources
            .patients
            .first()
            .and_then(|p| p["birthDate"].as_str().map(friendly_date)),
        measures: vitals::measures(&rows),
        latest_report: labs::lab_reports(sources.reports, sources.observations)
            .into_iter()
            .next(),
        conditions: clinical::active_now(&clinical::conditions(sources.conditions)),
        medicines: clinical::active_now(&clinical::medicines(
            sources.statements,
            sources.requests,
            sources.medications,
        )),
        allergies: clinical::active_now(&clinical::allergies(sources.allergies)),
    }
}
```

- [ ] **Step 4: Create `src/browser/routes/overview.rs`**

```rust
//! The Overview page.

use super::{App, Chrome, render, run};
use crate::browser::model::overview::{Overview, Sources, overview};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "overview.html")]
struct OverviewPage {
    chrome: Chrome,
    view: Overview,
    record_count: usize,
    document_count: usize,
}

async fn page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "overview", |cx| {
        let patients = cx.records.of_type("Patient")?;
        let observations = cx.records.of_type("Observation")?;
        let reports = cx.records.of_type("DiagnosticReport")?;
        let conditions = cx.records.of_type("Condition")?;
        let statements = cx.records.of_type("MedicationStatement")?;
        let requests = cx.records.of_type("MedicationRequest")?;
        let medications = cx.records.of_type("Medication")?;
        let allergies = cx.records.of_type("AllergyIntolerance")?;
        let view = overview(&Sources {
            patients: &patients,
            observations: &observations,
            reports: &reports,
            conditions: &conditions,
            statements: &statements,
            requests: &requests,
            medications: &medications,
            allergies: &allergies,
        });
        let mut record_count = 0;
        for name in cx.records.vault().resource_types()? {
            record_count += cx.records.vault().lines(&name)?.len();
        }
        let document_count = cx.records.vault().lines("DocumentReference")?.len();
        render(OverviewPage {
            chrome: cx.chrome()?,
            view,
            record_count,
            document_count,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new().route("/v/{vault}/overview", get(page))
}
```

- [ ] **Step 5: Create `templates/overview.html`**

```html
{% extends "base.html" %}
{% block title %}Overview{% endblock %}
{% block main %}
<h1>{{ chrome.person }}</h1>
<p class="sub">{% if let Some(born) = &view.birth_date %}Born {{ born }} · {% endif %}{{ record_count }} records · {{ document_count }} documents</p>
{% if view.measures.is_empty() %}
<p>No vitals yet.</p>
{% else %}
<div class="tiles">
  {% for m in &view.measures %}
  <a class="tile-link" href="/v/{{ chrome.vault }}/vitals/{{ m.latest.key_url }}"><div class="tile">
    <b>{% for line in &m.latest.lines %}{{ line }}{% if !loop.last %}<br>{% endif %}{% else %}See FHIR{% endfor %}</b>
    <span>{{ m.latest.name }} · {{ m.latest.date }}</span>
  </div></a>
  {% endfor %}
</div>
{% endif %}
<h2>Latest lab report</h2>
{% if let Some(report) = &view.latest_report %}
<p><a href="/v/{{ chrome.vault }}/labs/reports/{{ report.id }}">{{ report.name }}</a> <span class="muted">· {{ report.date }} · {{ report.result_count }} results</span></p>
{% else %}
<p>No labs yet.</p>
{% endif %}
<h2>Active now</h2>
<table>
  <tr><th>Conditions</th><td>{% for row in &view.conditions %}<div>{{ row.name }}{% if row.status == "Status not recorded" %} <span class="muted">Status not recorded</span>{% endif %}</div>{% else %}<span class="muted">None recorded</span>{% endfor %}</td></tr>
  <tr><th>Medicines</th><td>{% for row in &view.medicines %}<div>{{ row.name }}{% if row.status == "Status not recorded" %} <span class="muted">Status not recorded</span>{% endif %}</div>{% else %}<span class="muted">None recorded</span>{% endfor %}</td></tr>
  <tr><th>Allergies</th><td>{% for row in &view.allergies %}<div>{{ row.name }}{% if row.status == "Status not recorded" %} <span class="muted">Status not recorded</span>{% endif %}</div>{% else %}<span class="muted">None recorded</span>{% endfor %}</td></tr>
</table>
{% endblock %}
```

- [ ] **Step 6: Make the edits**

In `src/browser/model/mod.rs`, replace:

```rust
pub mod value;
```

with:

```rust
pub mod overview;
pub mod value;
```

In `src/browser/routes/mod.rs`, replace:

```rust
mod labs;
```

with:

```rust
mod labs;
mod overview;
```

In `src/browser/routes/mod.rs`, replace:

```rust
        .merge(documents::routes())
```

with:

```rust
        .merge(documents::routes())
        .merge(overview::routes())
```


- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --test browser_overview`

Expected: 3 tests pass.

- [ ] **Step 8: Commit**

```bash
git add src templates tests/browser_overview.rs
git commit -m "feat: add the overview page"
```


### Task 11: Crawl test and whole-page check

The crawl proves two promises from the spec at once: there are no dead links, and reading never changes the vault, not even by creating `.phv/lock`.

**Files:**

- Test: `tests/browser_crawl.rs`

- [ ] **Step 1: Write the failing tests in `tests/browser_crawl.rs`**

```rust
//! Follows every link from the home page against a copy of the sample vault.

mod browser_common;

use axum::http::StatusCode;
use browser_common::{copy_fixture, get};
use phv::browser::{Vaults, router};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn snapshot(root: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            snapshot(root, &path, into);
        } else {
            let key = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            into.insert(key, std::fs::read(&path).unwrap());
        }
    }
}

fn links(html: &str) -> Vec<String> {
    html.split(r#"href=""#)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|href| href.starts_with('/'))
        .map(|href| href.replace("&#38;", "&"))
        .collect()
}

#[tokio::test]
async fn crawling_every_page_finds_no_dead_links_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let mut before = BTreeMap::new();
    snapshot(dir.path(), dir.path(), &mut before);

    let app = router(Vaults::single(dir.path().to_path_buf()));
    let mut seen = BTreeSet::new();
    let mut queue = vec!["/v/vault/overview".to_string()];
    while let Some(uri) = queue.pop() {
        if !seen.insert(uri.clone()) {
            continue;
        }
        let reply = get(&app, &uri).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        if reply.header("content-type").starts_with("text/html") {
            queue.extend(links(&reply.text()));
        }
    }
    assert!(seen.len() > 40, "only reached {} pages", seen.len());
    for expected in [
        "/v/vault/vitals",
        "/v/vault/labs/reports/report-2026",
        "/v/vault/documents?category=receipt",
        "/v/vault/fhir/Encounter/encounter-1",
    ] {
        assert!(seen.contains(expected), "never reached {expected}");
    }

    let mut after = BTreeMap::new();
    snapshot(dir.path(), dir.path(), &mut after);
    assert_eq!(before, after, "the vault changed");
    assert!(
        !dir.path().join(".phv").exists(),
        "the browser created .phv"
    );
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test --test browser_crawl`

Expected: 1 tests pass.

- [ ] **Step 3: Run everything**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`

Expected: all pass. When this plan was written, the full suite was 204 tests on the `vault-core` of that day, so expect a higher number after merging newer work.

- [ ] **Step 4: Check the real pages in a real browser**

Automated tests can't see layout or browser PDF viewers.

```bash
cargo run --example browser -- --vault tests/fixtures/sample-vault --port 8791
```

Open `http://127.0.0.1:8791/` and confirm each of these:

- Overview shows `118/76 mmHg`, Metabolic panel, and Hypothyroidism marked "Status not recorded".
- On Vitals and a lab report, clicking a name expands its history in place. The browser console shows no Content Security Policy errors.
- On Documents, clicking a category chip updates the list and the URL.
- With JavaScript turned off, the same links go to full pages that work.
- `metabolic-panel.pdf` opens in **Chrome and Safari**. If either fails, change the PDF policy in `src/browser/routes/documents.rs` and report it. Do not add a sandbox back without checking again.
- At a phone-width window, there is no sideways page scroll.

Stop the server afterward.

- [ ] **Step 5: Commit**

```bash
git add tests/browser_crawl.rs
git commit -m "test: crawl every page and confirm the vault is untouched"
```


### Task 12: Category and date in `phv source add`

**Blocked until `vault-core` Task 8 is merged. This code was not compiled.** It is written against the design in `docs/superpowers/plans/2026-11-06-vault-core.md`, so check it against the merged `src/source.rs` first. Until this lands, the browser shows documents as Uncategorized with "No date", as the spec allows.

**Files:**
- Modify: `src/source.rs`, `src/cli.rs`, `src/api.rs`, `skills/phv-add-lab/SKILL.md`, `docs/superpowers/specs/2026-11-06-vault-core-design.md`
- Test: `tests/source.rs`

- [ ] **Step 1: Merge `vault-core` and confirm Task 8 is there**

```bash
git merge --no-edit vault-core
grep -n 'pub async fn source_add' src/source.rs
```

Expected: the merge succeeds and `grep` prints a line. If it prints nothing, stop, because this task is still blocked.

- [ ] **Step 2: Write the failing tests**

Add to `tests/source.rs`, reusing the `PDF` constant and `phv()` helper already in that file:

```rust
#[tokio::test]
async fn a_source_records_its_category_and_when_it_was_added() {
    let (_d, phv) = phv().await;
    let options = phv::source::SourceOptions {
        category: Some("receipt".into()),
        added_at: Some("2026-11-06T12:00:00Z".into()),
    };
    phv.source_add_with("a.pdf", PDF, &options).await.unwrap();
    let text = records(&phv, "DocumentReference");
    assert!(text.contains(r#""code":"receipt""#), "{text}");
    assert!(text.contains(r#""system":"urn:phv:document-category""#), "{text}");
    assert!(text.contains(r#""date":"2026-11-06T12:00:00Z""#), "{text}");
}

#[tokio::test]
async fn an_unknown_category_is_refused_and_stores_nothing() {
    let (_d, phv) = phv().await;
    let options = phv::source::SourceOptions {
        category: Some("uncategorized".into()),
        added_at: None,
    };
    let error = phv.source_add_with("a.pdf", PDF, &options).await.unwrap_err();
    assert!(matches!(error, PhvError::Usage(_)));
    assert_eq!(records(&phv, "DocumentReference"), "");
}

#[tokio::test]
async fn adding_the_same_file_again_keeps_the_first_category() {
    let (_d, phv) = phv().await;
    let first = phv::source::SourceOptions { category: Some("lab".into()), added_at: None };
    let second = phv::source::SourceOptions { category: Some("receipt".into()), added_at: None };
    phv.source_add_with("a.pdf", PDF, &first).await.unwrap();
    let again = phv.source_add_with("a.pdf", PDF, &second).await.unwrap();
    assert!(again.already_imported);
    assert!(records(&phv, "DocumentReference").contains(r#""code":"lab""#));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --test source`

Expected: compile error, because `SourceOptions` does not exist.

- [ ] **Step 4: Add the options to `src/source.rs`**

Add this near the top of the file:

```rust
use crate::browser::model::documents::{CATEGORIES, CATEGORY_SYSTEM, UNCATEGORIZED};

#[derive(Debug, Default, Clone)]
pub struct SourceOptions {
    pub category: Option<String>,
    /// When the file was added. The clock is read in the CLI and API layer, not here.
    pub added_at: Option<String>,
}

fn check_category(category: &str) -> Result<(), PhvError> {
    let known = category != UNCATEGORIZED && CATEGORIES.iter().any(|(code, _)| *code == category);
    if known {
        return Ok(());
    }
    Err(PhvError::Usage(format!(
        "unknown category {category:?}: use lab, receipt, visit-note, imaging or other"
    )))
}
```

Rename the existing `source_add` to `source_add_with`, give it an `options: &SourceOptions` parameter, and keep the old name delegating to it, so existing callers and tests do not change:

```rust
pub async fn source_add(&self, file_name: &str, bytes: &[u8]) -> Result<SourceReport, PhvError> {
    self.source_add_with(file_name, bytes, &SourceOptions::default()).await
}

pub async fn source_add_with(
    &self,
    file_name: &str,
    bytes: &[u8],
    options: &SourceOptions,
) -> Result<SourceReport, PhvError> {
```

As the first line of `source_add_with`'s body, before taking the lock, add:

```rust
if let Some(category) = &options.category {
    check_category(category)?;
}
```

After the block that sets `resource["subject"]`, add:

```rust
if let Some(category) = &options.category {
    resource["category"] = json!([{"coding": [{"system": CATEGORY_SYSTEM, "code": category}]}]);
}
if let Some(added_at) = &options.added_at {
    resource["date"] = json!(added_at);
}
```

- [ ] **Step 5: Add the flag to the CLI**

In `src/cli.rs`, add to `SourceCommand::Add`:

```rust
/// What kind of document this is: lab, receipt, visit-note, imaging or other.
#[arg(long)]
category: Option<String>,
```

In the `Command::Source(SourceCommand::Add { file, category })` arm, replace the `phv.source_add(&name, &bytes)` call with the following, using the same current-time helper the `add` command uses for `--recorded-at`:

```rust
let options = crate::source::SourceOptions {
    category,
    added_at: Some(now()),
};
let report = phv.source_add_with(&name, &bytes, &options).await?;
```

`now()` is the helper the vault-core plan defines in `src/cli.rs`, so change the call if the merged code named it differently. Use `crate::source::SourceOptions` here, because the CLI is inside the library.

- [ ] **Step 6: Add the parameter to the API**

In `src/api.rs`, add `category: Option<String>` to `SourceQuery` and change `add_source` to:

```rust
async fn add_source(State(phv): State<AppState>, Query(q): Query<SourceQuery>, body: Bytes) -> Result<Response, PhvError> {
    let options = crate::source::SourceOptions {
        category: q.category,
        added_at: Some(humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()),
    };
    Ok(Json(phv.source_add_with(&q.filename, &body, &options).await?).into_response())
}
```

- [ ] **Step 7: Update the skill and the design**

In `skills/phv-add-lab/SKILL.md`, change the store command to:

```bash
phv source add --category lab path/to/report.pdf
```

In `docs/superpowers/specs/2026-11-06-vault-core-design.md`, under "Sources and Provenance", add to the `DocumentReference` list:

```markdown
- `category`: from `--category`, one of `lab`, `receipt`, `visit-note`, `imaging` or `other`, with system `urn:phv:document-category`. Optional.
- `date`: when the file was added.
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --test source && cargo test --test browser_documents`

Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add src tests skills docs
git commit -m "feat: record a category and date when adding a source"
```

### Task 13: Mount the browser in `phv serve`

**Blocked until `vault-core` Tasks 11 and 12 are merged. This code was not compiled.** Check it against the merged `src/api.rs` and `src/cli.rs` first.

**Files:**
- Modify: `src/api.rs`, `src/cli.rs`, `src/browser/mod.rs`, `src/browser/guard.rs`
- Test: `tests/api.rs`

- [ ] **Step 1: Merge `vault-core` and confirm the API is there**

```bash
git merge --no-edit vault-core
grep -n 'pub async fn serve' src/api.rs
```

Expected: the merge succeeds and `grep` prints a line. If it prints nothing, stop, because this task is still blocked.

- [ ] **Step 2: Write the failing test**

Add to `tests/api.rs`:

```rust
#[tokio::test]
async fn the_browser_and_the_api_share_one_server() {
    let (dir, phv) = phv().await;
    let root = phv.vault().root().to_path_buf();
    let app = phv::api::app(Arc::new(phv), phv::browser::Vaults::single(root));
    let (status, _) = send(app.clone(), "GET", "/resources/Patient", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(app, "GET", "/v/vault/overview", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8(body).unwrap().contains("No vitals yet."));
    drop(dir);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --test api the_browser_and_the_api_share_one_server`

Expected: compile error, because `phv::api::app` does not exist.

- [ ] **Step 4: Share the local-only check**

The API and the browser must not drift apart on what counts as local. In `src/browser/mod.rs`, change `mod guard;` to `pub(crate) mod guard;`.

In `src/browser/guard.rs`, split the check out of `local_only`:

```rust
/// Whether a request's `Host` and `Origin` are both local. The REST API uses this too.
pub(crate) fn is_local_request(headers: &HeaderMap) -> bool {
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(is_local_host);
    let origin_ok = headers
        .get(header::ORIGIN)
        .is_none_or(|o| o.to_str().is_ok_and(is_local_origin));
    host_ok && origin_ok
}

pub async fn local_only(request: Request, next: Next) -> Response {
    if !is_local_request(request.headers()) {
        return (StatusCode::FORBIDDEN, "this server only answers local requests").into_response();
    }
    next.run(request).await
}
```

Add `HeaderMap` to that file's `axum::http` import.

In `src/api.rs`, delete `is_local`, and change its `local_only` so that the condition is:

```rust
if crate::browser::guard::is_local_request(request.headers()) {
```

- [ ] **Step 5: Merge the two routers**

In `src/api.rs`, add `app` and change `serve`. Only the browser has a fallback, so the merge is allowed:

```rust
/// The REST API and the FHIR browser on one server.
pub fn app(phv: AppState, vaults: crate::browser::Vaults) -> Router {
    router(phv).merge(crate::browser::router(vaults))
}

pub async fn serve(phv: Phv, vaults: crate::browser::Vaults, port: u16) -> Result<(), PhvError> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    println!("phv on http://127.0.0.1:{port}");
    axum::serve(listener, app(Arc::new(phv), vaults)).await?;
    Ok(())
}
```

- [ ] **Step 6: Resolve the vault list in the CLI**

In `src/cli.rs`, change the `Command::Serve` arm so the browser gets the same vault choice as the rest of the command. Use the same flag, environment and config values the CLI already passes to `resolve_vault_path`. These names are from the vault-core plan, so check them against the merged code:

```rust
Command::Serve { port } => {
    let vaults = crate::browser::Vaults::resolve(
        cli.vault.as_deref(),
        std::env::var("PHV_VAULT").ok().as_deref(),
        config_text.as_deref(),
    )?;
    crate::api::serve(phv, vaults, port).await?;
    Ok(ExitCode::SUCCESS)
}
```

Also change the command's help text to `Run the local REST API and the FHIR browser on 127.0.0.1.`

- [ ] **Step 7: Run everything**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`

Expected: all pass, including the REST API's existing `Host` and `Origin` tests.

- [ ] **Step 8: Check it by hand**

```bash
cargo run -q -- --vault tests/fixtures/sample-vault serve --port 8791
```

Open `http://127.0.0.1:8791/` and confirm the Overview loads, then confirm `http://127.0.0.1:8791/resources/Patient` returns NDJSON. Stop the server afterward.

- [ ] **Step 9: Remove the example and commit**

`phv serve` replaces the example, so remove it, along with any mention of it in the spec's "Starting it" section:

```bash
git rm examples/browser.rs
git add src tests docs
git commit -m "feat: serve the FHIR browser alongside the REST API"
```
