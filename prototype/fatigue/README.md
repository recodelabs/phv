# Synthetic fatigue scenario

A local, dependency-free prototype of the first PHR symptom-brief scenario. It selects evidence from one synthetic history, asks essential missing questions, and produces a brief whose statements resolve to sources or attributed answers. This is a development demonstration with a draft framework; it is not clinically approved or a triage service.

## Run

Requires a working Node.js 22 or newer. No package installation is needed.

From the repository root:

```sh
node prototype/fatigue/server.mjs
```

Open [the local preview](http://127.0.0.1:4280/). Stop with Ctrl-C. Choose another port using `--port 4281` if needed. On this Mac, `/opt/homebrew/bin/node` is the tested working binary; a bundled Node may require a newer macOS.

The preview serves explicit demo assets on loopback. When started with `--vault`, it also reads snapshots from that one configured synthetic vault. It does not expose arbitrary paths, import personal records, write a vault, call an AI API, or contact a terminology service. Nothing is saved across page reloads. Reference links navigate to the external guideline publishers only when opened.

## Try the full path

1. Submit an empty concern to see the validation message, then type a synthetic concern in your own words.
2. Inspect selected evidence and its reasons. The case includes older explicitly linked history, existing results, a medication-status ambiguity, an unrelated flagged result, and an unreviewed extraction.
3. Answer the onset and daily-impact questions. Try “Not sure” or “Skip,” then return and correct an answer. The goal question is optional.
4. Review a partial or completed brief. Inspect a statement's evidence and exact adapted guideline reference. Remove and restore an evidence item; edit a statement as a patient correction.
5. Copy/download the text, download the full trace JSON, or print the brief. “Print / Save PDF” uses the browser's print dialog; the demo does not implement an independent PDF renderer. The evidence appendix is opt-in.

All visible text input is treated as text. The free-text concern is preserved verbatim rather than interpreted by an LLM. Onset, impact, medication status, and goal are explicit structured follow-up answers; a concern containing those facts will still need structured confirmation in this first prototype. The wording/time budget is not a validated questionnaire.

## Command-line demonstration

```sh
node prototype/fatigue/cli.mjs
node prototype/fatigue/cli.mjs --trace
node prototype/fatigue/cli.mjs --input my-synthetic-answers.json
```

With no input, the CLI uses an illustrative conversation and the same synthetic evidence/engine as the UI. `--trace` returns the complete JSON result. An input file uses:

```json
{
  "concern": "I have been unusually tired and want to prepare for a primary care visit.",
  "answers": {
    "onset": {"status": "answered", "value": "Around mid September, exact date uncertain"},
    "impact": {"status": "answered", "value": "I need more breaks during household tasks"},
    "medication_current": {"status": "unknown", "value": ""},
    "goal": {"status": "skipped", "value": ""}
  },
  "excludedEvidenceIds": [],
  "claimEdits": {},
  "revision": 1
}
```

Supported answer statuses: `answered`, `unknown`, `skipped`, `declined`; an absent answer is unanswered. Unknown/skipped answers are never converted into a negative answer. Blank answered values are rejected. Removing evidence changes the generated brief, not the original fixture. Statement edits become attributed patient reports and retain the original text for inspection.

## Traceability

- Evidence has stable IDs, fixture text locators, and SHA-256 content hashes.
- Selection decisions include disposition, basis, rationale, rule ID, and exact source locator when supported.
- Questions carry gap rationale, evidence context, and adapted guidance citations where appropriate.
- Claims link to evidence/decisions or an attributed answer. The original concern is available at `trace.concern`; follow-up answers are in `trace.answers`.
- Snapshot IDs bind the fixture and normalized input. The framework and workflow versions are recorded; no hidden reasoning or model confidence is stored.
- Every run is marked synthetic and draft, regardless of fixture status metadata. Post-COVID selection, mood screening, and urgency handling remain inactive.

The concrete demonstration rules are deliberately in the engine, not a general-purpose guidance-pack interpreter. The broader schema/approval registry design is not implemented by this prototype. Real source-document snapshots and clinical-source hashes are still pending; hashes shown for the synthetic fixture are not hashes of guideline PDFs.

## Checks

```sh
PHV_TEST_BINARY="$PWD/target/debug/phv" node --test prototype/fatigue/engine.test.mjs prototype/fatigue/server.test.mjs prototype/fatigue/vault-adapter.test.mjs
```

The suite checks behavior and resolving traces, corrections, uncertainty, input errors, synthetic-only restrictions, and preview isolation. Browser verification and observed limitations are recorded in [the implementation report](../../docs/superpowers/plans/2026-11-06-fatigue-prototype-validation.md).

## Files and next boundary

- `fixture.json`: entirely synthetic excerpts and framework metadata.
- `engine.mjs`: pure asynchronous evaluation shared by browser and CLI.
- `app.mjs`, `index.html`, `styles.css`: in-memory guided conversation and review.
- `server.mjs`: loopback-only static preview; `cli.mjs`: headless demonstration.

The read-only synthetic vault integration is implemented below. The next clinical boundary is a reviewed fatigue pack and its eligibility checks. Source selection does not become clinically reliable merely because the citation graph resolves.


## Read-only vault integration

Build the current CLI, create a new vault outside this code repository using the validated import path, then start the preview against it:

```sh
cargo build --locked
node prototype/fatigue/create-synthetic-vault.mjs /tmp/phr-fatigue-vault "$PWD/target/debug/phv"
node prototype/fatigue/server.mjs --port 4281 --vault /tmp/phr-fatigue-vault --phv-bin "$PWD/target/debug/phv"
```

Use a new destination path when repeating the import. The helper creates fictional FHIR resources and a fictional original text document, then uses `phv init`, `phv add`, `phv source add`, and `phv check`. It does not bypass vault validation or modify the bundled demo vault.

Open [the vault preview](http://127.0.0.1:4281/). Import the snapshot, inspect the original source and structured fields, then check the records you reviewed. Leave the sleep extraction unchecked. Continue, type a concern, answer the follow-ups, inspect the generated brief, and correct a statement. The correction becomes a patient report while its original evidence remains pinned. The unrelated flagged result is excluded even when reviewed. Unchecked records stay deferred.

The browser imports a copy into memory. Review choices, answers, exclusions, and corrections never write back to the vault and disappear on reload. Re-import starts a new session and clears them. Before each brief revision, the preview compares the current vault fingerprint with the imported one. Changed or unavailable input blocks a new revision and retains the preceding draft. Re-import and review the new snapshot to continue.

`phv snapshot` opens an existing `.phv/lock` read-only and takes a shared lock, with a two-second busy timeout. All ordinary phv writers hold the matching exclusive lock. It hashes configuration, raw record lines, record files and referenced original files; the stable identity excludes capture time and includes vault binding. Original source filenames must match their content hashes. The snapshot command creates no directories or lock files and works with read-only vault files. A pristine or cloned vault with no lock must first have a normal vault write; the adapter does not create a lock on its behalf. Advisory locking coordinates phv writers; external editors must not modify the vault during capture. Per-file and total read limits are 16 MiB and 64 MiB.

The JSON snapshot is a read interface, not a substitute for `phv check` or full profile validation. Its symptom mapping is deliberately narrow: patient-bound Condition fatigue codes, MedicationStatement, and selected Observation LOINC codes. Other candidates are excluded or accounted for in coverage. Base FHIR validation belongs to ingestion. Date uncertainty and missing document provenance stay explicit. A matching fatigue code is a draft retrieval rule, not proof of a documented connection between episodes. Every clinical record and the sole Patient must carry `meta.tag` with system `urn:phv:demo` and code `synthetic`; adult Portugal context is required. This prototype does not enable personal clinical use.

The trace includes vault snapshot identity, capture time, adapter/mapping versions, patient binding, raw file hashes, per-record hashes and session review states. These hashes describe the synthetic vault, not clinical-guideline documents. Framework approval remains draft.

Run the full integration suite after building with the `PHV_TEST_BINARY` command above. Without that environment variable, the real-import test is explicitly skipped. Rust coverage lives in `tests/snapshot.rs`. CI builds the binary and runs the Node integration suite as well as the Rust tests.
