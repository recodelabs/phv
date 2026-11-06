#!/usr/bin/env bash
# Builds an IPS from the synthetic fixture vault and checks it with the HL7 Java validator,
# the independent check on phv's own IPS checks. Needs Java 17 or newer.
#
# Usage: scripts/check-ips-java.sh [path to phv binary]
# Set VALIDATOR_JAR to reuse a downloaded validator_cli.jar.
set -euo pipefail

VALIDATOR_VERSION=7.0.2
phv=${1:-target/debug/phv}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

"$phv" init "$work/vault" >/dev/null
"$phv" --vault "$work/vault" add tests/fixtures/ips/records.json >/dev/null
"$phv" --vault "$work/vault" ips --out "$work/ips.json"

jar=${VALIDATOR_JAR:-$work/validator_cli.jar}
if [ ! -f "$jar" ]; then
  curl -sSfL -o "$jar" \
    "https://github.com/hapifhir/org.hl7.fhir.core/releases/download/${VALIDATOR_VERSION}/validator_cli.jar"
fi

# -tx n/a keeps the check offline: codes are not checked against a terminology server.
java -jar "$jar" "$work/ips.json" -version 4.0.1 -ig hl7.fhir.uv.ips#2.0.0 \
  -profile http://hl7.org/fhir/uv/ips/StructureDefinition/Bundle-uv-ips -tx n/a
