<p align="center">
  <img src="assets/logo.svg" alt="phv logo: a folder shaped like a shield with a medical cross" width="140">
</p>

# phv: personal health vault

phv keeps your health records as FHIR R4 files on your own machine. AI can help map your documents to FHIR, while phv itself validates and writes every record. Git is optional and adds history and undo.

**This repo holds code only.** Your health data belongs in a separate, private vault. Never commit it here.

## Status

Early development. It runs on a laptop only. The main ways in are `phv setup`, the MCP server (`phv mcp`) and the lab skill. `phv import apple-activity` imports Apple activity exports, and `phv ips` builds an International Patient Summary. There is no web dashboard or health memory yet.

## Install

Needs Rust 1.98 or newer.

```bash
cargo install --path . --locked
```

## Quick start

```bash
phv setup
```

`setup` walks you through creating a vault for one person. Run it again for each family member. It asks whether to use Git and whether to create a private GitHub repo, and you can say no to both.

```bash
phv vaults                     # list vaults
phv use ben                    # change the default
phv --vault maya list Patient  # use a specific vault
```

With Git, review changes before committing them. `<vault>` is the vault's folder from `phv vaults`:

```bash
git -C <vault> status
git -C <vault> add -N . && git -C <vault> diff
git -C <vault> add -A && git -C <vault> commit -m "Add lab results"
```

Plain `git diff` shows nothing for a new file, so `add -N` first makes new files show up in the diff. Without Git, there is no history, so `phv replace` and `phv rm` cannot be undone.

To try phv with fake data, see [demo/README.md](demo/README.md).

## What a vault looks like

Each person gets their own vault, a plain folder of files. This is one after adding some records and a document:

```text
maya/
├── phv.toml                     # vault format and FHIR version
├── .gitignore                   # keeps .phv/ out of Git
├── records/                     # one file per resource type, created as needed
│   ├── Patient.ndjson           # exactly one person
│   ├── Observation.ndjson       # lab results, vitals, activity
│   ├── DiagnosticReport.ndjson
│   ├── DocumentReference.ndjson # one entry per stored document
│   ├── Provenance.ndjson        # links records to the document they came from
│   └── ...
├── sources/                     # your original files, never changed
│   └── <sha256>.pdf
└── .phv/                        # lock and temporary files, safe to delete when phv isn't running
```

Every `records/` file has one FHIR resource per line, sorted by ID:

```json
{"resourceType":"Observation","id":"7794f8aa3c537cd7108909de048cb529","code":{"text":"Creatinine"},"status":"final","valueQuantity":{"unit":"mg/dL","value":1.50}}
```

A change to one record changes one line, so Git diffs stay small. Values are kept exactly as written, so `1.50` stays `1.50`. If you choose Git in `phv setup`, the vault also has a `.git` folder.

## Add a lab result with Claude

Install the skill, then ask Claude Code to add a lab report:

```bash
mkdir -p ~/.claude/skills
ln -s "$(pwd)/skills/phv-add-lab" ~/.claude/skills/phv-add-lab
```

Claude stores the original file, maps the results to FHIR and shows you what it extracted to confirm. phv then validates and writes the records.

## Use a Claude session

`phv mcp` is a local MCP server, so a Claude Code session can store your documents and work with your records. It needs `phv` on your `PATH`. Each server serves one vault, so a session connected for one family member can't touch another's records.

`phv setup` prints the command for each vault. Add `--scope user` to use the vault from any folder:

```bash
claude mcp add --scope user phv-maya -- phv --vault maya mcp
```

Start a new Claude Code session, then check that it connected with `/mcp`. You can then ask things like "add this lab report: /path/to/report.pdf" or "what was my last creatinine?". Use a full path for a document.

| Tool | What it does |
|---|---|
| `vault_info` | Shows the vault's path and the person's name and birth date |
| `source_add` | Stores an original document from a full path on this machine |
| `validate` | Checks a resource or Bundle without writing |
| `add` | Validates and writes a resource or Bundle |
| `replace` | Corrects a record, keeping its ID |
| `get` | Reads one record |
| `list` | Lists records with an optional FHIRPath filter, 50 by default |
| `check` | Checks the whole vault |

A session can't delete records: use `phv rm` for that.

**Privacy:** the server runs on your machine, but Claude runs on Anthropic's servers, so anything a session reads, including records and documents, goes to Anthropic.

To stop using a vault in Claude Code, run `claude mcp remove --scope user phv-maya`.

## Share a summary with a clinician

```bash
phv ips --out ~/Desktop/summary.json
```

`phv ips` builds an [International Patient Summary](https://hl7.org/fhir/uv/ips/STU2/) (IPS 2.0.0) from the vault: problems, allergies, medications, immunizations, results, procedures, past illness, social history and vital signs. It is a FHIR document Bundle that IPS viewers and many health systems can read. It changes nothing in the vault, and `--out` must be outside it.

Each record is checked against its IPS profile. A record that does not meet it is left out, and phv lists each one with the reason, so you can correct it with `phv replace` and run `phv ips` again. Activity data from `phv import apple-activity` is not included.

## Commands

| Command | Purpose |
|---|---|
| `phv setup` | Walk through creating a vault for one person |
| `phv vaults` | List registered vaults |
| `phv use NAME` | Change the default vault |
| `phv init PATH` | Create a vault folder with no prompts |
| `phv validate FILE` | Check a resource or Bundle without writing |
| `phv add FILE` | Validate and write a resource or Bundle |
| `phv replace Type/id FILE` | Replace a resource, keeping its ID |
| `phv rm Type/id` | Remove a resource from the files |
| `phv get Type/id` | Print one resource |
| `phv list Type [--where EXPR]` | Print resources, with an optional FHIRPath filter |
| `phv source add FILE` | Store an original file |
| `phv check` | Check the whole vault |
| `phv snapshot` | Capture records and source hashes as JSON under a read-only shared lock |
| `phv import apple-activity FILE [--dry-run] [--keep-overlaps]` | Import steps and workouts from an Apple activity export |
| `phv ips [--out FILE]` | Build an International Patient Summary, changing nothing |
| `phv serve` | Run the REST API on `127.0.0.1:8747` |
| `phv mcp` | Run a local MCP server for one vault, for Claude sessions |

Use `-` as `FILE` for stdin with `validate`, `add` and `replace`, and `--json` for machine-readable output.

## Privacy and security

- **Readable files:** records are not encrypted by phv. Use full-disk encryption and keep your repo private.
- **Hosted repos:** pushing to a hosted private repo, including one created by `phv setup`, gives that host readable health data and original documents. phv has no encrypted backup yet, so it does not recommend this.
- **Git history:** in a vault with Git, `phv rm` removes a record from the files, but it stays in history until you rewrite it, and in any remote copies after that. Removing a `DocumentReference` record does not delete the original file in `sources/`: delete that file too if the document must go. It stays in history like any other removed data.
- **Summaries:** an IPS holds your whole clinical summary. Share it only with people who need it. The MCP `ips` tool sends all of it to Anthropic.
- **AI tools:** anything you send to a hosted AI is subject to that provider's terms. Use a local model for data that must stay on your device.
- **Not medical advice:** check every extracted value against the original document. phv checks the structure of records, not whether a value was read correctly.

## Known limits

- Validation covers base FHIR R4, not profiles.
- Some required code lists are not enforced, including currencies and MIME types. `cargo test only_the_known_required_bindings_are_uncovered -- --nocapture` lists them.
- Valid medication timing without a `period` produces a warning, caused by an upstream octofhir issue.
- Date filters need `.toDate()`: `effectiveDateTime.toDate() >= @2026-01-01`. Without it, `>=` and `<=` on a date give no answer because of an upstream octofhir issue. phv warns when records give no answer, so an empty result with a warning does not mean there are no records.
- Some FHIR rules on data types are not enforced upstream, such as a `Period` that ends before it starts, or a `Range` whose low is above its high. Check dates and ranges against the original document.
- An IPS needs the person's name and birth date on the `Patient`. Lab results need the lab that ran them: phv takes it from the result's report when the result has none.
- phv checks IPS records against their profiles with octofhir, which misses some rules, such as a vital sign's UCUM unit code. CI also checks a sample IPS with the HL7 Java validator.

## Design

See [the vault core design](docs/superpowers/specs/2026-11-06-vault-core-design.md) and [the IPS design](docs/superpowers/specs/2026-11-06-ips-design.md).

## License

MIT. See [LICENSE](LICENSE).
