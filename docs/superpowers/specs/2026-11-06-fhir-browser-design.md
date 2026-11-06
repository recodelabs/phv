# phv FHIR browser design

The FHIR browser is a local, read-only web interface for looking through a phv vault. It shows health records in everyday terms, such as vitals and lab reports, and keeps the underlying FHIR one click away. It replaces the first part of sub-project 6 in the vault core design.

## Goals

- Let a person find their current vitals, lab results, conditions, medicines, allergies, vaccines and original documents without knowing FHIR.
- Never hide a record: anything without a topic view stays reachable under All FHIR.
- Show what the record says, exactly as stored, without judging or converting it.
- Stay a single Rust binary that makes no outbound connections and needs no Node, warehouse or Git.

## Non-goals for the first version

| Not in this version | Why or where it goes |
|---|---|
| Web input of any kind: upload, edit, delete, recategorize | Agent skills and the `phv` CLI do this |
| Analytics, charts and trends | Later, on top of these pages |
| Parquet warehouse and fhir-pipes-rs | Only needed once analytics arrive |
| Timeline view | Could be added later as another view of the same data |
| Search across all records | Later, if browsing proves too slow |
| Hosted or remote access, login | Deferred, laptop only |
| Showing Git status, such as which records are uncommitted | Git is optional, and only `phv setup` runs it |

## Changes to the vault core design

- The dashboard is server-rendered HTML with htmx, not React.
- It reads the working files, not the last Git commit, so **it shows records that have not been committed yet**. Git is optional, so the last commit cannot be the source of truth for every vault.
- Browser upload is dropped from sub-project 6.
- `phv source add` gains `--category` and a `date` field. See [Changes to `phv source add`](#changes-to-phv-source-add).

## Architecture

The browser is a new `src/browser/` module, with little change to existing vault code.

| Part | Job | Depends on |
|---|---|---|
| `browser::router(vaults)` | Axum routes, GET only, security headers | `model`, templates, assets |
| `browser::model` | Turns raw NDJSON lines into plain view data. Has no HTTP or HTML in it | `store`, `canon` |
| `templates/` | Askama HTML with automatic escaping: one template per page, plus partials for htmx | View data only |
| `assets/` | One stylesheet and a pinned htmx 2.x, embedded in the binary | Nothing |

New dependency: `askama`. The existing test that keeps HTTP clients and SQLite out of the build still applies.

### Starting it

The browser ends up inside the planned `phv serve` (vault core Tasks 11 and 12), next to the REST API, on the same port. Until that exists, `examples/browser.rs` starts it, which avoids colliding with the CLI work:

```text
cargo run --example browser -- --vault tests/fixtures/sample-vault
```

### Vaults

`vaults` is a list of names and paths, loaded when the server starts.

- It comes from `~/.config/phv/config.toml`, and the first page is the default vault. A vault registered later needs a server restart.
- If a vault is given by path with `--vault` or `PHV_VAULT` and is not registered, it is the only entry and the switcher is hidden.
- The switcher lists vault names. Each page's heading shows the `Patient` name, falling back to the vault name, so the current person is always visible.
- A URL's vault name is only ever looked up in this list. Paths never come from a URL.

### Reading data

Each request reads the files it needs through `Vault::lines()`, inside `spawn_blocking`. There is no cache or index, so a change shows up on refresh. The browser never calls `Vault::lock()` or any write method.

## Routes

All routes are GET.

| Route | Page |
|---|---|
| `/` | Redirects to the default vault's overview |
| `/assets/{file}` | Embedded stylesheet and htmx |
| `/v/{vault}/overview` | Patient, latest vitals, latest lab report, what is active now |
| `/v/{vault}/vitals` | Latest value of each measure |
| `/v/{vault}/vitals/{measure}` | History of one measure |
| `/v/{vault}/labs` | Lab reports, newest first, then results not in a report |
| `/v/{vault}/labs/reports/{id}` | One report with its results |
| `/v/{vault}/labs/tests/{test}` | History of one test |
| `/v/{vault}/conditions`, `medicines`, `allergies`, `vaccines` | One list each |
| `/v/{vault}/documents?category=` | Catalog of original files |
| `/v/{vault}/sources/{sha256}` | The original file |
| `/v/{vault}/fhir` | Resource types with counts |
| `/v/{vault}/fhir/{type}?page=` | Records of one type, 200 per page |
| `/v/{vault}/fhir/{type}/{id}` | One record as formatted JSON |

`{measure}` and `{test}` are URL-encoded grouping keys, described under [Groups](#groups).

### htmx

htmx is used in only two places:

- Clicking a vital or a lab test expands its history in place, from the same history route.
- Clicking a document category updates the list and the URL, from the same documents route.

A route returns a partial when the `HX-Request` header is present and a full page otherwise, with `Vary: HX-Request`. Every page works with JavaScript turned off.

## Topics

A record's topic comes only from what the record states, never from guessing at its content.

| Topic | Records |
|---|---|
| Vitals | `Observation` with category `vital-signs` |
| Labs | `DiagnosticReport` with category `LAB`, or with at least one result that is a `laboratory` Observation. A `laboratory` Observation in no report appears under "Results not in a report" |
| Conditions | `Condition` |
| Medicines | `MedicationStatement`, `MedicationRequest` |
| Allergies | `AllergyIntolerance` |
| Vaccines | `Immunization` |
| Documents | `DocumentReference` |
| All FHIR only | Everything else, including an `Observation` with no category and a `DiagnosticReport` that is not a lab report |

Categories are matched on the code within their standard FHIR code system. The second Labs rule exists because the `phv-add-lab` skill's example `DiagnosticReport` has no category of its own.

## Showing values

- **Numbers and units:** as stored. `1.50` stays `1.50`, `<0.5` keeps its comparator, and units are never converted.
- **Dates:** as written, with no time zone conversion. A date-only value stays date-only. Records with no date sort last and show "No date".
- **Sort order:** by the date's text, then by `id`. This is correct except for times on the same day written with different time zone offsets, which is an accepted limit.
- **Names:** `code.text`, then the first coding's `display`, then its `code`.
- **Flags and ranges:** shown only when the record has them. phv never works out on its own whether a result is high or low.
- **Values with no display rule:** a result with no recognized `value[x]` shows "See FHIR", linking to the record.
- **Blood pressure:** one `Observation` with systolic (LOINC 8480-6) and diastolic (LOINC 8462-4) components shows as `118/76 mmHg`. Separate systolic and diastolic records stay separate rows.
- **Other components:** an `Observation` with other components lists each one as a line in its result.

### Groups

History pages and "latest" tiles group records by a key: the LOINC code when there is one, otherwise the exact `code.text`. Records with different names and no shared LOINC code are never merged. Rows always show their own unit, so a change of unit stays visible.

### Status

- A record with status `entered-in-error` is left out of the latest tiles, histories, topic lists and Overview. It stays visible under All FHIR.
- Topic list pages show each record's status. Vaccines marked `not-done` show "Not given".
- "Active now" on the Overview lists records with status `active` and records with no status, marked "Status not recorded". For conditions and allergies, this reads `clinicalStatus`.

### Overview tiles

There is one tile per measure, showing its newest record. Blood pressure, heart rate, body weight, height, temperature, oxygen saturation, respiratory rate and BMI come first, in that order, matched by LOINC code. Other vital measures follow by name.

### Links between records

- A lab report's results come from `DiagnosticReport.result`.
- A record's source file and agent come from a `Provenance` that lists it as a target and has a `DocumentReference` entity. A document's "Records from it" count comes from the same data.
- In the JSON view, a resolvable local reference is a link. A reference that does not resolve is shown as plain text.
- A medicine's name comes from `medicationCodeableConcept`, then the referenced `Medication`, then the reference's `display`, then "Unnamed medicine".

## Documents

The catalog lists each `DocumentReference` with its added date, file name (`attachment.title`), category and the count of records made from it, newest first. Chips filter it by category.

- **Category:** the code in `DocumentReference.category` with system `urn:phv:document-category`. A record with no such code counts as Uncategorized, even if it has a category from another system. That category is still visible in the FHIR view.
- **No file:** a `DocumentReference` with no file in `sources/`, such as a clinical note from another system, is listed with "No file stored".

### Changes to `phv source add`

These change vault core Task 8, which is not built yet. The browser works without them, showing documents as Uncategorized with "No date".

- `--category lab|receipt|visit-note|imaging|other` sets `DocumentReference.category`. Any other value is an error that can be fixed before retrying. The flag is optional.
- `date` is set to the time the file was added. As elsewhere, the clock is read in the CLI and API layer and passed into the library.
- `phv-add-lab` passes `--category lab`.
- Adding the same file again changes nothing, as now, and keeps the first category. `phv replace` corrects a category.

## Security

Record text and original files may hold anything, because they come from outside documents and AI mapping.

- **Read-only:** there are no write routes. A non-GET request gets 405.
- **Local only:** the server binds to `127.0.0.1` and rejects requests whose `Host` is not `127.0.0.1` or `localhost`, or whose `Origin` is from elsewhere, matching the REST API. The check is written in the browser module and shared with the API when `phv serve` brings them together.
- **Escaping:** Askama escapes all record text. No template marks record text as safe.
- **Page headers:** every page sends the headers below, and every response sends `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.

  ```text
  Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'
  Cache-Control: no-store
  ```

- **htmx settings:** `allowEval` is off, `selfRequestsOnly` is on and `includeIndicatorStyles` is off, so htmx works under this policy.
- **Original files:** the route accepts only 64 lowercase hex characters and looks for a regular file named `sources/<sha256>.<ext>`, refusing symlinks. A path stored in a record is never followed.
- **File types:** only PDF, PNG, JPEG and plain text open in the browser. Anything else, including HTML and SVG, downloads as `application/octet-stream`. Originals also send `Cache-Control: no-store`, and all but PDF send `Content-Security-Policy: sandbox`, because that header can stop browsers' built-in PDF viewers. Implementation must confirm PDFs open in Chrome and Safari.

## Error handling

| Situation | What the person sees |
|---|---|
| A line that cannot be parsed | The page renders the rest, with "N records couldn't be read. Run `phv check`." |
| A vault that is missing or unreadable, or whose format is newer than this phv | The switcher marks it unavailable, and its pages explain why. Other vaults keep working |
| Unknown vault, type, ID, measure or file | 404 page |
| No records for a topic | "No vitals yet", or the same for that topic, with no error |
| A referenced result that is missing | The report shows "Result not found" with the reference, and `phv check` is the fix |
| An unexpected read failure | 500 page with no record contents. The detail goes to the server's log, with no record contents there either |

## Testing

All test data is synthetic. `tests/fixtures/sample-vault/` is a small handwritten vault covering every topic, and a test runs it through the validator and `phv check` once they are merged, so it stays realistic. It stays out of the root-anchored `.gitignore` rules.

- **Model:** topic rules, including a `DiagnosticReport` with and without a category. Exact numbers (`1.50`, `<0.5`), date-only values, missing dates, blood pressure with components and as separate records, grouping by LOINC code and by text, `entered-in-error`, `not-done`, a missing status, unparseable lines and medicine name fallbacks.
- **Routes:** every page returns 200 against the fixture and 404 for unknown keys. Partials return only with `HX-Request`.
- **Read-only:** crawling every route leaves the vault byte-identical, with no `.phv/lock` created. Non-GET requests return 405.
- **Security:** a `<script>` in `code.text` comes out escaped. Bad `Host` and `Origin` headers are rejected. Required headers are present. Traversal attempts, a symlink under `sources/` and a stored HTML or SVG source do not behave as live pages.
- **Whole-page check:** run the example against the fixture in a real browser, with JavaScript on and off, and confirm that PDFs open.

## Build order

1. Merge the latest `vault-core` into this branch.
2. Router, security headers, layout, All FHIR, sample vault and `examples/browser.rs`. This is useful alone, because every record can then be browsed.
3. Overview and Vitals.
4. Labs.
5. Conditions, medicines, allergies and vaccines.
6. Documents, reading category and date when present.
7. The `phv source add` changes, once vault core Task 8 is merged.
8. Mount the browser in `phv serve`, once vault core Task 12 is merged.
