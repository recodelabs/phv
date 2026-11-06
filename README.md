<p align="center">
  <img src="assets/logo.svg" alt="phv logo: a folder shaped like a shield with a medical cross" width="140">
</p>

# phv: personal health vault

phv keeps your health records as FHIR R4 files on your own machine. AI can help map your documents to FHIR, while phv itself validates and writes every record. Git is optional and adds history and undo.

**This repo holds code only.** Your health data belongs in a separate, private vault. Never commit it here.

## Status

Early development. It runs on a laptop only. The main ways in are `phv setup`, the MCP server (`phv mcp`) and the lab skill. `phv import apple-activity` imports Apple activity exports. There is no web dashboard, health memory or IPS generation yet.

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

With Git, review changes with `git diff` and commit the ones you accept. Without Git, there is no history, so `phv replace` and `phv rm` cannot be undone.

To try phv with fake data, see [demo/README.md](demo/README.md).

## Add a lab result with Claude

Install the skill, then ask Claude Code to add a lab report:

```bash
ln -s "$(pwd)/skills/phv-add-lab" ~/.claude/skills/phv-add-lab
```

Claude stores the original file, maps the results to FHIR and shows you what it extracted to confirm. phv then validates and writes the records.

## Use a Claude session

`phv setup` prints the command to connect a vault to Claude Code, one server per person:

```bash
claude mcp add phv-maya -- phv --vault maya mcp
```

A session can then store documents, add and correct records, and search the vault. It cannot delete records: use `phv rm` for that. Claude runs on Anthropic's servers, so anything a session reads, including records and documents, goes to Anthropic.

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
| `phv import apple-activity FILE [--dry-run] [--keep-overlaps]` | Import steps and workouts from an Apple activity export |
| `phv serve` | Run the REST API on `127.0.0.1:8747` |
| `phv mcp` | Run a local MCP server for one vault, for Claude sessions |

Use `-` as `FILE` for stdin, and `--json` for machine-readable output.

## Privacy and security

- **Readable files:** records are not encrypted by phv. Use full-disk encryption and keep your repo private.
- **Hosted repos:** pushing to a hosted private repo, including one created by `phv setup`, gives that host readable health data and original documents. phv has no encrypted backup yet, so it does not recommend this.
- **Git history:** in a vault with Git, `phv rm` removes a record from the files, but it stays in history until you rewrite it, and in any remote copies after that.
- **AI tools:** anything you send to a hosted AI is subject to that provider's terms. Use a local model for data that must stay on your device.
- **Not medical advice:** check every extracted value against the original document. phv checks the structure of records, not whether a value was read correctly.

## Known limits

- Validation covers base FHIR R4, not profiles.
- Some required code lists are not enforced, including currencies and MIME types. `cargo test only_the_known_required_bindings_are_uncovered -- --nocapture` lists them.
- Valid medication timing without a `period` produces a warning, caused by an upstream octofhir issue.
- Date filters need `.toDate()`: `effectiveDateTime.toDate() >= @2026-01-01`. Without it, `>=` and `<=` on a date give no answer because of an upstream octofhir issue. phv warns when records give no answer, so an empty result with a warning does not mean there are no records.
- Some FHIR rules on data types are not enforced upstream, such as a `Period` that ends before it starts, or a `Range` whose low is above its high. Check dates and ranges against the original document.

## Design

See [the vault core design](docs/superpowers/specs/2026-11-06-vault-core-design.md).

## License

MIT. See [LICENSE](LICENSE).
