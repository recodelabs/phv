# Synthetic fatigue prototype: implementation and validation

Date: 2026-11-06. Status: implemented local demonstration; clinical approval pending. This report records observed checks rather than claiming production readiness.

## Delivered

- Buildless local browser interface and the same deterministic engine available through a CLI.
- Eight synthetic candidate records: six selected, one unrelated flagged result excluded, one unreviewed extraction deferred.
- Three essential follow-ups (onset, impact, current medication account) and an optional consultation-goal prompt. Absent, uncertain and skipped answers remain distinct.
- Traceable claims linked to source excerpts, decisions or attributed answers, including exact adapted guideline locators.
- Correcting answers updates the brief and input identity. Editing source-derived wording creates a patient report. Removing a dated medication source preserves an independently supplied current account.
- Source inspection, selection rationale, remove/restore, optional evidence appendix, copy/text/trace download controls, and browser print/save-PDF support.

## Automated verification

Command: `/opt/homebrew/bin/node --test prototype/fatigue/engine.test.mjs prototype/fatigue/server.test.mjs`.

Result: **21 tests passed**, rerun before handoff. The suite covers resolving claim/evidence/decision/answer references, deterministic input/evidence hashes, corrections without source mutation, uncertainty, exclusions and restoration, sparse records, prompt-like text as data, input validation, synthetic-only restrictions and source identities. Preview checks verify asset allowlisting, loopback Host/Origin checks, read-only methods and correct response types.

The CLI generated the illustrative completed brief successfully. Syntax checks passed for the interface. These checks do not establish clinical correctness or validate a general guidance-pack runtime; that runtime is not implemented.

## Built-in browser walkthrough

The actual manual-input path was exercised in the Codex built-in browser at the local preview:

1. Submitted an empty concern, saw its error, then typed a synthetic concern and proceeded.
2. Submitted a blank onset answer, saw its error, then entered an approximate date.
3. Entered daily impact, chose Not sure for medication status and Skip for the optional goal. The partial brief preserved both states.
4. Inspected a TSH claim, its exact synthetic source excerpt and its adapted DEGAM 5.3.1 / PDF p15 selection reference.
5. Returned to the onset answer, changed it from approximately September to approximately August, and observed the corrected patient-attributed statement in the brief.
6. Tried an empty statement correction, saw the error, then saved a nonempty correction. It became an attributed patient report.
7. Tried removing the edited source, chose to keep it, then explicitly confirmed removal. The dependent corrected statement disappeared. Restoring the evidence recovered the original source wording.
8. Changed the evidence-appendix checkbox.

A visible `null` caused by passing an absent alert to `replaceChildren` was found and fixed. The page was reloaded after the fix and the input/correction walkthrough repeated against the fresh DOM.

## Verification limits and known scope

- Browser download-event verification stalled in the browser tool and was interrupted. Successful downloaded files, clipboard behavior, print-dialog output and console cleanliness were not verified in that run. Do not claim that they were. The controls are implemented; export completion remains a follow-up manual check.
- The preview process ended after interruption. Run the README start command to reopen it; no hosted site was published.
- The installed Homebrew Node v23.11.0 runs the prototype. The app's bundled browser Node exhibited a macOS 13.1 compatibility error; a repair enabled the recorded walkthrough, but that environment repair did not persist after interruption. This does not prevent running the local server in a normal browser.
- Free text is preserved rather than medically interpreted. Structured follow-ups may repeat information already written in the concern. At that earlier checkpoint there was no LLM, vault adapter, persistent session storage, scored screening or triage. The synthetic vault adapter is now implemented as recorded below.
- PDF uses the browser's Print / Save PDF path; no standalone PDF renderer is implemented.
- Framework status remains draft, source PDF snapshots/hashes are not captured, and Portuguese clinical review is outstanding.

## Next work

The integration and export/console follow-up are recorded below. Remaining work is a reviewed guidance-pack eligibility boundary before personal clinical use, broader FHIR mapping, and evaluation of question usefulness and the two-minute target with representative users.


## Read-only vault integration verification

Date: 2026-11-06. Implementation base: upstream `586d891` plus local snapshot/adapter changes. The earlier export and adapter limits above describe the initial checkpoint; this section records the current verification.

Delivered `phv snapshot`, the Node vault adapter, synthetic import helper and explicit browser review step. Snapshot capture opens the existing lock read-only and shares the same advisory lock as phv writers. It produces a vault-bound stable identity, capture time, raw-record hashes, record-file hashes and original-source hashes. Original source content is checked against its filename hash. Snapshot reads create no files. The preview accepts one startup-configured vault and serves only synthetic adult Portugal data. Review state and brief corrections stay in browser memory.

Automated results:

- Full `cargo test --locked`: passed on this Mac using the repository-pinned Rust 1.99.1. The five new snapshot tests passed again after tightening the concurrent-write check.
- `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings`: passed.
- Full Node suite with `PHV_TEST_BINARY` set: **26 passed, zero failed, zero skipped**. This includes a real `phv init` / Patient add / source add / clinical Bundle add with Provenance / whole-vault check, followed by adapter import, deferred evidence, explicit review, selection, brief, correction and HTTP checks.
- Integration checks compare the entire vault file tree (content, modification time and permissions) before and after reads, reviews and corrections. No vault file changed. A subsequent deliberate CLI source import changed the snapshot identity; previously captured evidence remained unchanged.
- Rejection checks cover unmarked data, mismatched/duplicate patient records, unsupported patient context, missing provenance documents, entered-in-error observations, missing or tampered source bytes, symlinked records and missing lock files. The writer-lock test confirms that capture waits and sees the completed write.

Built-in browser verification against fresh files:

1. Imported eight clinical evidence items from the generated synthetic vault. Inspected the original text document with its SHA-256 and the exact structured record locator. All items initially showed unreviewed.
2. Reviewed seven items, left the sleep extraction unchecked, unchecked and rechecked glucose, and verified the review count.
3. Submitted an empty concern and an empty onset answer, observed both errors, then corrected them using manual input.
4. Entered approximate onset and daily impact, selected Not sure for medication and Skip for the optional goal. Generated a brief containing six selected record facts; the unrelated flagged result stayed excluded and sleep stayed deferred.
5. Followed the blood-count claim to its imported record. Tried a blank correction, observed its error, and saved a nonempty correction. The new wording showed patient-report attribution while the original record remained available.
6. Downloaded text and trace JSON from revision 6, checked matching content and appendix, verified the correction's original wording and all eight review entries, and checked copied text through the browser clipboard. Snapshot/file hashes still matched the actual vault afterward.
7. Deliberately imported another synthetic source through the CLI. Attempted a browser correction against the old snapshot. The correction dialog displayed the stale-vault error and the preceding draft remained unchanged. Re-import cleared answers, corrections and reviews; a new brief generated with all records unchecked contained only the attributed concern and no record facts.
8. Inspected the layout and fresh browser DOM; no console warnings or errors were recorded. An absent dialog child initially displayed as `null`; it was fixed and the source-review/correction flow repeated after reloading.

Scope: this is a read-only integration with generated synthetic vault data. Clinical mappings remain narrow and draft. There is no personal clinical mode, persistent review/brief store, AI call, general guidance interpreter or runtime deployment. Browser Print / Save PDF remains implemented but the native print dialog and produced PDF were not verified in this update. The preview and synthetic vault are retained locally for review; temporary build dependencies are removed after retaining the working CLI binary in ignored `target/debug/phv`.


## PR verification after updating the base

Rebased onto upstream main `1516558` on 2026-11-06. Fresh local results with Rust 1.99.1: **337 Rust tests passed**, zero failed or ignored; **26 Node tests passed**, zero failed or skipped, including validated synthetic import through the rebuilt CLI. Formatting, Clippy with warnings denied, and `git diff --check` passed. The interface files are unchanged from the built-in-browser verification above.


## CI compatibility fixes

Updated the PR branch with upstream IPS support from main `84dd95c`. The independent Java validator initially could not start because its shell script was not executable. After correcting that mode, terminology-disabled validation ambiguously matched laboratory and vital-sign observations to pregnancy slices. The CI script now uses the official HL7 R4 terminology service for its generated synthetic fixture. Four IPS section display names were corrected to the accepted LOINC labels and the golden document was regenerated through the real generator; clinical record values did not change.

Fresh local verification: **368 Rust tests passed**, **26 Node tests passed**, zero failed or skipped; formatting and Clippy passed. The exact repaired `check-ips-java.sh` completed successfully with Java 21 and validator 7.0.2: zero errors. A separate negative control with a required vital-sign UCUM code removed failed with the expected minimum-cardinality error. No validator error suppression or profile relaxation was added. The independent check requires the terminology service; its input remains the generated synthetic IPS only.
