# phv vault core design

phv (personal health vault) keeps one person's health records as FHIR R4 files on their own machine and uses Git for history. This spec covers the first sub-project, the vault core: the library, CLI and REST API that validate, write and read resources.

## Goals

- Store health records as normalized FHIR R4 in plain files that a person can read, diff and back up.
- Let AI map documents and photos to FHIR, while only deterministic code validates and writes the vault.
- Ship as a single Rust binary that runs on a laptop with no external services.
- Keep Git diffs small: a change to one resource shows up as a change to one line.

## Non-goals for the vault core

| Not in this sub-project | Where it goes |
|---|---|
| AI mapping skills for photos, visit notes and receipts | Sub-project 2 |
| Health memory (mood, sleep, diet) and AI access through MCP | Sub-project 3 |
| IPS and narrative summaries, IPS profile validation | Sub-project 4 |
| HealthKit, Strava and other importers | Sub-project 5 |
| Parquet warehouse through fhir-pipes-rs, web dashboard, browser upload | Sub-project 6 |
| Hosted or remote access | Deferred, laptop only |
| Encryption at rest, Git LFS | Deferred, see Security |
| More than one person in a vault | Not planned, use one vault per person |

## Two repositories

| Repository | Contents | Visibility |
|---|---|---|
| `recodelabs/phv` | Code only | Public |
| Personal data repo, one per person | Records and source files | Private, owned by the person |

phv finds the data repo in this order: `--vault PATH`, the `PHV_VAULT` environment variable, then `vault` in `~/.config/phv/config.toml`.

`phv init PATH` creates a data repo layout. It refuses to run inside a checkout whose `origin` remote is `recodelabs/phv`, and every command refuses a directory without `phv.toml`. This guards against personal data reaching the public code repo. The code repo's `.gitignore` also excludes `/records/`, `/sources/` and `/phv.toml`.

phv never commits, pushes or runs Git in the data repo. The person reviews with `git diff` and commits what they accept, so an uncommitted change is an unreviewed change.

## Data repo layout

```text
my-health-data/
├── phv.toml                     # format_version = 1, fhir_version = "4.0.1"
├── .gitignore                   # ignores .phv/
├── .phv/lock                    # write lock, not committed
├── records/
│   ├── Patient.ndjson
│   ├── Observation.ndjson
│   ├── DocumentReference.ndjson
│   └── Provenance.ndjson
└── sources/
    └── <sha256>.<ext>           # original files, never modified
```

A command refuses to run if `format_version` in `phv.toml` is newer than the binary supports.

## NDJSON format

Each `records/<ResourceType>.ndjson` file holds one resource per line.

- **Key order:** at the top level, `resourceType`, then `id`, then the remaining keys in bytewise order. Nested objects use bytewise order for all keys.
- **Whitespace:** none outside strings, and every line ends with `\n`.
- **Line order:** bytewise by `id`. Because every line starts with the same prefix and `"` sorts below every legal id character, `LC_ALL=C sort -c` verifies the order.
- **Numbers:** kept as written, so `1.50` stays `1.50`. This uses `serde_json`'s `arbitrary_precision` feature. Exponents are normalized (`1e2` becomes `1e+2`) with the same value.
- **Volatile fields:** `meta.lastUpdated` and `meta.versionId` are removed on write, because Git holds the history. An empty `meta` is removed too.

## Keeping Git diffs small

A write rewrites the whole file for each affected resource type. That is simple and Git handles it well, provided that lines which did not change stay byte-identical.

Rules:

1. A write touches only the files for resource types in that write, and only when their content changes.
2. A write that changes nothing leaves the file untouched.
3. Rewrites copy the raw text of other lines. They never parse and re-serialize them, so a later change to the serializer cannot rewrite existing data.
4. phv keeps no manifest, index or timestamp file in the data repo.
5. Any change to the format needs a new `format_version` and an explicit migration command. No ordinary command reformats existing lines.

Full rebuilds are for the analytics warehouse only, which is generated and stays out of Git.

We checked this against a real Git repo with a synthetic file of 100,000 resources (32 MB), rewriting the whole file for every change:

| Action | Result in Git |
|---|---|
| Rewrite with identical bytes | No change |
| Change one value | 1 line added, 1 removed |
| Add one resource mid-file | 1 line added |
| Reformat the same data | 100,000 lines added and removed, which is what rule 3 prevents |
| Two branches edit different lines | Merges cleanly and stays sorted |
| Two branches edit the same line | Conflict, as it should be |
| 30 small edits | 12 MB of `.git` before `git gc`, 816 KB after |

The synthetic lines were very similar, so real data will compress less well.

## Resource IDs

An ID is the first 32 hex characters of the SHA-256 of the canonical resource with `id` and volatile `meta` fields removed, prefixed with the resource type before hashing. References to other resources in the same submission are replaced with a fixed `Type/#` token before hashing, so an ID does not depend on temporary names chosen by the mapper.

- Adding the same content twice gives the same ID and changes nothing.
- Two identical resources in one submission collapse into one.
- A resource keeps its ID when replaced.
- An `id` in a submitted resource is only a temporary name within that submission.

## Write path

AI skills and other callers submit FHIR JSON. phv treats it as untrusted.

1. **Parse** one resource or a Bundle of type `transaction` or `collection`. Only `entry.resource` and `entry.fullUrl` are read from a Bundle.
2. **Assign IDs** and rewrite references to `urn:uuid:` full URLs and temporary IDs within the submission.
3. **Validate** every resource, as described below. Any error rejects the whole submission and writes nothing.
4. **Check references.** A relative `Type/id` reference must resolve in the submission or the vault. Absolute URLs and `#contained` references are left alone. A vault holds at most one `Patient`.
5. **Add Provenance** when a source is given, as described below.
6. **Write** under the lock. Each affected file is written to a temporary file in the same directory, flushed, then renamed over the original.

A crash between two renames can leave some files updated. These changes are uncommitted, so `git status` shows them and `git restore` undoes them, and `phv check` reports any dangling references.

## Validation

Validation applies base FHIR R4 on every write. It deliberately does not require IPS or other profiles, because real records are often partial and should still be stored. IPS is checked when generating an IPS document in sub-project 4.

The checks, in order:

| Check | Provided by |
|---|---|
| Structure, cardinality, types, formats | `octofhir-fhirschema` with embedded R4 schemas |
| FHIR invariants | `octofhir-fhirpath` as the evaluator for `octofhir-fhirschema` |
| Required code bindings | `InMemoryTerminologyService` loaded from generated data |
| One `value[x]` choice per element | phv, walking the schema's `choices` |
| `id` format | phv |
| Local references resolve | phv |

Findings from a spike on 2026-11-06 with `octofhir-fhirschema` 0.3.37 and `octofhir-fhirpath` 0.5.8 shape this design:

- **Required bindings:** the validator skips them unless it has a terminology service, and silently skips value sets the service does not know. A build script generates `data/required-valuesets.json` from `hl7.fhir.r4.core` for bindings that can be fully enumerated, keyed by URL with the `|4.0.1` suffix. A test lists required bindings that are not covered, so the gaps are visible.
- **Choice types:** the validator accepts `valueQuantity` and `valueString` together, so phv checks this itself.
- **False `tim-5` failure:** `Timing.repeat` without `period` fails with a FHIRPath type error. phv reports constraint results that contain `evaluation failed` as warnings and writes the resource. This logic lives in one function with a test, so it can be removed when the upstream bug is fixed. Constraints that evaluate to false remain errors.
- **`id` format:** the validator does not enforce it, so phv does.

Dependencies are pinned to exact versions (`=0.3.37`, `=0.5.8`), and `Cargo.lock` is committed, because these crates change often. The project needs Rust 1.98 or newer, and `rust-toolchain.toml` pins the tested 1.99.1.

Default features are disabled on both octofhir crates, keeping HTTP clients out of the build. A test fails if `reqwest`, `ureq` or a SQLite crate enters the dependency tree. Axum's server dependencies are expected and allowed.

## Sources and Provenance

`phv source add FILE` copies the file to `sources/<sha256>.<ext>` and creates a `DocumentReference`:

- `identifier`: system `urn:phv:sha256`, value is the file's SHA-256.
- `status`: `current`.
- `content[0].attachment`: `contentType` from the extension, and `url` set to the relative path.
- `subject`: the vault's `Patient`, when one exists.

Adding the same file again changes nothing and reports the existing `DocumentReference`, which warns the person that it has already been imported. Re-running AI on a source may still produce different resources, so the warning is what stops accidental duplicates.

`phv add --source <sha256> --agent NAME` also writes a `Provenance` linking the new resources to that `DocumentReference`, with `agent.who.display` set to `NAME`. `--agent` is required with `--source`.

`Provenance.recorded` comes from `--recorded-at`, defaulting to the current UTC time. The clock is read only in the CLI and API layer and passed into the library, so the library stays deterministic. The `Provenance` ID hashes the source hash, the sorted target references and the agent. If that ID already exists, nothing changes.

## Reads

Reads load the NDJSON file for a resource type from the working tree. There is no database or index. The spike scanned 100,000 resources in about 120 ms in a release build, which is enough for a personal vault.

Filtering uses FHIRPath expressions from `--where` or the `where` query parameter, evaluated against each resource and treated as true if the result is `true`.

Dashboard and analytics queries belong to the warehouse in sub-project 6, built from the last commit so unreviewed changes stay out.

## Interfaces

One package holds the library and a `phv` binary. Modules: `canon` (serialization and IDs), `store` (files and locking), `validate`, `write`, `read`, `cli`, `api`. The CLI calls the library directly and does not need a running server.

### CLI

```text
phv init PATH
phv validate FILE|-
phv add FILE|- [--source SHA256 --agent NAME] [--recorded-at TIME] [--dry-run]
phv replace Type/id FILE|- [--dry-run]
phv rm Type/id
phv get Type/id
phv list Type [--where EXPR]
phv source add FILE
phv check
phv serve [--port N]
```

`phv rm` refuses to remove a resource that another resource references. Removing a line does not remove it from Git history; deleting it fully needs a history rewrite by the person.

`phv check` validates every line, and checks line order, ID uniqueness, dangling references and that every `DocumentReference` has its source file. It reports problems and changes nothing. After a manual Git merge, this is the way to confirm the vault is sound. A write refuses to touch a file that is out of order and points to `phv check`, rather than quietly rewriting it.

`--json` gives machine-readable output on every command. Exit codes follow fhir-pipes: 0 for success, 1 when validation or `phv check` finds problems or a runtime failure occurs, and 2 for an error that can be fixed before retrying, such as a bad flag, malformed JSON or a missing vault.

### REST API

`phv serve` uses Axum and binds to `127.0.0.1` only, with no option to change that in this sub-project.

| Route | Purpose | Success |
|---|---|---|
| `POST /validate` | Validate without writing | 200 |
| `POST /resources` | Same as `phv add`; query parameters `source`, `agent`, `recorded_at` | 200 with created and unchanged lists |
| `GET /resources/{type}/{id}` | Read one | 200 |
| `PUT /resources/{type}/{id}` | Same as `phv replace` | 200 |
| `DELETE /resources/{type}/{id}` | Same as `phv rm` | 204 |
| `GET /resources/{type}?where=EXPR` | List, as NDJSON | 200 |
| `POST /sources` | Same as `phv source add`, raw body, `filename` query parameter | 200 |

Errors return a FHIR `OperationOutcome`: 400 for malformed input, 404 for a missing resource, 409 for reference conflicts and 422 for validation errors. Each issue carries the validator's code and the element path.

The server rejects requests whose `Host` header is not `127.0.0.1` or `localhost`, and any request with an `Origin` header from elsewhere. This stops a web page in the person's browser from reaching the API.

## Security

- **AI output is untrusted:** a mapper can only propose resources through validation. It has no path to edit files through phv.
- **No network use:** the core makes no outbound connections, and the test above keeps HTTP clients out of the build.
- **Data at rest:** files are readable on disk. For this sub-project, the person relies on full-disk encryption and a private repo.
- **Remote copies:** pushing to a hosted private repo gives that host readable health data. Encryption before push must be solved before we recommend hosted backups. Until then, the docs will say so plainly.
- **Git permanence:** deleted data stays in history until the person rewrites it, and in any existing remote copy after that.

## Testing

- **Canonical output:** golden tests for key order, whitespace and numbers (`1.50`, `0.10`, `100.0`, large decimals).
- **Small diffs:** after each write, confirm that only the expected lines and files changed, using real Git in a temp directory. Confirm that a no-op leaves the file untouched and that a hand-altered line in the file survives another write byte for byte.
- **Idempotency:** the same submission twice gives the same bytes, regardless of key order, temporary names and insert order.
- **Validation:** for each resource type used in the first skills (Patient, Observation, DiagnosticReport, Condition, MedicationStatement, AllergyIntolerance, Immunization, DocumentReference, Provenance), one valid case and broken cases. Regression tests pin each spike finding.
- **Rejection:** an invalid resource in a Bundle writes nothing at all.
- **Interfaces:** CLI and API give the same results for the same input. The API rejects bad `Host` and `Origin` headers.
- **fhir-pipes compatibility:** CI runs `fhir-pipes views run` over a sample vault's `records/` directory.
- **Build guard:** `cargo tree` check for HTTP clients and SQLite.

Test data is synthetic. Real health data never goes in this repo.

## Later sub-projects

Each gets its own spec and plan.

1. **Vault core:** this spec.
2. **Document ingestion:** AI skills that map photos, lab reports, visit notes and receipts to FHIR JSON and submit it through `phv add`.
3. **Health memory and AI access:** attributed entries for mood, sleep and diet, with AI inferences labeled as such and left out of doctor summaries by default, plus scoped AI access through MCP.
4. **Summaries:** IPS documents and narratives. Validate each entry against its IPS profile, loaded from package snapshots, add Bundle-level checks in phv, and run the HL7 Java validator in CI. The spike showed that octofhir's full Bundle profile check is not usable yet.
5. **Importers:** HealthKit, Strava and others as plain code, storing daily rollups as resources and raw samples outside `records/`.
6. **Warehouse and dashboard:** a Parquet warehouse built with `fhir-pipes refresh` from the last commit, a small React dashboard and browser upload.
