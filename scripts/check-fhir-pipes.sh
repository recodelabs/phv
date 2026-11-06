#!/usr/bin/env bash
# Confirms fhir-pipes can read a phv vault's records. Needs fhir-pipes on PATH.
set -euo pipefail

output=$(fhir-pipes views run tests/fixtures/observation_view.json --input tests/fixtures/sample-vault/records/Observation.ndjson)
echo "$output"
echo "$output" | grep -q 'Creatinine' || { echo 'missing Creatinine row' >&2; exit 1; }
