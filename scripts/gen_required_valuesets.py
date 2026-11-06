#!/usr/bin/env python3
"""Generate data/required-valuesets.json from an unpacked hl7.fhir.r4.core package.

Usage:
  mkdir -p /tmp/r4core
  curl -sL https://packages.fhir.org/hl7.fhir.r4.core/4.0.1 | tar -xz -C /tmp/r4core
  python3 scripts/gen_required_valuesets.py /tmp/r4core/package data/required-valuesets.json

Value sets that cannot be fully enumerated from the package are skipped and listed on stderr.
"""
import json
import os
import sys


def flatten(concepts):
    for concept in concepts:
        yield concept["code"]
        yield from flatten(concept.get("concept", []))


def main(package_dir, output_path):
    value_sets, code_systems, definitions = {}, {}, []
    for name in os.listdir(package_dir):
        if not name.endswith(".json") or name.startswith(".") or name == "package.json":
            continue
        with open(os.path.join(package_dir, name)) as handle:
            item = json.load(handle)
        kind = item.get("resourceType")
        if kind == "ValueSet":
            value_sets[item["url"]] = item
        elif kind == "CodeSystem":
            code_systems[item["url"]] = item
        elif kind == "StructureDefinition":
            definitions.append(item)

    needed = set()
    for definition in definitions:
        if definition.get("kind") not in ("resource", "complex-type"):
            continue
        for element in definition.get("snapshot", {}).get("element", []):
            binding = element.get("binding") or {}
            if binding.get("strength") == "required" and binding.get("valueSet"):
                needed.add(binding["valueSet"])

    output, skipped = {}, {}
    for url in sorted(needed):
        value_set = value_sets.get(url.split("|")[0])
        if value_set is None:
            skipped[url] = "value set not in package"
            continue
        compose = value_set.get("compose", {})
        if compose.get("exclude"):
            skipped[url] = "uses exclude"
            continue
        codes, reason = [], None
        for include in compose.get("include", []):
            if include.get("valueSet") or include.get("filter"):
                reason = "uses a filter or imports another value set"
                break
            system = include.get("system")
            if include.get("concept"):
                codes += [[concept["code"], system] for concept in include["concept"]]
                continue
            code_system = code_systems.get(system)
            if code_system is None or code_system.get("content") != "complete":
                reason = f"code system not available: {system}"
                break
            codes += [[code, system] for code in flatten(code_system.get("concept", []))]
        if reason or not codes:
            skipped[url] = reason or "empty"
            continue
        output[url] = sorted({tuple(pair) for pair in codes})

    with open(output_path, "w") as handle:
        json.dump(output, handle, sort_keys=True, separators=(",", ":"))
        handle.write("\n")
    print(f"required bindings: {len(needed)}, enumerated: {len(output)}, skipped: {len(skipped)}")
    for url, reason in skipped.items():
        print(f"  skipped {url}: {reason}", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
