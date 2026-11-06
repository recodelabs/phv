# IPS generation design

`phv ips` builds an [International Patient Summary](https://hl7.org/fhir/uv/ips/STU2/) document (`hl7.fhir.uv.ips#2.0.0`) from one vault. It is sub-project 4 of the [vault core design](2026-11-06-vault-core-design.md), covering the IPS part. Narrative summaries written by AI are not in scope.

The generator is plain code with no AI and no network use. It reads the vault and changes nothing in it. The same vault and the same time always give the same bytes.

## Interfaces

| Interface | Form | Output |
|---|---|---|
| CLI | `phv ips [--out FILE]` | The Bundle as pretty JSON on stdout, or written to `FILE`. Left-out records go to stderr. With `--json`, prints `{bundle, omitted, warnings}` |
| REST | `GET /ips` | The Bundle as `application/fhir+json`. An `x-phv-warning` header counts left-out records. `?report=true` returns `{bundle, omitted, warnings}` |
| MCP | `ips` tool | The Bundle, plus a second content item listing left-out records |

`--out` refuses a path inside the vault, so a summary never lands in `records/`. The IPS is derived from the records, so storing it would only duplicate them. Leaving records out is a reported outcome, so the exit code is still 0.

The clock is read in the CLI, API and MCP layers and passed to the library as `generated_at`.

An IPS holds the person's whole clinical summary. Through MCP it goes to Anthropic, and the tool description says so.

## Sections

Records are chosen by type, status and category. `entered-in-error` records are never included, and neither are `refuted` Conditions and allergies.

| Section | LOINC | Records |
|---|---|---|
| Problems (required) | 11450-4 | `Condition` with clinical status active, recurrence or relapse, or no status |
| Allergies (required) | 48765-2 | `AllergyIntolerance` with clinical status active or no status |
| Medications (required) | 10160-0 | `MedicationStatement` with status active, intended, on-hold or unknown, and `MedicationRequest` with status active, on-hold or unknown |
| Immunizations | 11369-6 | `Immunization` with status completed |
| Results | 30954-2 | `Observation` with category laboratory or imaging, and `DiagnosticReport`, except those cancelled |
| Procedures | 47519-4 | `Procedure` with status completed |
| Past illness | 11348-0 | `Condition` with clinical status inactive, remission or resolved |
| Vital signs | 8716-3 | `Observation` with category vital-signs, except those cancelled |
| Social history | 29762-2 | `Observation` with category social-history, except those cancelled |

Physical activity Observations from `phv import apple-activity` have the category `activity`, so they belong to no section. An IPS is a clinical summary, not an activity log.

Results and vital signs are not limited by date. Revisit this if summaries get too large.

A required section with nothing in it is still present, with `emptyReason` `unavailable` and a narrative saying so, as IPS 2.0.0 rule `ips-comp-1` expects. An empty optional section is left out.

Within a section, entries are sorted newest first by their clinical date, then by `Type/id`.

## The document

- **Entries:** the Composition, then the Patient, then each section's entries in section order, then supporting resources sorted by `Type/id`. Supporting resources are everything the entries refer to with a relative reference, followed transitively, such as Medication, Organization, Practitioner and Encounter. `Provenance` and source `DocumentReference` records are included only if an entry refers to them.
- **Full URLs:** each entry's `fullUrl` is `urn:uuid:` with a UUID derived from the SHA-256 of `Type/id`, and every reference between entries is rewritten to it. Resources keep their vault `id`, so a summary can be traced back to the vault.
- **Profiles:** each resource checked against an IPS profile gets that profile added to `meta.profile`. Other `meta` content is kept.
- **Composition:** status `final`, type LOINC 60591-5, title "International Patient Summary", `date` set to `generated_at`, and the Patient as subject and author. The person keeps their own vault, and IPS allows a Patient author.
- **Bundle:** type `document`, `timestamp` set to `generated_at`, and an `identifier` in system `urn:ietf:rfc:3986`. Its value is a `urn:uuid:` derived from the SHA-256 of the canonical Bundle without the identifier.
- **Narrative:** each section has generated XHTML, a table of its entries' names, values, dates and statuses, built only from escaped text. When records were left out, the narrative says how many. The Composition has no narrative of its own.

### Lab results and their performer

IPS lab results must name a `performer`. The lab skill puts the lab on the `DiagnosticReport` only, so every stored result would fail. In the IPS output, and only there, a laboratory Observation with no performer takes the `performer` of a `DiagnosticReport` that lists it in `result`. The vault is not changed. The lab skill now also puts the performer on each Observation.

## Checking the document

Each resource is checked against the profile for its type:

| Resource | Profile |
|---|---|
| Patient, Condition, AllergyIntolerance, MedicationStatement, MedicationRequest, Medication, Immunization, DiagnosticReport, Procedure, Organization, Practitioner, PractitionerRole, Device, DeviceUseStatement, Specimen, ImagingStudy, Flag | The IPS profile of that type |
| Observation with category laboratory | Observation-results-laboratory-pathology-uv-ips |
| Observation with category imaging | Observation-results-radiology-uv-ips |
| Observation with category vital-signs | Core `vitalsigns` |
| Observation coded LOINC 72166-2 or 74013-4 | Tobacco use or alcohol use |
| Composition | Composition-uv-ips |
| Anything else | Base FHIR R4 only, which every stored record already passed |

The profiles are snapshots from `hl7.fhir.uv.ips#2.0.0` and the core `vitalsigns` profile, with descriptive fields removed, embedded in the binary as `data/ips-profiles.json`. `scripts/gen_ips_profiles.py` generates the file. They are translated when the first IPS is built, so other commands are not slowed down.

A record that fails its profile is left out and reported with its findings. A record that refers to a left-out record is left out too, because its reference would dangle. This repeats until nothing more changes. If the Patient fails, there can be no IPS, so phv returns a validation error naming what the Patient lacks.

phv then checks the Bundle itself, because octofhir cannot check the Bundle profile:

- The Composition comes first, and the Patient is its subject.
- Problems, Allergies and Medications are present, and every section has entries or an `emptyReason` but not both (`ips-comp-1`, `cmp-2`).
- Every `section.entry` and every reference between entries resolves to a `fullUrl` in the Bundle.
- No relative `Type/id` reference is left.

A failure here is a bug in phv, so it is an error and no document is returned.

## Spike findings

From a spike on 2026-11-06 with `octofhir-fhirschema` 0.3.37 against the package's 44 examples and broken copies of them:

- **Entry profiles work.** All IPS profiles translate. 40 of 41 standalone examples pass, and removing required elements such as `Condition.code`, `Observation.category`, `Patient.birthDate` and `MedicationStatement.effective[x]` is caught.
- **The Bundle profile does not.** Every entry is reported as matching every slice (FS1008). phv makes the Bundle checks itself.
- **Slice constraints are not evaluated.** A Composition section with neither entries nor `emptyReason` passes, so phv checks `ips-comp-1`.
- **Extension slices are not told apart.** Any extension on a Patient, even the IPS `genderIdentity` extension, is reported as matching several slices (FS1008). phv drops FS1008 findings at an `extension` path, and a test pins this.
- **`tim-4` and `tim-5`** fail to evaluate, as in the vault core, and phv reports them only as warnings.
- **Text-only codes pass.** IPS bindings are not required, so a Condition with only `code.text` is valid.
- **A performer with only `display` passes** the lab result profile.
- **The core `vitalsigns` profile** does not catch a missing UCUM code. The Java validator in CI covers this.

## Testing

- A golden Bundle for a fixed synthetic vault and time, identical across runs and insert orders.
- Section selection and status rules, the empty required section form, and left-out records with their dependants.
- References all resolve to `urn:uuid:` full URLs, and narrative text is escaped.
- The vault is byte-identical after `phv ips`, and `--out` inside the vault is refused.
- CLI, API and MCP give the same Bundle.
- Pinning tests for each spike finding.
- CI generates an IPS from the fixture vault and runs the HL7 Java validator on it with `hl7.fhir.uv.ips#2.0.0`.
