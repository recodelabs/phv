# Apple activity import design

`phv import apple-activity FILE` turns an `activity-<date>.ndjson` file from the HealthKit export app into FHIR R4 `Observation` resources that follow the [Physical Activity IG](https://build.fhir.org/ig/HL7/physical-activity/) (`hl7.fhir.us.physical-activity#1.0.1`).

This is the first importer from sub-project 5 of the [vault core design](2026-11-06-vault-core-design.md). It is plain code with no AI, and only the library writes to the vault.

## Pipeline

```text
export file -> 1. read -> 2. triage -> 3. convert -> 4. validate -> 5. write
                  |          |
             no FHIR yet   reads the vault, decides what is worth converting
```

Everything through triage works on the export's own rows. The converter only sees rows that triage admits, so re-importing a full history costs a vault read and no conversion or validation.

## Reading the file

Each export is a full history snapshot, not an increment, so most of a later file is already in the vault.

The whole file is refused, and nothing is written, when:

- the last line is not a `summary`, because that means the file was cut off
- there is more than one `summary`
- the `summary` counts do not match the rows in the file
- a line is not valid JSON, or has an unknown `kind` or a `v` other than 1

A single row with a bad value, such as a negative step count, is rejected and reported, and the rest of the file continues. Non-empty `failures` in the summary are reported as a warning, because a failed query in the app means rows are missing from this file. phv never removes a stored record because a later export left it out.

## Finding duplicates

Triage has three layers.

### 1. Repeats in the file

Rows are keyed on `date` for days and `uuid` for workouts. Identical repeats collapse to one. Two different rows with one key cannot both be right, so neither is imported, and the key is reported as a conflict.

### 2. Rows already in the vault

Every resource carries its natural key as an `identifier`, and its ID is derived from that key rather than its content, so a changed row updates in place. The day or workout resource also holds the SHA-256 of its source row in `meta.tag` (system `urn:phv:source-row`). Triage compares a row's hash to the stored one without building any FHIR:

| Stored record | Row | Outcome |
|---|---|---|
| None | | Convert as new |
| Same hash | | Skip as unchanged |
| Different hash, day, same time zone | Total changed, such as a late Watch sync | Convert and update in place |
| Different hash, day, different time zone | The app's home zone changed, which moves every day's boundaries | Keep the stored record and report it |
| Different hash, workout | | Convert and update in place, removing members it no longer has |

Comparing the source hash and not the FHIR means a later change to the mapping does not rewrite stored lines, which the [vault core's small diff rules](2026-11-06-vault-core-design.md#keeping-git-diffs-small) require. Moving a mapping change into stored records would need an explicit migration.

### 3. One session from two sources

A Watch and a Peloton or Strava can record the same ride as two workouts with different UUIDs. HealthKit does not merge them, and counting both doubles the minutes and calories. The first real export had 20 such pairs and no other duplicates.

Two workouts are the same session when they have the same `activityType`, come from different sources, and overlap for at least half of the shorter one. A workout from one source never matches another from that source.

Only new workouts are checked, in this order, each kept unless it is the same session as one already kept:

1. Everything already in the vault, so a stored record is never displaced and a later export cannot change which one was kept.
2. New workouts, best first: most optional measures (`distanceMeters`, and `energyKcal` unless it is 0), then the longest `durationSeconds`, then the lowest `uuid`.

Skipped workouts are reported with the one they matched, on every import. The original rows stay in the export file. `--keep-overlaps` turns this layer off.

## FHIR mapping

All resources have `status: final`, both IG categories (`activity`, `PhysicalActivity`), `subject` and `performer` set to the vault's `Patient`, `meta.profile`, and `meta.source` of `urn:phv:apple-health:activity-export`. An import stops if the vault has no `Patient`.

### Daily steps

One [PA Observation - Supporting, Time-based](https://build.fhir.org/ig/HL7/physical-activity/StructureDefinition-pa-observation-time-measure.html) per day:

| Element | Value |
|---|---|
| `identifier` | `urn:phv:apple-health:daily-steps` / the date |
| `code` | LOINC `41950-7` Number of steps in 24 hour Measured |
| `effectiveDateTime` | The date, with the [`tz-code`](http://hl7.org/fhir/StructureDefinition/tz-code) extension holding `timeZone` |
| `valueQuantity` | The steps, UCUM `/d` |

### Workouts

One [Activity Group](https://build.fhir.org/ig/HL7/physical-activity/StructureDefinition-pa-observation-activity-group.html) panel (code `PAPanel`, no value) with `hasMember` pointing at one [Activity-based](https://build.fhir.org/ig/HL7/physical-activity/StructureDefinition-pa-observation-activity-measure.html) Observation per measure. All share an `effectivePeriod` from `start` and `end`, in UTC as exported, and the source's name in `device.display`. `timeZone`, when present, is a `tz-code` extension on the panel's `start`.

| Measure | Code | Value | When |
|---|---|---|---|
| Activity type | LOINC `73985-4` | See below | Always |
| Duration | LOINC `55411-3` | `durationSeconds` as `min`, to 2 places | Always |
| Calories | LOINC `55424-6` | `energyKcal` as `kcal` | If present and not 0 |
| Distance | LOINC `55412-1` | `distanceMeters` as `m` | If present |

The panel's identifier is `urn:phv:apple-health:workout` / the UUID, and each member's is `urn:phv:apple-health:workout-measure` / `<UUID>/<code>`.

`durationSeconds` is active time, and it can be shorter than `start` to `end` when a workout was paused, so the two are kept separately.

### Choices the IG leaves open

- **Distance:** the IG has no distance measure. Its activity code binding is `extensible`, which allows another code when the value set has no suitable one. `55412-1` is the same LOINC family as the IG's `55411-3`.
- **Activity types:** the IG's answer list `LL734-5` has only Bicycling, Jogging, Running, Swimming, Walking, Weights and Mixed. There is no "Other". The value always has a `urn:phv:apple-health:workout-activity-type` coding with Apple's name and readable `text`. A LOINC coding is added only where the meaning matches:

| Apple | LOINC answer |
|---|---|
| `walking` | `LA11834-1` Walking |
| `running` | `LA11836-6` Running |
| `cycling` | `LA11837-4` Bicycling |
| `swimming` | `LA11838-2` Swimming |
| `functionalStrengthTraining`, `traditionalStrengthTraining` | `LA11839-0` Weights |
| `mixedCardio` | `LA11840-8` Mixed |
| Anything else, such as `hiking` or `yoga` | None |

- **Time zones:** `timeZone` is kept with the standard extension so day boundaries stay explainable. Step rows carry the app's home zone, which can differ from where the steps were taken.

## Writing

The import holds the vault lock from triage through the write. Each row is validated with the vault's base R4 validator, and a row with an error is rejected alone, with all of its resources. A workout is never written without all its members.

`--dry-run` does everything except the write, and refuses what a real import would refuse. The same file imported twice leaves the vault byte-identical.

Exit codes follow the vault core: 2 for a refused file or a vault with no `Patient`, 1 if any row was rejected or conflicted, and 0 otherwise. `--json` prints the report.

## Known limits

- **Profiles are not enforced at runtime.** The vault's validator checks base R4 only. `tests/apple_activity_ig.rs` validates the output against the IG's own profile snapshots with octofhir, and asserts directly the rules octofhir 0.3.37 does not enforce from a snapshot: the panel's fixed code and prohibited value, `Period` start and end, the unit system's value, the quantity code and the time-based profile's prohibited `CodeableConcept`. The HL7 Java validator has not been run on this output.
- **Value sets are not checked by a validator.** The tests pin which codes are used, and the LOINC answers and displays were looked up on `tx.fhir.org` and Termbox on 2026-11-06.
- **No source archive or Provenance yet.** The vault core can now store a source file and link records to it, but this importer does not call it. `meta.source` and the identifiers show where a record came from. It should store the file and add a `Provenance` when an import writes something.
- **Deleted workouts stay.** The export does not say what was deleted in Health, so a workout removed there stays in the vault until `phv rm`.
- **`Weights` is approximate** for functional strength training, which may use body weight. Apple's own type is kept next to it.
