# Demo data

**Everything in this folder is fake.** It exists for demos and testing. Never put real health data here, because this repo is public. Keep real records in a private vault.

| Path | What it is |
|---|---|
| `maria-vault/` | A ready-made vault for Maria Rodriguez, holding her Bundle and claim files |
| `patient-fhir-bundle.json` | A fake patient, Maria Rodriguez, with 22 FHIR R4 records |
| `claims-835/` | 18 fake insurance claim payment files in X12 835 format |
| `lab-2025-02-10/` | A draft mapping of a lab report, with its review table |

## Try it

The quickest way is the ready-made vault:

```bash
phv --vault demo/maria-vault list Patient
```

Or build your own copy:

```bash
phv setup    # name it maria, and press Enter past the person's details
phv --vault maria add demo/patient-fhir-bundle.json
for f in demo/claims-835/*.edi; do phv --vault maria source add "$f"; done
phv --vault maria check
```

Skip the person's details in `phv setup`, because the Bundle brings its own `Patient` and a vault holds only one.

The loop above stores the claim files as original documents. To turn them into claim records as well:

```bash
phv --vault maria import x12-835 demo/claims-835/*.edi
```

The lab draft has its own placeholder `Patient`, so try it in a separate vault.

`tests/demo.rs` checks that this data keeps importing cleanly.
